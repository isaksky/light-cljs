//! Source-order effect lanes for compile-time atoms and initializer gensyms
//! (light-cljs-480; docs/compiler-concurrency.md, "Effect lanes
//! (light-cljs-480)").
//!
//! Every top-level pass segment and every unit a namespace's dispatch (or a
//! drain) schedules gets a *position*: a pass group reserved in source order,
//! followed by a counter within that group. Thus units discovered late by an
//! earlier pass still precede later passes. Within a group the order is the
//! one `--jobs 1` runs: a pass, its scheduled units, then the rest of the pass.
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
//! On the Pants build a namespace's compile-time code may touch only the
//! state of its require closure (light-cljs-672). An atom a Var initializer
//! makes, and the initializer's caller selection, are *homed* in the
//! initializer's namespace: a task touching them waits only for earlier
//! units and passes whose namespace's closure contains the home, and fails
//! if its own work's namespace does not require it, which makes the skip
//! sound ([`Lanes::check`]). Other state has no home and orders against
//! every earlier unit.
//!
//! The thread-local stack records the activities a thread runs, innermost
//! last; a *root* entry starts an activity unrelated to those below it (a
//! pool job, or one a waiting worker helps with), so inheritance looks no
//! further down than the topmost root.
#![cfg_attr(not(feature = "bbp-macros"), allow(dead_code))]
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A place in the `--jobs 1` order of passes and units: a reserved group in
/// the high half and a local sequence in the low half. Neither half depends
/// on the number of parsed forms (macros can introduce arbitrary units).
pub(crate) type Position = u128;

/// A unit's declaration order, independent of its catalog storage ID and
/// of drain segments (which move the effect position of a running pass).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct UnitOrder(u64, u64);
impl UnitOrder {
    pub(crate) const MAX: Self = Self(u64::MAX, u64::MAX);
    pub(crate) fn serial(id: usize) -> Self {
        Self(0, id as u64)
    }
}

fn position(group: u64, local: u64) -> Position {
    (Position::from(group) << 64) | Position::from(local)
}

struct Pass {
    position: Position,
    next: u64,
    next_unit: u64,
    source: Option<usize>,
    token: Option<u64>,
}

#[derive(Clone, Copy)]
enum Pending {
    Unit(usize),
    Pass(Option<usize>),
}

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
    next_group: u64,
    /// Reserved passes, including those that have not started on a worker.
    passes: HashMap<u64, Pass>,
    pass_groups: HashMap<u64, u64>,
    /// Unfinished units and pass barriers in source order, independent of
    /// the order in which workers discover units or finish their passes.
    pending: BTreeMap<Position, Pending>,
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
    /// The namespace of each catalog unit and of each scheduled position.
    unit_sources: HashMap<usize, usize>,
    position_sources: HashMap<Position, usize>,
    /// The home namespace of the atoms an activity makes, by its token: a
    /// Var initializer's namespace. Every other activity's atoms have no
    /// home and order against every earlier activity.
    homes: HashMap<u64, usize>,
    /// Per home, a lower bound for the next relevant pending position.
    /// Insertion of earlier work lowers these bounds; removal need not.
    home_cursors: HashMap<usize, Position>,
    /// The homed initializers each task runs, innermost last, wherever
    /// its steps run, and the task of each.
    initializer_tasks: HashMap<u64, Vec<u64>>,
    initializer_task_of: HashMap<u64, u64>,
    visibility_contexts: HashMap<(Option<usize>, Option<usize>, usize), usize>,
}

impl State {
    fn reserve_group(&mut self) -> u64 {
        let group = self.next_group;
        self.next_group = group.checked_add(1).expect("effect lane groups exhausted");
        group
    }
    /// Adding work can invalidate a home cursor that skipped an open pass
    /// or reached the end before this earlier group discovered its units.
    fn inserted_at(&mut self, position: Position) {
        for cursor in self.home_cursors.values_mut() {
            *cursor = (*cursor).min(position);
        }
    }
    /// A catalog source index is a resource home only once its namespace
    /// is known. Helper-only sources keep the conservative, unhomed lane.
    fn unit_home(&self, unit: usize) -> Option<usize> {
        self.unit_sources
            .get(&unit)
            .copied()
            .filter(|&source| self.names.get(source).is_some_and(|name| !name.is_empty()))
    }
    /// Whether namespace `source`'s require closure contains `home`; an
    /// unknown closure contains every namespace.
    fn reaches(&self, source: Option<usize>, home: usize) -> bool {
        if self.names.get(home).is_some_and(|name| name == "cljs.core") {
            return true;
        }
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
        let at = (self.home_cursors.get(&home).copied())
            .unwrap_or(0)
            .max(frontier);
        let next = self.pending.range(at..).find_map(|(&position, &pending)| {
            let source = match pending {
                Pending::Unit(unit) => self.unit_sources.get(&unit).copied(),
                Pending::Pass(source) => source,
            };
            self.reaches(source, home).then_some(position)
        });
        let next = next.unwrap_or(Position::MAX);
        self.home_cursors.insert(home, next);
        next
    }
    fn frontier(&self) -> Position {
        self.pending
            .first_key_value()
            .map_or(Position::MAX, |(&p, _)| p)
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
    /// Once pass reservations overlap, declaration order can differ from
    /// allocation order. Before then, IDs already give the canonical order.
    reserved_unit_order: AtomicBool,
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

/// A source-order barrier that may be sent to a worker before it starts a
/// pass. Dropping an unstarted reservation releases its barrier too.
#[must_use]
pub(crate) struct PassReservation {
    lanes: Option<Arc<Lanes>>,
    group: u64,
}
impl PassReservation {
    pub(crate) fn begin(self, forced: bool) -> PassGuard {
        let entry = if let Some(lanes) = &self.lanes {
            let position = lanes.lock().passes[&self.group].position;
            let (guard, _) = lanes.push(Some(position), None, true);
            {
                let mut s = lanes.lock();
                s.passes.get_mut(&self.group).unwrap().token = Some(guard.token);
                s.pass_groups.insert(guard.token, self.group);
            }
            if forced {
                lanes.wait_open(Some(position));
            }
            guard
        } else {
            EntryGuard {
                lanes: None,
                token: 0,
            }
        };
        PassGuard {
            _reservation: self,
            _entry: entry,
            _thread: std::marker::PhantomData,
        }
    }
}
impl Drop for PassReservation {
    fn drop(&mut self) {
        if let Some(lanes) = &self.lanes {
            {
                let mut s = lanes.lock();
                if let Some(pass) = s.passes.remove(&self.group) {
                    s.pending.remove(&pass.position);
                    if let Some(token) = pass.token {
                        s.pass_groups.remove(&token);
                    }
                }
            }
            lanes.changed();
        }
    }
}

/// Ends this thread's pass. Release the barrier before popping the entry,
/// whose destructor polls lanes, just as for an inline coordinator pass.
#[must_use]
pub(crate) struct PassGuard {
    _reservation: PassReservation,
    _entry: EntryGuard,
    // A pass runs on the thread that owns its stack entry. Its unstarted
    // reservation can move to a worker; the running pass cannot.
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
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

fn root_token(lanes: usize) -> Option<u64> {
    STACK.with(|stack| {
        stack
            .borrow()
            .iter()
            .rev()
            .find(|entry| entry.lanes == lanes && entry.root)
            .map(|entry| entry.token)
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
    pub(crate) fn enter_initializer(
        self: &Arc<Self>,
        cell: u64,
        unit: u64,
        task: u64,
    ) -> EntryGuard {
        let guard = self.enter_cell(cell);
        if guard.lanes.is_some() {
            let mut s = self.lock();
            if let Some(source) = usize::try_from(unit)
                .ok()
                .and_then(|unit| s.unit_home(unit))
            {
                s.homes.insert(guard.token, source);
            }
            // Unknown initializers must shadow an outer known home too:
            // the global hierarchy's special cell is one such initializer.
            s.initializer_tasks
                .entry(task)
                .or_default()
                .push(guard.token);
            s.initializer_task_of.insert(guard.token, task);
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
    /// Give a test fixture unit a home without registering a declaration.
    #[cfg(test)]
    pub(crate) fn note_unit(&self, unit: usize, source: usize) {
        if !self.enabled() {
            return;
        }
        self.lock().unit_sources.insert(unit, source);
    }
    /// Give a newly registered catalog unit its immutable declaration order.
    /// Reserved passes can register in any worker order. Outside a pass,
    /// registration remains serial and gets a fresh group. Disabled lanes
    /// use the existing serial catalog order without taking the lane lock.
    pub(crate) fn register_unit(&self, unit: usize, source: usize) -> UnitOrder {
        if !self.enabled() {
            return UnitOrder::serial(unit);
        }
        let mut s = self.lock();
        s.unit_sources.insert(unit, source);
        if !self.uses_reserved_unit_order() {
            return UnitOrder::serial(unit);
        }
        let token = root_token(self.key());
        let (group, local) = match token.and_then(|t| s.pass_groups.get(&t).copied()) {
            Some(group) => {
                let pass = s.passes.get_mut(&group).unwrap();
                let local = pass.next_unit;
                pass.next_unit = local
                    .checked_add(1)
                    .expect("unit declaration order exhausted");
                (group, local)
            }
            None => (s.reserve_group(), 0),
        };
        // Group zero holds registrations made before reservations overlapped.
        // reserve_group never returns u64::MAX, leaving room for this offset.
        UnitOrder(group + 1, local)
    }
    pub(crate) fn uses_reserved_unit_order(&self) -> bool {
        self.reserved_unit_order.load(Ordering::Acquire)
    }
    /// Reserve a namespace pass in canonical order, before launching its
    /// worker. Its barrier orders later work even while the pass is queued.
    /// The source index identifies resource reachability, not pass order:
    /// the same source can have multiple passes.
    pub(crate) fn reserve_pass(self: &Arc<Self>, source: Option<usize>) -> PassReservation {
        if !self.enabled() {
            return PassReservation {
                lanes: None,
                group: 0,
            };
        }
        let group = {
            let mut s = self.lock();
            if !s.passes.is_empty() {
                self.reserved_unit_order.store(true, Ordering::Release);
            }
            let group = s.reserve_group();
            let position = position(group, 0);
            if let Some(source) = source {
                s.position_sources.insert(position, source);
            }
            s.passes.insert(
                group,
                Pass {
                    position,
                    next: 1,
                    next_unit: 0,
                    source,
                    token: None,
                },
            );
            s.pending.insert(position, Pending::Pass(source));
            s.inserted_at(position);
            group
        };
        PassReservation {
            lanes: Some(self.clone()),
            group,
        }
    }
    /// Reserve and immediately start an inline namespace pass. A blocker's
    /// consumer (`forced`) waits for every earlier activity before starting.
    pub(crate) fn begin_pass(self: &Arc<Self>, forced: bool, source: Option<usize>) -> PassGuard {
        self.reserve_pass(source).begin(forced)
    }
    /// Give units positions within this thread's pass group. Only that
    /// pass moves after them; another worker's pass cannot move. Outside a
    /// pass, reserve a new group for the batch in discovery order.
    pub(crate) fn schedule(&self, units: impl IntoIterator<Item = usize>, forced: bool) {
        if !self.enabled() {
            return;
        }
        let key = self.key();
        let token = root_token(key);
        let mut s = self.lock();
        let pass_group = token.and_then(|t| s.pass_groups.get(&t).copied());
        let mut batch = None;
        for unit in units {
            if s.position_of.contains_key(&unit) {
                continue;
            }
            // A reserved pass's require closure bounds all work it may
            // still discover. Otherwise a later homed effect could have
            // skipped its barrier before this unit was known. Callers
            // scheduling mixed-source work must reserve an unknown source.
            if let Some(source) = pass_group.and_then(|g| s.passes[&g].source) {
                assert_eq!(
                    s.unit_sources.get(&unit),
                    Some(&source),
                    "a namespace pass scheduled a unit from another source"
                );
            }
            let (group, next) = batch.get_or_insert_with(|| match pass_group {
                Some(g) => (g, s.passes[&g].next),
                None => (s.reserve_group(), 0),
            });
            let position = position(*group, *next);
            *next = next
                .checked_add(1)
                .expect("effect lane positions exhausted");
            s.position_of.insert(unit, position);
            if !s.finished.contains(&unit) {
                s.pending.insert(position, Pending::Unit(unit));
            }
            if let Some(&source) = s.unit_sources.get(&unit) {
                s.position_sources.insert(position, source);
            }
            if forced {
                s.forced.insert(unit);
            }
        }
        if let Some((group, next)) = batch {
            // The previous barrier is a conservative lower bound for all
            // work added to this pass; a standalone batch starts at zero.
            let first = pass_group.map_or(position(group, 0), |g| s.passes[&g].position);
            s.inserted_at(first);
            if let Some(group) = pass_group {
                let position = position(group, next);
                let pass = s.passes.get_mut(&group).unwrap();
                let prior = pass.position;
                pass.position = position;
                pass.next = next
                    .checked_add(1)
                    .expect("effect lane positions exhausted");
                let source = pass.source;
                let token = pass.token.unwrap();
                s.pending.remove(&prior);
                s.pending.insert(position, Pending::Pass(source));
                if let Some(source) = source {
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
            if let Some(&position) = s.position_of.get(&unit) {
                s.pending.remove(&position);
            }
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
    /// makes; 0 outside any. A Var initializer owns what it makes. Its
    /// token comes negated only when its home is known; an unknown nested
    /// initializer keeps its own unhomed token, never the outer one's home.
    pub(crate) fn token(&self, task: u64) -> i64 {
        if !self.enabled() {
            return 0;
        }
        let s = self.lock();
        if let Some(&token) = s.initializer_tasks.get(&task).and_then(|t| t.last()) {
            return if s.homes.contains_key(&token) {
                -(token as i64)
            } else {
                token as i64
            };
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
        let s = self.lock();
        let Some(top) = s.tops.get(&task) else {
            return Ok(0);
        };
        let token = top.token;
        let initializing =
            |owner| (s.initializer_tasks.get(&task)).is_some_and(|tokens| tokens.contains(&owner));
        if owner != 0 && (owner == token || initializing(owner)) {
            return Ok(0);
        }
        let home = if owner == 0 {
            None
        } else {
            s.homes.get(&owner).copied()
        };
        drop(s);
        self.check_home(task, home)
    }
    /// Whether `task`'s current activity sees namespace `name` in the
    /// registry (`find-ns`, `all-ns`): a namespace of this compilation's
    /// sources only when its unit's or pass's namespace requires it, as
    /// the require closure rule says (light-cljs-672); every other name
    /// (`cljs.core`, `cljs.user`, `create-ns`) always.
    pub(crate) fn sees(&self, task: u64, name: &[u16]) -> bool {
        if !self.enabled() {
            return true;
        }
        let s = self.lock();
        let Some(top) = s.tops.get(&task) else {
            return true;
        };
        let Some(&home) = String::from_utf16(name)
            .ok()
            .and_then(|name| s.sources.get(&name))
        else {
            return true;
        };
        let owned: Vec<_> = top
            .chain
            .iter()
            .flat_map(|&(token, target)| [Some(Target::Activity(token)), target])
            .flatten()
            .collect();
        s.enforce(home, top.position, min(top.position, s.inherited(&owned)))
            .is_ok()
    }
    /// Macro provider admission before entering the runtime, using the
    /// actual activity rather than a dynamically rebound namespace name.
    pub(crate) fn sees_here(&self, name: &str) -> bool {
        if !self.enabled() {
            return true;
        }
        let Some(top) = current(self.key()) else {
            return true;
        };
        let s = self.lock();
        let Some(&home) = s.sources.get(name) else {
            return true;
        };
        let owned: Vec<_> = top
            .chain
            .iter()
            .flat_map(|&(token, target)| [Some(Target::Activity(token)), target])
            .flatten()
            .collect();
        s.enforce(home, top.position, min(top.position, s.inherited(&owned)))
            .is_ok()
    }
    /// Extension admission uses the defining unit, including bundles with no
    /// named products. Unknown defining namespaces remain conservative.
    pub(crate) fn sees_unit(&self, task: u64, unit: usize) -> bool {
        if !self.enabled() {
            return true;
        }
        let s = self.lock();
        let Some(top) = s.tops.get(&task) else {
            return true;
        };
        let Some(home) = s.unit_home(unit) else {
            return true;
        };
        let owned: Vec<_> = top
            .chain
            .iter()
            .flat_map(|&(token, target)| [Some(Target::Activity(token)), target])
            .flatten()
            .collect();
        s.enforce(home, top.position, min(top.position, s.inherited(&owned)))
            .is_ok()
    }
    /// Dispatch snapshots belong to the consumer's visibility context, which
    /// can change when a parked producer inherits an earlier caller's lane.
    pub(crate) fn visibility_context(&self, task: u64) -> usize {
        let mut s = self.lock();
        let Some(top) = s.tops.get(&task).filter(|_| self.enabled()) else {
            return 0;
        };
        let owned: Vec<_> = top
            .chain
            .iter()
            .flat_map(|&(token, target)| [Some(Target::Activity(token)), target])
            .flatten()
            .collect();
        let effective = min(top.position, s.inherited(&owned));
        let key = (
            top.position.and_then(|p| s.source_at(p)),
            effective.and_then(|p| s.source_at(p)),
            s.sources.len(),
        );
        let next = s.visibility_contexts.len() + 1;
        *s.visibility_contexts.entry(key).or_insert(next)
    }
    /// The namespaces `task`'s current activity does not see
    /// ([`Self::sees`]), as UTF-16 names.
    pub(crate) fn hidden(&self, task: u64) -> HashSet<Vec<u16>> {
        if !self.enabled() {
            return HashSet::new();
        }
        let s = self.lock();
        let Some(top) = s.tops.get(&task) else {
            return HashSet::new();
        };
        let owned: Vec<_> = top
            .chain
            .iter()
            .flat_map(|&(token, target)| [Some(Target::Activity(token)), target])
            .flatten()
            .collect();
        let effective = min(top.position, s.inherited(&owned));
        (s.sources.iter())
            .filter(|&(_, &home)| s.enforce(home, top.position, effective).is_err())
            .map(|(name, _)| name.encode_utf16().collect())
            .collect()
    }
    /// Before `task` selects the caller that runs the initializer of
    /// catalog unit `unit`: the lane of the unit's namespace, as for an atom
    /// homed there ([`Self::check`]). Only an activity whose require
    /// closure contains the namespace can call the initializer.
    pub(crate) fn check_initializer(&self, task: u64, unit: u64) -> Result<u64, String> {
        if !self.enabled() {
            return Ok(0);
        }
        let home = (usize::try_from(unit).ok()).and_then(|unit| self.lock().unit_home(unit));
        self.check_home(task, home)
    }
    fn check_home(&self, task: u64, home: Option<usize>) -> Result<u64, String> {
        let mut s = self.lock();
        let Some(top) = s.tops.get(&task) else {
            return Ok(0);
        };
        let (token, position) = (top.token, top.position);
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
            let Some((&_, &Pending::Unit(unit))) = s.pending.first_key_value() else {
                return;
            };
            if s.rescued.contains(&unit) {
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

    /// Namespaces `lib` (0), `grammar` (1, no requires), `ui.a` and `ui.b`
    /// (2 and 3, requiring `lib`); units 10, 11 and 12 of the last three,
    /// scheduled in that order, and unit 5 of `lib`, whose initializer has
    /// made an atom, by the returned owner token.
    fn closure_fixture() -> (Arc<Lanes>, u64) {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let lib = vec!["lib".to_owned()];
        lanes.register_namespace(0, "lib", &[]);
        lanes.register_namespace(1, "grammar", &["cljs.core".to_owned()]);
        lanes.register_namespace(2, "ui.a", &lib);
        lanes.register_namespace(3, "ui.b", &lib);
        for (unit, source) in [(5, 0), (10, 1), (11, 2), (12, 3)] {
            lanes.note_unit(unit, source);
        }
        lanes.schedule([10, 11, 12], false);
        let owner = {
            let _root = lanes.enter_root();
            let _initializer = lanes.enter_initializer(77, 5, 40);
            let owner = lanes.token(synced(&lanes, 40));
            assert!(owner < 0, "an initializer's atoms have a home");
            (-owner) as u64
        };
        (lanes, owner)
    }

    #[test]
    fn a_lane_skips_earlier_units_whose_closure_cannot_reach_the_atom() {
        let (lanes, owner) = closure_fixture();
        // `ui.b` touches `lib`'s atom: `grammar`'s unit 10 is still running
        // and cannot reach it; `ui.a`'s unit 11 can, and is waited for.
        let _unit = lanes.enter_unit(12);
        let task = synced(&lanes, 7);
        let cell = lanes.check(task, owner).unwrap();
        assert_ne!(cell, 0, "ui.a requires lib");
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        assert!(wait.try_recv().is_err());
        lanes.finish(11, false);
        wait.try_recv()
            .expect("grammar's unit does not hold the lane");
        assert_eq!(lanes.check(task, owner).unwrap(), 0);
        // An atom with no home still waits for every earlier unit.
        assert_ne!(lanes.check(task, 0).unwrap(), 0, "grammar is unfinished");
        assert_eq!(lanes.counts().skipped, 2);
    }

    #[test]
    fn a_unit_whose_closure_cannot_reach_a_homed_atom_fails_when_it_touches_it() {
        let (lanes, owner) = closure_fixture();
        let _unit = lanes.enter_unit(10);
        let task = synced(&lanes, 8);
        let error = lanes.check(task, owner).unwrap_err();
        assert!(
            error.contains("grammar reached compile-time state of lib"),
            "{error}"
        );
        // Its own and unhomed atoms are unaffected.
        assert_eq!(lanes.check(task, 0).unwrap(), 0);
    }

    #[test]
    fn extension_admission_and_dispatch_context_follow_consumer_closures() {
        let (lanes, _) = closure_fixture();
        lanes.note_unit(999, usize::MAX);
        let unknown = synced(&lanes, 7);
        assert!(lanes.sees_unit(unknown, 5));
        assert!(lanes.sees_unit(unknown, 999));
        let context = {
            let _unit = lanes.enter_unit(12);
            synced(&lanes, 7);
            assert!(lanes.sees_unit(7, 5));
            assert!(!lanes.sees_unit(7, 10));
            assert!(lanes.sees_here("lib"));
            assert!(!lanes.sees_here("grammar"));
            assert!(lanes.sees_unit(7, 999));
            assert_ne!(
                lanes.check_initializer(7, 999).unwrap(),
                0,
                "an unknown home still waits for every earlier activity"
            );
            let context = lanes.visibility_context(7);
            assert_eq!(context, lanes.visibility_context(7));
            context
        };
        let _unit = lanes.enter_unit(10);
        synced(&lanes, 7);
        assert!(!lanes.sees_unit(7, 5));
        assert!(lanes.sees_unit(7, 10));
        assert_ne!(context, lanes.visibility_context(7));
    }

    #[test]
    fn an_unknown_initializer_nested_in_a_known_one_keeps_its_own_unhomed_owner() {
        let (lanes, _) = closure_fixture();
        lanes.note_unit(999, usize::MAX);
        let _unit = lanes.enter_unit(12);
        let task = synced(&lanes, 7);
        let _known = lanes.enter_initializer(78, 5, task);
        let known = lanes.token(task);
        assert!(known < 0);
        {
            let _unknown = lanes.enter_initializer(79, 999, task);
            let unknown = lanes.token(task);
            assert!(
                unknown > 0,
                "unknown initializer inherited home token {unknown}"
            );
            assert_ne!(unknown, -known);
        }
        assert_eq!(lanes.token(task), known);
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
    fn reserved_passes_order_late_units_even_when_workers_start_backwards() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let earlier = lanes.reserve_pass(None);
        let later = lanes.reserve_pass(None);
        let (ready, started) = mpsc::channel();
        let (opened, wait) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let other = lanes.clone();
        let worker = std::thread::spawn(move || {
            let pass = later.begin(false);
            let at = other.namespace_tag_here().position;
            // This unit is discovered before any of the earlier pass's.
            other.schedule([20], false);
            let after = other.namespace_tag_here().position;
            assert!(after > at);
            let cell = other.check(synced(&other, 7), 0).unwrap();
            assert_ne!(cell, 0, "the unstarted earlier pass is a barrier");
            other.attach(cell, Box::new(move || opened.send(()).unwrap()));
            ready.send(after).unwrap();
            released.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(
                other.namespace_tag_here().position,
                after,
                "another worker's drain cannot move this pass"
            );
            drop(pass);
        });
        let later_position = started.recv_timeout(Duration::from_secs(5)).unwrap();
        let pass = earlier.begin(false);
        lanes.schedule([10], false);
        let first_segment = lanes.namespace_tag_here().position;
        lanes.schedule([11], false);
        let second_segment = lanes.namespace_tag_here().position;
        assert!(first_segment < second_segment && second_segment < later_position);
        {
            let s = lanes.lock();
            assert!(s.position_of[&10] < s.position_of[&11]);
            assert!(s.position_of[&11] < s.position_of[&20]);
        }
        lanes.finish(20, false);
        lanes.finish(11, false);
        drop(pass);
        assert!(
            wait.try_recv().is_err(),
            "the late-discovered unit 10 remains"
        );
        lanes.finish(10, false);
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(lanes.lock().frontier(), Position::MAX);
    }

    #[test]
    fn reservations_keep_home_filtering_and_repeat_sources_in_reservation_order() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.register_namespace(0, "lib", &[]);
        lanes.register_namespace(1, "unrelated", &[]);
        lanes.register_namespace(2, "app", &["lib".into()]);
        lanes.note_unit(5, 0);
        lanes.note_unit(10, 2);
        let owner = {
            let _root = lanes.enter_root();
            let _initializer = lanes.enter_initializer(77, 5, 40);
            (-lanes.token(synced(&lanes, 40))) as u64
        };
        let unrelated = lanes.reserve_pass(Some(1));
        let earlier = lanes.reserve_pass(Some(2));
        let later = lanes.reserve_pass(Some(2));
        let _pass = later.begin(false);
        let task = synced(&lanes, 7);
        let cell = lanes.check(task, owner).unwrap();
        assert_ne!(cell, 0);
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        let other = lanes.clone();
        std::thread::spawn(move || {
            let pass = earlier.begin(false);
            other.schedule([10], false);
            drop(pass);
        })
        .join()
        .unwrap();
        assert!(wait.try_recv().is_err(), "the earlier app unit is pending");
        lanes.finish(10, false);
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(lanes.check(task, owner).unwrap(), 0, "unrelated is skipped");
        let global = lanes.check(task, 0).unwrap();
        assert_ne!(global, 0, "unknown homes still wait for unrelated work");
        let (opened, wait) = mpsc::channel();
        lanes.attach(global, Box::new(move || opened.send(()).unwrap()));
        drop(unrelated);
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn dropping_an_unstarted_reservation_releases_a_forced_pass() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let earlier = lanes.reserve_pass(None);
        let later = lanes.reserve_pass(None);
        let (started, wait) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _pass = later.begin(true);
            started.send(()).unwrap();
        });
        assert!(wait.try_recv().is_err());
        drop(earlier);
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        assert!(lanes.lock().pending.is_empty());
    }

    #[test]
    fn a_reserved_earlier_pass_lends_its_position_to_later_work() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let earlier = lanes.reserve_pass(None);
        let later = lanes.reserve_pass(None);
        let (ready, started) = mpsc::channel();
        let (opened, wait) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let other = lanes.clone();
        let worker = std::thread::spawn(move || {
            let _pass = later.begin(false);
            other.schedule([20], false);
            let _root = other.enter_root();
            let _unit = other.enter_unit(20);
            let _helper = other.enter_cell(99);
            let task = synced(&other, 7);
            let cell = other.check(task, 0).unwrap();
            assert_ne!(cell, 0);
            other.attach(cell, Box::new(move || opened.send(()).unwrap()));
            ready.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        let _pass = earlier.begin(false);
        let at = lanes.namespace_tag_here().position;
        let _waiting = lanes.wait_for(Target::Cell(99));
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        lanes.namespace_floors(|floors| {
            assert_eq!(floors[&Target::Cell(99)].0, at);
        });
        release.send(()).unwrap();
        worker.join().unwrap();
        lanes.finish(20, false);
    }

    #[test]
    fn reserved_failures_wait_for_earlier_undiscovered_work_and_abandon_opens_all() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let first = lanes.reserve_pass(None);
        let failed = lanes.reserve_pass(None);
        let later = lanes.reserve_pass(None);
        {
            let _pass = failed.begin(false);
            lanes.schedule([10], false);
        }
        lanes.finish(10, true);
        let _pass = later.begin(false);
        let cell = lanes.check(synced(&lanes, 7), 0).unwrap();
        assert_ne!(
            cell, 0,
            "the first pass can still discover an earlier failure"
        );
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        drop(first);
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        // A fresh compilation can instead stop with reservations outstanding.
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let _reserved = lanes.reserve_pass(None);
        let _later = lanes.begin_pass(false, None);
        let cell = lanes.check(synced(&lanes, 7), 0).unwrap();
        let (opened, wait) = mpsc::channel();
        lanes.attach(cell, Box::new(move || opened.send(()).unwrap()));
        lanes.abandon();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn rescue_selects_source_order_and_never_overtakes_an_unstarted_pass() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let earlier = lanes.reserve_pass(None);
        let later = lanes.reserve_pass(None);
        {
            let _pass = later.begin(false);
            lanes.schedule([20], false);
        }
        let (rescued, wait) = mpsc::channel();
        lanes.set_rescuer(Some(Arc::new(move |unit| {
            rescued.send(unit).unwrap();
            None
        })));
        lanes.lock().starved = Some((20, Instant::now() - STARVED));
        lanes.rescue();
        assert!(
            wait.try_recv().is_err(),
            "earlier has not discovered its work"
        );
        {
            let _pass = earlier.begin(false);
            lanes.schedule([10], false);
        }
        lanes.lock().starved = Some((10, Instant::now() - STARVED));
        lanes.rescue();
        assert_eq!(wait.try_recv().unwrap(), 10);
        lanes.finish(10, false);
        lanes.lock().starved = Some((20, Instant::now() - STARVED));
        lanes.rescue();
        assert_eq!(wait.try_recv().unwrap(), 20);
        lanes.finish(20, false);
        // Demanded units can finish before the dispatch schedules them.
        lanes.finish(30, false);
        lanes.schedule([30], false);
        assert!(lanes.lock().pending.is_empty());
    }

    #[test]
    fn disabled_lanes_answer_every_check_as_open() {
        let lanes = Arc::new(Lanes::default());
        lanes.schedule([0], false);
        let _unit = lanes.enter_unit(0);
        assert_eq!(lanes.check(synced(&lanes, 1), 0).unwrap(), 0);
        assert_eq!(lanes.counts(), LaneCounts::default());
    }

    #[test]
    fn declaration_order_survives_drains_reverse_registration_and_enabling_lanes() {
        let lanes = Arc::new(Lanes::default());
        let boot = lanes.register_unit(99, 0);
        lanes.enable();
        let earlier = lanes.reserve_pass(Some(0));
        let later = lanes.reserve_pass(Some(0));
        let late_order = {
            let _pass = later.begin(false);
            lanes.register_unit(0, 0)
        };
        let (a, b) = {
            let _pass = earlier.begin(false);
            let a = lanes.register_unit(1, 0);
            lanes.schedule([1], false);
            let b = lanes.register_unit(2, 0);
            (a, b)
        };
        let outside = lanes.register_unit(3, 0);
        assert!(boot < a && a < b && b < late_order && late_order < outside);
    }

    #[test]
    fn overlapping_reservations_preserve_units_registered_in_the_serial_fast_path() {
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        let _earlier = lanes.begin_pass(false, Some(0));
        assert!(!lanes.uses_reserved_unit_order());
        let serial = lanes.register_unit(0, 0);
        assert_eq!(serial, UnitOrder::serial(0));
        let later = lanes.reserve_pass(Some(1));
        assert!(lanes.uses_reserved_unit_order());
        let late = {
            let _later = later.begin(false);
            lanes.register_unit(1, 1)
        };
        let early = lanes.register_unit(2, 0);
        assert!(serial < early && early < late);
        assert!(
            lanes.uses_reserved_unit_order(),
            "the transition is permanent"
        );
    }
}
