//! Dependency cells: versioned single-assignment results and the
//! read-or-subscribe operation (`docs/design.md` §§7–9).
//!
//! # Model
//!
//! * A [`DependencyKey`] (world/version, definition/query, result kind)
//!   names exactly one **cell** per runtime. Cells live in the runtime's
//!   dependency table, a private hash map keyed by the numeric identities;
//!   a lookup never runs user hashing, equality or getters. A cell is created
//!   on the first need or claim of its key and is never removed or reset in
//!   this slice (no invalidation), so a completed result stays stable.
//! * A cell is `Pending` until it reaches one immutable terminal outcome:
//!   `Ready(value)`, `Failed(error)` or `Cancelled`. Completion happens once;
//!   a second completion (of any outcome) is rejected with
//!   [`CompleteError::AlreadyComplete`] and leaves the first in place. No
//!   operation returns a cell to `Pending`.
//! * A cell has at most one **producer**. The first need of a pending,
//!   unclaimed cell (or an explicit [`Runtime::claim`](crate::Runtime::claim))
//!   claims it and returns a [`ProducerRequest`]: scheduler work for the
//!   host, never a call into host code. Only the claimed [`ProducerId`] may
//!   complete the cell, so a producer of another key, including the same
//!   definition in another world, cannot substitute its result
//!   ([`CompleteError::NotProducer`]).
//! * A **consumer** is a task. A waiter is its [`WakeHandle`] (task ID plus
//!   the run that subscribed) and a non-owning reference to the task, never
//!   a JavaScript callback. One entry per task: a need by the same task in a
//!   later run replaces the stale handle in place.
//!
//! # Read-or-subscribe
//!
//! A need ([`RunGuard::need`](crate::RunGuard::need)) for key `k` by task `t`:
//!
//! 1. finds or creates the cell for `k` (dependency-table read lock, or the
//!    write lock to create; released);
//! 2. **fast path**: reads the cell's outcome with no lock (an atomic
//!    `OnceLock` load); if set, copies it and skips to step 4;
//! 3. **slow path**, under the cell's lock, in **one** critical section:
//!    *rechecks* the outcome and copies it if set; otherwise adds `t`'s
//!    handle to the waiters and, if no producer has been claimed, claims
//!    one;
//! 4. records `k` in `t`'s [`DependencyLog`]: always as an edge (ready reads
//!    included), and as `t`'s unresolved wait while the cell is pending;
//! 5. after every lock is released, roots a terminal value for the caller.
//!
//! The outcome is only ever set while the cell lock is held, and completion
//! drains the waiters in that same critical section. So a need's slow path
//! is ordered entirely before a completion (and its handle is drained) or
//! entirely after it (and its recheck sees the outcome): a completion that
//! lands between the fast-path read and the slow path is never missed, and
//! neither is one between "found `Pending`" and "registered". The fast path
//! can only return an outcome that is really installed.
//!
//! # Completion and wakes
//!
//! [`Runtime::complete`](crate::Runtime::complete): unlocked validation and
//! publication check, then under the cell lock recheck, set the outcome and
//! take the waiter list; then, with **no lock held**, deliver each drained
//! handle to its task's state machine (one CAS, [`crate::task`]) and return
//! the results as [`Wakeups`]. Delivery only changes the task's state word
//! and reports what to enqueue; no JavaScript, host code or callback runs,
//! and the caller enqueues the runnable tasks after the call returns. A
//! wake that reaches a task still in its subscribing run is remembered by
//! the task, so a completion that races the consumer's park is not lost.
//!
//! # Cancellation
//!
//! Two different things, kept apart (`docs/design.md` §§8, 10):
//!
//! * **Cell cancellation** is a terminal *outcome* of a cell, `Cancelled`,
//!   installed exactly like `Ready` or `Failed`: once, under the cell lock,
//!   draining the waiters and then waking each of them exactly once with no
//!   lock held. Only two parties may install it: the cell's claimed
//!   producer ([`Runtime::complete`](crate::Runtime::complete) with
//!   [`Completion::Cancelled`]), and the embedder, which owns the scheduler
//!   and may cancel any pending cell without a producer id
//!   ([`Runtime::cancel_cell`](crate::Runtime::cancel_cell), for example
//!   when abandoning a key; the cell is created if absent). Tasks cannot
//!   cancel cells. Waiters observe it when they resume and need the key
//!   again: [`Need::Cancelled`], a value-free outcome; the consumer itself
//!   is not cancelled and decides what to do. It is terminal like every
//!   outcome: later needs read it, and a later completion, claim or
//!   cancellation of the cell is rejected (`AlreadyComplete`). A producer
//!   completion and an embedder cancellation that race are ordered by the
//!   recheck under the cell lock: exactly one installs.
//! * **Consumer cancellation** ([`Task::cancel`](crate::Task::cancel); the
//!   protocol is in [`crate::task`]) cancels one waiting task, never a
//!   cell. It does not change the outcome, the producer claim or the other
//!   waiters, so a producer still needed by other consumers stays alive
//!   (no consumer reference counting cancels a producer either: a cell
//!   whose consumers were all cancelled keeps its producer until the
//!   producer or the embedder settles it). The cancelled consumer's waiter
//!   entry is invalidated at once (its task's state word makes every
//!   delivery [`Wake::Stale`]) and removed by its terminal cleanup
//!   (`DependencyTable::unsubscribe`, one cell critical section). A need
//!   refused because its task is cancelled is [`NeedError::Cancelled`],
//!   not [`Need::Cancelled`].
//!
//! # Lock and ordering contract
//!
//! * **Dependency table lock** (`RwLock`): held only to find (read) or
//!   insert (write) a cell. Creating a cell registers it as a collector root
//!   source while holding the write lock, so the order is *table → heap
//!   `sources`*. Never held while taking a cell lock.
//! * **Cell lock** (the small per-cell pending-waiter lock): a leaf. Its
//!   critical sections read or write the producer and waiter bookkeeping
//!   (including a cancelled consumer's removal), set the outcome, and
//!   allocate producer IDs (an atomic counter); they acquire no other lock. The collector does not take it: it reads the
//!   outcome lock-free.
//! * **Task dependency log lock**: a leaf, taken only by the task's own run
//!   (exclusive through its execution claim) and by snapshot readers.
//! * No table, cell or log lock is held while rooting a value (heap `roots`),
//!   during the publication check (heap `objects`), while delivering wakes,
//!   or when control returns to the caller. No host code or JavaScript runs
//!   under any of them.
//! * **Orderings.** The outcome is published by `OnceLock::set` (a release
//!   store of its completion state) and read by `OnceLock::get` (an acquire
//!   load), so a fast-path reader that sees it also sees the value it
//!   names. The waiter list and producer are ordered by the cell mutex. A
//!   wake is a CAS (`AcqRel`) on the task's state word made after the
//!   outcome is set, and the next claim of the woken task loads that word
//!   with `Acquire`, so the resumed run's need sees the outcome on the fast
//!   path (the slow path's recheck would see it regardless). Producer IDs
//!   use a `Relaxed` counter (uniqueness only).
//!
//! # Collector boundary
//!
//! Each cell is a `RootSource`: while the cell exists, a `Ready` value or
//! `Failed` error it holds is a root (`RootCategory::DependencyOutcome`), so
//! a published result survives collection after the producer drops its own
//! handle. Values are checked with `share::check` before installation:
//! task-owned mutation (builders, plain objects) is rejected with
//! [`CompleteError::NotShareable`] and the cell stays pending. A terminal
//! outcome is immutable and the cell outlives every reader, so a value
//! copied out of the cell stays reachable until the reader roots it.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock, Weak};

use crate::heap::gc::{RootCategory, RootSource, Trace};
use crate::heap::{HandleError, Heap, ObjectId, Rooted};
use crate::ids::{DependencyKey, ProducerId, TaskId, WakeHandle};
use crate::task::{TaskInner, Wake, deliver_wake};
use crate::value::Value;
use crate::value::share::{self, PublishError};

/// Lock a dependency mutex, recovering from poisoning: every critical
/// section in this module is plain bookkeeping that cannot panic halfway.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A cell's terminal outcome, as raw values.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Terminal {
    Ready(Value),
    Failed(Value),
    Cancelled,
}

impl Terminal {
    fn value(self) -> Option<Value> {
        match self {
            Terminal::Ready(v) | Terminal::Failed(v) => Some(v),
            Terminal::Cancelled => None,
        }
    }
}

/// A registered consumer: its wake handle and a non-owning reference to the
/// task, used only to deliver the wake after every cell lock is released.
#[derive(Clone, Debug)]
pub(crate) struct Waiter {
    pub(crate) handle: WakeHandle,
    pub(crate) task: Weak<TaskInner>,
}

/// Pending bookkeeping, behind the cell's lock. Emptied by the completion
/// that installs the outcome and never filled again.
#[derive(Debug, Default)]
struct PendingState {
    producer: Option<ProducerId>,
    /// In subscription order, one entry per task.
    waiters: Vec<Waiter>,
    /// Index into `waiters` per task: registration is idempotent per task,
    /// and a later execution's re-subscription replaces the stale handle.
    subscribed: HashMap<TaskId, usize>,
}

#[derive(Debug, Default)]
struct Cell {
    /// The terminal outcome: written once, by the completion, while holding
    /// `pending`; read without any lock by the fast path.
    outcome: OnceLock<Terminal>,
    pending: Mutex<PendingState>,
}

/// A cell roots its terminal value or error while it exists. Reads the
/// outcome without taking the cell lock.
impl RootSource for Cell {
    fn trace_roots(&self, visit: &mut dyn FnMut(RootCategory, ObjectId)) {
        if let Some(value) = self.outcome.get().and_then(|t| t.value()) {
            value.trace(&mut |id| visit(RootCategory::DependencyOutcome, id));
        }
    }
}

/// What step 2 of a need found (module docs).
#[derive(Debug)]
pub(crate) enum Read {
    Terminal(Terminal),
    /// The consumer is registered as a waiter; `produce` is the producer
    /// request this need claimed, if it was the first.
    Waiting {
        produce: Option<ProducerRequest>,
    },
}

/// Points in the protocol where a test can run code with no lock held, to
/// force an interleaving deterministically (see [`DependencyTable::hook`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HookPoint {
    /// A need found no outcome on the fast path and is about to take the
    /// cell lock to recheck and subscribe.
    NeedAfterPendingRead,
    /// A completion passed its unlocked publication check and is about to
    /// take the cell lock to install.
    CompleteBeforeInstall,
    /// A completion installed its outcome and drained the waiters, released
    /// the cell lock, and is about to deliver the wakes.
    CompleteAfterDrain,
    /// A park read `Executing` without a notification and is about to CAS
    /// to `Parked`.
    ParkBeforeCas,
    /// A wake read a state it can change and is about to CAS.
    WakeBeforeCas,
    /// A consumer cancellation read a state it can change and is about to
    /// CAS.
    CancelBeforeCas,
}

/// Test hooks: code run at a [`HookPoint`], once or every time.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Hooks(Mutex<HashMap<HookPoint, (Hook, bool)>>);

#[cfg(test)]
type Hook = Arc<dyn Fn() + Send + Sync>;

#[cfg(test)]
impl Hooks {
    /// Run `f` every time `point` is reached.
    pub(crate) fn always(&self, point: HookPoint, f: impl Fn() + Send + Sync + 'static) {
        lock(&self.0).insert(point, (Arc::new(f), false));
    }

    /// Run `f` the next time `point` is reached, then forget it.
    pub(crate) fn once(&self, point: HookPoint, f: impl Fn() + Send + Sync + 'static) {
        lock(&self.0).insert(point, (Arc::new(f), true));
    }

    fn fire(&self, point: HookPoint) {
        // Taken out first, so the hook runs with no lock held and may itself
        // reach other hook points.
        let hook = {
            let mut hooks = lock(&self.0);
            match hooks.get(&point) {
                Some((_, true)) => hooks.remove(&point).map(|(f, _)| f),
                Some((f, false)) => Some(Arc::clone(f)),
                None => None,
            }
        };
        if let Some(hook) = hook {
            hook();
        }
    }
}

/// The dependency cells of one runtime. See the module docs.
pub(crate) struct DependencyTable {
    heap: Arc<Heap>,
    /// Process-unique identity of the owning runtime, carried by every wake
    /// handle its tasks register.
    pub(crate) runtime: u64,
    cells: RwLock<HashMap<DependencyKey, Arc<Cell>>>,
    next_producer: AtomicU64,
    #[cfg(test)]
    pub(crate) hooks: Hooks,
}

impl fmt::Debug for DependencyTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DependencyTable")
            .field("cells", &self.read_cells().len())
            .finish_non_exhaustive()
    }
}

impl DependencyTable {
    pub(crate) fn new(heap: Arc<Heap>) -> Self {
        // Relaxed: the counter only needs uniqueness, not ordering.
        static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
        DependencyTable {
            heap,
            runtime: NEXT_RUNTIME.fetch_add(1, Ordering::Relaxed),
            cells: RwLock::new(HashMap::new()),
            next_producer: AtomicU64::new(1),
            #[cfg(test)]
            hooks: Hooks::default(),
        }
    }

    /// Run the test hook for `point`, if any. Called with no lock held; a
    /// no-op outside tests.
    #[inline]
    pub(crate) fn hook(&self, point: HookPoint) {
        #[cfg(test)]
        self.hooks.fire(point);
        #[cfg(not(test))]
        let _ = point;
    }

    fn read_cells(&self) -> std::sync::RwLockReadGuard<'_, HashMap<DependencyKey, Arc<Cell>>> {
        self.cells.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cell for `key`, created (and registered as a root source) if
    /// absent. Lock order: table → heap `sources`.
    fn cell(&self, key: DependencyKey) -> Arc<Cell> {
        if let Some(cell) = self.existing(key) {
            return cell;
        }
        let mut cells = self.cells.write().unwrap_or_else(PoisonError::into_inner);
        match cells.entry(key) {
            Entry::Occupied(e) => Arc::clone(e.get()),
            Entry::Vacant(e) => {
                let cell = Arc::new(Cell::default());
                // Registered before the cell is visible to anyone, so no
                // outcome can be installed in an untraced cell.
                let source: Weak<dyn RootSource> = Arc::downgrade(&cell) as Weak<Cell>;
                self.heap.register_source(source);
                Arc::clone(e.insert(cell))
            }
        }
    }

    fn existing(&self, key: DependencyKey) -> Option<Arc<Cell>> {
        self.read_cells().get(&key).cloned()
    }

    /// Claim the producer of a pending cell whose producer is unclaimed.
    /// Called with the cell lock held; allocates from an atomic counter only.
    fn claim_locked(
        &self,
        key: DependencyKey,
        producer: &mut Option<ProducerId>,
    ) -> ProducerRequest {
        // Relaxed: the counter only needs uniqueness, not ordering.
        let id = ProducerId::from_raw(self.next_producer.fetch_add(1, Ordering::Relaxed));
        *producer = Some(id);
        ProducerRequest { key, producer: id }
    }

    /// Step 2 of a need (module docs): the atomic fast path, then one cell
    /// critical section that rechecks the outcome and either reads it or
    /// registers `waiter` and claims the producer if nobody has.
    pub(crate) fn read_or_subscribe(&self, waiter: Waiter, key: DependencyKey) -> Read {
        let cell = self.cell(key);
        // Fast path. Acquire (inside `OnceLock::get`) pairs with the
        // completion's Release, so the outcome's values are visible.
        if let Some(terminal) = cell.outcome.get() {
            return Read::Terminal(*terminal);
        }
        self.hook(HookPoint::NeedAfterPendingRead);
        let mut state = lock(&cell.pending);
        // Recheck: the outcome is only ever set while this lock is held, so
        // under it "no outcome" is stable until the waiter is registered.
        if let Some(terminal) = cell.outcome.get() {
            return Read::Terminal(*terminal);
        }
        let PendingState {
            producer,
            waiters,
            subscribed,
        } = &mut *state;
        match subscribed.entry(waiter.handle.task()) {
            // The same task again: keep its place, refresh its handle (a
            // later execution must not be left with a stale one).
            Entry::Occupied(e) => waiters[*e.get()] = waiter,
            Entry::Vacant(e) => {
                e.insert(waiters.len());
                waiters.push(waiter);
            }
        }
        let produce = producer.is_none().then(|| self.claim_locked(key, producer));
        Read::Waiting { produce }
    }

    /// Claim the producer of `key` without subscribing.
    pub(crate) fn claim(&self, key: DependencyKey) -> Result<ProducerRequest, ClaimError> {
        let cell = self.cell(key);
        let mut state = lock(&cell.pending);
        if cell.outcome.get().is_some() {
            return Err(ClaimError::AlreadyComplete { key });
        }
        match state.producer {
            Some(claimed) => Err(ClaimError::AlreadyClaimed { key, claimed }),
            None => Ok(self.claim_locked(key, &mut state.producer)),
        }
    }

    /// Remove `task`'s waiter entry from `key`'s cell, if it has one: terminal
    /// cleanup of a cancelled consumer. The cell's producer, outcome and
    /// other waiters are untouched, so a producer still needed by other
    /// consumers keeps running and its completion still wakes them. Returns
    /// whether an entry was removed (`false` if the cell completed first and
    /// drained it, or the task never subscribed).
    pub(crate) fn unsubscribe(&self, key: DependencyKey, task: TaskId) -> bool {
        let Some(cell) = self.existing(key) else {
            return false;
        };
        let mut state = lock(&cell.pending);
        let Some(index) = state.subscribed.remove(&task) else {
            return false;
        };
        state.waiters.remove(index);
        for i in state.subscribed.values_mut() {
            if *i > index {
                *i -= 1;
            }
        }
        true
    }

    /// Install `outcome` as the cell's terminal outcome and return the
    /// drained waiters (subscription order). The caller keeps any value in
    /// `outcome` rooted for the duration of the call, and delivers the wakes
    /// after this returns (every lock released).
    pub(crate) fn complete(
        &self,
        key: DependencyKey,
        producer: ProducerId,
        outcome: Terminal,
    ) -> Result<Vec<Waiter>, CompleteError> {
        let Some(cell) = self.existing(key) else {
            return Err(CompleteError::NotProducer {
                key,
                producer,
                claimed: None,
            });
        };
        self.settle(&cell, key, Some(producer), outcome)
    }

    /// Cancel `key`'s cell on the embedder's authority, whoever its producer
    /// is (none included), creating the cell if it does not exist so later
    /// needs read `Cancelled`. Returns the drained waiters like
    /// [`DependencyTable::complete`].
    pub(crate) fn cancel_cell(&self, key: DependencyKey) -> Result<Vec<Waiter>, CompleteError> {
        let cell = self.cell(key);
        self.settle(&cell, key, None, Terminal::Cancelled)
    }

    /// The install shared by producer completion (`producer` is `Some`: it
    /// must be the claimed one) and embedder cancellation (`None`).
    fn settle(
        &self,
        cell: &Cell,
        key: DependencyKey,
        producer: Option<ProducerId>,
        outcome: Terminal,
    ) -> Result<Vec<Waiter>, CompleteError> {
        // Cheap rejections first, so a duplicate completion is reported as
        // such rather than as a publication failure.
        Self::may_complete(cell, &lock(&cell.pending), key, producer)?;
        // The publication check takes heap object locks: never under the
        // cell lock. Its result stays valid because `value` is rooted by the
        // caller and its shareability cannot change.
        if let Some(value) = outcome.value() {
            share::check(&self.heap, value).map_err(CompleteError::NotShareable)?;
        }
        self.hook(HookPoint::CompleteBeforeInstall);
        let mut state = lock(&cell.pending);
        // Recheck: a concurrent completion (by the same producer, or an
        // embedder cancellation) may have won.
        Self::may_complete(cell, &state, key, producer)?;
        // Publication point. Release (inside `OnceLock::set`) pairs with the
        // fast path's Acquire. Under the lock, so a need's recheck cannot
        // miss it and a need that registered before it is in `waiters`.
        if cell.outcome.set(outcome).is_err() {
            unreachable!("may_complete rejected a completed cell");
        }
        state.subscribed = HashMap::new();
        Ok(std::mem::take(&mut state.waiters))
    }

    /// `producer`: `None` for the embedder, who may settle any pending cell.
    fn may_complete(
        cell: &Cell,
        state: &PendingState,
        key: DependencyKey,
        producer: Option<ProducerId>,
    ) -> Result<(), CompleteError> {
        if cell.outcome.get().is_some() {
            return Err(CompleteError::AlreadyComplete { key });
        }
        if let Some(producer) = producer
            && state.producer != Some(producer)
        {
            return Err(CompleteError::NotProducer {
                key,
                producer,
                claimed: state.producer,
            });
        }
        Ok(())
    }

    /// The embedder-facing status of `key`'s cell. A terminal value is
    /// copied without a lock and rooted (see [`root_terminal`]).
    pub(crate) fn status(&self, key: DependencyKey) -> CellStatus {
        let Some(cell) = self.existing(key) else {
            return CellStatus::Absent;
        };
        let terminal = {
            let state = lock(&cell.pending);
            match cell.outcome.get() {
                Some(terminal) => *terminal,
                None => {
                    return CellStatus::Pending {
                        producer: state.producer,
                        waiters: state.waiters.iter().map(|w| w.handle.task()).collect(),
                    };
                }
            }
        };
        match root_terminal(&self.heap, terminal) {
            Need::Ready(v) => CellStatus::Ready(v),
            Need::Failed(e) => CellStatus::Failed(e),
            Need::Cancelled => CellStatus::Cancelled,
            Need::Waiting { .. } => unreachable!("root_terminal never waits"),
        }
    }

    /// The heap whose values the cells hold.
    pub(crate) fn heap(&self) -> &Arc<Heap> {
        &self.heap
    }
}

/// Root a terminal outcome for the embedder. Safe without further
/// precautions: the value is held by a cell that outlives this call and
/// whose outcome never changes, so it is traced until the new root exists.
fn root_terminal(heap: &Arc<Heap>, terminal: Terminal) -> Need {
    match terminal {
        Terminal::Ready(v) => Need::Ready(Rooted::new(heap, v)),
        Terminal::Failed(e) => Need::Failed(Rooted::new(heap, e)),
        Terminal::Cancelled => Need::Cancelled,
    }
}

/// Steps 2 and 3 of a need, for the consumer `waiter` with log `log` (module
/// docs). The caller holds the consumer's run guard, which makes it the only
/// writer of `log`. A terminal value is returned raw: it stays reachable
/// through its cell (see [`root_terminal`]).
pub(crate) fn read_and_record(
    table: &DependencyTable,
    log: &Mutex<DependencyLog>,
    waiter: Waiter,
    key: DependencyKey,
) -> Result<Read, NeedError> {
    if let Some(waiting) = lock(log).waiting
        && waiting != key
    {
        return Err(NeedError::AlreadyWaiting {
            waiting,
            requested: key,
        });
    }
    let read = table.read_or_subscribe(waiter, key);
    let mut log = lock(log);
    log.record(key);
    log.waiting = match read {
        Read::Terminal(_) => None,
        Read::Waiting { .. } => Some(key),
    };
    Ok(read)
}

/// Terminal cleanup of a cancelled consumer's dependency state, by the owner
/// of its execution: resolve its unresolved wait (a cancelled task waits for
/// nothing) and remove its waiter entry from that cell. Recorded edges stay.
/// The log lock is released before the cell lock is taken.
pub(crate) fn withdraw(table: &DependencyTable, log: &Mutex<DependencyLog>, task: TaskId) {
    let waiting = lock(log).waiting.take();
    if let Some(key) = waiting {
        table.unsubscribe(key, task);
    }
}

/// A need of `key` with the terminal value rooted for the embedder.
pub(crate) fn rooted(heap: &Arc<Heap>, key: DependencyKey, read: Read) -> Need {
    match read {
        Read::Terminal(terminal) => root_terminal(heap, terminal),
        Read::Waiting { produce } => Need::Waiting { key, produce },
    }
}

/// Complete with embedder handles: checks they belong to the table's heap,
/// installs the outcome, then (every lock released) delivers the drained
/// wakes.
pub(crate) fn complete(
    table: &DependencyTable,
    key: DependencyKey,
    producer: ProducerId,
    completion: Completion<'_>,
) -> Result<Wakeups, CompleteError> {
    let heap = table.heap();
    let outcome = match completion {
        Completion::Ready(v) => Terminal::Ready(v.value_in(heap).map_err(CompleteError::Handle)?),
        Completion::Failed(e) => Terminal::Failed(e.value_in(heap).map_err(CompleteError::Handle)?),
        Completion::Cancelled => Terminal::Cancelled,
    };
    let drained = table.complete(key, producer, outcome)?;
    Ok(deliver(table, drained))
}

/// Cancel a cell on the embedder's authority (module docs, "Cancellation"),
/// then (every lock released) deliver the drained wakes.
pub(crate) fn cancel_cell(
    table: &DependencyTable,
    key: DependencyKey,
) -> Result<Wakeups, CompleteError> {
    let drained = table.cancel_cell(key)?;
    Ok(deliver(table, drained))
}

/// Deliver drained wakes, with no lock held.
fn deliver(table: &DependencyTable, drained: Vec<Waiter>) -> Wakeups {
    table.hook(HookPoint::CompleteAfterDrain);
    Wakeups(
        drained
            .into_iter()
            .map(|w| (w.handle, deliver_wake(&w.task, w.handle)))
            .collect(),
    )
}

/// The wakes one completion delivered, in subscription order: every
/// consumer that was registered on the cell, with what its wake did.
///
/// The completion has already applied each wake to its task's state; the
/// caller's only job is to enqueue the [`Wake::Runnable`] tasks
/// ([`Wakeups::into_runnable`]), each exactly once. A
/// [`Wake::Remembered`] task is still in the run that subscribed; that run
/// ends `Yielded` instead of parking, and its runner requeues it.
#[derive(Debug)]
#[must_use = "runnable tasks in the wakeups must be enqueued"]
pub struct Wakeups(Vec<(WakeHandle, Wake)>);

impl Wakeups {
    /// Every drained wake handle with its delivery, in subscription order.
    pub fn deliveries(&self) -> &[(WakeHandle, Wake)] {
        &self.0
    }

    /// The tasks of the drained wake handles, in subscription order.
    pub fn task_ids(&self) -> Vec<TaskId> {
        self.0.iter().map(|(h, _)| h.task()).collect()
    }

    /// The tasks this completion made runnable, to enqueue.
    pub fn into_runnable(self) -> Vec<crate::Task> {
        self.0
            .into_iter()
            .filter_map(|(_, wake)| match wake {
                Wake::Runnable(task) => Some(task),
                Wake::Remembered | Wake::Stale => None,
            })
            .collect()
    }
}

/// Scheduler work returned by a need or a claim: "start a producer for this
/// key". Exactly one is ever issued per cell. It is plain data; the runtime
/// calls no host code to issue it. The holder completes the cell with
/// [`Runtime::complete`](crate::Runtime::complete), passing
/// [`ProducerRequest::key`] and [`ProducerRequest::producer`].
#[derive(Debug, PartialEq, Eq)]
#[must_use = "dropping a producer request leaves its cell without a producer"]
pub struct ProducerRequest {
    key: DependencyKey,
    producer: ProducerId,
}

impl ProducerRequest {
    /// The key to produce.
    pub fn key(&self) -> DependencyKey {
        self.key
    }

    /// The claim's identity, required to complete the cell.
    pub fn producer(&self) -> ProducerId {
        self.producer
    }
}

/// Result of a need ([`RunGuard::need`](crate::RunGuard::need)).
#[derive(Debug)]
#[must_use]
pub enum Need {
    /// The result is ready: the shared value itself (same identity for every
    /// reader), rooted for the caller.
    Ready(Rooted),
    /// The producer failed: the published error value.
    Failed(Rooted),
    /// The cell's terminal outcome is `Cancelled` (by its producer or the
    /// embedder; module docs, "Cancellation"). The needing task itself is
    /// not cancelled and may continue; compare [`NeedError::Cancelled`].
    Cancelled,
    /// Not complete. The task is registered as a waiter on `key` and will be
    /// among the wake handles its completion returns. `produce` is `Some`
    /// for exactly one need (or claim) per cell: the producer request this
    /// call claimed, which the caller must hand to its scheduler.
    Waiting {
        key: DependencyKey,
        produce: Option<ProducerRequest>,
    },
}

/// Why a need was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedError {
    /// The task already has an unresolved wait on another key: a suspended
    /// task resumes by needing that key again, not by demanding another.
    AlreadyWaiting {
        waiting: DependencyKey,
        requested: DependencyKey,
    },
    /// The consumer task is cancelled ([`Task::cancel`](crate::Task::cancel)):
    /// it subscribes to nothing and claims no producer. This is consumer
    /// cancellation, not the cell outcome [`Need::Cancelled`]. The run
    /// should end; [`RunGuard::park`](crate::RunGuard::park) or
    /// [`RunGuard::run_with`](crate::RunGuard::run_with) then performs
    /// terminal cleanup.
    Cancelled { requested: DependencyKey },
}

impl fmt::Display for NeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NeedError::AlreadyWaiting { waiting, requested } => write!(
                f,
                "cannot need {requested}: the task is still waiting on {waiting}"
            ),
            NeedError::Cancelled { requested } => {
                write!(f, "cannot need {requested}: the task is cancelled")
            }
        }
    }
}

impl std::error::Error for NeedError {}

/// A terminal outcome to install with
/// [`Runtime::complete`](crate::Runtime::complete).
#[derive(Clone, Copy, Debug)]
pub enum Completion<'a> {
    /// The result value. Must pass the publication check.
    Ready(&'a Rooted),
    /// The producer failed with this error value. Must pass the publication
    /// check; it wakes waiters exactly like success.
    Failed(&'a Rooted),
    /// The cell is cancelled by its producer. (The embedder may also cancel
    /// any pending cell without a producer id:
    /// [`Runtime::cancel_cell`](crate::Runtime::cancel_cell).)
    Cancelled,
}

/// Why a completion was rejected. A rejected completion changes nothing:
/// the cell stays pending and keeps its waiters.
#[derive(Clone, Debug, PartialEq)]
pub enum CompleteError {
    /// A handle belongs to another runtime.
    Handle(HandleError),
    /// The value reaches task-owned mutation; see
    /// [`Runtime::publish`](crate::Runtime::publish).
    NotShareable(PublishError),
    /// `producer` is not the claimed producer of `key`'s cell (`claimed`,
    /// `None` if the cell has no producer or does not exist). This rejects
    /// substituting a result produced for another key, including the same
    /// definition and kind in another world.
    NotProducer {
        key: DependencyKey,
        producer: ProducerId,
        claimed: Option<ProducerId>,
    },
    /// The cell already has a terminal outcome; it is never replaced.
    AlreadyComplete { key: DependencyKey },
}

impl fmt::Display for CompleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompleteError::Handle(e) => write!(f, "cannot complete: {e}"),
            CompleteError::NotShareable(e) => write!(f, "cannot complete: {e}"),
            CompleteError::NotProducer {
                key,
                producer,
                claimed,
            } => {
                write!(f, "{producer} is not the producer of {key}")?;
                match claimed {
                    Some(claimed) => write!(f, " (claimed by {claimed})"),
                    None => f.write_str(" (no producer claimed)"),
                }
            }
            CompleteError::AlreadyComplete { key } => write!(f, "{key} is already complete"),
        }
    }
}

impl std::error::Error for CompleteError {}

/// Why a producer claim was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimError {
    /// Another claim holds the cell.
    AlreadyClaimed {
        key: DependencyKey,
        claimed: ProducerId,
    },
    /// The cell is already complete.
    AlreadyComplete { key: DependencyKey },
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaimError::AlreadyClaimed { key, claimed } => {
                write!(f, "{key} is already claimed by {claimed}")
            }
            ClaimError::AlreadyComplete { key } => write!(f, "{key} is already complete"),
        }
    }
}

impl std::error::Error for ClaimError {}

/// Snapshot of one cell ([`Runtime::cell_status`](crate::Runtime::cell_status)).
#[derive(Debug)]
pub enum CellStatus {
    /// No need or claim has created the cell yet.
    Absent,
    /// Not complete: the claimed producer, if any, and the registered wake
    /// handles in subscription order.
    Pending {
        producer: Option<ProducerId>,
        waiters: Vec<TaskId>,
    },
    Ready(Rooted),
    Failed(Rooted),
    Cancelled,
}

/// A task's dependency log (`docs/design.md` §9). Two relationships, kept
/// apart:
///
/// * **recorded edges**: every key the task has needed, in first-need order,
///   whatever the outcome (ready reads included). "This computation used
///   that result."
/// * **unresolved wait**: the key the task is currently suspended on, if
///   any: its last need found the cell pending and it has not needed it
///   again since. Only these are candidates for deadlock reports; a
///   recorded edge alone never is.
#[derive(Clone, Debug, Default)]
pub struct DependencyLog {
    edges: Vec<DependencyKey>,
    recorded: HashSet<DependencyKey>,
    waiting: Option<DependencyKey>,
}

impl DependencyLog {
    fn record(&mut self, key: DependencyKey) {
        if self.recorded.insert(key) {
            self.edges.push(key);
        }
    }

    /// Recorded dependency edges, in first-need order, without duplicates.
    pub fn edges(&self) -> &[DependencyKey] {
        &self.edges
    }

    /// True if the task has needed `key`.
    pub fn depends_on(&self, key: DependencyKey) -> bool {
        self.recorded.contains(&key)
    }

    /// The task's unresolved wait, if any.
    pub fn waiting(&self) -> Option<DependencyKey> {
        self.waiting
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{DependencyId, ResultKind, WorldId};

    fn key(def: u64) -> DependencyKey {
        DependencyKey::new(WorldId::INITIAL, DependencyId::new(def), ResultKind::new(0))
    }

    /// A waiter for task `id`, execution 1, with no live task behind it.
    fn w(id: u64) -> Waiter {
        Waiter {
            handle: WakeHandle::new(0, TaskId::from_raw(id), 1),
            task: Weak::new(),
        }
    }

    fn ids(r: Result<Vec<Waiter>, CompleteError>) -> Result<Vec<TaskId>, CompleteError> {
        r.map(|ws| ws.iter().map(|w| w.handle.task()).collect())
    }

    #[test]
    fn completed_cells_report_their_values_as_dependency_outcome_roots() {
        let heap = Arc::new(Heap::default());
        let table = DependencyTable::new(Arc::clone(&heap));
        let ready = heap.alloc(crate::heap::HeapObject::Plain).unwrap();
        let failed = heap.alloc(crate::heap::HeapObject::Plain).unwrap();
        // Plain objects are not shareable; install directly to test tracing.
        for (def, outcome) in [
            (1, Terminal::Ready(Value::Object(ready))),
            (2, Terminal::Failed(Value::Object(failed))),
            (3, Terminal::Cancelled),
            (4, Terminal::Ready(Value::Number(1.0))),
        ] {
            let cell = table.cell(key(def));
            cell.outcome.set(outcome).unwrap();
        }
        let pending = table.read_or_subscribe(w(9), key(5));
        assert!(matches!(pending, Read::Waiting { produce: Some(_) }));
        let mut roots = heap.root_snapshot().unwrap();
        roots.sort_by_key(|(_, id)| id.to_bits());
        assert_eq!(
            roots,
            vec![
                (RootCategory::DependencyOutcome, ready),
                (RootCategory::DependencyOutcome, failed)
            ]
        );
        assert_eq!(heap.collect().unwrap().reclaimed, 0);
        drop(table);
        assert_eq!(
            heap.collect().unwrap().reclaimed,
            2,
            "dropped cells root nothing"
        );
    }

    #[test]
    fn a_rejected_completion_leaves_the_cell_pending_with_its_waiters() {
        let heap = Arc::new(Heap::default());
        let table = DependencyTable::new(Arc::clone(&heap));
        let Read::Waiting { produce: Some(req) } = table.read_or_subscribe(w(1), key(1)) else {
            panic!("first need claims the producer");
        };
        let plain = heap.alloc(crate::heap::HeapObject::Plain).unwrap();
        let _root = Rooted::new(&heap, Value::Object(plain));
        let err = table
            .complete(
                key(1),
                req.producer(),
                Terminal::Ready(Value::Object(plain)),
            )
            .unwrap_err();
        assert!(matches!(err, CompleteError::NotShareable(_)), "{err:?}");
        assert!(matches!(
            table.status(key(1)),
            CellStatus::Pending { waiters, .. } if waiters == [TaskId::from_raw(1)]
        ));
        assert_eq!(
            ids(table.complete(key(1), req.producer(), Terminal::Cancelled)),
            Ok(vec![TaskId::from_raw(1)])
        );
    }

    /// Removing a cancelled consumer's waiter keeps the per-task index
    /// consistent: a task behind it that re-subscribes in a later run
    /// refreshes its own entry, and completion drains the rest in
    /// subscription order. Removal is idempotent and leaves the producer.
    #[test]
    fn unsubscribe_keeps_the_remaining_waiters_in_order() {
        let heap = Arc::new(Heap::default());
        let table = DependencyTable::new(Arc::clone(&heap));
        let Read::Waiting { produce: Some(req) } = table.read_or_subscribe(w(1), key(1)) else {
            panic!("first need claims the producer");
        };
        for id in [2, 3] {
            assert!(matches!(
                table.read_or_subscribe(w(id), key(1)),
                Read::Waiting { produce: None }
            ));
        }
        let t = TaskId::from_raw;
        assert!(table.unsubscribe(key(1), t(2)));
        assert!(!table.unsubscribe(key(1), t(2)), "idempotent");
        assert!(!table.unsubscribe(key(9), t(1)), "absent cell");
        let later = Waiter {
            handle: WakeHandle::new(0, t(3), 2),
            task: Weak::new(),
        };
        assert!(matches!(
            table.read_or_subscribe(later, key(1)),
            Read::Waiting { produce: None }
        ));
        assert!(matches!(
            table.status(key(1)),
            CellStatus::Pending { producer: Some(p), waiters } if p == req.producer() && waiters == [t(1), t(3)]
        ));
        let drained = table
            .complete(key(1), req.producer(), Terminal::Cancelled)
            .unwrap();
        let got: Vec<_> = drained
            .iter()
            .map(|w| (w.handle.task(), w.handle.execution()))
            .collect();
        assert_eq!(got, [(t(1), 1), (t(3), 2)]);
        assert!(!table.unsubscribe(key(1), t(1)), "already drained");
    }

    /// Two completions by the same producer both pass the first validation
    /// and the unlocked publication check before either installs (forced by
    /// a barrier in the hook). The recheck under the cell lock lets exactly
    /// one install and wake; the other is `AlreadyComplete` and the winning
    /// outcome stays.
    #[test]
    fn concurrent_completions_publish_once_and_wake_once() {
        let heap = Arc::new(Heap::default());
        let table = DependencyTable::new(Arc::clone(&heap));
        let waiter = TaskId::from_raw(1);
        let Read::Waiting { produce: Some(req) } = table.read_or_subscribe(w(1), key(1)) else {
            panic!("first need claims the producer");
        };
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let hook_barrier = Arc::clone(&barrier);
        table
            .hooks
            .always(HookPoint::CompleteBeforeInstall, move || {
                hook_barrier.wait();
            });
        let results: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = [1.0, 2.0]
                .map(|n| {
                    let table = &table;
                    let producer = req.producer();
                    s.spawn(move || {
                        let r = ids(table.complete(
                            key(1),
                            producer,
                            Terminal::Ready(Value::Number(n)),
                        ));
                        (n, r)
                    })
                })
                .into_iter()
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let winners: Vec<f64> = results
            .iter()
            .filter(|(_, r)| r.as_ref().is_ok_and(|w| w == &[waiter]))
            .map(|&(n, _)| n)
            .collect();
        let losers = results
            .iter()
            .filter(|(_, r)| *r == Err(CompleteError::AlreadyComplete { key: key(1) }))
            .count();
        assert_eq!((winners.len(), losers), (1, 1), "{results:?}");
        let Read::Terminal(Terminal::Ready(Value::Number(n))) =
            table.read_or_subscribe(w(2), key(1))
        else {
            panic!("ready");
        };
        assert_eq!(n, winners[0], "the winning outcome is never replaced");
    }
}
