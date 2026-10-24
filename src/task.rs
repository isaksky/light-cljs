//! Tasks, the execution state machine, and run outcomes.
//!
//! A [`Task`] owns its VM state (`VmState`: instruction position, operands,
//! locals, diagnostic context). It is a cheap-to-clone, `Send + Sync` handle so
//! an embedder's scheduler can keep it in queues and hand it to whichever of
//! its own threads is free; the runtime owns no threads (`docs/design.md` §3).
//!
//! # Execution claim
//!
//! Running requires a [`RunGuard`], obtained with [`Task::try_begin_run`].
//! Acquisition never waits for another run: it is one compare-and-swap on the
//! task's atomic state word (`Runnable → Executing`). (Registering the
//! thread as a heap mutator first may wait while a collection request is
//! pending, if the thread is not already in heap access; see
//! [`crate::heap::gc`].) If the task is executing on any
//! thread (including this one) it fails with [`RunError::AlreadyRunning`], if
//! it is parked with [`RunError::Waiting`], if it finished with
//! [`RunError::Finished`]. So a task can never execute on two workers at
//! once, and a worker only borrows task state for the duration of one run.
//! The guard is never held across a suspension: every run ends (yield, park,
//! completion, failure, or dropping the guard) before anything waits.
//!
//! **Budgeted runs.** [`Task::run_with`] takes a [`Budget`] and a
//! [`Host`]. When the budget runs out the run returns
//! [`RunOutcome::Yielded`], releases the guard, and leaves the task `Ready`
//! with its VM state saved; running it again (on any thread) continues at the
//! saved instruction. No Rust stack frame survives between runs.
//!
//! # State machine (`docs/design.md` §§3, 8, 10)
//!
//! Each task has one atomic word: a phase, a `NOTIFIED` bit, a `CANCELLED`
//! bit, and an **execution number** (how many runs have been claimed). A
//! need in run `m` registers the wake handle `(task, m)` ([`WakeHandle`]).
//!
//! ```text
//!               claim                         park, no NOTIFIED
//! Runnable(n) ────────► Executing(m = n+1) ─────────────────────► Parked(m)
//!     ▲                   │      │      │                            │
//!     │ yield, guard      │      │      │ complete / fail, or        │
//!     │ dropped, or park  │      │      │ cancellation observed      │
//!     │ with NOTIFIED     │      │      ▼                            │
//!     │                   │      │   Terminal(m)                     │
//!     ├───────────────────┘      │                                   │
//!     │                          └─ wake(m): set NOTIFIED            │
//!     │                             cancel: set CANCELLED            │
//!     │                             (stays Executing)                │
//!     └──────────────── wake(m), or cancel (sets CANCELLED) ─────────┘
//! ```
//!
//! | Transition | By | Linearization point | Result |
//! |---|---|---|---|
//! | `Runnable(n) → Executing(n+1)` | [`Task::try_begin_run`] | CAS (`Acquire`) | the only owner of execution `n+1`; `CANCELLED` is kept |
//! | `Executing → Runnable` | yield, or dropping a guard | CAS (`Release`) | runnable; a pending `NOTIFIED` is dropped (it is runnable anyway), `CANCELLED` is kept |
//! | `Executing → Terminal` | completion or failure | CAS (`Release`) | every later claim fails, every later wake is stale; an unobserved `CANCELLED` is dropped |
//! | `Executing+CANCELLED → Terminal+CANCELLED` | the run observing a cancellation, after terminal cleanup | CAS (`Release`) | run returns [`RunOutcome::Cancelled`]; [`TaskStatus::Cancelled`] |
//! | `Executing(m) → Parked(m)` | [`RunGuard::park`], no `NOTIFIED` or `CANCELLED` | CAS (`Release`) | run returns [`RunOutcome::Waiting`]; only a wake for `m` or a cancellation resumes it |
//! | `Executing(m)+NOTIFIED → Runnable(m)` | [`RunGuard::park`], no `CANCELLED` | CAS (`Release`) | run returns [`RunOutcome::Yielded`]: the runner requeues it |
//! | `Executing(m) → Executing(m)+NOTIFIED` | wake for `m` | CAS (`AcqRel`) | [`Wake::Remembered`]: nothing to enqueue |
//! | `Parked(m) → Runnable(m)` | wake for `m` | CAS (`AcqRel`) | [`Wake::Runnable`]: the waker enqueues it |
//! | `Runnable` or `Executing` `→ +CANCELLED` | [`Task::cancel`] | CAS (`AcqRel`) | [`Cancel::Requested`]: the current or next run observes it |
//! | `Parked(m) → Runnable(m)+CANCELLED` | [`Task::cancel`] | CAS (`AcqRel`) | [`Cancel::Runnable`]: the canceller enqueues it |
//! | anything else | wake / cancel | read | [`Wake::Stale`] (any `CANCELLED` word included) / [`Cancel::AlreadyRequested`] or [`Cancel::Finished`]: nothing changes |
//!
//! **Exactly once per suspension.** A suspension is one execution `m` that
//! subscribed. A wake for `m` changes the word only from `Executing(m)`
//! without `NOTIFIED` or from `Parked(m)`, and its result is a word that no
//! other wake for `m` accepts, so at most one wake per suspension takes
//! effect; all others (duplicates, wakes for older executions, wakes after
//! the task finished or ran again) are [`Wake::Stale`]. The owner's park and
//! that one wake are ordered by the word: if the wake comes first, the park
//! sees `NOTIFIED` and the run returns `Yielded` (the runner requeues); if the
//! park comes first, the wake finds `Parked` and returns the task to enqueue.
//! Either way exactly one party makes the task runnable, and a claim then
//! gives it exactly one owner. A cancellation is one more party racing for
//! the same word: from `Parked(m)` it and the wake CAS from the same value,
//! so exactly one of them returns the task to enqueue and the other rereads
//! (a wake that finds `CANCELLED` is stale; a cancellation that finds the
//! task runnable only sets its bit).
//!
//! **Completion racing the consumer.** A need registers the handle and the
//! completion drains it in the same cell critical section ([`crate::dep`]),
//! so the four windows resolve as follows: completion before registration or
//! between the consumer's fast-path read and its subscription: the need
//! reads the outcome and the task never parks; after subscription but
//! before park: the wake is remembered and the park returns `Yielded`;
//! after park: the wake returns the task as runnable.
//!
//! # Consumer cancellation (`docs/design.md` §§8, 10)
//!
//! [`Task::cancel`] cancels one consumer: one CAS that sets the sticky
//! `CANCELLED` bit (and, if the task is parked, makes it runnable for the
//! canceller to enqueue). It never touches a dependency cell's outcome or
//! producer: a producer still needed by other consumers keeps running and
//! its completion still wakes them. It takes no lock and runs no
//! JavaScript or host code, so any thread may call it, including a leaf
//! host effect of the task's own run.
//!
//! From the request on, the task can never be resurrected into ordinary
//! execution: every wake is [`Wake::Stale`] (a completion that still finds
//! its waiter entry enqueues nothing), [`RunGuard::need`] refuses with
//! [`NeedError::Cancelled`] and subscribes nothing, and a park cannot park.
//!
//! **Observation points.** The cancellation is observed by the task's own
//! run, never inline by the canceller, so a later exception/finally policy
//! (pellet clj-quickjs-27) can run language cleanup there. A run observes
//! it at run start ([`RunGuard::run_with`], before executing anything), at
//! [`RunGuard::need`] (the error above; the caller ends the run), at
//! [`RunGuard::park`] (up to its CAS, ahead of a remembered wake), and at
//! the budget boundary of a run that would otherwise yield.
//! [`RunGuard::cancel_requested`] lets a run poll. The VM dispatch loop does
//! not poll between instructions, so a run with an unlimited budget and no
//! need observes nothing until it returns or fails, in which case that
//! outcome stands and the request is dropped. A request that lands after a
//! need's initial check is observed by the park: the need still subscribes
//! and still returns the producer request it claimed, so the producer is
//! never lost for the other consumers.
//!
//! **Terminal cleanup** is done by the observing run while it still owns the
//! execution: (1) resolve the task's unresolved wait and remove its waiter
//! entry from that cell (cell lock, a leaf; recorded dependency edges stay);
//! (2) release its transient builders and its VM state (frames, operands,
//! locals), so nothing the suspended state held is a root any more; (3) end
//! the execution `Terminal` with `CANCELLED`. The run returns
//! [`RunOutcome::Cancelled`], which is neither a suspension (`Waiting`) nor
//! an error (`Failed`); the task's status becomes [`TaskStatus::Cancelled`],
//! later claims fail with [`RunError::Finished`] and later cancellations
//! return [`Cancel::Finished`]. Until that run happens the request is only
//! recorded: a cancelled task that its embedder never runs again keeps its
//! VM state as roots until its last [`Task`] handle is dropped (cells hold
//! only a `Weak` reference), and its waiter entry is inert.
//!
//! The windows (`src/task/cancel_tests.rs`, forced with test hooks):
//! before registration (need refused); inside the need between the
//! fast-path read and the subscription (observed by the park; producer
//! claim kept); after subscription (observed by the park, also with a
//! remembered wake); inside the park between its read and CAS (CAS fails,
//! park rereads); after park (canceller enqueues; the completion's wake is
//! stale whether it comes before or after the cleanup); inside a wake
//! between its read and CAS and vice versa (one enqueue); while running
//! (budget boundary) and between runs (run start).
//!
//! **Locks.** The state word is not a lock. The task body mutex is taken only
//! by the claim's owner (and for an instant by the collector, which never locks the body of an
//! executing task, or by a diagnostic reader such as
//! `Task::source_location`); the claim only `try_lock`s it, reporting
//! `AlreadyRunning` on contention, so starting a run never waits. The
//! dependency log mutex is a leaf; terminal cleanup releases it before
//! taking the cell lock. No lock is held while a wake or a cancellation is
//! delivered, and neither runs JavaScript or host code: each is one CAS
//! that reports what the caller should enqueue.
//!
//! # Dependency waits in the VM (`docs/design.md` §§7, 10)
//!
//! `__rt.need(definition, kind)` compiles to the VM `need` instruction
//! ([`crate::frontend::compile_script`]; the intrinsic contract is in
//! `crate::frontend::intrinsics`). The dispatch loop only decodes its
//! operands and returns to the run ([`RunGuard::run_with`]), which performs
//! the same protocol as [`RunGuard::need`] in the task's own world:
//!
//! 1. **Record** the dependency in the task's log, ready reads included.
//! 2. **Ready**: push the shared value (the cell's object itself, never a
//!    copy) onto the running frame's operands and keep executing in the same
//!    run and budget.
//! 3. **Failed** or a **cancelled cell**: the run fails at the `need`
//!    ([`VmErrorKind::DependencyFailed`] / [`VmErrorKind::DependencyCancelled`]),
//!    its diagnostic naming the instruction, its source position and the
//!    call chain, so later exception handling (clj-quickjs-11) can raise it
//!    there instead. The published error value stays in the cell.
//! 4. **Pending**: the task is subscribed and, if this need was the cell's
//!    first, has claimed its single producer. The claimed
//!    [`ProducerRequest`] is kept in the task for the embedder
//!    ([`Task::take_producer_requests`]), since the run cannot return it.
//! 5. **Suspend**: the run saves the pending result destination (the key and
//!    the instruction, `PendingNeed` in the VM state; the result slot is the
//!    running frame's operand top) and ends through [`RunGuard::park`], the
//!    same path as an embedder's need: [`RunOutcome::Waiting`] with the key,
//!    or `Yielded` if the completion already arrived, or `Cancelled`.
//!    Every frame, operand, local, closure and builder stays in the task;
//!    no Rust frame survives the run, so any thread may resume it.
//! 6. **Resume**: the next run, before executing anything, completes the
//!    suspended instruction by needing the same key again (cancellation is
//!    observed first, at run start): normally the cell is complete and the
//!    value is pushed (or the failure reported) exactly as in 2 and 3, and
//!    execution continues after the `need`. A spurious resume finds it
//!    still pending and suspends again. Nothing before the `need` runs
//!    again: no effect is replayed and no function is restarted.
//!
//! A consumer cancellation is observed as for [`RunGuard::need`]: a need
//! refused because the task is cancelled ends the run
//! [`RunOutcome::Cancelled`], and one that lands after the subscription is
//! observed by the park (any producer request it claimed is still kept for
//! the embedder). Injecting a result on resume is part of the suspended
//! instruction, so it happens even with a zero [`Budget`].

#[cfg(test)]
mod cancel_tests;
#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod need_tests;
mod state;

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError, Weak};

use crate::dep::{
    self, DependencyLog, DependencyTable, HookPoint, Need, NeedError, ProducerRequest, Read,
    Terminal, Waiter,
};
use crate::error::{SourceLocation, VmError, VmErrorKind};
use crate::heap::gc::{Access, MutatorToken, RootCategory, RootSource, Trace};
use crate::heap::{Heap, ObjectId, Rooted};
use crate::host::{Host, NoHost};
use crate::ids::{DependencyId, DependencyKey, ResultKind, TaskId, WakeHandle, WorldId};
use crate::value::Value;
use crate::vm::Executable;
use crate::vm::dispatch::{Exit, execute};
use crate::vm::frames::{PendingNeed, VmState};
use state::{CancelDelivery, Delivery, End, ExecState, NotRunnable, Park, Phase};

/// Result of one run of a task.
#[derive(Debug)]
pub enum RunOutcome {
    /// The task returned a value, delivered as a persistent root so it stays
    /// valid after the task is dropped.
    Complete(Rooted),
    /// The task failed at run time.
    Failed(VmError),
    /// The task is parked waiting on this dependency: it occupies no worker,
    /// cannot be run, and becomes runnable exactly once, through the
    /// [`Wakeups`](crate::Wakeups) of the completion that publishes the
    /// result. Produced by [`RunGuard::park`], which a run also uses when a
    /// VM `need` (`__rt.need`) finds its result pending (module docs,
    /// "Dependency waits in the VM").
    Waiting(DependencyKey),
    /// The task is runnable and should be run again: it used up its
    /// execution [`Budget`] (its instruction position, operands and locals
    /// are saved; the next run, on any thread, continues there), or a park
    /// found its wake already delivered.
    Yielded,
    /// The run observed the task's cancellation ([`Task::cancel`]) and
    /// performed terminal cleanup: its waiter entry is removed, its VM state
    /// and transient builders are released, and the task is finished
    /// ([`TaskStatus::Cancelled`]). Distinct from `Waiting` (a suspension)
    /// and `Failed` (an error). See the module docs, "Consumer cancellation".
    Cancelled,
}

/// How many instructions one run may execute before it returns
/// [`RunOutcome::Yielded`].
///
/// The budget is checked between instructions only, so every instruction
/// (including a leaf host effect) executes completely in exactly one run. A
/// budget of zero yields immediately without executing anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget(Option<u64>);

impl Budget {
    /// Run until the task completes or fails.
    pub const UNLIMITED: Budget = Budget(None);

    /// Execute at most `n` instructions in this run.
    pub const fn instructions(n: u64) -> Budget {
        Budget(Some(n))
    }
}

/// Why a run could not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunError {
    /// The task is currently being run (its execution guard is held).
    AlreadyRunning,
    /// The task already completed, failed or was cancelled; its VM state is
    /// gone.
    Finished,
    /// A previous run panicked while holding the guard, so the task state may
    /// be inconsistent. The VM does not panic by design; this is defensive.
    Poisoned,
    /// The calling thread is inside a leaf host effect, which may not reenter
    /// the VM (see [`crate::host`]).
    InsideHostEffect,
    /// The task is parked on a dependency; only the wake from its
    /// completion makes it runnable again.
    Waiting,
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RunError::AlreadyRunning => "task is already running",
            RunError::Finished => "task has finished",
            RunError::Poisoned => "task state is poisoned by an earlier panic",
            RunError::InsideHostEffect => "a host effect may not reenter the VM",
            RunError::Waiting => "task is parked waiting on a dependency",
        })
    }
}

impl std::error::Error for RunError {}

/// Observable task status: a snapshot of the state machine (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    /// Can be run.
    Ready,
    /// Its execution guard is held.
    Running,
    /// Completed or failed (or poisoned by a panicking run).
    Finished,
    /// Parked on a dependency until its completion wakes it.
    Waiting,
    /// A run observed its cancellation and performed terminal cleanup. (A
    /// cancellation that is requested but not yet observed shows as `Ready`
    /// or `Running`; see [`Task::cancel`].)
    Cancelled,
}

/// What requesting a consumer's cancellation did ([`Task::cancel`]).
#[derive(Debug)]
#[must_use = "a task returned as runnable must be enqueued so its run can clean up"]
pub enum Cancel {
    /// The task was parked. It is now runnable (no wake can make it runnable
    /// again), and the canceller enqueues it: its next run observes the
    /// cancellation and performs terminal cleanup. Exactly one of this and a
    /// racing completion's wake returns the task to enqueue.
    Runnable(Task),
    /// The task was runnable or executing. Its current run observes the
    /// cancellation at its next observation point, or its next run does;
    /// whoever already holds or queues the task runs it. Nothing to enqueue.
    Requested,
    /// Cancellation was already requested: nothing changed.
    AlreadyRequested,
    /// The task already completed, failed or was cancelled: nothing changed.
    Finished,
}

/// What delivering a wake did ([`Task::wake`], [`Wakeups`](crate::Wakeups)).
#[derive(Debug)]
pub enum Wake {
    /// The task was parked on this suspension and is now runnable. The
    /// receiver enqueues it; exactly one wake per suspension returns this.
    Runnable(Task),
    /// The task is still in the run that subscribed. The wake is remembered:
    /// that run's [`RunGuard::park`] returns [`RunOutcome::Yielded`] instead
    /// of parking, and its runner requeues the task. Nothing to enqueue.
    Remembered,
    /// Nothing happened: this suspension was already woken or remembered
    /// (a duplicate), the run ended without parking, the task has run again
    /// or finished, the task no longer exists, or the handle names another
    /// task. Harmless.
    Stale,
}

/// Task-owned state behind the execution guard.
#[derive(Debug)]
enum TaskBody {
    /// Boxed: the VM state (frames, value stack, handler stack) is much
    /// larger than the finished marker.
    Ready(Box<VmState>),
    Finished,
}

impl Trace for TaskBody {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match self {
            TaskBody::Ready(vm) => vm.trace(visit),
            TaskBody::Finished => {}
        }
    }
}

#[derive(Debug)]
pub(crate) struct TaskInner {
    id: TaskId,
    world: WorldId,
    executable: Executable,
    heap: Arc<Heap>,
    /// The runtime's dependency cells.
    deps: Arc<DependencyTable>,
    /// The execution state machine (module docs).
    exec: ExecState,
    /// Written only by the holder of the run guard; outlives the VM state.
    /// Holds keys only, so it is not a root.
    log: Mutex<DependencyLog>,
    /// Locked only by the execution claim's owner (and the collector).
    body: Mutex<TaskBody>,
    /// Producer requests claimed by this task's VM `Need` instructions, until
    /// the embedder takes them ([`Task::take_producer_requests`]). A leaf
    /// lock; holds no heap values.
    produce: Mutex<Vec<ProducerRequest>>,
}

/// A task is a root source while any handle to it exists: its saved frames
/// (`TaskFrame`) and its executable's constants (`ExecutableConstant`).
///
/// An executing task's body is held by its run guard, whose thread is
/// stopped at a VM safepoint during any collection and reported these
/// frames itself (`crate::heap::gc`); its body is skipped, never waited
/// for. Otherwise the body is free or held for an instant by a diagnostic
/// reader, so the lock is short.
impl RootSource for TaskInner {
    fn trace_roots(&self, visit: &mut dyn FnMut(RootCategory, ObjectId)) {
        self.executable
            .0
            .trace(&mut |id| visit(RootCategory::ExecutableConstant, id));
        // During a collection no thread is between registering for a run
        // and holding its body, or between releasing the body and
        // deregistering (those threads are in heap access and not stopped),
        // so `Executing` here means a run parked at a safepoint.
        if let Phase::Executing { .. } = self.exec.snapshot().phase {
            return;
        }
        // A poisoned body (a run panicked) is traced as-is: its values are
        // conservative roots until the task handle is dropped.
        let body = self.body.lock().unwrap_or_else(PoisonError::into_inner);
        body.trace(&mut |id| visit(RootCategory::TaskFrame, id));
    }
}

impl TaskInner {
    /// Deliver a wake for `handle`'s execution (module docs).
    fn deliver(self: &Arc<Self>, handle: WakeHandle) -> Wake {
        if handle.runtime() != self.deps.runtime || handle.task() != self.id {
            return Wake::Stale;
        }
        match self.exec.wake(handle.execution(), || {
            self.deps.hook(HookPoint::WakeBeforeCas)
        }) {
            Delivery::Runnable => Wake::Runnable(Task(Arc::clone(self))),
            Delivery::Remembered => Wake::Remembered,
            Delivery::Stale => Wake::Stale,
        }
    }
}

/// Terminal cleanup of a cancelled task, by the owner of its execution
/// (module docs, "Consumer cancellation"): leave the cell it waits on, then
/// release its transient builders and VM state, so nothing it held stays a
/// root. The caller then releases the claim with [`End::Cancelled`].
fn cleanup_cancelled(task: &TaskInner, body: &mut TaskBody) {
    dep::withdraw(&task.deps, &task.log, task.id);
    if let TaskBody::Ready(vm) = body {
        vm.builders.release_all(&task.heap, task.id);
    }
    *body = TaskBody::Finished;
}

/// Deliver a drained wake handle to its task, if the task still exists.
/// Called by completion with no lock held.
pub(crate) fn deliver_wake(task: &Weak<TaskInner>, handle: WakeHandle) -> Wake {
    match task.upgrade() {
        Some(inner) => inner.deliver(handle),
        None => Wake::Stale,
    }
}

/// A resumable unit of execution. See the module docs.
#[derive(Clone, Debug)]
pub struct Task(Arc<TaskInner>);

/// The execution claim held by a run guard. Ending it is the guard's last
/// state transition; dropping it unended makes the task runnable again
/// (a guard dropped without an outcome, or a run unwinding from a panic).
struct ExecutionClaim<'t> {
    state: &'t ExecState,
    number: u64,
    ended: bool,
}

impl ExecutionClaim<'_> {
    fn end(mut self, end: End) {
        self.ended = true;
        self.state.release(self.number, end);
    }

    /// Park; on [`Park::Cancelled`] the claim is still held, and the caller
    /// ends it after terminal cleanup.
    fn park(&mut self, before_cas: impl FnMut()) -> Park {
        let park = self.state.park(self.number, before_cas);
        if park != Park::Cancelled {
            self.ended = true;
        }
        park
    }
}

impl Drop for ExecutionClaim<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.state.release(self.number, End::Runnable);
        }
    }
}

/// Exclusive right to run a task, held for one execution. Not `Send`: the
/// run happens on the thread that acquired it.
///
/// A guard is a registered heap mutator from acquisition to drop: a
/// collection requested on another thread waits until the run stops at a VM
/// safepoint or the guard is dropped, and
/// [`Runtime::collect`](crate::Runtime::collect) refuses to run meanwhile
/// (see [`crate::heap::gc`]). The guard's thread cannot collect.
pub struct RunGuard<'t> {
    task: &'t Arc<TaskInner>,
    // Field order matters (fields drop in order): the body lock is released
    // first, then the execution claim (so the next owner never finds the
    // body still locked), then the mutator registration (so a collector
    // never sees zero mutators while the body is still locked by a run).
    body: MutexGuard<'t, TaskBody>,
    claim: ExecutionClaim<'t>,
    /// The key this execution's last need subscribed to, if that need found
    /// the cell pending: what [`RunGuard::park`] parks on.
    subscribed: Option<DependencyKey>,
    _mutator: MutatorToken<'t>,
}

impl Task {
    pub(crate) fn new(
        id: TaskId,
        world: WorldId,
        executable: Executable,
        heap: Arc<Heap>,
        deps: Arc<DependencyTable>,
        vm: VmState,
    ) -> Task {
        let inner = Arc::new(TaskInner {
            id,
            world,
            executable,
            heap,
            deps,
            exec: ExecState::new(),
            log: Mutex::new(DependencyLog::default()),
            body: Mutex::new(TaskBody::Ready(Box::new(vm))),
            produce: Mutex::new(Vec::new()),
        });
        let source: Weak<dyn RootSource> = Arc::downgrade(&inner) as Weak<TaskInner>;
        inner.heap.register_source(source);
        Task(inner)
    }

    pub fn id(&self) -> TaskId {
        self.0.id
    }

    /// Process-unique identity of the runtime this task belongs to.
    pub(crate) fn runtime_id(&self) -> u64 {
        self.0.deps.runtime
    }

    /// The world/version this task executes in.
    pub fn world(&self) -> WorldId {
        self.0.world
    }

    /// The shared code this task runs.
    pub fn executable(&self) -> &Executable {
        &self.0.executable
    }

    /// A snapshot of the task's dependency log: recorded edges (every key it
    /// has needed, ready reads included) and its unresolved wait, if any.
    pub fn dependency_log(&self) -> DependencyLog {
        self.0
            .log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Source location of the next instruction this task will execute, for
    /// a ready task running code compiled from JavaScript source. `None` for
    /// hand-assembled code, or while the task is running or finished.
    /// While the task is suspended at a `Need` instruction (`__rt.need`), the
    /// location of that instruction.
    pub fn source_location(&self) -> Option<SourceLocation> {
        match &*self.0.body.try_lock().ok()? {
            TaskBody::Ready(vm) => {
                let pc = vm.pending_need.map_or(vm.top().pc, |p| p.pc);
                vm.top().code.location(pc)
            }
            TaskBody::Finished => None,
        }
    }

    /// Take the producer requests this task's VM `Need` instructions
    /// (`__rt.need`) claimed since the last call, oldest first (module docs,
    /// "Dependency waits in the VM").
    ///
    /// The first need of a pending, unclaimed cell claims its single
    /// producer; when that need is executed by the VM rather than through
    /// [`RunGuard::need`], the request is kept here instead of being
    /// returned. The embedder takes it after the run (normally one that
    /// ended [`RunOutcome::Waiting`]) and hands it to its scheduler; dropping
    /// it leaves the cell without a producer. Requests stay here until taken,
    /// whatever the task does next, so none is lost.
    pub fn take_producer_requests(&self) -> Vec<ProducerRequest> {
        std::mem::take(
            &mut *self
                .0
                .produce
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// Current status. A snapshot: it may change as soon as it is returned.
    pub fn status(&self) -> TaskStatus {
        let snapshot = self.0.exec.snapshot();
        match snapshot.phase {
            Phase::Runnable if self.0.body.is_poisoned() => TaskStatus::Finished,
            Phase::Runnable => TaskStatus::Ready,
            Phase::Executing { .. } => TaskStatus::Running,
            Phase::Parked => TaskStatus::Waiting,
            Phase::Terminal if snapshot.cancelled => TaskStatus::Cancelled,
            Phase::Terminal => TaskStatus::Finished,
        }
    }

    /// True once cancellation has been requested ([`Task::cancel`]), whether
    /// or not a run has observed it yet; false again only if a run completed
    /// or failed before observing it. A snapshot.
    pub fn cancel_requested(&self) -> bool {
        self.0.exec.snapshot().cancelled
    }

    /// Cancel this task as a consumer (module docs, "Consumer cancellation").
    ///
    /// One CAS on the task's state word; it takes no lock, runs no
    /// JavaScript or host code, and never cancels a dependency cell or its
    /// producer, which other consumers may still need. The request is
    /// sticky: from now on no wake makes the task runnable and no need
    /// subscribes it. Terminal cleanup (removing its waiter entry,
    /// releasing its VM state and builders) is done by the task's own run
    /// when it observes the request: at run start, [`RunGuard::need`],
    /// [`RunGuard::park`], or the end of a budgeted run; that run returns
    /// [`RunOutcome::Cancelled`]. If this returns [`Cancel::Runnable`] the
    /// caller enqueues the task for that run. Allowed from any thread,
    /// including inside a leaf host effect of the task's own run.
    pub fn cancel(&self) -> Cancel {
        match self
            .0
            .exec
            .cancel(|| self.0.deps.hook(HookPoint::CancelBeforeCas))
        {
            CancelDelivery::Runnable => Cancel::Runnable(self.clone()),
            CancelDelivery::Requested => Cancel::Requested,
            CancelDelivery::AlreadyRequested => Cancel::AlreadyRequested,
            CancelDelivery::Finished => Cancel::Finished,
        }
    }

    /// Deliver `handle` (normally from [`Wakeups`](crate::Wakeups)) to this
    /// task. A completion already delivers every handle it drains, so an
    /// embedder calling this again gets [`Wake::Stale`]: duplicate
    /// notifications are harmless. See [`Wake`].
    pub fn wake(&self, handle: WakeHandle) -> Wake {
        self.0.deliver(handle)
    }

    /// Acquire the execution guard without waiting for another run (the
    /// claim, module docs). Entering heap access waits for a pending
    /// collection request ([`Runtime::request_collection`](crate::Runtime::request_collection)).
    ///
    /// The guard's thread delays every requested collection until its run
    /// reaches a VM safepoint or the guard is dropped; a guard acquired and
    /// held without running does not reach one.
    pub fn try_begin_run(&self) -> Result<RunGuard<'_>, RunError> {
        if crate::host::inside_effect() {
            return Err(RunError::InsideHostEffect);
        }
        // Register as a mutator before claiming and taking the body, so a
        // collector that sees this thread stopped never sees an executing
        // task whose frames nobody reported. On the error paths
        // below the token is dropped, deregistering again.
        let mutator = self.0.heap.enter_mutator(Access::Run);
        let claim = match self.0.exec.claim() {
            Ok(number) => ExecutionClaim {
                state: &self.0.exec,
                number,
                ended: false,
            },
            Err(NotRunnable::Executing) => return Err(RunError::AlreadyRunning),
            Err(NotRunnable::Parked) => return Err(RunError::Waiting),
            Err(NotRunnable::Terminal) => return Err(RunError::Finished),
        };
        // The claim makes this thread the only runner; the body may still be
        // held for an instant by a diagnostic reader (`source_location`, test
        // helpers). Never wait for it: report `AlreadyRunning` as before the
        // state machine existed. On the error paths the claim is dropped,
        // making the task runnable again.
        let body = match self.0.body.try_lock() {
            Ok(body) => body,
            Err(TryLockError::WouldBlock) => return Err(RunError::AlreadyRunning),
            Err(TryLockError::Poisoned(_)) => return Err(RunError::Poisoned),
        };
        if let TaskBody::Finished = *body {
            // Unreachable: a finished body always comes with `Terminal`.
            drop(body);
            claim.end(End::Finished);
            return Err(RunError::Finished);
        }
        Ok(RunGuard {
            task: &self.0,
            body,
            claim,
            subscribed: None,
            _mutator: mutator,
        })
    }

    /// Acquire the guard and run on the calling thread with an unlimited
    /// budget and no host (host effects fail the task).
    pub fn run(&self) -> Result<RunOutcome, RunError> {
        Ok(self.try_begin_run()?.run())
    }

    /// Acquire the guard and run on the calling thread for at most `budget`
    /// instructions, sending leaf host effects to `host`.
    pub fn run_with(&self, budget: Budget, host: &mut dyn Host) -> Result<RunOutcome, RunError> {
        Ok(self.try_begin_run()?.run_with(budget, host))
    }

    /// Hold the task body's lock until the returned guard drops, as a
    /// diagnostic reader does for an instant: claims meanwhile fail with
    /// `AlreadyRunning` and leave the task runnable. Test helper.
    #[cfg(test)]
    pub(crate) fn hold_body(&self) -> impl Sized + '_ {
        self.0.body.lock().unwrap()
    }

    /// The saved instruction position (index of the next instruction) and the
    /// saved operands and locals, or `None` while running or finished.
    #[cfg(test)]
    pub(crate) fn saved_state(&self) -> Option<(usize, Vec<Value>, Vec<Value>)> {
        match &*self.0.body.try_lock().ok()? {
            TaskBody::Ready(vm) => Some(vm.snapshot()),
            TaskBody::Finished => None,
        }
    }

    /// Visit heap references in the task's VM state; `false` if the state is
    /// not accessible (the task is running). Test helper; the collector
    /// reaches tasks through their `RootSource` registration.
    #[cfg(test)]
    pub(crate) fn trace_frames(&self, visit: &mut dyn FnMut(ObjectId)) -> bool {
        match self.0.body.try_lock() {
            Ok(body) => {
                body.trace(visit);
                true
            }
            Err(_) => false,
        }
    }
}

impl RunGuard<'_> {
    /// The task being run.
    pub fn task_id(&self) -> TaskId {
        self.task.id
    }

    /// The wake handle of this run: what a need in this run registers.
    /// Delivering it with [`Task::wake`] while the run is executing is a
    /// spurious but harmless wake (the park returns `Yielded`).
    pub fn wake_handle(&self) -> WakeHandle {
        WakeHandle::new(self.task.deps.runtime, self.task.id, self.claim.number)
    }

    /// Read or subscribe to the result `kind` of `dependency` in this task's
    /// world: the native protocol behind `__rt.need` (`docs/design.md` §7),
    /// for an embedder issuing needs from Rust. The VM `need` instruction
    /// runs the same protocol (module docs, "Dependency waits in the VM").
    ///
    /// Records the dependency in the task's log in every case. Returns the
    /// shared value ([`Need::Ready`]) or published error ([`Need::Failed`])
    /// when the cell is complete, or [`Need::Cancelled`]. Otherwise registers
    /// this run's [`WakeHandle`] and returns [`Need::Waiting`], carrying the
    /// cell's producer request if this call was the first to ask for it; the
    /// caller hands that to its scheduler and normally ends the run with
    /// [`RunGuard::park`]. A completed cell is read through an atomic fast
    /// path; otherwise checking the cell and registering happen in one
    /// critical section, so no completion can fall between them; see
    /// [`crate::dep`] for the lock and ordering contract. Needing the same
    /// key again after it completes resolves the wait.
    ///
    /// Fails with [`NeedError::AlreadyWaiting`], changing nothing, if the
    /// task's unresolved wait is on a different key, and with
    /// [`NeedError::Cancelled`], changing nothing, if the task's
    /// cancellation has been requested (an observation point: the run should
    /// end, and [`RunGuard::park`] or [`RunGuard::run_with`] cleans up). A
    /// cancellation that arrives after this check is observed by the park
    /// instead: the need still subscribes and still returns any producer
    /// request it claimed, so the cell keeps a producer for its other
    /// consumers.
    pub fn need(&mut self, dependency: DependencyId, kind: ResultKind) -> Result<Need, NeedError> {
        let (key, read) = self.read(dependency, kind)?;
        Ok(dep::rooted(self.task.deps.heap(), key, read))
    }

    /// The protocol shared by [`RunGuard::need`] and the VM's `Need`: the
    /// cancellation check, read-or-subscribe with the dependency recorded,
    /// and the key a later [`RunGuard::park`] parks on. A terminal value is
    /// returned raw; it stays reachable through its cell.
    fn read(
        &mut self,
        dependency: DependencyId,
        kind: ResultKind,
    ) -> Result<(DependencyKey, Read), NeedError> {
        let key = DependencyKey::new(self.task.world, dependency, kind);
        if self.task.exec.snapshot().cancelled {
            return Err(NeedError::Cancelled { requested: key });
        }
        let waiter = Waiter {
            handle: self.wake_handle(),
            task: Arc::downgrade(self.task),
        };
        let read = dep::read_and_record(&self.task.deps, &self.task.log, waiter, key)?;
        self.subscribed = match read {
            Read::Waiting { .. } => Some(key),
            Read::Terminal(_) => None,
        };
        Ok((key, read))
    }

    /// The task's VM state; `None` only for a finished body, which
    /// `try_begin_run` never hands out.
    fn vm(&mut self) -> Option<&mut VmState> {
        match &mut *self.body {
            TaskBody::Ready(vm) => Some(vm),
            TaskBody::Finished => None,
        }
    }

    /// Execute a VM `Need` of result `kind` of `dependency` at instruction
    /// `pc` of the running frame (module docs, "Dependency waits in the
    /// VM"): inject a terminal result, or record the pending result
    /// destination and keep the claimed producer request for the embedder.
    fn vm_need(&mut self, dependency: DependencyId, kind: ResultKind, pc: usize) -> VmNeed {
        let read = self.read(dependency, kind);
        let task = self.task;
        let Some(vm) = self.vm() else {
            return VmNeed::Fail(Box::new(guard_failure(
                task,
                VmErrorKind::Internal,
                "run guard held for a finished task",
                pc,
            )));
        };
        let (key, terminal) = match read {
            Err(NeedError::Cancelled { .. }) => return VmNeed::Cancelled,
            Err(e @ NeedError::AlreadyWaiting { .. }) => {
                return VmNeed::Fail(Box::new(vm.failure(
                    VmErrorKind::Internal,
                    e.to_string(),
                    pc,
                )));
            }
            Ok((key, Read::Waiting { produce })) => {
                vm.pending_need = Some(PendingNeed { key, pc });
                if let Some(request) = produce {
                    task.produce
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(request);
                }
                return VmNeed::Park;
            }
            Ok((key, Read::Terminal(terminal))) => (key, terminal),
        };
        vm.pending_need = None;
        let (kind, message) = match terminal {
            Terminal::Ready(value) => {
                // Pushed raw: the cell keeps the value alive, and this guard
                // (a registered mutator) excludes collection until the
                // operand stack, a root, holds it.
                vm.push(value);
                return VmNeed::Continue;
            }
            Terminal::Failed(error) => (
                VmErrorKind::DependencyFailed(key),
                format!("dependency {key} failed with {}", describe(error)),
            ),
            Terminal::Cancelled => (
                VmErrorKind::DependencyCancelled(key),
                format!("dependency {key} was cancelled"),
            ),
        };
        VmNeed::Fail(Box::new(vm.failure(kind, message, pc)))
    }

    /// Abandon this run's pending need without cancelling the task: clear the
    /// task's unresolved wait and remove its waiter entry from that cell, as
    /// consumer cancellation's cleanup does; recorded edges stay. For a
    /// scheduler that refuses the wait (a wait cycle, [`crate::dep::DependencyGraph`]):
    /// the task will not be woken by that cell, and a later
    /// [`RunGuard::park`] in this run yields instead of parking. The cell, its
    /// producer and its other waiters are unchanged.
    pub fn abandon_wait(&mut self) {
        dep::withdraw(&self.task.deps, &self.task.log, self.task.id);
        self.subscribed = None;
    }

    /// End this run suspended on the dependency this run's last need found
    /// pending, and release the guard.
    ///
    /// Returns [`RunOutcome::Waiting`] if the task parked: it cannot be run
    /// until the completion's wake makes it runnable, and that completion
    /// returns it in [`Wakeups::into_runnable`](crate::Wakeups::into_runnable)
    /// exactly once. Returns [`RunOutcome::Yielded`], with the task runnable,
    /// if the result was published after the need subscribed but before this
    /// park (the wake was remembered), or if this run has no pending need
    /// (nothing to wait for): the caller requeues the task itself, and its
    /// next run reads the result. The task's VM state is kept either way.
    ///
    /// A wake remembered for an earlier subscription of this same run (a
    /// need that was pending, then completed and was needed again) also
    /// makes the park yield. That resume is spurious but harmless: the next
    /// run needs the key again, finds it pending, and parks with a fresh
    /// handle.
    ///
    /// Returns [`RunOutcome::Cancelled`] if the task's cancellation has been
    /// requested, at any point up to the park's CAS (an observation point;
    /// it takes precedence over a remembered wake): the run performs
    /// terminal cleanup instead of parking or yielding.
    pub fn park(self) -> RunOutcome {
        if self.task.exec.snapshot().cancelled {
            return self.end_cancelled();
        }
        let RunGuard {
            task,
            body,
            mut claim,
            subscribed,
            _mutator,
        } = self;
        drop(body);
        let Some(key) = subscribed else {
            // A cancellation arriving now stays requested for the next run.
            claim.end(End::Runnable);
            return RunOutcome::Yielded;
        };
        match claim.park(|| task.deps.hook(HookPoint::ParkBeforeCas)) {
            Park::Parked => RunOutcome::Waiting(key),
            Park::AlreadyWoken => RunOutcome::Yielded,
            Park::Cancelled => {
                // Cancelled after the body was released: the execution is
                // still ours, so nobody else can hold the body for long
                // (only a diagnostic reader, for an instant).
                let mut body = task.body.lock().unwrap_or_else(PoisonError::into_inner);
                cleanup_cancelled(task, &mut body);
                drop(body);
                claim.end(End::Cancelled);
                RunOutcome::Cancelled
            }
        }
    }

    /// True once the task's cancellation has been requested: the run should
    /// end, and [`RunGuard::park`] or [`RunGuard::run_with`] then performs
    /// terminal cleanup. A snapshot (module docs, "Consumer cancellation").
    pub fn cancel_requested(&self) -> bool {
        self.task.exec.snapshot().cancelled
    }

    /// Observe the cancellation: terminal cleanup, then end the execution.
    fn end_cancelled(self) -> RunOutcome {
        let RunGuard {
            task,
            mut body,
            claim,
            _mutator,
            ..
        } = self;
        cleanup_cancelled(task, &mut body);
        drop(body);
        claim.end(End::Cancelled);
        RunOutcome::Cancelled
    }

    /// Execute until the task returns or fails, then release the guard.
    /// Unlimited budget, no host: see [`Task::run`].
    pub fn run(self) -> RunOutcome {
        self.run_with(Budget::UNLIMITED, &mut NoHost)
    }

    /// Execute until the task returns or fails or `budget` is used up, then
    /// release the guard. See [`Task::run_with`].
    ///
    /// Cancellation is observed at run start (nothing executes) and when the
    /// budget runs out (instead of yielding); either way the run performs
    /// terminal cleanup and returns [`RunOutcome::Cancelled`]. A run that
    /// returns or fails first reports that outcome and the request is moot.
    /// The dispatch loop does not poll for cancellation between
    /// instructions; a bounded budget bounds how long a request waits.
    pub fn run_with(mut self, budget: Budget, host: &mut dyn Host) -> RunOutcome {
        let task = self.task;
        if task.exec.snapshot().cancelled {
            return self.end_cancelled();
        }
        let Some(vm) = self.vm() else {
            // `try_begin_run` only hands out guards for ready tasks, and only
            // `run_with` (which consumes the guard) changes the body.
            return RunOutcome::Failed(guard_failure(
                task,
                VmErrorKind::Internal,
                "run guard held for a finished task",
                0,
            ));
        };
        let outcome = if crate::host::inside_effect() {
            // Reached only with a guard acquired before the effect started
            // (`try_begin_run` refuses inside an effect). Fail this task rather
            // than reenter the VM from a leaf effect.
            RunOutcome::Failed(vm.failure(
                VmErrorKind::HostEffect,
                "a host effect may not reenter the VM".to_string(),
                vm.top().pc,
            ))
        } else {
            let mut budget = budget.0;
            let exit = loop {
                let Some(vm) = self.vm() else {
                    // Unreachable: checked above, and nothing in this loop
                    // finishes the body.
                    return RunOutcome::Failed(guard_failure(
                        task,
                        VmErrorKind::Internal,
                        "run guard held for a finished task",
                        0,
                    ));
                };
                // A task suspended at a `Need` resumes by completing that
                // instruction: the same read, now normally finding the cell
                // complete (module docs, "Dependency waits in the VM").
                let exit = match vm.pending_need {
                    Some(PendingNeed { key, pc }) => Exit::Need {
                        dependency: key.dependency(),
                        kind: key.kind(),
                        pc,
                        budget,
                    },
                    None => execute(vm, &task.heap, host, budget),
                };
                let Exit::Need {
                    dependency,
                    kind,
                    pc,
                    budget: left,
                } = exit
                else {
                    break exit;
                };
                budget = left;
                match self.vm_need(dependency, kind, pc) {
                    VmNeed::Continue => {}
                    VmNeed::Fail(error) => break Exit::Fail(*error),
                    // The same suspension path as an embedder's need: park
                    // on the subscribed key (or yield, or observe a
                    // cancellation), keeping the VM state in the task.
                    VmNeed::Park => return self.park(),
                    VmNeed::Cancelled => return self.end_cancelled(),
                }
            };
            // The returned value is rooted while this guard (a registered
            // mutator) still exists, with no safepoint in between, so no
            // collection can run while the value is held only in a Rust local.
            match exit {
                Exit::Return(value) => RunOutcome::Complete(Rooted::new(&task.heap, value)),
                Exit::Fail(error) => RunOutcome::Failed(error),
                // The budget boundary is an observation point.
                Exit::Yield if task.exec.snapshot().cancelled => {
                    return self.end_cancelled();
                }
                // The VM state stays in the task; dropping the guard releases
                // the body and makes the task runnable.
                Exit::Yield => return RunOutcome::Yielded,
                // Unreachable: the loop above handles every need.
                Exit::Need { pc, .. } => RunOutcome::Failed(guard_failure(
                    task,
                    VmErrorKind::Internal,
                    "need left unhandled",
                    pc,
                )),
            }
        };
        // Completion or failure releases the task's frames and its open
        // transient builders; a returned value stays alive through its
        // persistent root, not through the task.
        if let Some(vm) = self.vm() {
            vm.builders.release_all(&task.heap, task.id);
        }
        *self.body = TaskBody::Finished;
        let RunGuard {
            body,
            claim,
            _mutator,
            ..
        } = self;
        drop(body);
        claim.end(End::Finished);
        outcome
    }
}

/// What a VM `Need` did ([`RunGuard::vm_need`]).
enum VmNeed {
    /// The result was pushed; execution continues after the instruction.
    Continue,
    /// The task fails at the instruction (a failed or cancelled result).
    Fail(Box<VmError>),
    /// The cell is pending and the task subscribed: end the run parked.
    Park,
    /// The task's cancellation was observed: end the run cancelled.
    Cancelled,
}

/// A dependency's error value for a diagnostic message.
fn describe(value: Value) -> String {
    match value.to_primitive() {
        Some(p) => format!("{p:?}"),
        None => format!("an {}", value.type_name()),
    }
}

#[cold]
fn guard_failure(task: &TaskInner, kind: VmErrorKind, message: &str, pc: usize) -> VmError {
    let code = &task.executable.0;
    VmError {
        kind,
        message: message.to_string(),
        diagnostic: crate::error::Diagnostic {
            executable: code.name.clone(),
            task: task.id,
            world: task.world,
            instruction: pc,
            byte_offset: code.offsets.get(pc).copied(),
            opcode: code.instrs.get(pc).map(|i| i.opcode()),
            source: code.location(pc),
            call_chain: Vec::new(),
        },
    }
}

/// Early VM tests: hand-assembled bytecode, not JavaScript source support.
#[cfg(test)]
mod tests {
    use super::*;

    use crate::value::{Primitive, ValueView};
    use crate::vm::executable::Code;
    use crate::vm::instr::Instr;
    use crate::vm::{Assembler, Opcode};
    use crate::{Runtime, ValidationErrorKind};

    fn num(n: f64) -> Primitive {
        Primitive::Number(n)
    }

    fn complete(outcome: RunOutcome) -> ValueView {
        match outcome {
            RunOutcome::Complete(v) => v.view(),
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    fn failed(outcome: RunOutcome) -> VmError {
        match outcome {
            RunOutcome::Failed(e) => e,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// `function f(n) { let acc = 1; while (0 < n) { acc = acc * n; n = n - 1; } return acc; }`
    fn factorial() -> Executable {
        let mut asm = Assembler::new();
        let (top, done) = (asm.label(), asm.label());
        asm.constant(num(1.0)).set_local(1);
        asm.bind(top);
        asm.constant(num(0.0))
            .get_local(0)
            .op(Opcode::Lt)
            .jump_if_false(done);
        asm.get_local(1).get_local(0).op(Opcode::Mul).set_local(1);
        asm.get_local(0)
            .constant(num(1.0))
            .op(Opcode::Sub)
            .set_local(0);
        asm.jump(top);
        asm.bind(done);
        asm.get_local(1).op(Opcode::Return);
        asm.finish("factorial", 1, 2).unwrap()
    }

    #[test]
    fn completes_with_constants_locals_branches_and_return() {
        let rt = Runtime::new();
        let five = rt.root(num(5.0));
        let task = rt.spawn(&factorial(), &[&five]).unwrap();
        assert_eq!(task.status(), TaskStatus::Ready);
        assert_eq!(complete(task.run().unwrap()), ValueView::Number(120.0));
        assert_eq!(task.status(), TaskStatus::Finished);
        assert_eq!(task.run().unwrap_err(), RunError::Finished);
    }

    #[test]
    fn primitive_operations_follow_documented_semantics() {
        use Opcode::*;
        let cases: Vec<(Vec<Primitive>, Vec<Opcode>, ValueView)> = vec![
            (vec![num(7.0), num(2.0)], vec![Sub], ValueView::Number(5.0)),
            (vec![num(1.0), num(2.0)], vec![Lt], ValueView::Bool(true)),
            (vec![num(2.0), num(1.0)], vec![Lt], ValueView::Bool(false)),
            (
                vec![num(0.0), num(-0.0)],
                vec![StrictEq],
                ValueView::Bool(true),
            ),
            (
                vec![num(f64::NAN)],
                vec![Dup, StrictEq],
                ValueView::Bool(false),
            ),
            (
                vec![Primitive::Null, Primitive::Undefined],
                vec![StrictEq],
                ValueView::Bool(false),
            ),
            (vec![num(0.0)], vec![Not], ValueView::Bool(true)),
            (
                vec![Primitive::Null, Primitive::Bool(true)],
                vec![Pop, Not],
                ValueView::Bool(true),
            ),
        ];
        let rt = Runtime::new();
        for (consts, ops, expected) in cases {
            let mut asm = Assembler::new();
            for c in &consts {
                asm.constant(*c);
            }
            for op in &ops {
                asm.op(*op);
            }
            asm.op(Return);
            let exe = asm.finish("case", 0, 0).unwrap();
            let got = complete(rt.spawn(&exe, &[]).unwrap().run().unwrap());
            assert_eq!(got, expected, "{consts:?} {ops:?}");
        }
    }

    #[test]
    fn new_object_identity_and_strict_equality() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.op(Opcode::NewObject).set_local(0);
        // local1 = (local0 === local0)  -> true
        asm.get_local(0)
            .get_local(0)
            .op(Opcode::StrictEq)
            .set_local(1);
        // (local0 === {}) -> false; return that === !local1
        asm.get_local(0).op(Opcode::NewObject).op(Opcode::StrictEq);
        asm.get_local(1)
            .op(Opcode::Not)
            .op(Opcode::StrictEq)
            .op(Opcode::Return);
        let exe = asm.finish("identity", 0, 2).unwrap();
        let outcome = rt.spawn(&exe, &[]).unwrap().run().unwrap();
        assert_eq!(complete(outcome), ValueView::Bool(true));
        assert_eq!(rt.heap_stats().objects, 2);
    }

    #[test]
    fn runtime_failure_reports_diagnostic_context() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.constant(num(1.0)).op(Opcode::NewObject).op(Opcode::Add);
        asm.op(Opcode::Return);
        let exe = asm.finish("bad-add", 0, 0).unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        let err = failed(task.run().unwrap());
        assert_eq!(err.kind(), VmErrorKind::UnsupportedOperand);
        assert!(
            err.message().contains("number and object"),
            "{}",
            err.message()
        );
        let d = err.diagnostic();
        assert_eq!(&*d.executable, "bad-add");
        assert_eq!(d.instruction, 2);
        assert_eq!(d.byte_offset, Some(4));
        assert_eq!(d.opcode, Some(Opcode::Add));
        assert_eq!((d.task, d.world), (task.id(), task.world()));
        assert_eq!(task.status(), TaskStatus::Finished);
        assert_eq!(task.run().unwrap_err(), RunError::Finished);
    }

    #[test]
    fn tasks_have_independent_state_over_shared_code() {
        let rt = Runtime::new();
        let exe = factorial();
        let (three, four) = (rt.root(num(3.0)), rt.root(num(4.0)));
        let a = rt.spawn(&exe, &[&three]).unwrap();
        let b = rt.spawn(&exe, &[&four]).unwrap();
        let c = rt.spawn(&exe, &[&three]).unwrap();
        assert!(Executable::same_code(a.executable(), b.executable()));
        assert_ne!(a.id(), b.id());
        // Run in a different order from spawn; each sees only its own locals.
        assert_eq!(complete(b.run().unwrap()), ValueView::Number(24.0));
        assert_eq!(c.status(), TaskStatus::Ready);
        assert_eq!(complete(a.run().unwrap()), ValueView::Number(6.0));
        assert_eq!(complete(c.run().unwrap()), ValueView::Number(6.0));
    }

    /// The opcodes added for the source compiler, on operands source code
    /// cannot produce yet (objects), and `throw_error` without source text.
    #[test]
    fn compiler_opcodes_reject_objects_and_throw_without_source() {
        use Opcode::*;
        let rt = Runtime::new();
        let run = |build: &dyn Fn(&mut Assembler)| {
            let mut asm = Assembler::new();
            build(&mut asm);
            asm.op(Return);
            let exe = asm.finish("ops", 0, 1).unwrap();
            rt.spawn(&exe, &[]).unwrap().run().unwrap()
        };
        let unsupported: [&dyn Fn(&mut Assembler); 5] = [
            &|a| {
                a.op(NewObject).op(ToNumber);
            },
            &|a| {
                a.op(NewObject).op(Neg);
            },
            &|a| {
                a.constant(num(1.0)).op(NewObject).op(ToNumberPair).op(Pop);
            },
            &|a| {
                a.op(NewObject).constant(num(2.0)).op(Div);
            },
            &|a| {
                a.op(NewObject).constant(Primitive::Bool(true)).op(LooseEq);
            },
        ];
        for build in unsupported {
            assert_eq!(failed(run(build)).kind(), VmErrorKind::UnsupportedOperand);
        }
        // `==` on objects is identity; an object never equals null.
        let same = run(&|a| {
            a.op(NewObject).set_local(0).get_local(0).get_local(0);
            a.op(LooseEq);
        });
        assert_eq!(complete(same), ValueView::Bool(true));
        let null = run(&|a| {
            a.op(NewObject).constant(Primitive::Null).op(LooseEq);
        });
        assert_eq!(complete(null), ValueView::Bool(false));
        // `neg` is number-only (a compiler emits `to_number` first).
        let neg = run(&|a| {
            a.constant(Primitive::Bool(true)).op(Neg);
        });
        assert_eq!(failed(neg).kind(), VmErrorKind::UnsupportedOperand);

        let thrown = failed(run(&|a| {
            a.op(ThrowError).raw(2).raw(0);
        }));
        assert_eq!(thrown.kind(), VmErrorKind::TypeError);
        assert_eq!(thrown.message(), "assignment to a constant variable");
        assert_eq!(thrown.diagnostic().opcode, Some(ThrowError));
        assert_eq!(thrown.diagnostic().source, None);
    }

    #[test]
    fn missing_arguments_are_undefined_and_extra_ones_ignored() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.get_local(1).op(Opcode::Return);
        let exe = asm.finish("second-param", 2, 2).unwrap();
        let one = rt.root(num(1.0));
        let t = rt.spawn(&exe, &[&one]).unwrap();
        assert_eq!(complete(t.run().unwrap()), ValueView::Undefined);
        let t = rt.spawn(&exe, &[&one, &one, &one]).unwrap();
        assert_eq!(complete(t.run().unwrap()), ValueView::Number(1.0));
    }

    #[test]
    fn overlapping_run_is_rejected_by_the_execution_guard() {
        let rt = Runtime::new();
        let task = rt.spawn(&factorial(), &[]).unwrap();
        let alias = task.clone();
        let guard = task.try_begin_run().unwrap();
        assert_eq!(alias.status(), TaskStatus::Running);
        assert_eq!(alias.try_begin_run().err(), Some(RunError::AlreadyRunning));
        assert_eq!(alias.run().unwrap_err(), RunError::AlreadyRunning);
        assert_eq!(guard.task_id(), task.id());
        // factorial(undefined): `0 < undefined` is an unsupported operand.
        assert_eq!(failed(guard.run()).kind(), VmErrorKind::UnsupportedOperand);
        assert_eq!(alias.try_begin_run().err(), Some(RunError::Finished));
    }

    #[test]
    fn dropping_unused_guard_keeps_task_runnable() {
        let rt = Runtime::new();
        let four = rt.root(num(4.0));
        let task = rt.spawn(&factorial(), &[&four]).unwrap();
        let guard = task.try_begin_run().unwrap();
        assert_eq!(task.try_begin_run().err(), Some(RunError::AlreadyRunning));
        drop(guard);
        assert_eq!(task.status(), TaskStatus::Ready);
        let guard = task.try_begin_run().unwrap();
        assert_eq!(complete(guard.run()), ValueView::Number(24.0));
    }

    #[test]
    fn task_frames_are_traceable_until_the_task_finishes() {
        let rt = Runtime::new();
        let obj = rt.scope(|s| {
            let o = s.new_object();
            s.persist(o)
        });
        let mut asm = Assembler::new();
        asm.get_local(0).op(Opcode::Return);
        let task = rt.spawn(&asm.finish("id", 1, 1).unwrap(), &[&obj]).unwrap();
        let mut seen = Vec::new();
        assert!(task.trace_frames(&mut |o| seen.push(o)));
        assert_eq!(seen.len(), 1);
        let guard = task.try_begin_run().unwrap();
        assert!(!task.trace_frames(&mut |_| {}));
        let result = guard.run();
        seen.clear();
        assert!(task.trace_frames(&mut |o| seen.push(o)));
        assert!(seen.is_empty());
        match result {
            RunOutcome::Complete(r) => assert!(r.same_object(&obj)),
            other => panic!("{other:?}"),
        }
    }

    /// Corrupted code that bypasses validation still fails cleanly.
    #[test]
    fn invariant_violations_fail_the_task_instead_of_panicking() {
        let programs: Vec<Vec<Instr>> = vec![
            vec![Instr::Pop],
            vec![Instr::Add],
            vec![Instr::Dup],
            vec![Instr::Return],
            vec![Instr::Const(9)],
            vec![Instr::GetLocal(9)],
            vec![Instr::Undefined, Instr::SetLocal(9)],
            vec![Instr::Jump(99)],
            vec![Instr::Undefined],
        ];
        let rt = Runtime::new();
        for instrs in programs {
            let code = Code {
                name: "corrupt".into(),
                params: 0,
                locals: 0,
                constants: Box::new([]),
                functions: Box::new([]),
                captures: 0,
                offsets: (0..instrs.len() as u32).collect(),
                instrs: instrs.clone().into(),
                max_stack: 0,
                debug: None,
            };
            let exe = Executable(Arc::new(code));
            let err = failed(rt.spawn(&exe, &[]).unwrap().run().unwrap());
            assert_eq!(err.kind(), VmErrorKind::Internal, "{instrs:?}");
        }
    }

    #[test]
    fn malformed_bytecode_never_reaches_a_task() {
        let mut asm = Assembler::new();
        asm.raw(0xEE).op(Opcode::Return);
        let e = asm.finish("bad", 0, 0).unwrap_err();
        assert_eq!(e.kind, ValidationErrorKind::UnknownOpcode { byte: 0xEE });
        let mut asm = Assembler::new();
        let never_bound = asm.label();
        asm.jump(never_bound);
        let e = asm.finish("bad", 0, 0).unwrap_err();
        assert_eq!(
            e.kind,
            ValidationErrorKind::BadBranchTarget { target: u32::MAX }
        );
    }
}

/// Budgeted yield/resume and leaf host effects. Early VM tests: hand-assembled
/// bytecode, not JavaScript source support.
#[cfg(test)]
mod resume_tests {
    use super::*;
    use crate::host::HostCall;
    use crate::value::{Primitive, ValueView};
    use crate::vm::{Assembler, Opcode};
    use crate::{Runtime, TaskId};

    fn num(n: f64) -> Value {
        Value::Number(n)
    }

    fn complete(outcome: RunOutcome) -> ValueView {
        match outcome {
            RunOutcome::Complete(v) => v.view(),
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    fn failed(outcome: RunOutcome) -> VmError {
        match outcome {
            RunOutcome::Failed(e) => e,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    fn yielded(outcome: RunOutcome) {
        assert!(matches!(outcome, RunOutcome::Yielded), "{outcome:?}");
    }

    /// Host whose effect increments a counter and returns its new value.
    #[derive(Default)]
    struct Counter {
        log: Vec<HostCall>,
    }

    impl Host for Counter {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            self.log.push(call);
            Ok(Primitive::Number(self.log.len() as f64))
        }
    }

    impl Counter {
        fn args_of(&self, task: TaskId) -> Vec<f64> {
            self.log
                .iter()
                .filter(|c| c.task == task)
                .map(|c| match c.arg {
                    Primitive::Number(n) => n,
                    other => panic!("{other:?}"),
                })
                .collect()
        }
    }

    const EFFECT_OP: u16 = 7;
    /// Instruction index of the loop head (`top`) in [`effect_loop`].
    const TOP: usize = 4;
    /// Instruction index of the loop exit (`done`) in [`effect_loop`].
    const DONE: usize = 16;

    /// `function f(n) { let i = 0, last; while (i < n) { last = effect(i); i = i + 1; } return last; }`
    ///
    /// Locals: 0 = n, 1 = i, 2 = last. Executes `4 + 12 n + 6` instructions.
    fn effect_loop() -> Executable {
        let mut asm = Assembler::new();
        let (top, done) = (asm.label(), asm.label());
        asm.constant(Primitive::Number(0.0)).set_local(1); // 0, 1
        asm.op(Opcode::Undefined).set_local(2); // 2, 3
        asm.bind(top); // 4
        asm.get_local(1).get_local(0).op(Opcode::Lt); // 4, 5, 6
        asm.jump_if_false(done); // 7
        asm.get_local(1).host_effect(EFFECT_OP).set_local(2); // 8, 9, 10
        asm.get_local(1)
            .constant(Primitive::Number(1.0))
            .op(Opcode::Add)
            .set_local(1); // 11..14
        asm.jump(top); // 15
        asm.bind(done); // 16
        asm.get_local(2).op(Opcode::Return); // 16, 17
        asm.finish("effect-loop", 1, 3).unwrap()
    }

    fn effect_loop_len(n: u64) -> u64 {
        4 + 12 * n + 6
    }

    #[test]
    fn yield_resumes_at_the_saved_instruction() {
        // (2 + 3) -> local 0; return local0 * 4
        let mut asm = Assembler::new();
        asm.constant(Primitive::Number(2.0))
            .constant(Primitive::Number(3.0))
            .op(Opcode::Add)
            .set_local(0);
        asm.get_local(0)
            .constant(Primitive::Number(4.0))
            .op(Opcode::Mul)
            .op(Opcode::Return);
        let exe = asm.finish("straight", 0, 1).unwrap();
        let rt = Runtime::new();
        let task = rt.spawn(&exe, &[]).unwrap();
        let mut host = Counter::default();

        yielded(task.run_with(Budget::instructions(2), &mut host).unwrap());
        assert_eq!(task.status(), TaskStatus::Ready);
        let (pc, operands, locals) = task.saved_state().unwrap();
        assert_eq!(pc, 2);
        assert_eq!(operands, vec![num(2.0), num(3.0)]);
        assert_eq!(locals, vec![Value::Undefined]);

        yielded(task.run_with(Budget::instructions(3), &mut host).unwrap());
        let (pc, operands, locals) = task.saved_state().unwrap();
        assert_eq!(pc, 5);
        assert_eq!(operands, vec![num(5.0)]);
        assert_eq!(locals, vec![num(5.0)]);

        let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(20.0));
        assert_eq!(task.status(), TaskStatus::Finished);
        assert!(task.saved_state().is_none());
    }

    #[test]
    fn zero_budget_yields_without_progress() {
        let rt = Runtime::new();
        let n = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&n]).unwrap();
        let mut host = Counter::default();
        for _ in 0..3 {
            yielded(task.run_with(Budget::instructions(0), &mut host).unwrap());
            assert_eq!(task.saved_state().unwrap().0, 0);
        }
        assert!(host.log.is_empty());
        let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(1.0));
    }

    /// Every possible yield point: for each budget, the run sequence executes
    /// each instruction exactly once (the run count is exact) and each effect
    /// exactly once, in program order.
    #[test]
    fn host_effects_run_once_across_yields() {
        let n = 4;
        let total = effect_loop_len(n);
        let rt = Runtime::new();
        let exe = effect_loop();
        let arg = rt.root(Primitive::Number(n as f64));
        for b in 1..=total + 1 {
            let task = rt.spawn(&exe, &[&arg]).unwrap();
            let mut host = Counter::default();
            let mut runs = 0;
            let result = loop {
                runs += 1;
                match task.run_with(Budget::instructions(b), &mut host).unwrap() {
                    RunOutcome::Yielded => continue,
                    other => break complete(other),
                }
            };
            assert_eq!(runs, total.div_ceil(b), "budget {b}");
            assert_eq!(
                host.args_of(task.id()),
                vec![0.0, 1.0, 2.0, 3.0],
                "budget {b}"
            );
            assert!(host.log.iter().all(|c| c.op == EFFECT_OP));
            // The last effect returned the counter value 4.
            assert_eq!(result, ValueView::Number(4.0), "budget {b}");
        }
    }

    #[test]
    fn budget_exhaustion_at_branch_targets() {
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let mut host = Counter::default();

        // Setup plus one iteration ends with the backward `jump top`.
        yielded(task.run_with(Budget::instructions(16), &mut host).unwrap());
        let (pc, operands, locals) = task.saved_state().unwrap();
        assert_eq!(pc, TOP, "stopped at the backward branch target");
        assert!(operands.is_empty());
        assert_eq!(locals, vec![num(1.0), num(1.0), num(1.0)]);
        assert_eq!(host.log.len(), 1);

        // `1 < 1` is false: the taken `jump_if_false` lands on `done`.
        yielded(task.run_with(Budget::instructions(4), &mut host).unwrap());
        let (pc, operands, _) = task.saved_state().unwrap();
        assert_eq!(pc, DONE, "stopped at the forward branch target");
        assert!(operands.is_empty());

        let outcome = task.run_with(Budget::instructions(2), &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(1.0));
        assert_eq!(host.log.len(), 1, "no effect replayed after branch yields");
    }

    #[test]
    fn independent_tasks_resume_in_interleaved_order() {
        let rt = Runtime::new();
        let exe = effect_loop();
        let sizes = [2.0, 3.0, 5.0];
        let roots: Vec<Rooted> = sizes
            .iter()
            .map(|&n| rt.root(Primitive::Number(n)))
            .collect();
        let tasks: Vec<Task> = roots
            .iter()
            .map(|r| rt.spawn(&exe, &[r]).unwrap())
            .collect();
        let mut host = Counter::default();
        let mut results: Vec<Option<ValueView>> = vec![None; tasks.len()];
        let mut round = 0;
        while results.iter().any(Option::is_none) {
            // Rotate and alternate direction so the run order differs per round.
            let mut order: Vec<usize> = (0..tasks.len())
                .map(|i| (i + round) % tasks.len())
                .collect();
            if round % 2 == 1 {
                order.reverse();
            }
            for i in order {
                if results[i].is_some() {
                    continue;
                }
                match tasks[i]
                    .run_with(Budget::instructions(7), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {}
                    other => results[i] = Some(complete(other)),
                }
            }
            round += 1;
            assert!(round < 100, "tasks failed to finish");
        }
        for (task, &n) in tasks.iter().zip(&sizes) {
            let expected: Vec<f64> = (0..n as usize).map(|i| i as f64).collect();
            assert_eq!(host.args_of(task.id()), expected);
        }
        // Effects really interleaved: the log does not run task by task.
        let switches = host
            .log
            .windows(2)
            .filter(|w| w[0].task != w[1].task)
            .count();
        assert!(switches > tasks.len(), "{switches} switches");
        assert_eq!(host.log.len(), 10);
        // Each task's result is the shared counter value at its last effect.
        for (i, task) in tasks.iter().enumerate() {
            let last = host.log.iter().rposition(|c| c.task == task.id()).unwrap();
            assert_eq!(results[i], Some(ValueView::Number((last + 1) as f64)));
        }
    }

    #[test]
    fn yielded_state_stays_traceable_and_owned_by_the_task() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.op(Opcode::NewObject).set_local(0);
        asm.get_local(0).op(Opcode::Return);
        let task = rt.spawn(&asm.finish("hold", 0, 1).unwrap(), &[]).unwrap();
        yielded(
            task.run_with(Budget::instructions(2), &mut Counter::default())
                .unwrap(),
        );
        let mut seen = Vec::new();
        assert!(task.trace_frames(&mut |o| seen.push(o)));
        assert_eq!(
            seen.len(),
            1,
            "the object is reachable from the saved locals"
        );
        let r = match task.run().unwrap() {
            RunOutcome::Complete(r) => r,
            other => panic!("{other:?}"),
        };
        assert_eq!(r.view(), ValueView::Object);
        assert_eq!(rt.heap_stats().objects, 1);
    }

    #[test]
    fn host_errors_and_missing_host_fail_the_task() {
        struct Refuse;
        impl Host for Refuse {
            fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
                Err(format!("refused op {}", call.op))
            }
        }
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let err = failed(task.run_with(Budget::UNLIMITED, &mut Refuse).unwrap());
        assert_eq!(err.kind(), VmErrorKind::HostEffect);
        assert_eq!(err.message(), "refused op 7");
        assert_eq!(err.diagnostic().instruction, 9);
        assert_eq!(err.diagnostic().opcode, Some(Opcode::HostEffect));
        assert_eq!(task.status(), TaskStatus::Finished);

        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let err = failed(task.run().unwrap());
        assert_eq!(err.kind(), VmErrorKind::HostEffect);
        assert!(err.message().contains("no host"), "{}", err.message());
    }

    #[test]
    fn panicking_host_clears_the_effect_guard_and_poisons_only_its_task() {
        struct Panic;
        impl Host for Panic {
            fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
                panic!("host bug")
            }
        }
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let run = std::panic::catch_unwind(|| task.run_with(Budget::UNLIMITED, &mut Panic));
        assert!(run.is_err());
        assert!(!crate::host::inside_effect(), "flag cleared on unwind");
        assert_eq!(task.run().unwrap_err(), RunError::Poisoned);
        // The same thread can run other tasks and open heap scopes again.
        let other = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let mut host = Counter::default();
        let outcome = other.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(1.0));
        rt.scope(|_| ());
    }

    #[test]
    fn host_effect_rejects_object_arguments_before_calling_the_host() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.op(Opcode::NewObject).host_effect(0).op(Opcode::Return);
        let task = rt
            .spawn(&asm.finish("obj-effect", 0, 0).unwrap(), &[])
            .unwrap();
        let mut host = Counter::default();
        let err = failed(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
        assert_eq!(err.kind(), VmErrorKind::UnsupportedOperand);
        assert!(host.log.is_empty());
    }

    #[test]
    fn host_effects_cannot_reenter_the_vm_or_open_heap_scopes() {
        struct Reenter<'t> {
            rt: Runtime,
            own: Task,
            other: Task,
            other_guard: Option<RunGuard<'t>>,
            seen: Vec<String>,
        }
        impl Host for Reenter<'_> {
            fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
                self.seen.push(format!("{:?}", self.own.run().err()));
                self.seen.push(format!("{:?}", self.other.run().err()));
                let guard = self.other_guard.take().unwrap();
                self.seen.push(format!("{:?}", failed(guard.run()).kind()));
                let rt = self.rt.clone();
                let scope = std::panic::catch_unwind(move || rt.scope(|_| ()));
                self.seen.push(format!("scope ok: {}", scope.is_ok()));
                Ok(Primitive::Null)
            }
        }
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let exe = effect_loop();
        let own = rt.spawn(&exe, &[&one]).unwrap();
        let other = rt.spawn(&exe, &[&one]).unwrap();
        let third = rt.spawn(&exe, &[&one]).unwrap();
        let mut host = Reenter {
            rt: rt.clone(),
            own: own.clone(),
            other: other.clone(),
            other_guard: Some(third.try_begin_run().unwrap()),
            seen: Vec::new(),
        };
        let outcome = own.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Null);
        assert_eq!(
            host.seen,
            [
                "Some(InsideHostEffect)",
                "Some(InsideHostEffect)",
                "HostEffect",
                "scope ok: false",
            ]
        );
        // The untouched task is still runnable once no effect is executing;
        // the one entered through a pre-acquired guard was failed.
        drop(host);
        assert_eq!(other.status(), TaskStatus::Ready);
        assert_eq!(third.status(), TaskStatus::Finished);
        rt.scope(|_| ());
    }
}
