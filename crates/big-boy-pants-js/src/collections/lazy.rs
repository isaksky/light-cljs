//! Lazy sequences as shareable once-realized cells (light-cljs-479).
//!
//! A ClojureScript `LazySeq` is a thunk run at most once, whose result is
//! the sequence's contents. Here it is a native form ([`FormKind::Lazy`],
//! `formKind` `"lazy-seq"`, made by `__rt.lazySeq(thunk)`) whose body is a
//! [`LazyCell`]: a dependency cell local to a value instead of keyed by a
//! world (`crate::dep`).
//!
//! # Realization
//!
//! * **One producer.** The first task that needs the contents *claims* the
//!   cell and runs the thunk as an ordinary resumable call in its own task:
//!   the clj intrinsic that met the unrealized cell starts a native frame
//!   (`crate::vm::native`, `NativeState::Realize`) that calls the thunk and,
//!   when it returns, installs its value and executes the intrinsic again
//!   with the same operands. Intrinsics are pure, so the restart observes
//!   nothing twice; the work they repeat is reading the realized prefix.
//! * **Racing readers park.** A task that needs a cell another task is
//!   realizing subscribes to it (a [`Waiter`]: the task's wake handle, as for
//!   a dependency) and suspends exactly as at a pending `need`: its run ends
//!   [`RunOutcome::Waiting`](crate::RunOutcome::Waiting) with the key
//!   [`lazy_key`] (result kind [`LAZY_REALIZATION`]), and no Rust frame
//!   survives. The realizer's settlement drains the waiters under the cell
//!   lock and wakes them with no lock held; the woken tasks are queued for
//!   the embedder ([`crate::Runtime::take_lazy_wakeups`]) and the cell's key
//!   reports [`CellStatus::Pending`](crate::CellStatus::Pending) until they
//!   have been woken ([`crate::Runtime::cell_status`]). A woken reader runs
//!   its intrinsic again and finds the value. The thunk never runs twice for
//!   one successful realization.
//! * **No JavaScript under a lock.** Claiming, subscribing and settling are
//!   short critical sections on the cell's leaf mutex; the thunk runs in the
//!   claiming task's frames, and wakes are delivered after the lock is
//!   released.
//! * **Realized values are read lock-free** (a `OnceLock`).
//!
//! # Exceptions
//!
//! ClojureScript's `LazySeq.sval` assigns its result only after the thunk
//! returns: a thunk that throws leaves the thunk installed, and a *later*
//! realization attempt runs it again. This cell keeps that rule. Every
//! reader that was parked on the failed attempt observes the same failure,
//! even if a later attempt has ended since (the cell keeps the failure until
//! each of them has read it): the thrown value itself when it is shareable
//! (a string, a number, a form), otherwise a new `Error` whose message
//! describes it (an `Error` object belongs to the throwing task). Readers
//! that arrive after the failed attempt claim the cell again and rerun the
//! thunk, as later attempts do in ClojureScript.
//!
//! A thunk that needs its own cell again while running it (`(def xs
//! (lazy-seq (first xs)))`) recurses until the stack overflows in
//! ClojureScript; here the reentrant claim fails at once with a catchable
//! `RangeError` (`lazy sequence realization reentered its own thunk`). A
//! realizing task that is cancelled, or whose frames are dropped, gives its
//! claim up: its waiters wake and the next one claims the cell.
//!
//! # Sharing
//!
//! A lazy form is persistent in the publication sense
//! (`crate::value::share`): publishable when its thunk is (before
//! realization) or its value is (after). A thunk that captured a task-owned
//! binding is not, and the publication error names the path through
//! `.thunk`. A successful publication check marks the cell shared, under
//! the cell's lock and only if the cell is still in the state the check read
//! (otherwise the check reads it again); a settlement decides under the same
//! lock, so a value installed without a check is never in a shared cell.
//! The value a shared cell's realization installs must be shareable: one
//! that is not fails that attempt, for the realizer and for every reader
//! parked on it, with a catchable `TypeError` naming the path, and the cell
//! stays unrealized. A cell that was never published takes any value.
//! Collection traces the thunk until realization, then the value, and the
//! failures not yet delivered.
//!
//! # Sequences
//!
//! [`FormKind::Cons`] (`formKind` `"cons"`) is the sequence cell `cons`
//! and `chunk-cons` build when the tail is a lazy sequence, and a vector's
//! seq: a nonempty run of realized elements (a chunk, or one element)
//! followed by a tail (`nil`, a list, or a lazy or cons sequence) that is not
//! realized. A `concat` that involves a lazy or cons sequence is a lazy cell
//! whose thunk is the list of its collections, realized in Rust one
//! collection at a time (`crate::collections::clj::concat_step`). A realized
//! value is stored as a sequence: a thunk's vector, map, set or string
//! result becomes its seq, so a realized chain holds only `nil`, lists, cons
//! cells and lazy cells. `crate::collections::form::realized_head` follows
//! realized cells transparently; meeting an unrealized cell is a
//! realization request. A realized sequence that contains itself (in its
//! tail or among its elements) is infinite: walking, hashing or deeply
//! realizing it is a catchable `RangeError`, and equality takes the pairs
//! it meets again as equal.
//!
//! A record result also retains its original receiver in the cell, reusing
//! the thunk slot after realization. Task-local language protocol dispatch
//! follows that receiver; structural operations still see the built-in
//! sequence. Collection traces both values and publication checks both.
//!
//! A clj operation over a sequence of `k` lazy cells realizes them one at a
//! time, executing again from its start after each. `count`, `cljList` and
//! `cljRealizeAll`, which read their sequence to its end, realize the rest of
//! a chain in the same realization frame (in chain order, the order they
//! would request it), so they stay linear in its length. `cljRealizeAll`
//! also continues its walk of a nested form where it stopped instead of
//! from its start (light-cljs-622, `crate::collections::clj::RealizeAll`),
//! so it is linear in the size of the whole form. Equality compares
//! sequences element by element and stops at the first unequal pair, so it
//! realizes no further; `hash` of an unrealized sequence of `n` elements
//! reads `O(k * n)` realized elements. Chunked sources (32 elements a cell)
//! keep `k` small.
//!
//! # Thread-local requests
//!
//! Equality and hashing (`crate::collections::form`) are total functions
//! that cannot return a request. When they meet an unrealized cell they
//! record it with [`request`], compare it by identity, and do not cache a
//! hash; the intrinsic that called them takes the request ([`take_request`])
//! and realizes the cell before executing again. Outside an intrinsic, an
//! embedder's structural operations (`Scope::new_form_map`, `form_get`,
//! `form_assoc` and the others that hash keys) fail with `WrongType` on an
//! unrealized lazy key, and `Scope::form_equals` compares one by identity.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use super::PVector;
use crate::dep::Waiter;
use crate::heap::gc::{Trace, TraceEdge};
use crate::ids::{DependencyId, DependencyKey, ResultKind, TaskId, WorldId};
use crate::task::{Task, TaskInner, Wake, deliver_wake};
use crate::value::{JsString, Value};

#[cfg(doc)]
use super::form::FormKind;

/// The result kind of a lazy realization wait: a task parked on a lazy
/// sequence another task is realizing ends its run
/// [`RunOutcome::Waiting`](crate::RunOutcome::Waiting) with a key of this
/// kind. No dependency cell has this kind.
pub const LAZY_REALIZATION: ResultKind = ResultKind::new(u32::MAX);

/// The wait key of the lazy cell with serial `serial`.
pub(crate) fn lazy_key(world: WorldId, serial: u64) -> DependencyKey {
    DependencyKey::new(world, DependencyId::new(serial), LAZY_REALIZATION)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a failed realization attempt left for the readers parked on it.
#[derive(Clone, Debug)]
pub(crate) enum LazyFailure {
    /// The thrown value, shareable: every reader throws it.
    Thrown(Value),
    /// A description of a task-owned thrown value: readers on other tasks
    /// throw a new `Error` with this message.
    Message(JsString),
    /// The value the thunk returned could not be stored (it is not
    /// shareable in a shared cell): readers throw a `TypeError`.
    Type(JsString),
}

/// A failed attempt kept until each reader parked on it has observed it.
#[derive(Debug)]
struct Failed {
    attempt: u64,
    failure: LazyFailure,
    /// The parked readers that have not yet observed it.
    readers: Vec<TaskId>,
}

#[derive(Debug)]
struct LazyState {
    /// The thunk until realization; afterward, the original record when
    /// its structural sequence was cached. Protocol dispatch needs the
    /// record itself, while internal comparison uses the cached sequence.
    thunk: Value,
    /// The realizing task and its attempt number.
    realizer: Option<(TaskId, u64)>,
    attempts: u64,
    /// Failed attempts whose parked readers have not all observed them.
    failures: Vec<Failed>,
    /// Readers parked on the current attempt.
    waiters: Vec<Waiter>,
    /// The runtime's registry, once a reader parked.
    waits: Option<Arc<LazyWaits>>,
    /// Set once a publication check proved the cell shareable (under this
    /// lock, against the state the check read): a value installed from
    /// then on must be shareable.
    shared: bool,
}

/// The once-realized cell of a lazy sequence. See the module docs.
#[derive(Debug)]
pub(crate) struct LazyCell {
    serial: u64,
    /// A lazy `concat`: the thunk is the list of its collections, realized
    /// in Rust (`crate::collections::clj::concat_step`), not called.
    concat: bool,
    /// A `delay` (light-cljs-613): the thunk's value is kept as it is, not
    /// made a sequence, and no sequence operation reads through the cell.
    delay: bool,
    /// A lock-free copy of `LazyState::shared` for the realizer's early
    /// check; `settle` decides under the lock.
    shared: AtomicBool,
    /// Set once an embedder attributed effects to a realization of this
    /// cell ([`Realizing::attribute`]): reads of its realized value are
    /// then noted for the reading task ([`note_consumed`]).
    attributed: AtomicBool,
    realized: OnceLock<Value>,
    state: Mutex<LazyState>,
}

/// What a claim found ([`LazyCell::claim`]).
#[derive(Debug)]
pub(crate) enum Claim {
    /// Realized.
    Realized,
    /// This task now realizes attempt `attempt` by calling `thunk`.
    Run { thunk: Value, attempt: u64 },
    /// Another task is realizing attempt `attempt`.
    Busy { attempt: u64 },
    /// The attempt this reader waited on failed.
    Failed(LazyFailure),
    /// This task is already realizing the cell.
    Reentered,
}

/// Why [`LazyCell::settle`] did not install a value.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unsettled {
    /// The cell became shared after the caller decided not to check the
    /// value: check it, then settle again with `checked`.
    NeedsCheck,
}

impl LazyCell {
    pub(crate) fn new(thunk: Value) -> LazyCell {
        LazyCell::of(thunk, false, false)
    }

    /// The cell of a lazy `concat` of the collections in the list `colls`.
    pub(crate) fn concat(colls: Value) -> LazyCell {
        LazyCell::of(colls, true, false)
    }

    /// The cell of a `delay` whose body is `thunk` (light-cljs-613).
    pub(crate) fn delay(thunk: Value) -> LazyCell {
        LazyCell::of(thunk, false, true)
    }

    fn of(thunk: Value, concat: bool, delay: bool) -> LazyCell {
        // Relaxed: uniqueness only.
        static NEXT: AtomicU64 = AtomicU64::new(1);
        LazyCell {
            serial: NEXT.fetch_add(1, Ordering::Relaxed),
            concat,
            delay,
            shared: AtomicBool::new(false),
            attributed: AtomicBool::new(false),
            realized: OnceLock::new(),
            state: Mutex::new(LazyState {
                thunk,
                realizer: None,
                attempts: 0,
                failures: Vec::new(),
                waiters: Vec::new(),
                waits: None,
                shared: false,
            }),
        }
    }

    /// Whether this is a lazy `concat`, realized in Rust.
    pub(crate) fn is_concat(&self) -> bool {
        self.concat
    }

    /// Whether this is a `delay`'s cell, whose value is not a sequence.
    pub(crate) fn is_delay(&self) -> bool {
        self.delay
    }

    /// The process-unique serial naming this cell's wait key.
    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }

    /// Record that a publication check proved the cell shareable, unless
    /// its realization state changed since the check read it
    /// (`saw_realized`): then the check must read it again. Ordered with
    /// [`LazyCell::settle`] by the cell lock, so a value installed without a
    /// check is never in a cell marked shared.
    pub(crate) fn mark_shared(&self, saw_realized: bool) -> bool {
        let mut state = lock(&self.state);
        if self.realized.get().is_some() != saw_realized {
            return false;
        }
        state.shared = true;
        // Release/Acquire: a realizer that sees the flag sees the
        // publication that set it.
        self.shared.store(true, Ordering::Release);
        true
    }

    /// Whether the cell may be read by other tasks: its realized value must
    /// be shareable. A hint; [`LazyCell::settle`] decides under its lock.
    pub(crate) fn is_shared(&self) -> bool {
        self.shared.load(Ordering::Acquire)
    }

    /// Whether an embedder attributed effects to a realization of this
    /// cell. Relaxed suffices: the realizing task sets the flag before it
    /// settles the cell, and a reader checks it after the realized value's
    /// acquire.
    pub(crate) fn is_attributed(&self) -> bool {
        self.attributed.load(Ordering::Relaxed)
    }

    /// The realized value, read without a lock.
    pub(crate) fn realized(&self) -> Option<&Value> {
        self.realized.get()
    }

    /// The realized receiver for task-local language protocol dispatch.
    pub(crate) fn protocol_value(&self) -> Option<Value> {
        let value = self.realized.get()?;
        let state = lock(&self.state);
        Some(if state.thunk == Value::Undefined {
            value.clone()
        } else {
            state.thunk.clone()
        })
    }

    /// Claim the cell for `task`, or report why it cannot. `waited` is the
    /// attempt this reader parked on, if it did: a failure of that attempt
    /// is reported to it even if a later attempt has ended since.
    pub(crate) fn claim(&self, task: TaskId, waited: Option<u64>) -> Claim {
        if waited.is_none() && self.realized.get().is_some() {
            return Claim::Realized;
        }
        let mut state = lock(&self.state);
        if let Some(waited) = waited
            && let Some(i) = state
                .failures
                .iter()
                .position(|f| f.attempt == waited && f.readers.contains(&task))
        {
            let failed = &mut state.failures[i];
            let failure = failed.failure.clone();
            failed.readers.retain(|t| *t != task);
            if failed.readers.is_empty() {
                state.failures.remove(i);
            }
            return Claim::Failed(failure);
        }
        if self.realized.get().is_some() {
            return Claim::Realized;
        }
        match state.realizer {
            Some((t, _)) if t == task => Claim::Reentered,
            Some((_, attempt)) => Claim::Busy { attempt },
            None => {
                state.attempts += 1;
                let attempt = state.attempts;
                state.realizer = Some((task, attempt));
                Claim::Run {
                    thunk: state.thunk.clone(),
                    attempt,
                }
            }
        }
    }

    /// Park `waiter` on attempt `attempt`, registering the wait in `waits`.
    /// False when that attempt is already over (the reader runs again at
    /// once instead of parking).
    pub(crate) fn subscribe(&self, waiter: Waiter, attempt: u64, waits: &Arc<LazyWaits>) -> bool {
        let mut state = lock(&self.state);
        if self.realized.get().is_some() || state.realizer.map(|(_, a)| a) != Some(attempt) {
            return false;
        }
        match state
            .waiters
            .iter_mut()
            .find(|w| w.handle.task() == waiter.handle.task())
        {
            Some(existing) => *existing = waiter,
            None => state.waiters.push(waiter),
        }
        // Lock order: cell, then registry.
        waits.insert(
            self.serial,
            attempt,
            state.realizer.expect("active attempt").0,
        );
        state.waits = Some(waits.clone());
        true
    }

    /// Withdraw `task`, a reader that parked on attempt `attempt` and will
    /// not read the cell again (its frame was dropped or cancelled): its
    /// waiter entry, or its claim on that attempt's retained failure.
    pub(crate) fn withdraw(&self, task: TaskId, attempt: u64) {
        let mut state = lock(&self.state);
        if state.realizer.map(|(_, a)| a) == Some(attempt) {
            state.waiters.retain(|w| w.handle.task() != task);
        }
        if let Some(i) = state.failures.iter().position(|f| f.attempt == attempt) {
            state.failures[i].readers.retain(|t| *t != task);
            if state.failures[i].readers.is_empty() {
                state.failures.remove(i);
            }
        }
    }

    /// End attempt `attempt`: install `value` (`Ok`; `checked` when the
    /// caller proved it shareable), record a failure, or (`None`) give the
    /// claim up; then wake the parked readers with no lock held. The caller
    /// keeps `value` and the failure rooted. A value not `checked` is not
    /// installed in a cell a publication marked shared meanwhile.
    pub(crate) fn settle(
        &self,
        attempt: u64,
        outcome: Option<Result<Value, LazyFailure>>,
        checked: bool,
    ) -> Result<(), Unsettled> {
        self.settle_record(attempt, outcome, checked, None)
    }

    /// Settle with the original record as well as its structural sequence.
    /// Both values must have passed the publication check when `checked`.
    pub(crate) fn settle_record(
        &self,
        attempt: u64,
        outcome: Option<Result<Value, LazyFailure>>,
        checked: bool,
        record: Option<Value>,
    ) -> Result<(), Unsettled> {
        let (waiters, waits) = {
            let mut state = lock(&self.state);
            if state.realizer.map(|(_, a)| a) != Some(attempt) {
                return Ok(());
            }
            if state.shared && !checked && matches!(outcome, Some(Ok(_))) {
                return Err(Unsettled::NeedsCheck);
            }
            state.realizer = None;
            let waiters = std::mem::take(&mut state.waiters);
            match outcome {
                Some(Ok(value)) => {
                    // Under the lock: a reader's recheck cannot miss it.
                    let _ = self.realized.set(value);
                    state.thunk = record.unwrap_or(Value::Undefined);
                }
                Some(Err(failure)) if !waiters.is_empty() => state.failures.push(Failed {
                    attempt,
                    failure,
                    readers: waiters.iter().map(|w| w.handle.task()).collect(),
                }),
                Some(Err(_)) | None => {}
            }
            (waiters, state.waits.clone())
        };
        let mut runnable = Vec::new();
        for w in waiters {
            if let Wake::Runnable(task) = deliver_wake(&w.task, w.handle) {
                runnable.push(task);
            }
        }
        if let Some(waits) = waits {
            waits.woken(runnable);
            // After the wakes: the key reports pending until its readers can
            // run again.
            waits.remove(self.serial, attempt);
        }
        Ok(())
    }
}

impl Trace for LazyCell {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        if let Some(v) = self.realized.get() {
            v.trace(visit);
        }
        // A leaf lock; collection runs while every mutator is stopped at a
        // safepoint, and no safepoint falls inside a critical section.
        let state = lock(&self.state);
        state.thunk.trace(visit);
        for failed in &state.failures {
            if let LazyFailure::Thrown(v) = &failed.failure {
                v.trace(visit);
            }
        }
    }
}

impl LazyCell {
    /// The value the publication walk descends into, read under the lock:
    /// whether it is realized, the structural value or thunk, and any
    /// original record retained for protocol dispatch.
    pub(crate) fn publication_edge(&self) -> (bool, Value, Option<Value>) {
        let state = lock(&self.state);
        match self.realized.get() {
            Some(v) => (
                true,
                v.clone(),
                (state.thunk != Value::Undefined).then(|| state.thunk.clone()),
            ),
            None => (false, state.thunk.clone(), None),
        }
    }
}

/// The lazy cells each task of the runtime is realizing, outermost first:
/// what an embedder reads to attribute a task's effects to the realization
/// a reader on another task may wait for ([`crate::Runtime::realizing`]),
/// or may read once it has settled ([`crate::Runtime::attribute_realizing`]).
/// A leaf metadata lock; the realizing frames keep its cells alive.
#[derive(Debug, Default)]
pub(crate) struct Realizing(Mutex<HashMap<TaskId, Vec<Arc<LazyCell>>>>);

impl Realizing {
    /// Note that `task` starts realizing `cell`; the realization ends when
    /// the returned guard is dropped.
    pub(crate) fn begin(self: &Arc<Self>, task: TaskId, cell: &Arc<LazyCell>) -> Realization {
        lock(&self.0).entry(task).or_default().push(cell.clone());
        Realization {
            registry: self.clone(),
            task,
            serial: cell.serial,
        }
    }

    pub(crate) fn of(&self, task: TaskId) -> Vec<u64> {
        let tasks = lock(&self.0);
        (tasks.get(&task).into_iter().flatten())
            .map(|cell| cell.serial)
            .collect()
    }

    /// As [`Self::of`], and mark those cells attributed: from then on a
    /// task that reads one of their realized values notes it
    /// ([`note_consumed`]).
    pub(crate) fn attribute(&self, task: TaskId) -> Vec<u64> {
        let tasks = lock(&self.0);
        (tasks.get(&task).into_iter().flatten())
            .map(|cell| {
                cell.attributed.store(true, Ordering::Relaxed);
                cell.serial
            })
            .collect()
    }
}

/// A realization in [`Realizing`], until dropped.
#[derive(Debug)]
pub(crate) struct Realization {
    registry: Arc<Realizing>,
    task: TaskId,
    serial: u64,
}

impl Drop for Realization {
    fn drop(&mut self) {
        let mut tasks = lock(&self.registry.0);
        if let Some(cells) = tasks.get_mut(&self.task) {
            if let Some(at) = cells.iter().rposition(|c| c.serial == self.serial) {
                cells.remove(at);
            }
            if cells.is_empty() {
                tasks.remove(&self.task);
            }
        }
    }
}

/// The runtime's pending lazy waits and the readers their settlements woke.
/// Woken readers are held weakly: one nobody else holds would never run.
#[derive(Debug, Default)]
pub(crate) struct LazyWaits {
    pending: Mutex<HashMap<u64, (u64, TaskId)>>,
    woken: Mutex<Vec<Weak<TaskInner>>>,
}

impl LazyWaits {
    fn insert(&self, serial: u64, attempt: u64, realizer: TaskId) {
        lock(&self.pending).insert(serial, (attempt, realizer));
    }

    fn remove(&self, serial: u64, attempt: u64) {
        let mut pending = lock(&self.pending);
        if pending
            .get(&serial)
            .is_some_and(|&(active, _)| active == attempt)
        {
            pending.remove(&serial);
        }
    }

    fn woken(&self, tasks: Vec<Task>) {
        if !tasks.is_empty() {
            let mut woken = lock(&self.woken);
            // An embedder that never takes them (one that polls the wait
            // key instead) leaves only dropped tasks behind.
            woken.retain(|w| w.strong_count() > 0);
            woken.extend(tasks.iter().map(Task::downgrade));
        }
    }

    /// Whether readers wait on the cell with serial `serial`.
    pub(crate) fn is_pending(&self, serial: u64) -> bool {
        lock(&self.pending).contains_key(&serial)
    }

    pub(crate) fn realizer(&self, serial: u64) -> Option<TaskId> {
        lock(&self.pending).get(&serial).map(|&(_, task)| task)
    }

    /// The readers woken since the last call, to enqueue.
    pub(crate) fn take_woken(&self) -> Vec<Task> {
        std::mem::take(&mut *lock(&self.woken))
            .iter()
            .filter_map(Task::upgrade)
            .collect()
    }
}

// ---- Realization requests ----

struct Consumption {
    owner: (u64, TaskId),
    serials: Vec<u64>,
}

thread_local! {
    static REQUEST: RefCell<Option<Value>> = const { RefCell::new(None) };
    static CYCLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The runtime and task whose run this thread executes now or executed
    /// last, and the serials of the attributed cells whose realized values
    /// it read ([`note_consumed`]). Task IDs are runtime-local.
    static CONSUMED: RefCell<Option<Consumption>> = const { RefCell::new(None) };
}

/// Note the attributed cells that `task`'s run on this thread reads
/// ([`note_consumed`]). What another task's run left untaken is dropped.
pub(crate) fn begin_consumption(runtime: u64, task: TaskId) {
    let owner = (runtime, task);
    CONSUMED.with(|c| {
        let mut c = c.borrow_mut();
        if c.as_ref().is_none_or(|previous| previous.owner != owner) {
            *c = Some(Consumption {
                owner,
                serials: Vec::new(),
            });
        }
    });
}

/// Note that the running task read the realized value of `cell`, if an
/// embedder attributed effects to its realization: the embedder counts
/// those effects for this reader too ([`take_consumed`]).
pub(crate) fn note_consumed(cell: &LazyCell) {
    if !cell.is_attributed() {
        return;
    }
    CONSUMED.with(|c| {
        if let Some(consumed) = &mut *c.borrow_mut()
            && !consumed.serials.contains(&cell.serial)
        {
            consumed.serials.push(cell.serial);
        }
    });
}

/// Take what [`note_consumed`] noted for `task`'s run on this thread.
pub(crate) fn take_consumed(runtime: u64, task: TaskId) -> Vec<u64> {
    CONSUMED.with(|c| match &mut *c.borrow_mut() {
        Some(consumed) if consumed.owner == (runtime, task) => {
            std::mem::take(&mut consumed.serials)
        }
        _ => Vec::new(),
    })
}

/// Record that the running operation met a realized sequence that contains
/// itself (hashing it would not end): the operation fails with a
/// `RangeError`.
pub(crate) fn note_cycle() {
    CYCLE.with(|c| c.set(true));
}

/// Take the recorded cycle.
pub(crate) fn take_cycle() -> bool {
    CYCLE.with(|c| c.replace(false))
}

/// Record that the running operation met the unrealized lazy form `v`
/// (the first one met wins).
pub(crate) fn request(v: Value) {
    REQUEST.with(|r| {
        let mut r = r.borrow_mut();
        if r.is_none() {
            *r = Some(v);
        }
    });
}

/// Take the recorded request.
pub(crate) fn take_request() -> Option<Value> {
    REQUEST.with(|r| r.borrow_mut().take())
}

/// Whether a request (or a cycle) is recorded.
pub(crate) fn has_request() -> bool {
    REQUEST.with(|r| r.borrow().is_some()) || CYCLE.with(std::cell::Cell::get)
}

// ---- Sequences ----

/// What built a cons sequence, which decides how it reads and exports like
/// the light-cljs runtime's sequence it stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConsKind {
    /// `cons` (a `Cons`).
    Cons,
    /// `concat` (a `LazySeq`). Its `seq` is chunked (`chunked`) when its
    /// first collection's seq was ([`ConsKind::plain`]).
    Concat { chunked: bool },
    /// `chunk-cons` (a `ChunkedCons`): `chunked-seq?` is true.
    Chunked,
    /// A chunked seq without metadata: a vector's seq (a `ChunkedSeq`) or a
    /// finite `range` (an `IntegerRange`).
    ChunkedSeq,
}

impl ConsKind {
    /// The kind of the seq of a lazy sequence that realized a sequence of
    /// this kind, or of a `concat`: the runtime's seq of its `LazySeq` has
    /// no metadata, and is chunked as before.
    pub(crate) fn plain(self) -> ConsKind {
        match self {
            ConsKind::Concat { chunked: true } | ConsKind::Chunked => ConsKind::ChunkedSeq,
            ConsKind::Concat { chunked: false } => ConsKind::Cons,
            other => other,
        }
    }

    /// The kind of what follows the first element: a multi-element `Cons`
    /// stands for the runtime's `concat` (its `rest` is a lazy `concat`).
    pub(crate) fn rest(self) -> ConsKind {
        match self {
            ConsKind::Cons => ConsKind::Concat { chunked: false },
            other => other,
        }
    }
}

/// The cons body: `items[start..]` (nonempty) followed by `rest`.
#[derive(Clone, Debug)]
pub(crate) struct ConsBody {
    pub(crate) items: PVector,
    pub(crate) start: usize,
    pub(crate) rest: Value,
    pub(crate) kind: ConsKind,
    /// A counted range's arithmetic. Metadata copies and shared tails retain
    /// it; the actual head determines the count even when a step rounds back
    /// to the same number beyond JavaScript's safe-integer boundary.
    pub(crate) range: Option<RangeCount>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RangeCount {
    pub(crate) end: f64,
    pub(crate) step: f64,
}

impl ConsBody {
    pub(crate) fn count(&self) -> Option<f64> {
        let range = self.range?;
        let Value::Number(head) = self.first() else {
            return None;
        };
        Some(((range.end - head) / range.step).ceil())
    }

    /// `chunked-seq?`.
    pub(crate) fn chunked(&self) -> bool {
        matches!(self.kind, ConsKind::Chunked | ConsKind::ChunkedSeq)
    }

    /// Whether the runtime's sequence it stands for has metadata (`nil`
    /// when none was given), which the compiler bridge exports, as for a
    /// lazy sequence.
    pub(crate) fn has_metadata(&self) -> bool {
        matches!(self.kind, ConsKind::Concat { .. } | ConsKind::Chunked)
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len() - self.start
    }

    pub(crate) fn first(&self) -> Value {
        self.items.get(self.start).unwrap_or(Value::Null)
    }

    pub(crate) fn elements(&self) -> Vec<Value> {
        (self.start..self.items.len())
            .filter_map(|i| self.items.get(i))
            .collect()
    }
}

impl Trace for ConsBody {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.items.trace(visit);
        self.rest.trace(visit);
    }
}

/// Cells already followed by one walk, to stop at a cycle (a realized
/// sequence that contains itself is infinite). By cell, since `with-meta`
/// copies share one.
#[derive(Default)]
pub(crate) struct Visited(HashSet<usize>);

impl Visited {
    /// False when `cell` was visited before.
    pub(crate) fn enter(&mut self, cell: &Arc<LazyCell>) -> bool {
        self.0.insert(Arc::as_ptr(cell) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cell: &LazyCell) -> u64 {
        match cell.claim(TaskId::from_raw(1), None) {
            Claim::Run { attempt, .. } => attempt,
            other => panic!("expected a claim, got {other:?}"),
        }
    }

    /// Publication and settlement are ordered by the cell lock: a value
    /// installed unchecked is never in a cell marked shared.
    #[test]
    fn publication_and_settlement_agree_on_what_was_checked() {
        // Marked shared while unrealized: an unchecked value is refused.
        let cell = LazyCell::new(Value::Undefined);
        let attempt = run(&cell);
        assert!(cell.mark_shared(false));
        let value = Value::Number(1.0);
        assert_eq!(
            cell.settle(attempt, Some(Ok(value.clone())), false),
            Err(Unsettled::NeedsCheck)
        );
        assert!(cell.realized().is_none());
        assert_eq!(cell.settle(attempt, Some(Ok(value)), true), Ok(()));
        assert!(cell.realized().is_some());
        // Realized unchecked before the mark: the publication must read the
        // realized value again.
        let cell = LazyCell::new(Value::Undefined);
        let attempt = run(&cell);
        assert_eq!(cell.settle(attempt, Some(Ok(Value::Null)), false), Ok(()));
        assert!(!cell.mark_shared(false));
        assert!(!cell.is_shared());
        assert!(cell.mark_shared(true));
        assert!(cell.is_shared());
    }
}

#[cfg(test)]
mod reader_tests {
    use super::*;
    use crate::ids::WakeHandle;

    fn waiter(task: u64) -> Waiter {
        Waiter {
            handle: WakeHandle::new(0, TaskId::from_raw(task), 1),
            task: Weak::new(),
        }
    }

    /// A failure is kept for the readers that parked on its attempt: a
    /// reader that saw the attempt busy but did not park cannot take it,
    /// and a withdrawn reader releases it.
    #[test]
    fn failures_belong_to_the_readers_that_parked() {
        let (a, b, c) = (
            TaskId::from_raw(1),
            TaskId::from_raw(2),
            TaskId::from_raw(3),
        );
        let waits = Arc::new(LazyWaits::default());
        let cell = LazyCell::new(Value::Undefined);
        let Claim::Run { attempt, .. } = cell.claim(a, None) else {
            panic!("no claim");
        };
        assert!(matches!(cell.claim(b, None), Claim::Busy { .. }));
        assert!(cell.subscribe(waiter(2), attempt, &waits));
        // C sees attempt 1 busy, then it fails before C parks.
        assert!(matches!(cell.claim(c, None), Claim::Busy { .. }));
        let failure = LazyFailure::Message(JsString::from("boom"));
        assert_eq!(cell.settle(attempt, Some(Err(failure)), false), Ok(()));
        assert!(!cell.subscribe(waiter(3), attempt, &waits));
        // C runs a new attempt; B still receives attempt 1's failure.
        let Claim::Run {
            attempt: second, ..
        } = cell.claim(c, Some(attempt))
        else {
            panic!("C did not claim a new attempt");
        };
        assert!(matches!(cell.claim(b, Some(attempt)), Claim::Failed(_)));
        assert_eq!(cell.settle(second, None, false), Ok(()));
        // A withdrawn reader releases the failure kept for it.
        let Claim::Run { attempt: third, .. } = cell.claim(a, None) else {
            panic!("no claim");
        };
        assert!(cell.subscribe(waiter(2), third, &waits));
        let failure = LazyFailure::Message(JsString::from("again"));
        assert_eq!(cell.settle(third, Some(Err(failure)), false), Ok(()));
        cell.withdraw(b, third);
        assert!(lock(&cell.state).failures.is_empty());
    }
}
