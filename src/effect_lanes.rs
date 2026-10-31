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
    /// An active compiler activity, for a task realizing a shared lazy value.
    Activity(u64),
    /// The realization of a shared lazy cell, by serial: only a namespace
    /// floor names it ([`Lanes::namespace_floors`]).
    Lazy(u64),
}

/// The next number of one order of namespace creations and floors, which
/// break ties between records at one position ([`NamespaceTag`]).
pub(crate) fn next_order() -> u64 {
    next_orders(1)
}

/// The first of `count` consecutive numbers of that order, for creations
/// recorded together (a registered namespace table).
pub(crate) fn next_orders(count: u64) -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(count, Ordering::Relaxed)
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
#[derive(Clone, Debug)]
struct Top {
    token: u64,
    root_token: u64,
    position: Option<Position>,
    /// Its activity chain, innermost first, down to the topmost root:
    /// each entry's token and what it owns, which waiters lend their
    /// positions.
    chain: Vec<(u64, Option<Target>)>,
}

/// Where an activity's namespace creation goes in the compilation's
/// namespace registry (light-cljs-589): its position, 0 outside any, and
/// the work it is part of: the units and dependency cells of its activity
/// chain and the shared lazy cells its task is realizing. An earlier
/// activity that waits for one of these lends it its position for good
/// ([`Lanes::namespace_floors`]): with one slot, that work runs inside the
/// waiter, creations included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NamespaceTag {
    pub position: Position,
    pub owners: Vec<Target>,
}

/// What a reader of the namespace registry sees, once its lane is open:
/// the creations whose (floored) position is not after its effective
/// position, which an earlier activity waiting for its work may have
/// lowered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NamespaceView {
    pub effective: Position,
}

impl NamespaceView {
    /// Everything recorded: outside any activity, or without lanes.
    pub(crate) const ALL: Self = Self {
        effective: Position::MAX,
    };
}

/// This thread's innermost activity of `lanes`, if any.
fn current(lanes: usize) -> Option<Top> {
    STACK.with(|stack| {
        let stack = stack.borrow();
        let mut entries = stack.iter().rev().filter(|e| e.lanes == lanes).peekable();
        let top = *entries.peek()?;
        let mut chain = Vec::new();
        let mut root = top;
        for entry in entries {
            root = entry;
            chain.push((entry.token, entry.target));
            if entry.root {
                break;
            }
        }
        Some(Top {
            token: top.token,
            root_token: root.token,
            position: top.position,
            chain,
        })
    })
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
    home: Option<usize>,
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
    /// Whether any task parked for its lane.
    parked_any: bool,
    /// The lowest position an activity waited for each unit, cell and lazy
    /// realization at, with its place in [`next_order`] then: the
    /// namespace creations of that work count there ([`NamespaceTag`]).
    floors: HashMap<Target, (Position, u64)>,
    rescued: HashSet<usize>,
    /// Each namespace's require closure, by source index, as a dense
    /// bitset of source indices (light-cljs-672); `None` when unknown.
    closures: Vec<Option<Box<[u64]>>>,
    names: Vec<String>,
    sources: HashMap<String, usize>,
    /// The namespace of each catalog unit, of each scheduled position and
    /// of the open pass segment.
    unit_sources: HashMap<usize, usize>,
    position_sources: HashMap<Position, usize>,
    pass_source: Option<usize>,
    /// The home namespace of the atoms an activity makes, by its token: a
    /// Var initializer's namespace. Every other activity's atoms have no
    /// home and order against every earlier activity.
    homes: HashMap<u64, usize>,
    /// Per home, the first scheduled unit not yet known to be finished or
    /// outside the home's reach ([`State::frontier_for`]).
    home_cursors: HashMap<usize, usize>,
    /// The homed initializers each task runs, innermost last, wherever
    /// its steps run, and the task of each.
    initializer_tasks: HashMap<u64, Vec<u64>>,
    initializer_task_of: HashMap<u64, u64>,
}

impl State {
    /// Whether namespace `source`'s require closure contains `home`; an
    /// unknown closure contains every namespace.
    fn reaches(&self, source: Option<usize>, home: usize) -> bool {
        let Some(Some(closure)) = source.and_then(|s| self.closures.get(s)) else {
            return true;
        };
        closure
            .get(home / 64)
            .is_some_and(|word| word & (1 << (home % 64)) != 0)
    }
    /// The lowest position of an unfinished scheduled unit or the open pass
    /// segment whose namespace can reach a resource homed in `home`: one
    /// whose require closure contains it, or whose closure is unknown.
    fn frontier_for(&mut self, home: Option<usize>) -> Position {
        let frontier = self.frontier();
        let Some(home) = home else {
            return frontier;
        };
        let mut at = (self.home_cursors.get(&home).copied())
            .unwrap_or(0)
            .max(self.cursor);
        while let Some(&(_, unit)) = self.scheduled.get(at) {
            if !self.finished.contains(&unit)
                && self.reaches(self.unit_sources.get(&unit).copied(), home)
            {
                break;
            }
            at += 1;
        }
        self.home_cursors.insert(home, at);
        let unit = self.scheduled.get(at).map_or(Position::MAX, |&(p, _)| p);
        let pass = match self.pass {
            Some((p, _)) if self.reaches(self.pass_source, home) => p,
            _ => Position::MAX,
        };
        unit.min(pass)
    }
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
        self.open_for(position, None)
    }
    /// Whether an activity at effective `position` may touch a resource
    /// homed in `home` (`None`: reachable from every namespace).
    fn open_for(&mut self, position: Option<Position>, home: Option<usize>) -> bool {
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
        frontier >= position || (home.is_some() && self.frontier_for(home) >= position)
    }
    /// The namespace whose unit or pass segment holds `position`.
    fn source_at(&self, position: Position) -> Option<usize> {
        self.position_sources.get(&position).copied()
    }
    /// The require closure rule for a resource homed in `home` that a task
    /// at its own `position` and effective position `effective` touches:
    /// the namespaces whose work the access counts as, with one slot, must
    /// require the home, or another activity's lane may have skipped them.
    fn enforce(
        &self,
        home: usize,
        position: Option<Position>,
        effective: Option<Position>,
    ) -> Result<(), String> {
        for at in [position, effective].into_iter().flatten() {
            let source = self.source_at(at);
            if !self.reaches(source, home) {
                let name = |s: usize| self.names.get(s).map_or("?", String::as_str);
                return Err(format!(
                    "{} reached compile-time state of {}, which it does not require: a namespace's macros see only its require closure (add a require of {})",
                    name(source.expect("a known closure")),
                    name(home),
                    name(home),
                ));
            }
        }
        Ok(())
    }
}

fn namespace_tag(top: &Top) -> NamespaceTag {
    NamespaceTag {
        position: top.position.unwrap_or(0),
        owners: top.chain.iter().filter_map(|&(_, target)| target).collect(),
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
    skipped: AtomicU64,
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
    /// Lane accesses that went on, or parked lanes that opened, only
    /// because the earlier unfinished activities cannot reach the
    /// resource (light-cljs-672).
    pub skipped: u64,
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
            lanes.forget_initializer(self.token);
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
                s.pass_source = None;
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
    /// The lazy realization a reader waits for through its task's root.
    realization: Option<Target>,
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
                if let Some(position) = position {
                    let floored = [self.target].into_iter().chain(self.realization);
                    for target in floored.filter(|t| !matches!(t, Target::Activity(_))) {
                        let floor = s.floors.entry(target).or_insert((Position::MAX, 0));
                        if position < floor.0 {
                            *floor = (position, next_order());
                        }
                    }
                }
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
            targets.push(Target::Activity(entry.token));
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
    /// [`Self::enter_cell`] for the initializer of catalog unit `unit`:
    /// the atoms it makes are homed in the unit's namespace
    /// (light-cljs-672).
    pub(crate) fn enter_initializer(self: &Arc<Self>, cell: u64, unit: u64, task: u64) -> EntryGuard {
        let guard = self.enter_cell(cell);
        if guard.lanes.is_some() {
            let mut s = self.lock();
            if let Some(&source) = usize::try_from(unit)
                .ok()
                .and_then(|unit| s.unit_sources.get(&unit))
            {
                s.homes.insert(guard.token, source);
                s.initializer_tasks.entry(task).or_default().push(guard.token);
                s.initializer_task_of.insert(guard.token, task);
            }
        }
        guard
    }
    /// Record namespace `name`, source index `source`, whose direct requires
    /// are `requires`: its require closure is itself and the closures of
    /// the requires recorded before it (light-cljs-672). Requires that name
    /// no recorded source (`cljs.core`, JavaScript packages, host
    /// namespaces) add nothing.
    pub(crate) fn register_namespace(&self, source: usize, name: &str, requires: &[String]) {
        if !self.enabled() {
            return;
        }
        let mut s = self.lock();
        let words = source / 64 + 1;
        let mut closure = vec![0u64; words];
        closure[source / 64] |= 1 << (source % 64);
        for require in requires {
            let Some(&required) = s.sources.get(require.as_str()) else {
                continue;
            };
            match s.closures.get(required) {
                Some(Some(theirs)) => {
                    for (word, bits) in closure.iter_mut().zip(theirs.iter()) {
                        *word |= bits;
                    }
                }
                _ => {
                    // A require whose closure is unknown: so is this one's.
                    if s.closures.len() <= source {
                        s.closures.resize(source + 1, None);
                        s.names.resize(source + 1, String::new());
                    }
                    s.names[source] = name.to_owned();
                    s.sources.insert(name.to_owned(), source);
                    s.closures[source] = None;
                    return;
                }
            }
        }
        if s.closures.len() <= source {
            s.closures.resize(source + 1, None);
            s.names.resize(source + 1, String::new());
        }
        s.names[source] = name.to_owned();
        s.sources.insert(name.to_owned(), source);
        s.closures[source] = Some(closure.into_boxed_slice());
    }
    /// Note that catalog unit `unit` belongs to namespace `source`.
    pub(crate) fn note_unit(&self, unit: usize, source: usize) {
        if !self.enabled() {
            return;
        }
        self.lock().unit_sources.insert(unit, source);
    }
    /// Start a namespace's top-level pass on the coordinator: its segment
    /// is the next position. A blocker's consumer (`forced`) waits for its
    /// lane first, so it runs after every earlier unit, as with one slot.
    /// `source` is the namespace's source index, when known.
    pub(crate) fn begin_pass(self: &Arc<Self>, forced: bool, source: Option<usize>) -> PassGuard {
        if !self.enabled() {
            return PassGuard(self.push(None, None, true).0);
        }
        let position = {
            let mut s = self.lock();
            let position = s.next;
            s.next += 1;
            if let Some(source) = source {
                s.position_sources.insert(position, source);
            }
            s.pass_source = source;
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
            if let Some(&source) = s.unit_sources.get(&unit) {
                s.position_sources.insert(position, source);
            }
            if forced {
                s.forced.insert(unit);
            }
            any = true;
        }
        if any && let Some((_, token)) = s.pass {
            let position = s.next;
            s.next += 1;
            s.pass = Some((position, token));
            if let Some(source) = s.pass_source {
                s.position_sources.insert(position, source);
            }
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
        let top = current(self.key());
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
    /// Where namespace creation by this thread's current activity goes.
    pub(crate) fn namespace_tag_here(&self) -> NamespaceTag {
        if !self.enabled() {
            return NamespaceTag::default();
        }
        current(self.key()).map_or_else(NamespaceTag::default, |top| namespace_tag(&top))
    }
    /// Where namespace creation by `task`'s current activity goes.
    pub(crate) fn namespace_tag(&self, task: u64) -> NamespaceTag {
        if !self.enabled() {
            return NamespaceTag::default();
        }
        (self.lock().tops.get(&task)).map_or_else(NamespaceTag::default, namespace_tag)
    }
    /// What `task`'s current activity sees of the namespace registry, once
    /// its lane is open ([`Self::check`]): at the position its lane opened
    /// at, which an earlier activity waiting for part of its chain may have
    /// lent it.
    pub(crate) fn namespace_view(&self, task: u64) -> NamespaceView {
        if !self.enabled() {
            return NamespaceView::ALL;
        }
        let s = self.lock();
        let Some(top) = s.tops.get(&task) else {
            return NamespaceView::ALL;
        };
        let Some(own) = top.position else {
            return NamespaceView::ALL;
        };
        let owned: Vec<Target> = (top.chain.iter())
            .flat_map(|&(token, target)| [Some(Target::Activity(token)), target])
            .flatten()
            .collect();
        NamespaceView {
            effective: min(Some(own), s.inherited(&owned)).unwrap_or(own),
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
    /// makes; 0 outside any. A Var initializer running on this thread owns
    /// what it makes, and its token comes negated: its atoms have a home
    /// ([`Self::enter_initializer`]), which every task must see.
    pub(crate) fn token(&self, task: u64) -> i64 {
        if !self.enabled() {
            return 0;
        }
        let s = self.lock();
        if let Some(&token) = s.initializer_tasks.get(&task).and_then(|t| t.last()) {
            return -(token as i64);
        }
        s.tops.get(&task).map_or(0, |top| top.token as i64)
    }
    fn forget_initializer(&self, token: u64) {
        let mut s = self.lock();
        if let Some(task) = s.initializer_task_of.remove(&token)
            && let Some(tokens) = s.initializer_tasks.get_mut(&task)
        {
            tokens.retain(|&t| t != token);
            if tokens.is_empty() {
                s.initializer_tasks.remove(&task);
            }
        }
    }
    /// Before `task` touches an atom `owner` made: 0 if it may go on, else a
    /// fresh lane cell it parks on ([`Self::attach`]). An atom an
    /// initializer made is homed in the initializer's namespace: the lane
    /// skips earlier activities whose require closure does not contain it,
    /// and a task whose own work cannot reach it fails (light-cljs-672).
    pub(crate) fn check(&self, task: u64, owner: u64) -> Result<u64, String> {
        if !self.enabled() {
            return Ok(0);
        }
        let mut s = self.lock();
        let Some(top) = s.tops.get(&task) else {
            return Ok(0);
        };
        let (token, position) = (top.token, top.position);
        let initializing = |owner| {
            (s.initializer_tasks.get(&task)).is_some_and(|tokens| tokens.contains(&owner))
        };
        if owner != 0 && (owner == token || initializing(owner)) {
            return Ok(0);
        }
        let home = if owner == 0 {
            None
        } else {
            s.homes.get(&owner).copied()
        };
        if let Some(home) = home {
            let owned: Vec<Target> = (top.chain.iter())
                .flat_map(|&(token, target)| [Some(Target::Activity(token)), target])
                .flatten()
                .collect();
            let effective = min(position, s.inherited(&owned));
            s.enforce(home, position, effective)?;
        }
        if s.entered.insert(token) {
            self.entered.fetch_add(1, Ordering::Relaxed);
        }
        if s.open(position) {
            return Ok(0);
        }
        if s.open_for(position, home) {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return Ok(0);
        }
        s.next_cell += 1;
        let cell = s.next_cell;
        s.parked.insert(
            cell,
            Parked {
                position,
                targets: Vec::new(),
                since: Instant::now(),
                complete: None,
                home,
            },
        );
        Ok(cell)
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
                    s.parked_any = true;
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
                let home = parked.home;
                if s.open(position) {
                    let lent = position != own && !s.open(own);
                    let parked = s.parked.remove(&cell).expect("parked lane");
                    ready.push((parked, lent));
                } else if s.open_for(position, home) {
                    let lent = position != own && !s.open_for(own, home);
                    self.skipped.fetch_add(1, Ordering::Relaxed);
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
                realization: None,
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
            realization: None,
        };
        guard.refresh();
        guard
    }
    /// Lend this reader's position to the root activity running `task`.
    /// Initializer entries can come and go while a realization is suspended;
    /// their enclosing root keeps the loan through subsequent helper/effect
    /// waits, including when it has already parked for its lane. The
    /// realization of lazy cell `serial` it waits for gets this reader's
    /// position as its namespace floor.
    pub(crate) fn wait_for_task(self: &Arc<Self>, task: u64, serial: u64) -> Option<WaitGuard> {
        let target = self
            .lock()
            .tops
            .get(&task)
            .map(|top| Target::Activity(top.root_token))?;
        if !self.enabled() {
            return Some(self.wait_for(target));
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
            realization: Some(Target::Lazy(serial)),
        };
        guard.refresh();
        Some(guard)
    }
    /// `task`'s current activity read the settled values of the shared lazy
    /// cells `serials`: with one slot, the earliest such reader realizes a
    /// cell, so each realization gets the activity's effective position as
    /// its namespace floor, as for a reader that waited for it
    /// ([`Self::wait_for_task`]; light-cljs-601).
    pub(crate) fn floor_consumed(&self, task: u64, serials: &[u64]) {
        if serials.is_empty() || !self.enabled() {
            return;
        }
        let effective = self.namespace_view(task).effective;
        if effective == Position::MAX {
            return;
        }
        let mut s = self.lock();
        for &serial in serials {
            let floor = (s.floors)
                .entry(Target::Lazy(serial))
                .or_insert((Position::MAX, 0));
            if effective < floor.0 {
                *floor = (effective, next_order());
            }
        }
    }
    /// Run `f` with the namespace floors: for each unit, cell and lazy
    /// realization an earlier activity waited for, the lowest such
    /// position and its order ([`NamespaceTag`]).
    pub(crate) fn namespace_floors<R>(
        &self,
        f: impl FnOnce(&HashMap<Target, (Position, u64)>) -> R,
    ) -> R {
        f(&self.lock().floors)
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
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
    /// Block until some task has parked for its lane, or `limit` passes
    /// (the test-only [`crate::scheduler::Perturbation::lane_hold_any`]).
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    pub(crate) fn wait_parked_any(&self, limit: Duration) {
        let since = Instant::now();
        let mut s = self.lock();
        while !s.parked_any && since.elapsed() < limit {
            s = self
                .opened
                .wait_timeout(s, Duration::from_millis(20))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
    /// Block until `unit` has finished, or `limit` passes (the test-only
    /// [`crate::scheduler::Perturbation::hold_until`]).
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    pub(crate) fn wait_finished(&self, unit: usize, limit: Duration) {
        let since = Instant::now();
        let mut s = self.lock();
        while !s.finished.contains(&unit) && since.elapsed() < limit {
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
            skipped: self.skipped.load(Ordering::Relaxed),
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
        let own = lanes.token(task) as u64;
        assert_eq!(lanes.check(task, own).unwrap(), 0, "its own atom");
        let cell = lanes.check(task, 0).unwrap();
        assert_ne!(cell, 0);
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        lanes.finish(1, false);
        assert!(wait.try_recv().is_err(), "unit 0 is still running");
        lanes.finish(0, false);
        wait.try_recv().expect("the lane opened");
        assert_eq!(lanes.check(task, 0).unwrap(), 0);
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
        let cell = lanes.check(task, 0).unwrap();
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
    fn a_registry_reader_lent_a_position_reads_there_and_its_work_counts_there() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        // Unit 2 produces a helper that reads the namespace registry.
        let _unit = lanes.enter_unit(2);
        let unit = lanes.namespace_tag_here();
        let _helper = lanes.enter_cell(99);
        let task = synced(&lanes, 7);
        let helper = lanes.namespace_tag(task);
        assert_eq!(helper, lanes.namespace_tag_here());
        assert_eq!(helper.owners, [Target::Cell(99), Target::Unit(2)]);
        let alone = lanes.namespace_view(task);
        assert_eq!(alone.effective, 2);
        lanes.namespace_floors(|floors| assert!(floors.is_empty()));
        // Unit 1 waits for the helper, on another thread.
        let other = lanes.clone();
        let (waiting, waiter) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            let _root = other.enter_root();
            let _unit = other.enter_unit(1);
            let wait = other.wait_for(Target::Cell(99));
            waiting.send(()).unwrap();
            released.recv().unwrap();
            drop(wait);
        });
        waiter.recv().unwrap();
        assert_eq!(lanes.namespace_view(task).effective, 1);
        release.send(()).unwrap();
        thread.join().unwrap();
        // The helper's work counts at unit 1 for good; unit 2's own does not.
        assert_eq!(lanes.namespace_view(task), alone);
        lanes.namespace_floors(|floors| {
            assert_eq!(floors.get(&Target::Cell(99)).map(|f| f.0), Some(1));
            assert!(!unit.owners.iter().any(|o| floors.contains_key(o)));
        });
    }

    #[test]
    fn a_lazy_reader_floors_the_realization_it_waits_for() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        let _unit = lanes.enter_unit(2);
        let task = synced(&lanes, 7);
        let other = lanes.clone();
        std::thread::spawn(move || {
            let _root = other.enter_root();
            let _unit = other.enter_unit(1);
            drop(other.wait_for_task(task, 5).expect("active realizer"));
        })
        .join()
        .unwrap();
        lanes.namespace_floors(|floors| {
            assert_eq!(floors.get(&Target::Lazy(5)).map(|f| f.0), Some(1));
            assert_eq!(floors.len(), 1, "not the realizer's whole activity");
        });
    }

    #[test]
    fn a_reader_of_a_settled_realization_floors_it_at_its_place() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        let _unit = lanes.enter_unit(1);
        let task = synced(&lanes, 7);
        lanes.floor_consumed(task, &[5]);
        // A later reader does not move it back.
        let other = lanes.clone();
        std::thread::spawn(move || {
            let _root = other.enter_root();
            let _unit = other.enter_unit(2);
            other.floor_consumed(synced(&other, 8), &[5, 6]);
        })
        .join()
        .unwrap();
        lanes.namespace_floors(|floors| {
            assert_eq!(floors.get(&Target::Lazy(5)).map(|f| f.0), Some(1));
            assert_eq!(floors.get(&Target::Lazy(6)).map(|f| f.0), Some(2));
            assert_eq!(floors.len(), 2);
        });
    }

    #[test]
    fn a_lazy_reader_lends_its_position_to_the_realizing_task_and_nested_helpers() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        let _unit = lanes.enter_unit(2);
        let task = synced(&lanes, 7);
        // The realizer enters a cold helper, which first waits for its lane.
        // The task's recorded activity precedes that nested helper.
        let _helper = lanes.enter_cell(99);
        let cell = lanes.check(task, 0).unwrap();
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        let other = lanes.clone();
        std::thread::spawn(move || {
            let _root = other.enter_root();
            let _unit = other.enter_unit(1);
            let waiting = other.wait_for_task(task, 1).expect("active realizer");
            assert!(wait.try_recv().is_err(), "unit 0 is still running");
            other.finish(0, false);
            wait.try_recv()
                .expect("the helper inherits its lazy reader's position");
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
        let pass = lanes.begin_pass(false, None);
        lanes.schedule([0, 1], false);
        // The pass, after its drained units, waits for them.
        let task = synced(&lanes, 3);
        let cell = lanes.check(task, 0).unwrap();
        assert_ne!(cell, 0);
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        // Unit 0 failed: the build reports it, so later lanes go on.
        lanes.finish(0, true);
        wait.try_recv().expect("lanes after a failure are open");
        drop(pass);
        let _unit = lanes.enter_unit(1);
        assert_eq!(lanes.check(synced(&lanes, 4), 0).unwrap(), 0);
    }

    #[test]
    fn later_failure_does_not_release_effects_before_unfinished_earlier_units() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        let _unit = lanes.enter_unit(2);
        let task = synced(&lanes, 5);
        let cell = lanes.check(task, 0).unwrap();
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
        assert_eq!(lanes.check(synced(&lanes, 1), 0).unwrap(), 0);
        assert_eq!(lanes.counts(), LaneCounts::default());
    }
}
