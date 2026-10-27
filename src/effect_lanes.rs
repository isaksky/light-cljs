//! Source-order effect lanes for compile-time atoms and initializer gensyms
//! (light-cljs-480; docs/compiler-concurrency.md, "Effect lanes
//! (light-cljs-480)").
//!
//! Every top-level pass segment and every unit a namespace's dispatch (or a
//! drain) schedules gets a *position*: one counter that the coordinator
//! advances in the order `--jobs 1` runs them, so a pass, then the units it
//! scheduled in unit order, then the next pass (or the rest of this one).
//! An *activity* is what runs at a position on a thread: a pass, a unit a
//! pool slot, a rescue thread or a dispatch analyzes, a unit demanded inside
//! another activity, a Var initializer or a helper's production. Its
//! *effective position* is the lowest of its own position, the position of
//! the activity it runs inside (priority inheritance from the demander) and
//! the effective positions of the activities waiting for something it owns:
//! a unit it analyzes or a dependency cell it produces.
//!
//! A task that reads or writes a compile-time atom another activity made
//! asks [`Lanes::check`] first. The lane is open when every scheduled unit
//! and pass segment before the task's effective position has finished;
//! otherwise the task needs a fresh lane cell, which parks it before the
//! access (`__rt.need`), and [`Lanes::poll`] completes the cell once the
//! lane opens. So atom effects happen in the `--jobs 1` order at every
//! `--jobs`, and nothing is replayed. An initializer's `gensym` uses the same
//! lane and the earliest caller's generated-name scope (light-cljs-512).
//!
//! The thread-local stack records the activities a thread runs, innermost
//! last; a *root* entry starts an activity unrelated to those below it (a
//! pool job, or one a waiting worker helps with), so inheritance looks no
//! further down than the topmost root.
#![cfg_attr(not(feature = "bbp-macros"), allow(dead_code))]
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A place in the `--jobs 1` order of passes and units.
pub(crate) type Position = u64;

/// What an activity owns that other activities can wait for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    /// A catalog unit it analyzes.
    Unit(usize),
    /// A dependency cell it produces, by [`identity`] of its key.
    Cell(u64),
}

/// A stable identity for an engine key or task id, which the compiler sees
/// only as opaque `Hash` values.
pub(crate) fn identity(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// How long the earliest unfinished unit may stay unstarted while a lane
/// waits for it before a rescue thread analyzes it ([`Lanes::rescue`]).
const STARVED: Duration = Duration::from_millis(30);

/// A task's current activity, which its host effects read on any thread.
#[derive(Clone, Copy, Debug)]
struct Top {
    token: u64,
    position: Option<Position>,
}

/// A caller's activity before entering the initializer it demands. Retain
/// its ancestry so owner selection sees waiters that arrive after reservation.
#[derive(Default)]
pub(crate) struct Caller {
    position: Option<Position>,
    targets: Vec<Target>,
}

/// A task parked for its lane: where it is, what its activity chain owns,
/// and how to complete its lane cell.
struct Parked {
    position: Option<Position>,
    targets: Vec<Target>,
    since: Instant,
    complete: Option<Box<dyn FnOnce() + Send>>,
}

#[derive(Default)]
struct State {
    next: Position,
    /// The open pass segment's position and its entry's token.
    pass: Option<(Position, u64)>,
    /// Scheduled units in position order, and the first unfinished one.
    scheduled: Vec<(Position, usize)>,
    cursor: usize,
    position_of: HashMap<usize, Position>,
    finished: HashSet<usize>,
    /// The lowest position of a failed unit: once every unit before it has
    /// finished, lanes after it no longer order anything, since the build
    /// reports that unit's error or an earlier one.
    failed: Option<Position>,
    abandoned: bool,
    tops: HashMap<u64, Top>,
    waits: HashMap<u64, (Target, Option<Position>)>,
    parked: HashMap<u64, Parked>,
    entered: HashSet<u64>,
    /// Units of a blocker's consumer, which wait for their lane before
    /// they start.
    forced: HashSet<usize>,
    next_token: u64,
    next_cell: u64,
    next_wait: u64,
    starved: Option<(usize, Instant)>,
    /// Whether a task parked inside an initializer or a helper's production.
    parked_in_cell: bool,
    rescued: HashSet<usize>,
}

impl State {
    fn frontier(&mut self) -> Position {
        while let Some(&(_, unit)) = self.scheduled.get(self.cursor) {
            if !self.finished.contains(&unit) {
                break;
            }
            self.cursor += 1;
        }
        let unit = self
            .scheduled
            .get(self.cursor)
            .map_or(Position::MAX, |&(p, _)| p);
        unit.min(self.pass.map_or(Position::MAX, |(p, _)| p))
    }
    /// The lowest effective position of the activities waiting for one of
    /// `targets`.
    fn inherited(&self, targets: &[Target]) -> Option<Position> {
        if targets.is_empty() {
            return None;
        }
        (self.waits.values())
            .filter(|(target, _)| targets.contains(target))
            .filter_map(|&(_, position)| position)
            .min()
    }
    /// Whether an activity at effective `position` may touch shared atoms.
    fn open(&mut self, position: Option<Position>) -> bool {
        let Some(position) = position else {
            return true;
        };
        if self.abandoned {
            return true;
        }
        let frontier = self.frontier();
        // Units before the failure still run in order: one of them may
        // fail first, as with one slot.
        if self
            .failed
            .is_some_and(|failed| position > failed && frontier >= failed)
        {
            return true;
        }
        frontier >= position
    }
}

/// The compilation's effect lanes ([module docs](self)).
#[derive(Default)]
pub(crate) struct Lanes {
    enabled: AtomicBool,
    state: Mutex<State>,
    opened: Condvar,
    rescuer: Mutex<Option<Rescuer>>,
    rescues: Mutex<Vec<std::thread::JoinHandle<()>>>,
    entered: AtomicU64,
    waits: AtomicU64,
    inherited: AtomicU64,
    wait_nanos: AtomicU64,
    longest_nanos: AtomicU64,
    rescued: AtomicU64,
}

/// What a lane that waits for an unstarted earliest unit calls to have it
/// analyzed on a thread of its own: the unit, then its thread, if it
/// started one.
pub(crate) type Rescuer = Arc<dyn Fn(usize) -> Option<std::thread::JoinHandle<()>> + Send + Sync>;

/// `--stats` lane counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneCounts {
    /// Activities that touched an atom another activity made.
    pub entered: u64,
    /// Of those accesses, how many parked for their lane.
    pub waits: u64,
    /// Parked lanes that opened at a position an earlier waiter lent them.
    pub inherited: u64,
    pub wait_nanos: u64,
    pub longest_wait_nanos: u64,
    /// Earliest units a waiting lane had analyzed on a thread of its own.
    pub rescued: u64,
}

thread_local! {
    static STACK: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

struct Entry {
    lanes: usize,
    token: u64,
    position: Option<Position>,
    target: Option<Target>,
    root: bool,
}

/// Pops its entry from this thread's stack when dropped.
#[must_use]
pub(crate) struct EntryGuard {
    lanes: Option<Arc<Lanes>>,
    token: u64,
}
impl Drop for EntryGuard {
    fn drop(&mut self) {
        if let Some(lanes) = &self.lanes {
            STACK.with(|stack| {
                let mut stack = stack.borrow_mut();
                if let Some(at) = stack.iter().rposition(|e| e.token == self.token) {
                    stack.remove(at);
                }
            });
            lanes.changed();
        }
    }
}

/// Ends the open pass segment when dropped.
#[must_use]
pub(crate) struct PassGuard(EntryGuard);
impl Drop for PassGuard {
    fn drop(&mut self) {
        if let Some(lanes) = &self.0.lanes {
            let mut s = lanes.lock();
            if s.pass.is_some_and(|(_, token)| token == self.0.token) {
                s.pass = None;
            }
        }
    }
}

/// A registered wait for a target, refreshed while it lasts.
#[must_use]
pub(crate) struct WaitGuard {
    lanes: Option<Arc<Lanes>>,
    id: u64,
    target: Target,
}
impl WaitGuard {
    /// Record this thread's current effective position for the wait, so
    /// that the target's activity inherits it, and open lanes it unblocks.
    pub(crate) fn refresh(&self) {
        if let Some(lanes) = &self.lanes {
            let key = lanes.key();
            let (position, targets) = chain(key);
            {
                let mut s = lanes.lock();
                let position = min(position, s.inherited(&targets));
                s.waits.insert(self.id, (self.target, position));
            }
            lanes.poll();
        }
    }
}
impl Drop for WaitGuard {
    fn drop(&mut self) {
        if let Some(lanes) = &self.lanes {
            lanes.lock().waits.remove(&self.id);
        }
    }
}

fn min(a: Option<Position>, b: Option<Position>) -> Option<Position> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// This thread's activity chain for `lanes`: the innermost entry's position
/// and the targets of every entry above the topmost root.
fn chain(lanes: usize) -> (Option<Position>, Vec<Target>) {
    STACK.with(|stack| {
        let stack = stack.borrow();
        let mut targets = Vec::new();
        let mut position = None;
        let mut top = true;
        for entry in stack.iter().rev().filter(|e| e.lanes == lanes) {
            if top {
                position = entry.position;
                top = false;
            }
            targets.extend(entry.target);
            if entry.root {
                break;
            }
        }
        (position, targets)
    })
}

impl Lanes {
    /// Order compile-time atom effects in this compilation (the native
    /// backend); a disabled one answers every check as open.
    pub(crate) fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
    fn key(&self) -> usize {
        self as *const Lanes as usize
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn push(
        self: &Arc<Self>,
        own: Option<Position>,
        target: Option<Target>,
        root: bool,
    ) -> (EntryGuard, Option<Position>) {
        if !self.enabled() {
            return (
                EntryGuard {
                    lanes: None,
                    token: 0,
                },
                None,
            );
        }
        let key = self.key();
        let token = {
            let mut s = self.lock();
            s.next_token += 1;
            s.next_token
        };
        let position = if root { own } else { min(chain(key).0, own) };
        STACK.with(|stack| {
            stack.borrow_mut().push(Entry {
                lanes: key,
                token,
                position,
                target,
                root,
            })
        });
        (
            EntryGuard {
                lanes: Some(self.clone()),
                token,
            },
            position,
        )
    }
    /// Start an activity unrelated to what this thread runs below it: a
    /// pool job, a kept unit at its dispatch, a rescue.
    pub(crate) fn enter_root(self: &Arc<Self>) -> EntryGuard {
        self.push(None, None, true).0
    }
    /// Start the analysis of `unit` on this thread: at its own position if
    /// it was scheduled, or lower if the activity it runs inside is earlier.
    pub(crate) fn enter_unit(self: &Arc<Self>, unit: usize) -> EntryGuard {
        let (own, forced) = {
            let s = self.lock();
            (s.position_of.get(&unit).copied(), s.forced.contains(&unit))
        };
        let (guard, position) = self.push(own, Some(Target::Unit(unit)), false);
        if forced {
            self.wait_open(position);
        }
        guard
    }
    /// Start producing dependency cell `cell` ([`identity`]) inside the
    /// current activity, which it inherits: a Var initializer or a helper.
    pub(crate) fn enter_cell(self: &Arc<Self>, cell: u64) -> EntryGuard {
        self.push(None, Some(Target::Cell(cell)), false).0
    }
    /// Start a namespace's top-level pass on the coordinator: its segment
    /// is the next position. A blocker's consumer (`forced`) waits for its
    /// lane first, so it runs after every earlier unit, as with one slot.
    pub(crate) fn begin_pass(self: &Arc<Self>, forced: bool) -> PassGuard {
        if !self.enabled() {
            return PassGuard(self.push(None, None, true).0);
        }
        let position = {
            let mut s = self.lock();
            let position = s.next;
            s.next += 1;
            position
        };
        let (guard, _) = self.push(Some(position), None, true);
        self.lock().pass = Some((position, guard.token));
        if forced {
            self.wait_open(Some(position));
        }
        PassGuard(guard)
    }
    /// Give `units` their positions, in order, after everything scheduled
    /// so far. An open pass segment moves after them: the pass continues
    /// after these units, as with one slot.
    pub(crate) fn schedule(&self, units: impl IntoIterator<Item = usize>, forced: bool) {
        if !self.enabled() {
            return;
        }
        let key = self.key();
        let mut s = self.lock();
        let mut any = false;
        for unit in units {
            if s.position_of.contains_key(&unit) {
                continue;
            }
            let position = s.next;
            s.next += 1;
            s.position_of.insert(unit, position);
            s.scheduled.push((position, unit));
            if forced {
                s.forced.insert(unit);
            }
            any = true;
        }
        if any && let Some((_, token)) = s.pass {
            let position = s.next;
            s.next += 1;
            s.pass = Some((position, token));
            STACK.with(|stack| {
                if let Some(entry) = (stack.borrow_mut().iter_mut())
                    .rev()
                    .find(|e| e.lanes == key && e.token == token)
                {
                    entry.position = Some(position);
                }
            });
        }
    }
    /// Note that `unit`'s analysis ended (analyzed or failed), which may
    /// open lanes after it.
    pub(crate) fn finish(&self, unit: usize, failed: bool) {
        if !self.enabled() {
            return;
        }
        {
            let mut s = self.lock();
            s.finished.insert(unit);
            if failed && let Some(&position) = s.position_of.get(&unit) {
                s.failed = Some(s.failed.map_or(position, |f| f.min(position)));
            }
        }
        self.poll();
    }
    /// Open every lane: the build stopped.
    pub(crate) fn abandon(&self) {
        self.lock().abandoned = true;
        self.poll();
    }
    fn changed(&self) {
        self.poll();
    }
    /// Make `task`'s current activity this thread's innermost one, which
    /// its host effects read wherever its steps run.
    pub(crate) fn sync_task(&self, task: u64) {
        if !self.enabled() {
            return;
        }
        let key = self.key();
        let top = STACK.with(|stack| {
            (stack.borrow().iter().rev())
                .find(|e| e.lanes == key)
                .map(|e| Top {
                    token: e.token,
                    position: e.position,
                })
        });
        let mut s = self.lock();
        match top {
            Some(top) => {
                s.tops.insert(task, top);
            }
            None => {
                s.tops.remove(&task);
            }
        }
    }
    /// The caller's source position, before an initializer inherits a waiter.
    pub(crate) fn caller_position(&self, task: u64) -> Option<Position> {
        self.lock().tops.get(&task).and_then(|top| top.position)
    }
    pub(crate) fn caller(&self) -> Caller {
        let (position, targets) = chain(self.key());
        Caller { position, targets }
    }
    /// Select at the callers' current inherited positions, under one lane
    /// snapshot. The initializer being elected cannot lend one candidate's
    /// priority to its competitors; only their caller ancestry counts.
    pub(crate) fn earliest_caller<'a>(
        &self,
        callers: impl Iterator<Item = &'a Caller>,
        initializer: Target,
    ) -> usize {
        let s = self.lock();
        callers
            .enumerate()
            .min_by_key(|(_, caller)| {
                let inherited = s
                    .waits
                    .values()
                    .filter(|(target, _)| *target != initializer && caller.targets.contains(target))
                    .filter_map(|&(_, position)| position)
                    .min();
                min(caller.position, inherited).unwrap_or(0)
            })
            .expect("an initializer has at least one caller")
            .0
    }
    /// The token of `task`'s current activity, which owns the atoms it
    /// makes; 0 outside any.
    pub(crate) fn token(&self, task: u64) -> u64 {
        if !self.enabled() {
            return 0;
        }
        self.lock().tops.get(&task).map_or(0, |top| top.token)
    }
    /// Before `task` touches an atom `owner` made: 0 if it may go on, else a
    /// fresh lane cell it parks on ([`Self::attach`]).
    pub(crate) fn check(&self, task: u64, owner: u64) -> u64 {
        if !self.enabled() {
            return 0;
        }
        let mut s = self.lock();
        let Some(top) = s.tops.get(&task).copied() else {
            return 0;
        };
        if owner != 0 && owner == top.token {
            return 0;
        }
        if s.entered.insert(top.token) {
            self.entered.fetch_add(1, Ordering::Relaxed);
        }
        if s.open(top.position) {
            return 0;
        }
        s.next_cell += 1;
        let cell = s.next_cell;
        s.parked.insert(
            cell,
            Parked {
                position: top.position,
                targets: Vec::new(),
                since: Instant::now(),
                complete: None,
            },
        );
        cell
    }
    /// The parked task's thread gives lane cell `cell` what its activity
    /// chain owns and how to complete it, which happens once the lane
    /// opens, possibly at once.
    pub(crate) fn attach(&self, cell: u64, complete: Box<dyn FnOnce() + Send>) {
        let (_, targets) = chain(self.key());
        {
            let mut s = self.lock();
            match s.parked.contains_key(&cell) {
                true => {
                    if targets.iter().any(|t| matches!(t, Target::Cell(_))) {
                        s.parked_in_cell = true;
                    }
                    let parked = s.parked.get_mut(&cell).expect("parked lane");
                    parked.targets = targets;
                    parked.complete = Some(complete);
                }
                false => {
                    drop(s);
                    complete();
                    return;
                }
            }
        }
        self.waits.fetch_add(1, Ordering::Relaxed);
        self.poll();
    }
    /// Complete the lane cells whose lanes are open now.
    pub(crate) fn poll(&self) {
        if !self.enabled() {
            return;
        }
        self.opened.notify_all();
        let ready = {
            let mut s = self.lock();
            if s.parked.is_empty() {
                return;
            }
            let cells: Vec<u64> = s.parked.keys().copied().collect();
            let mut ready = Vec::new();
            for cell in cells {
                let parked = &s.parked[&cell];
                if parked.complete.is_none() {
                    continue;
                }
                let own = parked.position;
                let position = min(own, s.inherited(&parked.targets));
                if s.open(position) {
                    let lent = position != own && !s.open(own);
                    let parked = s.parked.remove(&cell).expect("parked lane");
                    ready.push((parked, lent));
                }
            }
            ready
        };
        for (parked, lent) in ready {
            if lent {
                self.inherited.fetch_add(1, Ordering::Relaxed);
            }
            let waited = parked.since.elapsed().as_nanos() as u64;
            self.wait_nanos.fetch_add(waited, Ordering::Relaxed);
            self.longest_nanos.fetch_max(waited, Ordering::Relaxed);
            if let Some(complete) = parked.complete {
                complete();
            }
        }
    }
    /// Register this thread's wait for `target`, which the target's
    /// activity inherits ([`WaitGuard::refresh`]).
    pub(crate) fn wait_for(self: &Arc<Self>, target: Target) -> WaitGuard {
        if !self.enabled() {
            return WaitGuard {
                lanes: None,
                id: 0,
                target,
            };
        }
        let id = {
            let mut s = self.lock();
            s.next_wait += 1;
            s.next_wait
        };
        let guard = WaitGuard {
            lanes: Some(self.clone()),
            id,
            target,
        };
        guard.refresh();
        guard
    }
    /// Block this thread until the lane at `position` opens: a blocker's
    /// consumer, which waits before it runs anything.
    pub(crate) fn wait_open(&self, position: Option<Position>) {
        if !self.enabled() || self.lock().open(position) {
            return;
        }
        let since = Instant::now();
        self.waits.fetch_add(1, Ordering::Relaxed);
        loop {
            let mut s = self.lock();
            if s.open(position) {
                break;
            }
            drop(
                self.opened
                    .wait_timeout(s, Duration::from_millis(20))
                    .unwrap_or_else(PoisonError::into_inner),
            );
            self.rescue();
        }
        let waited = since.elapsed().as_nanos() as u64;
        self.wait_nanos.fetch_add(waited, Ordering::Relaxed);
        self.longest_nanos.fetch_max(waited, Ordering::Relaxed);
    }
    /// Block until some task has parked for its lane inside an initializer
    /// or a helper's production, or `limit` passes (the test-only
    /// [`crate::scheduler::Perturbation::lane_hold`]).
    #[cfg(debug_assertions)]
    pub(crate) fn wait_parked(&self, limit: Duration) {
        let since = Instant::now();
        let mut s = self.lock();
        while !s.parked_in_cell && since.elapsed() < limit {
            s = self
                .opened
                .wait_timeout(s, Duration::from_millis(20))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
    /// Install what analyzes a starved earliest unit, until the pool ends.
    pub(crate) fn set_rescuer(&self, rescuer: Option<Rescuer>) {
        *self.rescuer.lock().unwrap_or_else(PoisonError::into_inner) = rescuer;
    }
    /// Called while a lane waits: if the earliest unfinished unit has stayed
    /// so for [`STARVED`] with no thread analyzing it, every thread that
    /// could may be waiting for lanes behind it (a worker's own queue holds
    /// it); a thread of its own analyzes it, as a demand would claim it.
    pub(crate) fn rescue(&self) {
        let unit = {
            let mut s = self.lock();
            let frontier = s.frontier();
            let Some(&(position, unit)) = s.scheduled.get(s.cursor) else {
                return;
            };
            if position != frontier || s.rescued.contains(&unit) {
                return;
            }
            match s.starved {
                Some((starved, since)) if starved == unit => {
                    if since.elapsed() < STARVED {
                        return;
                    }
                }
                _ => {
                    s.starved = Some((unit, Instant::now()));
                    return;
                }
            }
            s.rescued.insert(unit);
            unit
        };
        let rescuer = self
            .rescuer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(handle) = rescuer.and_then(|rescue| rescue(unit)) {
            self.rescued.fetch_add(1, Ordering::Relaxed);
            (self.rescues.lock().unwrap_or_else(PoisonError::into_inner)).push(handle);
        }
    }
    /// Wait for the rescue threads to end.
    pub(crate) fn join_rescues(&self) {
        let handles =
            std::mem::take(&mut *self.rescues.lock().unwrap_or_else(PoisonError::into_inner));
        for handle in handles {
            let _ = handle.join();
        }
    }
    pub(crate) fn counts(&self) -> LaneCounts {
        LaneCounts {
            entered: self.entered.load(Ordering::Relaxed),
            waits: self.waits.load(Ordering::Relaxed),
            inherited: self.inherited.load(Ordering::Relaxed),
            wait_nanos: self.wait_nanos.load(Ordering::Relaxed),
            longest_wait_nanos: self.longest_nanos.load(Ordering::Relaxed),
            rescued: self.rescued.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A task id whose current activity is this thread's innermost.
    fn synced(lanes: &Lanes, task: u64) -> u64 {
        lanes.sync_task(task);
        task
    }

    #[test]
    fn a_later_unit_waits_for_every_earlier_one_and_its_own_atoms_never_wait() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        // Unit 2 touches an atom unit 0 made: unit 0 and 1 are unfinished.
        let _unit = lanes.enter_unit(2);
        let task = synced(&lanes, 7);
        let own = lanes.token(task);
        assert_eq!(lanes.check(task, own), 0, "its own atom");
        let cell = lanes.check(task, 0);
        assert_ne!(cell, 0);
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        lanes.finish(1, false);
        assert!(wait.try_recv().is_err(), "unit 0 is still running");
        lanes.finish(0, false);
        wait.try_recv().expect("the lane opened");
        assert_eq!(lanes.check(task, 0), 0);
        let counts = lanes.counts();
        assert_eq!((counts.entered, counts.waits, counts.inherited), (1, 1, 0));
    }

    #[test]
    fn a_waiting_earlier_unit_lends_its_position_to_what_it_waits_for() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        // Unit 2's initializer parks behind units 0 and 1.
        let _unit = lanes.enter_unit(2);
        let _initializer = lanes.enter_cell(99);
        let task = synced(&lanes, 7);
        let cell = lanes.check(task, 0);
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        // Unit 1 waits for the initializer, on another thread.
        let other = lanes.clone();
        std::thread::spawn(move || {
            let _root = other.enter_root();
            let _unit = other.enter_unit(1);
            let waiting = other.wait_for(Target::Cell(99));
            assert!(wait.try_recv().is_err(), "unit 0 is still running");
            other.finish(0, false);
            wait.try_recv()
                .expect("the initializer runs at unit 1's place");
            drop(waiting);
        })
        .join()
        .unwrap();
        assert_eq!(lanes.counts().inherited, 1);
    }

    #[test]
    fn a_drain_inside_a_pass_moves_the_pass_after_its_units_and_a_failure_frees_later_lanes() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let pass = lanes.begin_pass(false);
        lanes.schedule([0, 1], false);
        // The pass, after its drained units, waits for them.
        let task = synced(&lanes, 3);
        let cell = lanes.check(task, 0);
        assert_ne!(cell, 0);
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        // Unit 0 failed: the build reports it, so later lanes go on.
        lanes.finish(0, true);
        wait.try_recv().expect("lanes after a failure are open");
        drop(pass);
        let _unit = lanes.enter_unit(1);
        assert_eq!(lanes.check(synced(&lanes, 4), 0), 0);
    }

    #[test]
    fn later_failure_does_not_release_effects_before_unfinished_earlier_units() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        let _unit = lanes.enter_unit(2);
        let task = synced(&lanes, 5);
        let cell = lanes.check(task, 0);
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        // Unit 1 fails while unit 0 still runs: unit 0 may fail first.
        lanes.finish(1, true);
        assert!(wait.try_recv().is_err(), "unit 0 is still running");
        lanes.finish(0, false);
        wait.try_recv()
            .expect("every unit before the failure finished");
    }

    #[test]
    fn disabled_lanes_answer_every_check_as_open() {
        let lanes = Arc::new(Lanes::default());
        lanes.schedule([0], false);
        let _unit = lanes.enter_unit(0);
        assert_eq!(lanes.check(synced(&lanes, 1), 0), 0);
        assert_eq!(lanes.counts(), LaneCounts::default());
    }
}
