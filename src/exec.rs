//! A small reference executor: worker OS threads driving the embedding run
//! API (`docs/design.md` §§3, 13, 15).
//!
//! The executor is optional host/example code built only on the public task
//! API ([`Task::try_begin_run`], [`RunGuard::run_with`], [`RunGuard::park`],
//! [`Task::cancel`], [`Wakeups`]). The runtime never starts it, owns no
//! threads, and works the same without it: an embedder (a compiler with its
//! own pool) can drive tasks by hand exactly as these workers do.
//!
//! # Model
//!
//! * **Ready queue.** One FIFO of runnable tasks, each entry with a
//!   [`Placement`]: any worker, or one named worker. A worker takes the
//!   first entry it is eligible for. Tasks enter through
//!   [`Executor::submit`] (or [`Executor::submit_with`]), a run that ended
//!   [`RunOutcome::Yielded`] (budget used up, or a park that found its wake
//!   already delivered), [`Executor::wake`] (the runnable tasks of a
//!   completion's [`Wakeups`]), and [`Executor::cancel`] (a parked task the
//!   cancellation made runnable, so its own run cleans up).
//! * **Ownership per run.** A worker acquires the task's execution claim
//!   ([`Task::try_begin_run`]) *before* dispatch and the claim is returned
//!   when the run ends: on yield, park, or a terminal outcome. The claim is
//!   the task's atomic state word, so a task never runs on two workers at
//!   once. The executor also never dispatches one task twice at once: a
//!   queue entry for a task whose previous dispatch has not finished
//!   recording and routing its outcome is ineligible until it has, so the
//!   record shows each run's release before the next run's dispatch. A
//!   claim that fails is recorded ([`EventKind::NotDispatched`]) and routed
//!   by the task's state: still runnable (a diagnostic reader held the task
//!   body for an instant) is requeued ([`EnqueueReason::Retried`]), parked
//!   is kept with the parked tasks, and running elsewhere (an embedder's own
//!   run) or finished drops the entry.
//! * **One runtime.** An executor serves the [`Runtime`] it was started
//!   for; task ids are only unique within a runtime. Tasks of another
//!   runtime are handed back ([`ForeignTasks`]) at every entry point.
//! * **Waiting occupies no worker.** A run that parks ends
//!   ([`RunOutcome::Waiting`]); the worker moves on. The executor keeps the
//!   parked task's handle (dependency cells hold only weak references) until
//!   a wake or cancellation enqueues it, and any eligible worker runs it
//!   then. Its VM state never left the task, so nothing is replayed: the run
//!   continues at the saved instruction.
//! * **Budgets.** Every run gets [`ExecutorConfig::budget`] instructions
//!   (default 10 000). Budgets bound how long a worker is busy with one run,
//!   which is what bounds shutdown and the delay before a running task
//!   observes cancellation (the VM checks it at the budget boundary).
//! * **Driver.** Workers hand the acquired [`RunGuard`] to a driver
//!   ([`ExecutorConfig::driver`]); the default runs the VM
//!   (`guard.run_with(budget, host)`). Until the VM can need a dependency on
//!   its own (pellet clj-quickjs-23) a driver is also where a run reads or
//!   subscribes with [`RunGuard::need`] and parks. A driver must return the
//!   outcome of the run it was given; `Waiting` only from
//!   [`RunGuard::park`].
//! * **Hosts.** Each worker owns one [`Host`] for leaf effects, made on the
//!   worker's thread by [`ExecutorConfig::host`].
//! * **Placement hook.** [`ExecutorConfig::placement`] decides, for each
//!   enqueue, which worker may run the task, given the reason and the worker
//!   that last ran it ([`DispatchRequest`]). It is how a test sends a task
//!   that waited to a *different* worker, deterministically.
//!
//! # Events
//!
//! Every transition is recorded in order ([`Event`]): workers starting (with
//! their OS thread id) and stopping, enqueues (reason and placement),
//! dispatch (claim acquired, with the execution number), release (claim
//! returned, with how the run ended; [`OutcomeKind::Completed`] is useful
//! work finished), and refused claims. [`Executor::wait_for`] blocks with a
//! bound until a predicate over the events holds, so tests synchronize on
//! transitions instead of sleeping. The predicate runs on a snapshot with
//! no executor lock held, so it may call the executor.
//!
//! # Locks
//!
//! Two short mutexes: the scheduling state (queue, parked set, dispatches
//! in flight, last worker per task) and the record (events, terminal
//! outcomes). The record is the innermost lock: it may be taken while the
//! scheduling lock is held (so an enqueue is recorded before any worker can
//! dispatch it), never the other way round. Neither is held while a task
//! runs or suspends, while the driver, a host, the placement hook or a
//! [`Executor::wait_for`] predicate runs, or while a wake or cancellation
//! is delivered. An idle worker waits on the scheduling
//! condition variable with no task claimed. There is no lock around VM
//! execution: two workers execute instructions at the same time.
//!
//! # Shutdown and cancellation
//!
//! [`Executor::begin_shutdown`] stops dispatch: workers finish the run they
//! are in (bounded by the budget) and exit. [`Executor::shutdown`] waits a
//! bounded time for that and returns what the executor still held: the
//! queued tasks that were never run again ([`Shutdown::pending`], still
//! runnable, VM state intact) and the parked ones ([`Shutdown::parked`]),
//! so the embedder can run, cancel or drop them itself. A worker still busy
//! at the deadline (a run with an unlimited budget, or a blocking host
//! effect) makes it return [`ShutdownTimeout`], which names the busy
//! workers and gives the executor back to retry. [`Executor::cancel`]
//! requests a task's cancellation ([`Task::cancel`]) and enqueues it if it
//! was parked; the task's own next run cleans up and ends
//! [`RunOutcome::Cancelled`]. Dropping an executor begins shutdown and
//! detaches its workers.
//!
//! # Native jobs
//!
//! [`Executor::submit_native`] queues native work next to tasks: a worker
//! takes it in queue order (with the same placement rules) and calls it on
//! its own thread, outside any run and with no executor lock held. Native
//! heap access inside a job goes through [`Runtime::scope`], the no-GC
//! access scope of the safepoint contract.
//!
//! # Collection
//!
//! Workers and the collector cooperate through the safepoint contract of
//! [`crate::heap::gc`]; any thread may request a collection
//! ([`Runtime::request_collection`]), and [`Executor::schedule_collection`]
//! has a worker request one.
//!
//! * **Workers are registered.** Each worker holds a
//!   [`ThreadRegistration`](crate::ThreadRegistration) for its lifetime, so
//!   every collection names it and how it stopped
//!   ([`MutatorState`]). The registration is not heap access: a worker
//!   enters heap access only through a run guard (dispatch), or a scope or
//!   atom operation of a native job.
//! * **Running tasks** stop at the VM's safepoints with their frames
//!   reported ([`MutatorState::Safepoint`]). A leaf host effect runs inside
//!   its run's heap access, so one that blocks (as the tests' gates do)
//!   delays a request until it returns.
//! * **Native scopes** held by a job delay a request until the job closes
//!   or releases them ([`MutatorState::Released`]).
//! * **Idle workers never delay a collection.** A worker waiting for the
//!   ready queue, or routing an outcome, holds no heap access
//!   ([`MutatorState::Idle`]). One that takes a task while a request is
//!   pending waits in [`Task::try_begin_run`] (entering heap access) with no
//!   executor lock held, and runs the task after the collection.
//! * **Roots.** Every task handle is a root source
//!   (a `RootSource`, [`crate::heap::gc`]): queued, parked (kept by the
//!   executor; cells hold weak references), and in-flight tasks are traced
//!   through their saved frames, and a running task through the frames its
//!   run reported at the safepoint. Completed dependency cells report their
//!   results, and the executor's terminal outcomes are [`Rooted`](crate::Rooted)
//!   handles. The collector takes none of the executor's locks, and no
//!   worker holds the scheduling or record lock while it enters heap access
//!   or requests a collection, so the rendezvous never waits on them.
//! * **Events.** The executor observes its runtime's collections
//!   ([`Runtime::observe_collections`]) and records each as
//!   [`EventKind::Collected`]: the requesting worker (if any), every
//!   participant with its [`MutatorState`], and the [`CollectStats`]
//!   (reclaimed count). It is recorded by the collecting thread after the
//!   rendezvous ended, so events of runs that resumed may precede it.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use crate::dep::Wakeups;
use crate::heap::{CollectStats, CollectionObserver, CollectionReport, MutatorState};
use crate::host::{Host, NoHost};
use crate::ids::{DependencyKey, TaskId};
use crate::runtime::Runtime;
use crate::task::{Budget, Cancel, RunError, RunGuard, RunOutcome, Task, TaskStatus};

/// Tasks of another runtime than the executor's, handed back unscheduled.
#[derive(Debug)]
pub struct ForeignTasks(pub Vec<Task>);

impl fmt::Display for ForeignTasks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} task(s) belong to another runtime than the executor's",
            self.0.len()
        )
    }
}

impl std::error::Error for ForeignTasks {}

/// Index of one executor worker thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkerId(usize);

impl WorkerId {
    pub const fn new(index: usize) -> WorkerId {
        WorkerId(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "worker {}", self.0)
    }
}

/// Which workers may run a queued task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// The first idle worker.
    Any,
    /// Only this worker. A worker index the executor does not have is
    /// treated as [`Placement::Any`].
    Worker(WorkerId),
}

/// Why a task entered the ready queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueReason {
    /// [`Executor::submit`] or [`Executor::submit_with`].
    Submitted,
    /// Its run ended [`RunOutcome::Yielded`].
    Yielded,
    /// A completion's wake made it runnable ([`Executor::wake`]).
    Woken,
    /// Its cancellation made it runnable ([`Executor::cancel`]).
    Cancelled,
    /// A claim failed while the task stayed runnable (module docs,
    /// "Ownership per run").
    Retried,
}

/// Input to the placement hook ([`ExecutorConfig::placement`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchRequest {
    pub task: TaskId,
    pub reason: EnqueueReason,
    /// The worker that ran the task's previous run on this executor.
    pub last_worker: Option<WorkerId>,
    /// How many workers the executor has.
    pub workers: usize,
}

/// How a dispatched run ended, as recorded in [`EventKind::Released`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeKind {
    /// [`RunOutcome::Complete`]: useful work finished.
    Completed,
    /// [`RunOutcome::Failed`].
    Failed,
    /// [`RunOutcome::Waiting`]: parked on this key, occupying no worker.
    Waiting(DependencyKey),
    /// [`RunOutcome::Yielded`]: requeued.
    Yielded,
    /// [`RunOutcome::Cancelled`]: terminal cleanup done.
    Cancelled,
}

impl OutcomeKind {
    fn of(outcome: &RunOutcome) -> OutcomeKind {
        match outcome {
            RunOutcome::Complete(_) => OutcomeKind::Completed,
            RunOutcome::Failed(_) => OutcomeKind::Failed,
            RunOutcome::Waiting(key) => OutcomeKind::Waiting(*key),
            RunOutcome::Yielded => OutcomeKind::Yielded,
            RunOutcome::Cancelled => OutcomeKind::Cancelled,
        }
    }
}

/// What happened (module docs, "Events").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// A worker thread started and registered as a collection participant
    /// (module docs, "Collection").
    WorkerStarted { thread: ThreadId },
    /// The task entered the ready queue.
    Enqueued {
        reason: EnqueueReason,
        placement: Placement,
    },
    /// The worker acquired the task's execution claim for execution number
    /// `execution` and handed it to the driver.
    Dispatched { execution: u64 },
    /// The worker took the task from the queue but could not claim it; the
    /// entry was dropped.
    NotDispatched(RunError),
    /// The run ended and the claim was returned.
    Released(OutcomeKind),
    /// The driver panicked; unwinding returned the claim and poisoned the
    /// task. The worker continues.
    Panicked,
    /// A worker thread exited.
    WorkerStopped,
    /// A native job entered the ready queue ([`Executor::submit_native`]).
    NativeEnqueued { job: JobId, placement: Placement },
    /// The worker took the native job and started it, holding no run guard
    /// and no executor lock.
    NativeStarted { job: JobId },
    /// The native job returned, or panicked (the worker continues).
    NativeFinished { job: JobId, panicked: bool },
    /// A collection of the executor's runtime completed, whichever thread
    /// requested it (module docs, "Collection"). The event's `worker` is the
    /// requester when that is one of this executor's workers.
    Collected(CollectionEvent),
}

/// Identity of a native job ([`Executor::submit_native`]), unique within
/// one executor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(u64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job {}", self.0)
    }
}

/// One thread that took part in a recorded collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Participant {
    /// The executor worker running on `thread`, if it is one.
    pub worker: Option<WorkerId>,
    pub thread: ThreadId,
    /// How the thread was stopped while the collection ran.
    pub state: MutatorState,
}

/// A collection as recorded in [`EventKind::Collected`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionEvent {
    /// The runtime's collection count after this one (1 for the first).
    pub number: u64,
    /// The worker that requested (and ran) the collection, or `None` for a
    /// thread that is not one of this executor's workers.
    pub requester: Option<WorkerId>,
    pub requester_thread: ThreadId,
    /// The requester waited for a rendezvous
    /// ([`Runtime::request_collection`]) rather than finding every thread
    /// already stopped.
    pub waited: bool,
    /// Every other registered or heap-accessing thread: this executor's
    /// workers first (by index), then other threads.
    pub participants: Vec<Participant>,
    pub stats: CollectStats,
}

impl CollectionEvent {
    /// How `worker` was stopped, if it took part.
    pub fn state_of(&self, worker: WorkerId) -> Option<MutatorState> {
        self.participants
            .iter()
            .find(|p| p.worker == Some(worker))
            .map(|p| p.state)
    }

    /// How `thread` was stopped, if it took part.
    pub fn state_of_thread(&self, thread: ThreadId) -> Option<MutatorState> {
        self.participants
            .iter()
            .find(|p| p.thread == thread)
            .map(|p| p.state)
    }
}

/// One recorded transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Position in the record, from 0.
    pub seq: usize,
    pub worker: Option<WorkerId>,
    pub task: Option<TaskId>,
    pub kind: EventKind,
}

/// What a driver gets besides the run guard.
pub struct RunContext<'a> {
    /// The worker running this dispatch.
    pub worker: WorkerId,
    /// The executor's per-run budget.
    pub budget: Budget,
    /// The worker's host for leaf effects.
    pub host: &'a mut dyn Host,
}

type DriverFn = dyn Fn(RunGuard<'_>, &mut RunContext<'_>) -> RunOutcome + Send + Sync;
type HostFactory = dyn Fn(WorkerId) -> Box<dyn Host> + Send + Sync;
type PlacementFn = dyn Fn(&DispatchRequest) -> Placement + Send + Sync;

/// Executor settings; [`ExecutorConfig::start`] starts the workers.
pub struct ExecutorConfig {
    workers: usize,
    budget: Budget,
    host: Arc<HostFactory>,
    driver: Arc<DriverFn>,
    placement: Arc<PlacementFn>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        ExecutorConfig::new()
    }
}

impl fmt::Debug for ExecutorConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorConfig")
            .field("workers", &self.workers)
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl ExecutorConfig {
    /// Two workers, 10 000 instructions per run, hosts that refuse every
    /// effect, the VM driver, and [`Placement::Any`] for every enqueue.
    pub fn new() -> ExecutorConfig {
        ExecutorConfig {
            workers: 2,
            budget: Budget::instructions(10_000),
            host: Arc::new(|_| Box::new(NoHost)),
            driver: Arc::new(|guard, cx| guard.run_with(cx.budget, &mut *cx.host)),
            placement: Arc::new(|_| Placement::Any),
        }
    }

    /// The number of worker threads (at least 1).
    pub fn workers(mut self, n: usize) -> Self {
        self.workers = n.max(1);
        self
    }

    /// The instruction budget of every run. [`Budget::UNLIMITED`] is
    /// allowed, but then neither shutdown nor cancellation of a running task
    /// is bounded.
    pub fn budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Make each worker's leaf-effect host, on that worker's thread.
    pub fn host(mut self, f: impl Fn(WorkerId) -> Box<dyn Host> + Send + Sync + 'static) -> Self {
        self.host = Arc::new(f);
        self
    }

    /// Replace the driver (module docs, "Driver").
    pub fn driver(
        mut self,
        f: impl Fn(RunGuard<'_>, &mut RunContext<'_>) -> RunOutcome + Send + Sync + 'static,
    ) -> Self {
        self.driver = Arc::new(f);
        self
    }

    /// Replace the placement hook (module docs, "Placement hook"). Called on
    /// the enqueuing thread with no executor lock held.
    pub fn placement(
        mut self,
        f: impl Fn(&DispatchRequest) -> Placement + Send + Sync + 'static,
    ) -> Self {
        self.placement = Arc::new(f);
        self
    }

    /// Start the worker threads, serving tasks of `runtime`.
    pub fn start(self, runtime: &Runtime) -> std::io::Result<Executor> {
        let workers = self.workers;
        let shared = Arc::new(Shared {
            runtime: runtime.runtime_id(),
            rt: runtime.clone(),
            sched: Mutex::new(Sched {
                queue: VecDeque::new(),
                parked: HashMap::new(),
                in_flight: HashSet::new(),
                last_worker: HashMap::new(),
                running: vec![None; workers],
                running_native: vec![None; workers],
                next_job: 0,
                stopping: false,
            }),
            ready: Condvar::new(),
            record: Mutex::new(Record {
                events: Vec::new(),
                outcomes: HashMap::new(),
                live_workers: workers,
                worker_threads: HashMap::new(),
            }),
            recorded: Condvar::new(),
            config: self,
        });
        // Weak: the runtime does not keep the executor alive.
        let observer: Arc<dyn CollectionObserver> = shared.clone();
        runtime.observe_collections(Arc::downgrade(&observer));
        drop(observer);
        let mut handles = Vec::with_capacity(workers);
        for index in 0..workers {
            let worker_shared = Arc::clone(&shared);
            let spawned = thread::Builder::new()
                .name(format!("clj-quickjs-worker-{index}"))
                .spawn(move || worker_main(&worker_shared, WorkerId(index)));
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(e) => {
                    // Workers that never started are not live.
                    shared.lock_record().live_workers -= workers - index;
                    let executor = Executor {
                        shared,
                        workers: handles,
                    };
                    drop(executor.shutdown(Duration::from_secs(10)));
                    return Err(e);
                }
            }
        }
        Ok(Executor {
            shared,
            workers: handles,
        })
    }
}

/// A native job's work ([`Executor::submit_native`]).
type NativeFn = Box<dyn FnOnce(WorkerId) + Send>;

/// What a ready-queue entry asks a worker to do.
enum Job {
    /// Dispatch a task.
    Run(Task),
    /// Run native code on the worker, outside any run.
    Native(JobId, NativeFn),
}

struct Entry {
    job: Job,
    placement: Placement,
}

impl Entry {
    fn task_id(&self) -> Option<TaskId> {
        match &self.job {
            Job::Run(task) => Some(task.id()),
            Job::Native(..) => None,
        }
    }
}

/// Scheduling state, behind `Shared::sched`.
struct Sched {
    queue: VecDeque<Entry>,
    /// Parked tasks, kept alive here (cells hold weak references) until an
    /// enqueue removes them. A parking worker inserts its task only if it is
    /// still parked, checked under this lock, so a wake that won the race
    /// (its enqueue takes this lock too) never leaves a stale entry.
    parked: HashMap<TaskId, Task>,
    /// Tasks taken by a worker whose dispatch has not finished routing its
    /// outcome; their queue entries are ineligible meanwhile.
    in_flight: HashSet<TaskId>,
    /// The worker that last acquired each task's claim.
    last_worker: HashMap<TaskId, WorkerId>,
    /// The task each worker is dispatching, by worker index.
    running: Vec<Option<TaskId>>,
    /// The native job each worker is running, by worker index.
    running_native: Vec<Option<JobId>>,
    /// The next native job's id.
    next_job: u64,
    stopping: bool,
}

/// Events and terminal outcomes, behind `Shared::record` (innermost lock).
struct Record {
    events: Vec<Event>,
    outcomes: HashMap<TaskId, RunOutcome>,
    live_workers: usize,
    /// Each started worker's OS thread, to name workers in collection
    /// reports.
    worker_threads: HashMap<ThreadId, WorkerId>,
}

struct Shared {
    /// Identity of the runtime whose tasks this executor serves.
    runtime: u64,
    /// That runtime: workers register with it and native jobs may use it.
    rt: Runtime,
    sched: Mutex<Sched>,
    /// Signalled when the queue gains an entry or shutdown begins.
    ready: Condvar,
    record: Mutex<Record>,
    /// Signalled on every new event.
    recorded: Condvar,
    config: ExecutorConfig,
}

impl Shared {
    // A panic never happens while these locks are held (no user code runs
    // under them), so poisoning is not expected; recover the data anyway.
    fn lock_sched(&self) -> MutexGuard<'_, Sched> {
        self.sched.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_record(&self) -> MutexGuard<'_, Record> {
        self.record.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push_event(
        record: &mut Record,
        worker: Option<WorkerId>,
        task: Option<TaskId>,
        kind: EventKind,
    ) {
        let seq = record.events.len();
        record.events.push(Event {
            seq,
            worker,
            task,
            kind,
        });
    }

    fn record(&self, worker: Option<WorkerId>, task: Option<TaskId>, kind: EventKind) {
        Self::push_event(&mut self.lock_record(), worker, task, kind);
        self.recorded.notify_all();
    }

    fn placement_for(&self, task: TaskId, reason: EnqueueReason) -> Placement {
        let last_worker = self.lock_sched().last_worker.get(&task).copied();
        let request = DispatchRequest {
            task,
            reason,
            last_worker,
            workers: self.config.workers,
        };
        // User code: no lock held.
        match (self.config.placement)(&request) {
            Placement::Worker(w) if w.0 >= self.config.workers => Placement::Any,
            placement => placement,
        }
    }

    /// Enqueue with the placement hook's decision.
    fn enqueue(&self, task: Task, reason: EnqueueReason) {
        let placement = self.placement_for(task.id(), reason);
        self.enqueue_at(task, reason, placement);
    }

    fn enqueue_at(&self, task: Task, reason: EnqueueReason, placement: Placement) {
        let id = task.id();
        let placement = match placement {
            Placement::Worker(w) if w.0 >= self.config.workers => Placement::Any,
            placement => placement,
        };
        let mut sched = self.lock_sched();
        sched.parked.remove(&id);
        sched.queue.push_back(Entry {
            job: Job::Run(task),
            placement,
        });
        // Recorded under the scheduling lock, so no worker can dispatch the
        // entry before its enqueue is in the record.
        self.record(None, Some(id), EventKind::Enqueued { reason, placement });
        drop(sched);
        self.ready.notify_all();
    }

    fn enqueue_native(&self, f: NativeFn, placement: Placement) -> JobId {
        let placement = match placement {
            Placement::Worker(w) if w.0 >= self.config.workers => Placement::Any,
            placement => placement,
        };
        let mut sched = self.lock_sched();
        let job = JobId(sched.next_job);
        sched.next_job += 1;
        sched.queue.push_back(Entry {
            job: Job::Native(job, f),
            placement,
        });
        self.record(None, None, EventKind::NativeEnqueued { job, placement });
        drop(sched);
        self.ready.notify_all();
        job
    }

    /// Wait for an eligible entry; `None` once shutdown has begun. The
    /// scheduling lock is released before the job starts: a worker never
    /// holds it while it enters heap access (which may wait for a pending
    /// collection) or runs anything.
    fn next_job(&self, worker: WorkerId) -> Option<Job> {
        let mut sched = self.lock_sched();
        loop {
            if sched.stopping {
                return None;
            }
            let in_flight = &sched.in_flight;
            let eligible = sched.queue.iter().position(|e| {
                let placed = match e.placement {
                    Placement::Any => true,
                    Placement::Worker(w) => w == worker,
                };
                placed && e.task_id().is_none_or(|id| !in_flight.contains(&id))
            });
            if let Some(entry) = eligible.and_then(|i| sched.queue.remove(i)) {
                match &entry.job {
                    Job::Run(task) => {
                        let id = task.id();
                        sched.running[worker.0] = Some(id);
                        sched.in_flight.insert(id);
                    }
                    Job::Native(job, _) => sched.running_native[worker.0] = Some(*job),
                }
                return Some(entry.job);
            }
            sched = self
                .ready
                .wait(sched)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// One dispatch: claim, drive, record, route the outcome.
    fn dispatch(&self, worker: WorkerId, task: Task, host: &mut dyn Host) {
        let id = task.id();
        let guard = match task.try_begin_run() {
            Ok(guard) => guard,
            Err(e) => {
                self.record(Some(worker), Some(id), EventKind::NotDispatched(e));
                // The match borrows `task` for the guard; a handle clone is cheap.
                self.route_refused(task.clone());
                return;
            }
        };
        let execution = guard.wake_handle().execution();
        self.lock_sched().last_worker.insert(id, worker);
        self.record(Some(worker), Some(id), EventKind::Dispatched { execution });
        let mut cx = RunContext {
            worker,
            budget: self.config.budget,
            host,
        };
        let driver = &self.config.driver;
        // No executor lock is held while the task runs or suspends.
        let outcome = match panic::catch_unwind(AssertUnwindSafe(|| driver(guard, &mut cx))) {
            Ok(outcome) => outcome,
            Err(_) => {
                self.lock_sched().last_worker.remove(&id);
                self.record(Some(worker), Some(id), EventKind::Panicked);
                return;
            }
        };
        // The guard was consumed by the driver: the claim is returned.
        let kind = OutcomeKind::of(&outcome);
        match outcome {
            RunOutcome::Yielded => {
                self.record(Some(worker), Some(id), EventKind::Released(kind));
                self.enqueue(task, EnqueueReason::Yielded);
            }
            RunOutcome::Waiting(_) => {
                let mut sched = self.lock_sched();
                if task.status() == TaskStatus::Waiting {
                    sched.parked.insert(id, task);
                }
                self.record(Some(worker), Some(id), EventKind::Released(kind));
            }
            terminal => {
                let mut sched = self.lock_sched();
                sched.parked.remove(&id);
                sched.last_worker.remove(&id);
                let mut record = self.lock_record();
                record.outcomes.insert(id, terminal);
                Self::push_event(
                    &mut record,
                    Some(worker),
                    Some(id),
                    EventKind::Released(kind),
                );
                drop(record);
                drop(sched);
                self.recorded.notify_all();
            }
        }
    }

    /// Route a task whose claim was refused, by its state now (module docs,
    /// "Ownership per run"). This executor never dispatches a task twice at
    /// once, so the refusal came from a diagnostic reader's momentary body
    /// lock, an embedder's own run, a park, or the end of the task.
    fn route_refused(&self, task: Task) {
        match task.status() {
            // Requeued: its entry stays ineligible until this dispatch ends.
            TaskStatus::Ready => self.enqueue(task, EnqueueReason::Retried),
            TaskStatus::Waiting => {
                let mut sched = self.lock_sched();
                // Rechecked under the lock a waking enqueue takes.
                if task.status() == TaskStatus::Waiting {
                    sched.parked.insert(task.id(), task);
                }
            }
            // Running elsewhere: its runner routes it. Finished: nothing to do.
            TaskStatus::Running | TaskStatus::Finished | TaskStatus::Cancelled => {}
        }
    }

    /// Block until `ready(record)` holds or `timeout` passes.
    fn wait_record<T>(
        &self,
        timeout: Duration,
        mut ready: impl FnMut(&mut Record) -> Option<T>,
    ) -> Option<T> {
        let deadline = Instant::now() + timeout;
        let mut record = self.lock_record();
        loop {
            if let Some(found) = ready(&mut record) {
                return Some(found);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            record = self
                .recorded
                .wait_timeout(record, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// Records the worker's exit however its thread ends.
struct WorkerExit<'a> {
    shared: &'a Shared,
    worker: WorkerId,
}

impl Drop for WorkerExit<'_> {
    fn drop(&mut self) {
        let mut sched = self.shared.lock_sched();
        if let Some(id) = sched.running[self.worker.0].take() {
            sched.in_flight.remove(&id);
        }
        sched.running_native[self.worker.0] = None;
        drop(sched);
        let mut record = self.shared.lock_record();
        record.live_workers -= 1;
        Shared::push_event(
            &mut record,
            Some(self.worker),
            None,
            EventKind::WorkerStopped,
        );
        drop(record);
        self.shared.recorded.notify_all();
    }
}

fn worker_main(shared: &Shared, worker: WorkerId) {
    let _exit = WorkerExit { shared, worker };
    let thread = thread::current().id();
    // Named before it registers, so every collection report naming the
    // thread maps it to this worker.
    shared.lock_record().worker_threads.insert(thread, worker);
    // A collection participant for the worker's lifetime (dropped before
    // `_exit` records the stop). It is not heap access: while the worker
    // waits for the ready queue, or routes an outcome, it is idle and
    // stopped; it enters heap access only through a run guard, or a scope
    // or atom operation of a native job (module docs, "Collection").
    let _registration = shared.rt.register_thread();
    shared.record(Some(worker), None, EventKind::WorkerStarted { thread });
    let mut host = (shared.config.host)(worker);
    while let Some(job) = shared.next_job(worker) {
        match job {
            Job::Run(task) => {
                let id = task.id();
                shared.dispatch(worker, task, &mut *host);
                let mut sched = shared.lock_sched();
                sched.running[worker.0] = None;
                sched.in_flight.remove(&id);
                drop(sched);
                // A queued entry of this task may have become eligible.
                shared.ready.notify_all();
            }
            Job::Native(job, f) => {
                shared.record(Some(worker), None, EventKind::NativeStarted { job });
                // No executor lock is held while native code runs.
                let panicked = panic::catch_unwind(AssertUnwindSafe(|| f(worker))).is_err();
                shared.lock_sched().running_native[worker.0] = None;
                shared.record(
                    Some(worker),
                    None,
                    EventKind::NativeFinished { job, panicked },
                );
            }
        }
    }
}

impl CollectionObserver for Shared {
    /// Record the collection (module docs, "Collection"). Runs on the
    /// collecting thread after the rendezvous, holding only the record lock.
    fn collected(&self, report: &CollectionReport) {
        let mut record = self.lock_record();
        let worker_of = |thread: &ThreadId| record.worker_threads.get(thread).copied();
        let mut participants: Vec<Participant> = report
            .participants
            .iter()
            .map(|&(thread, state)| Participant {
                worker: worker_of(&thread),
                thread,
                state,
            })
            .collect();
        // Workers by index, then other threads (in report order).
        participants.sort_by_key(|p| p.worker.map_or(usize::MAX, WorkerId::index));
        let requester = worker_of(&report.requester);
        let event = CollectionEvent {
            number: report.number,
            requester,
            requester_thread: report.requester,
            waited: report.waited,
            participants,
            stats: report.stats,
        };
        Shared::push_event(&mut record, requester, None, EventKind::Collected(event));
        drop(record);
        self.recorded.notify_all();
    }
}

/// A running reference executor. See the module docs.
pub struct Executor {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl fmt::Debug for Executor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Executor")
            .field("workers", &self.workers.len())
            .finish_non_exhaustive()
    }
}

/// What a completed [`Executor::shutdown`] hands back.
#[derive(Debug)]
pub struct Shutdown {
    /// Queued tasks that were not run again, in queue order. Still runnable,
    /// with their VM state: the embedder may run them by hand.
    pub pending: Vec<Task>,
    /// Tasks parked on a dependency when the executor stopped. A later
    /// completion still makes them runnable (through its
    /// [`Wakeups`]), for the embedder to run.
    pub parked: Vec<Task>,
    /// Queued native jobs that never started; they were dropped unrun.
    pub dropped_native: Vec<JobId>,
    /// Terminal outcomes not yet taken with [`Executor::take_outcome`].
    pub outcomes: HashMap<TaskId, RunOutcome>,
    /// The full event record.
    pub events: Vec<Event>,
}

/// [`Executor::shutdown`] passed its deadline with workers still busy.
#[derive(Debug)]
pub struct ShutdownTimeout {
    /// Each busy worker and the task it is dispatching.
    pub busy: Vec<(WorkerId, TaskId)>,
    /// Each worker busy with a native job, and the job.
    pub busy_native: Vec<(WorkerId, JobId)>,
    /// The executor, still stopping: call [`Executor::shutdown`] again.
    pub executor: Executor,
}

impl Executor {
    /// Start an executor with [`ExecutorConfig::new`] settings, serving tasks
    /// of `runtime`.
    pub fn start(runtime: &Runtime) -> std::io::Result<Executor> {
        ExecutorConfig::new().start(runtime)
    }

    /// The number of workers.
    pub fn workers(&self) -> usize {
        self.shared.config.workers
    }

    fn check_own(&self, task: Task) -> Result<Task, ForeignTasks> {
        if task.runtime_id() == self.shared.runtime {
            Ok(task)
        } else {
            Err(ForeignTasks(vec![task]))
        }
    }

    /// Enqueue a runnable task with the placement hook's decision. A task
    /// of another runtime is handed back.
    pub fn submit(&self, task: Task) -> Result<(), ForeignTasks> {
        let task = self.check_own(task)?;
        self.shared.enqueue(task, EnqueueReason::Submitted);
        Ok(())
    }

    /// Enqueue a runnable task on `placement`, bypassing the hook. A task of
    /// another runtime is handed back.
    pub fn submit_with(&self, task: Task, placement: Placement) -> Result<(), ForeignTasks> {
        let task = self.check_own(task)?;
        self.shared
            .enqueue_at(task, EnqueueReason::Submitted, placement);
        Ok(())
    }

    /// Enqueue the tasks a completion made runnable (normally the result of
    /// [`Runtime::complete`](crate::Runtime::complete) or
    /// [`Runtime::cancel_cell`](crate::Runtime::cancel_cell)); returns how
    /// many. Remembered wakes need nothing (their run requeues itself) and
    /// stale ones are ignored. If any task belongs to another runtime,
    /// nothing is enqueued and every runnable task is handed back.
    pub fn wake(&self, wakeups: Wakeups) -> Result<usize, ForeignTasks> {
        let runnable = wakeups.into_runnable();
        if runnable
            .iter()
            .any(|t| t.runtime_id() != self.shared.runtime)
        {
            return Err(ForeignTasks(runnable));
        }
        let n = runnable.len();
        for task in runnable {
            self.shared.enqueue(task, EnqueueReason::Woken);
        }
        Ok(n)
    }

    /// Request `task`'s cancellation ([`Task::cancel`]). If the task was
    /// parked it is now runnable and is enqueued, so its own run performs
    /// terminal cleanup and ends [`RunOutcome::Cancelled`]; returns `true`
    /// then. Otherwise (`false`) the current or next run observes the
    /// request, or the task had already finished or been cancelled. A task
    /// of another runtime is handed back uncancelled.
    pub fn cancel(&self, task: &Task) -> Result<bool, ForeignTasks> {
        let task = self.check_own(task.clone())?;
        Ok(match task.cancel() {
            Cancel::Runnable(task) => {
                self.shared.enqueue(task, EnqueueReason::Cancelled);
                true
            }
            Cancel::Requested | Cancel::AlreadyRequested | Cancel::Finished => false,
        })
    }

    /// Enqueue native work: an eligible worker takes it from the ready
    /// queue in order with tasks and calls `f(worker)` on its thread, outside
    /// any run, with no executor lock held. Recorded as
    /// [`EventKind::NativeEnqueued`], [`EventKind::NativeStarted`] and
    /// [`EventKind::NativeFinished`]; a panic is caught and recorded.
    ///
    /// Native heap access goes through [`Runtime::scope`]: a scope is a
    /// no-GC access scope, so a job that keeps one open delays every
    /// collection until it closes or releases it
    /// ([`Scope::released`](crate::Scope::released),
    /// [`Scope::safepoint`](crate::Scope::safepoint)). The job may also
    /// request a collection ([`Executor::schedule_collection`]). It must not
    /// run a task, and while it runs the worker dispatches nothing else.
    pub fn submit_native(
        &self,
        placement: Placement,
        f: impl FnOnce(WorkerId) + Send + 'static,
    ) -> JobId {
        self.shared.enqueue_native(Box::new(f), placement)
    }

    /// Enqueue a native job that requests a collection
    /// ([`Runtime::request_collection`]) from the worker that takes it: the
    /// worker holds no run guard, so the request cannot fail, and it blocks
    /// that worker until every other mutator has stopped (module docs,
    /// "Collection"). The collection is recorded as
    /// [`EventKind::Collected`] with that worker as requester.
    pub fn schedule_collection(&self, placement: Placement) -> JobId {
        let rt = self.shared.rt.clone();
        self.submit_native(placement, move |_| {
            // Cannot fail: a native job's thread holds no run guard or atom
            // operation. The observer records the collection.
            let _ = rt.request_collection();
        })
    }

    /// A snapshot of the event record.
    pub fn events(&self) -> Vec<Event> {
        self.shared.lock_record().events.clone()
    }

    /// Wait until `ready(events)` holds; `false` if `timeout` passes first.
    /// `ready` sees a snapshot taken whenever the record has grown, and runs
    /// with no executor lock held, so it may call the executor.
    pub fn wait_for(&self, timeout: Duration, mut ready: impl FnMut(&[Event]) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        let mut seen = None;
        loop {
            let snapshot = {
                let mut record = self.shared.lock_record();
                // Events are only appended: a new length is a new state.
                while seen == Some(record.events.len()) {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return false;
                    }
                    record = self
                        .shared
                        .recorded
                        .wait_timeout(record, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                record.events.clone()
            };
            seen = Some(snapshot.len());
            if ready(&snapshot) {
                return true;
            }
        }
    }

    /// Wait for `task`'s terminal outcome (`Complete`, `Failed` or
    /// `Cancelled`) and take it; `None` if `timeout` passes first.
    pub fn take_outcome(&self, task: TaskId, timeout: Duration) -> Option<RunOutcome> {
        self.shared
            .wait_record(timeout, |record| record.outcomes.remove(&task))
    }

    /// Stop dispatching: each worker finishes its current run and exits.
    /// Tasks enqueued from now on stay queued and are returned by
    /// [`Executor::shutdown`].
    pub fn begin_shutdown(&self) {
        self.shared.lock_sched().stopping = true;
        self.shared.ready.notify_all();
    }

    /// Stop dispatching, wait up to `timeout` for every worker to finish its
    /// current run and exit, and return what the executor still held (module
    /// docs, "Shutdown and cancellation").
    pub fn shutdown(mut self, timeout: Duration) -> Result<Shutdown, ShutdownTimeout> {
        self.begin_shutdown();
        let stopped = self
            .shared
            .wait_record(timeout, |record| (record.live_workers == 0).then_some(()));
        if stopped.is_none() {
            let sched = self.shared.lock_sched();
            let busy = sched
                .running
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.map(|t| (WorkerId(i), t)))
                .collect();
            let busy_native = sched
                .running_native
                .iter()
                .enumerate()
                .filter_map(|(i, j)| j.map(|j| (WorkerId(i), j)))
                .collect();
            drop(sched);
            return Err(ShutdownTimeout {
                busy,
                busy_native,
                executor: self,
            });
        }
        for handle in std::mem::take(&mut self.workers) {
            // Every worker has recorded its exit; a join error would mean a
            // panic after that, which nothing there can cause.
            let _ = handle.join();
        }
        let mut sched = self.shared.lock_sched();
        let queue: Vec<Entry> = sched.queue.drain(..).collect();
        let parked = sched
            .parked
            .drain()
            .map(|(_, t)| t)
            .filter(|t| t.status() == TaskStatus::Waiting)
            .collect();
        drop(sched);
        // Unrun native jobs are dropped here, with no executor lock held.
        let mut pending = Vec::new();
        let mut dropped_native = Vec::new();
        for entry in queue {
            match entry.job {
                Job::Run(task) => pending.push(task),
                Job::Native(job, _) => dropped_native.push(job),
            }
        }
        let mut record = self.shared.lock_record();
        Ok(Shutdown {
            pending,
            parked,
            dropped_native,
            outcomes: std::mem::take(&mut record.outcomes),
            events: record.events.clone(),
        })
    }
}

impl Drop for Executor {
    /// Begins shutdown and detaches the workers: each exits after its
    /// current run. Use [`Executor::shutdown`] to wait and recover tasks.
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}
