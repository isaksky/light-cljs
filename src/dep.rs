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
//! * A **consumer** is a task; its [`TaskId`] is its wake handle. Waiters are
//!   task IDs, never JavaScript callbacks. Completion returns the drained
//!   wake handles to the caller, which enqueues them after the call returns:
//!   nothing runs inline inside completion.
//!
//! # Read-or-subscribe
//!
//! A need ([`RunGuard::need`](crate::RunGuard::need)) for key `k` by task `t`:
//!
//! 1. finds or creates the cell for `k` (dependency-table lock, released);
//! 2. under the cell's lock, in **one** critical section: if the cell is
//!    terminal, copies the outcome; otherwise adds `t` to the waiters (once)
//!    and, if no producer has been claimed, claims one;
//! 3. records `k` in `t`'s [`DependencyLog`]: always as an edge (ready reads
//!    included), and as `t`'s unresolved wait while the cell is pending;
//! 4. after every lock is released, roots a terminal value for the caller.
//!
//! Step 2 is the linearization point. Completion takes the same cell lock to
//! install the outcome and drain the waiters, so a completion is ordered
//! either before the need (which then sees the terminal outcome) or after it
//! (which then finds `t` among the drained waiters). There is no window
//! between checking `Pending` and registering the waiter in which a
//! completion can be missed. Remembering a wake that arrives before the
//! consumer has finished parking is the scheduler's part (pellet
//! clj-quickjs-7); the cell guarantees the wake handle is delivered.
//!
//! # Lock and ordering contract
//!
//! * **Dependency table lock**: held only to find or insert a cell. Creating
//!   a cell registers it as a collector root source while holding it, so the
//!   order is *table → heap `sources`*. Never held while taking a cell lock.
//! * **Cell lock**: a leaf. Its critical sections read or write the cell's
//!   bookkeeping and allocate producer IDs (an atomic counter), and acquire
//!   no other lock. The collector takes it while tracing root sources (under
//!   the heap's `gc → roots → scopes` locks), which cannot deadlock because a
//!   cell holder waits for nothing.
//! * **Task dependency log lock**: a leaf, taken only by the task's own run
//!   (exclusive through its run guard) and by snapshot readers.
//! * No table, cell or log lock is held while rooting a value (heap `roots`),
//!   during the publication check (heap `objects`), or when control returns
//!   to the caller. No host code or JavaScript runs under any of them.
//! * Memory ordering comes from the mutexes: the outcome and the drained
//!   waiter list are published by the cell lock's release and observed by
//!   its acquire. Producer IDs use a `Relaxed` counter (uniqueness only).
//!
//! An atomic completed-result fast path that skips the cell lock is a later
//! optimization (pellet clj-quickjs-7); every read takes the cell lock now.
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
//! copied out under the cell lock stays reachable until the reader roots it.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use crate::heap::gc::{RootCategory, RootSource, Trace};
use crate::heap::{HandleError, Heap, ObjectId, Rooted};
use crate::ids::{DependencyKey, ProducerId, TaskId};
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

#[derive(Debug)]
enum Phase {
    /// Not complete. `waiters` in subscription order; `subscribed` makes
    /// registration idempotent per task.
    Pending {
        waiters: Vec<TaskId>,
        subscribed: HashSet<TaskId>,
    },
    /// Complete; never changes again.
    Done(Terminal),
}

#[derive(Debug)]
struct CellState {
    producer: Option<ProducerId>,
    phase: Phase,
}

#[derive(Debug)]
struct Cell {
    state: Mutex<CellState>,
}

/// A cell roots its terminal value or error while it exists.
impl RootSource for Cell {
    fn trace_roots(&self, visit: &mut dyn FnMut(RootCategory, ObjectId)) {
        if let Phase::Done(terminal) = lock(&self.state).phase
            && let Some(value) = terminal.value()
        {
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

/// The dependency cells of one runtime. See the module docs.
pub(crate) struct DependencyTable {
    heap: Arc<Heap>,
    cells: Mutex<HashMap<DependencyKey, Arc<Cell>>>,
    next_producer: AtomicU64,
}

impl fmt::Debug for DependencyTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DependencyTable")
            .field("cells", &lock(&self.cells).len())
            .finish_non_exhaustive()
    }
}

impl DependencyTable {
    pub(crate) fn new(heap: Arc<Heap>) -> Self {
        DependencyTable {
            heap,
            cells: Mutex::new(HashMap::new()),
            next_producer: AtomicU64::new(1),
        }
    }

    /// The cell for `key`, created (and registered as a root source) if
    /// absent. Lock order: table → heap `sources`.
    fn cell(&self, key: DependencyKey) -> Arc<Cell> {
        let mut cells = lock(&self.cells);
        match cells.entry(key) {
            Entry::Occupied(e) => Arc::clone(e.get()),
            Entry::Vacant(e) => {
                let cell = Arc::new(Cell {
                    state: Mutex::new(CellState {
                        producer: None,
                        phase: Phase::Pending {
                            waiters: Vec::new(),
                            subscribed: HashSet::new(),
                        },
                    }),
                });
                // Registered before the cell is visible to anyone, so no
                // outcome can be installed in an untraced cell.
                let source: Weak<dyn RootSource> = Arc::downgrade(&cell) as Weak<Cell>;
                self.heap.register_source(source);
                Arc::clone(e.insert(cell))
            }
        }
    }

    fn existing(&self, key: DependencyKey) -> Option<Arc<Cell>> {
        lock(&self.cells).get(&key).cloned()
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

    /// Step 2 of a need (module docs): one cell critical section that either
    /// reads the terminal outcome or registers `consumer` and claims the
    /// producer if nobody has.
    pub(crate) fn read_or_subscribe(&self, consumer: TaskId, key: DependencyKey) -> Read {
        let cell = self.cell(key);
        let mut state = lock(&cell.state);
        let CellState { producer, phase } = &mut *state;
        match phase {
            Phase::Done(terminal) => Read::Terminal(*terminal),
            Phase::Pending {
                waiters,
                subscribed,
            } => {
                if subscribed.insert(consumer) {
                    waiters.push(consumer);
                }
                let produce = producer.is_none().then(|| self.claim_locked(key, producer));
                Read::Waiting { produce }
            }
        }
    }

    /// Claim the producer of `key` without subscribing.
    pub(crate) fn claim(&self, key: DependencyKey) -> Result<ProducerRequest, ClaimError> {
        let cell = self.cell(key);
        let mut state = lock(&cell.state);
        let CellState { producer, phase } = &mut *state;
        match (phase, *producer) {
            (Phase::Done(_), _) => Err(ClaimError::AlreadyComplete { key }),
            (Phase::Pending { .. }, Some(claimed)) => {
                Err(ClaimError::AlreadyClaimed { key, claimed })
            }
            (Phase::Pending { .. }, None) => Ok(self.claim_locked(key, producer)),
        }
    }

    /// Install `outcome` as the cell's terminal outcome and return the
    /// drained wake handles (subscription order). The caller keeps any value
    /// in `outcome` rooted for the duration of the call.
    pub(crate) fn complete(
        &self,
        key: DependencyKey,
        producer: ProducerId,
        outcome: Terminal,
    ) -> Result<Vec<TaskId>, CompleteError> {
        let Some(cell) = self.existing(key) else {
            return Err(CompleteError::NotProducer {
                key,
                producer,
                claimed: None,
            });
        };
        // Cheap rejections first, so a duplicate completion is reported as
        // such rather than as a publication failure.
        Self::may_complete(&lock(&cell.state), key, producer)?;
        // The publication check takes heap object locks: never under the
        // cell lock. Its result stays valid because `value` is rooted by the
        // caller and its shareability cannot change.
        if let Some(value) = outcome.value() {
            share::check(&self.heap, value).map_err(CompleteError::NotShareable)?;
        }
        let mut state = lock(&cell.state);
        // Recheck: a concurrent completion by the same producer may have won.
        Self::may_complete(&state, key, producer)?;
        match std::mem::replace(&mut state.phase, Phase::Done(outcome)) {
            Phase::Pending { waiters, .. } => Ok(waiters),
            Phase::Done(_) => unreachable!("may_complete rejected a completed cell"),
        }
    }

    fn may_complete(
        state: &CellState,
        key: DependencyKey,
        producer: ProducerId,
    ) -> Result<(), CompleteError> {
        if let Phase::Done(_) = state.phase {
            return Err(CompleteError::AlreadyComplete { key });
        }
        if state.producer != Some(producer) {
            return Err(CompleteError::NotProducer {
                key,
                producer,
                claimed: state.producer,
            });
        }
        Ok(())
    }

    /// The embedder-facing status of `key`'s cell. A terminal value is
    /// copied under the cell lock and rooted after it is released (see
    /// [`root_terminal`]).
    pub(crate) fn status(&self, key: DependencyKey) -> CellStatus {
        let Some(cell) = self.existing(key) else {
            return CellStatus::Absent;
        };
        let terminal = {
            let state = lock(&cell.state);
            match &state.phase {
                Phase::Done(terminal) => *terminal,
                Phase::Pending { waiters, .. } => {
                    return CellStatus::Pending {
                        producer: state.producer,
                        waiters: waiters.clone(),
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

/// Steps 2 and 3 of a need, for task `consumer` with log `log` (module
/// docs). The caller holds `consumer`'s run guard, which makes it the only
/// writer of `log`. A terminal value is returned raw: it stays reachable
/// through its cell (see [`root_terminal`]).
pub(crate) fn read_and_record(
    table: &DependencyTable,
    log: &Mutex<DependencyLog>,
    consumer: TaskId,
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
    let read = table.read_or_subscribe(consumer, key);
    let mut log = lock(log);
    log.record(key);
    log.waiting = match read {
        Read::Terminal(_) => None,
        Read::Waiting { .. } => Some(key),
    };
    Ok(read)
}

/// A need with the terminal value rooted for the embedder.
pub(crate) fn need(
    table: &DependencyTable,
    log: &Mutex<DependencyLog>,
    consumer: TaskId,
    key: DependencyKey,
) -> Result<Need, NeedError> {
    Ok(match read_and_record(table, log, consumer, key)? {
        Read::Terminal(terminal) => root_terminal(table.heap(), terminal),
        Read::Waiting { produce } => Need::Waiting { key, produce },
    })
}

/// Complete with embedder handles: checks they belong to the table's heap.
pub(crate) fn complete(
    table: &DependencyTable,
    key: DependencyKey,
    producer: ProducerId,
    completion: Completion<'_>,
) -> Result<Vec<TaskId>, CompleteError> {
    let heap = table.heap();
    let outcome = match completion {
        Completion::Ready(v) => Terminal::Ready(v.value_in(heap).map_err(CompleteError::Handle)?),
        Completion::Failed(e) => Terminal::Failed(e.value_in(heap).map_err(CompleteError::Handle)?),
        Completion::Cancelled => Terminal::Cancelled,
    };
    table.complete(key, producer, outcome)
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
    /// The cell was cancelled.
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
}

impl fmt::Display for NeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NeedError::AlreadyWaiting { waiting, requested } => write!(
                f,
                "cannot need {requested}: the task is still waiting on {waiting}"
            ),
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
    /// The cell is cancelled.
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
            lock(&cell.state).phase = Phase::Done(outcome);
        }
        let pending = table.read_or_subscribe(TaskId::from_raw(9), key(5));
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
        let Read::Waiting { produce: Some(req) } =
            table.read_or_subscribe(TaskId::from_raw(1), key(1))
        else {
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
            table.complete(key(1), req.producer(), Terminal::Cancelled),
            Ok(vec![TaskId::from_raw(1)])
        );
    }
}
