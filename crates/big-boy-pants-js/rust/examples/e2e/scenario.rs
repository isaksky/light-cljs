//! The end-to-end synthetic compiler-host scenario (`docs/design.md` §13;
//! pellets clj-quickjs-13 and clj-quickjs-29), shared by
//! `cargo run --example e2e` and `tests/e2e.rs`.
//!
//! This file is example host/driver glue built only on the public embedding
//! API: the runtime, the reference executor (`exec`), dependency cells and
//! JavaScript source compiled by `frontend`. The program's control flow is
//! the JavaScript of `consumer.js`, `producer.js`, `work.js` and
//! `companion.js` (synthetic fixtures, not light-cljs output) executed by
//! the VM; the host below only provides leaf effects, schedules tasks and
//! publishes results.
//!
//! # The scenario ([`Variant::Success`])
//!
//! 1. Rust builds and roots one nested persistent form-like input.
//! 2. Two consumer tasks of `consumer.js` receive the same form root and run
//!    on two executor workers, pinned one per worker. Each runs its pre-wait
//!    JavaScript (PRE effect, then a MEET effect that returns only when the
//!    other consumer is inside its own MEET: both execute at once) and
//!    reaches `__rt.need(7, 2)` inside a `try` with a `catch` and a
//!    `finally`: the CALLABLE result of definition 7 in the scenario's
//!    world. The first need claims the cell's single producer request (the
//!    driver takes it with `Task::take_producer_requests`); the host holds
//!    it, so both consumers park (`Waiting`) and leave their workers, their
//!    finally blocks still pending.
//! 3. Each worker runs one unrelated `work.js` task to completion while the
//!    consumers wait.
//! 4. A worker requests a collection (`Executor::schedule_collection`)
//!    while both consumers are parked.
//! 5. Only then the host runs the producer, a VM task of `producer.js`,
//!    which returns a helper function compiled by this runtime, and
//!    publishes it as the cell's result (`Runtime::complete`): the one
//!    completion wakes both consumers.
//! 6. The placement hook sends each woken consumer to the *other* worker.
//!    Each continues after its `need` (no pre-wait effect runs again), runs
//!    its finally block (CLEANUP), calls the shared helper and returns a
//!    subtree of the input with its original identity.
//!
//! # Variants
//!
//! The same fixtures, host, driver and steps, with one change each:
//!
//! * [`Variant::ProducerFailure`]: in step 5 the producer task is given a
//!   negative slot, so `producer.js` throws and the task fails. The host
//!   publishes that failure (`Completion::Failed`, an error value naming
//!   the throw's source position); the one completion wakes both
//!   consumers, whose `need` raises the dependency error on the other
//!   worker: each catch block observes it (CAUGHT), the finally block runs
//!   (CLEANUP) and the consumer returns the error object, whose cause is the
//!   published value.
//! * [`Variant::ConsumerCancel`]: right after both consumers park, the host
//!   cancels the one whose need claimed the producer request
//!   (`Executor::cancel`). Its next run (on the other worker) leaves the
//!   cell, runs its pending finally block (CLEANUP, no catch) and ends
//!   `Cancelled`. The cell and its producer are untouched: the steps go on,
//!   the producer runs, and its publication wakes only the other consumer,
//!   which completes as in the success scenario.
//!
//! [`run_companion`] is a companion scenario of the same example and host:
//! one `companion.js` task reaches the helper from inside `__rt.reduce`'s
//! JavaScript callback (native-to-JavaScript reentry), with a live
//! closure-captured local and two pending finally blocks. It suspends
//! inside the fold, is collected while parked, migrates when woken, and the
//! fold completes without repeating a step.
//!
//! Every wait of the host is bounded and synchronizes on the executor's
//! event record, terminal outcomes or the MEET rendezvous; nothing sleeps.
//! A timeout makes a run return an error (or a MEET effect fail its task),
//! never hang.

// Shared by the example and the integration test; each uses a different
// part of the report.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};
use std::time::Duration;

use big_boy_pants_js::exec::{
    CollectionEvent, EnqueueReason, Event, EventKind, Executor, ExecutorConfig, OutcomeKind,
    Placement, WorkerId,
};
use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    CellStatus, Completion, DependencyId, DependencyKey, Executable, Host, HostCall, Primitive,
    ProducerId, ProducerRequest, ResultKind, Rooted, RunOutcome, Runtime, SourceLocation, Task,
    TaskId, TaskStatus, ValueView, VmError, WorldId,
};

/// The definition consumers need (`__rt.need(7, 2)` in `consumer.js`).
pub const HELPER: DependencyId = DependencyId::new(7);
/// The result kind consumers need: a callable.
pub const CALLABLE: ResultKind = ResultKind::new(2);

/// Host operations of `consumer.js`.
pub const PRE: u16 = 1;
pub const MEET: u16 = 2;
pub const POST: u16 = 3;
pub const REPORT: u16 = 4;
pub const CAUGHT: u16 = 5;
pub const CLEANUP: u16 = 6;
/// Host operations of `companion.js`.
pub const VISIT: u16 = 7;
pub const INNER: u16 = 8;
pub const OUTER: u16 = 9;

/// Bound of every wait of the host, and of the MEET rendezvous.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The slot `producer.js`'s helper descends through: the body of each form.
pub const SLOT: f64 = 2.0;
/// The slot of [`Variant::ProducerFailure`]: `producer.js` throws it.
pub const BAD_SLOT: f64 = -1.0;
/// `work.js` arguments, one task per worker, and their results.
pub const WORK: [(f64, f64); 2] = [(20.0, 2870.0), (30.0, 9455.0)];

/// What each consumer's REPORT effect must see: three noted values in its
/// task-owned builder (`seen`), then `scratch[0] * 10 + scratch[1]` (head
/// symbol 1, arity 2), all kept across the wait and the collection.
pub const EXPECTED_REPORT: f64 = 312.0;

/// The companion task's `tag`.
pub const COMPANION_TAG: f64 = 5.0;

/// Symbol numbers of the synthetic form `(defn area [w h] (let [sq (* w h)]
/// (do sq)))` plus a metadata map: the subset has no strings, so symbols are
/// numbers.
const DEFN: f64 = 1.0;
const LET: f64 = 2.0;
const MUL: f64 = 3.0;
const DO: f64 = 4.0;
const W: f64 = 11.0;
const H: f64 = 12.0;
const SQ: f64 = 13.0;
const LINE: f64 = 100.0;
const COLUMN: f64 = 101.0;

/// Which ending of the consumer scenario to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The producer returns the helper; both consumers complete.
    Success,
    /// The producer task fails; both consumers catch the dependency error.
    ProducerFailure,
    /// The consumer that claimed the producer request is cancelled while
    /// parked; the producer and the other consumer complete.
    ConsumerCancel,
}

impl Variant {
    pub const ALL: [Variant; 3] = [
        Variant::Success,
        Variant::ProducerFailure,
        Variant::ConsumerCancel,
    ];

    /// The command-line name (`cargo run --example e2e -- <name>`).
    pub fn name(self) -> &'static str {
        match self {
            Variant::Success => "success",
            Variant::ProducerFailure => "failure",
            Variant::ConsumerCancel => "cancel",
        }
    }
}

impl FromStr for Variant {
    type Err = String;

    fn from_str(s: &str) -> Result<Variant, String> {
        Variant::ALL
            .into_iter()
            .find(|v| v.name() == s)
            .ok_or_else(|| format!("unknown variant {s:?}"))
    }
}

/// One consumer's parameters: `tag` names it in effects, `depth` is how far
/// the helper descends, and `start` the worker its first run is pinned to.
#[derive(Clone, Copy, Debug)]
pub struct ConsumerSpec {
    pub tag: f64,
    pub depth: f64,
    pub start: WorkerId,
}

pub const CONSUMERS: [ConsumerSpec; 2] = [
    ConsumerSpec {
        tag: 1.0,
        depth: 1.0,
        start: WorkerId::new(0),
    },
    ConsumerSpec {
        tag: 2.0,
        depth: 2.0,
        start: WorkerId::new(1),
    },
];

/// What the host published for the helper's cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishedAs {
    /// `Completion::Ready` with the producer's helper.
    Helper,
    /// `Completion::Failed` with an error value describing the producer
    /// task's failure.
    Failure,
}

/// One host-side observation, in the order the host made them (one lock,
/// so this is a true order across workers and the host thread).
#[derive(Clone, Debug)]
pub enum Trace {
    /// A leaf host effect a task executed on a worker.
    Effect {
        task: TaskId,
        worker: WorkerId,
        thread: ThreadId,
        op: u16,
        arg: f64,
    },
    /// A run ended `Waiting` at the `__rt.need` at `at` (driver, before the
    /// executor records the release).
    Suspended {
        task: TaskId,
        worker: WorkerId,
        key: DependencyKey,
        at: Option<SourceLocation>,
    },
    /// A run's need claimed the cell's producer request; the host keeps it.
    Requested {
        task: TaskId,
        worker: WorkerId,
        key: DependencyKey,
        producer: ProducerId,
    },
    /// The host is about to request `task`'s cancellation
    /// (`Executor::cancel`; recorded first, since the cleanup run may start
    /// at once on a worker).
    CancelRequested { task: TaskId },
    /// A run ended `Cancelled`: terminal cleanup (its finally blocks
    /// included) is done (driver).
    Cancelled { task: TaskId, worker: WorkerId },
    /// A run failed at `at` (driver).
    Failed {
        task: TaskId,
        worker: WorkerId,
        at: Option<SourceLocation>,
        message: String,
    },
    /// The host observed the forced collection while the consumers waited.
    Collected {
        number: u64,
        live: usize,
        reclaimed: usize,
    },
    /// The host published the producer task's outcome for `key`, waking
    /// `woke`.
    Published {
        key: DependencyKey,
        producer_task: TaskId,
        published: PublishedAs,
        woke: Vec<TaskId>,
    },
    /// A task's completed value, taken by the host.
    Finished { task: TaskId, value: ValueView },
}

/// The name of a host operation.
pub fn op_name(op: u16) -> &'static str {
    match op {
        PRE => "PRE",
        MEET => "MEET",
        POST => "POST",
        REPORT => "REPORT",
        CAUGHT => "CAUGHT",
        CLEANUP => "CLEANUP",
        VISIT => "VISIT",
        INNER => "INNER",
        OUTER => "OUTER",
        _ => "?",
    }
}

fn at(f: &mut fmt::Formatter<'_>, at: &Option<SourceLocation>) -> fmt::Result {
    match at {
        Some(at) => write!(f, " at {}:{}", at.line, at.column),
        None => Ok(()),
    }
}

impl fmt::Display for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trace::Effect {
                task,
                worker,
                thread,
                op,
                arg,
            } => write!(
                f,
                "{task} on {worker} ({thread:?}): effect {}({arg})",
                op_name(*op)
            ),
            Trace::Suspended {
                task,
                worker,
                key,
                at: loc,
            } => {
                write!(f, "{task} on {worker}: suspended on {key}")?;
                at(f, loc)
            }
            Trace::Requested {
                task,
                worker,
                key,
                producer,
            } => write!(
                f,
                "{task} on {worker}: claimed producer request {producer} for {key} (held)"
            ),
            Trace::CancelRequested { task } => write!(f, "host cancels {task}"),
            Trace::Cancelled { task, worker } => {
                write!(f, "{task} on {worker}: cancelled, cleanup done")
            }
            Trace::Failed {
                task,
                worker,
                at: loc,
                message,
            } => {
                write!(f, "{task} on {worker}: failed ({message})")?;
                at(f, loc)
            }
            Trace::Collected {
                number,
                live,
                reclaimed,
            } => write!(
                f,
                "collection #{number} while parked: {live} live, {reclaimed} reclaimed"
            ),
            Trace::Published {
                key,
                producer_task,
                published,
                woke,
            } => write!(
                f,
                "published {producer_task}'s {} for {key}; woke [{}]",
                match published {
                    PublishedAs::Helper => "helper",
                    PublishedAs::Failure => "failure",
                },
                woke.iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Trace::Finished { task, value } => write!(f, "{task} finished: {value:?}"),
        }
    }
}

/// How a consumer ended.
pub enum ConsumerEnd {
    /// It returned this value (a subtree, or a caught error object).
    Returned(Rooted),
    /// It was cancelled.
    Cancelled,
}

/// A consumer after the scenario.
pub struct ConsumerReport {
    pub spec: ConsumerSpec,
    pub task: TaskId,
    pub end: ConsumerEnd,
    /// The subtree of the input the success path returns, navigated by Rust.
    pub expected: Rooted,
    /// The source location it was suspended at, while parked.
    pub suspended_at: Option<SourceLocation>,
    /// Its status when the collection had completed.
    pub status_after_collection: TaskStatus,
}

impl ConsumerReport {
    /// The value it returned; panics if it was cancelled.
    pub fn result(&self) -> &Rooted {
        match &self.end {
            ConsumerEnd::Returned(v) => v,
            ConsumerEnd::Cancelled => panic!("{} was cancelled", self.task),
        }
    }

    pub fn cancelled(&self) -> bool {
        matches!(self.end, ConsumerEnd::Cancelled)
    }
}

/// Everything a consumer scenario observed.
pub struct Report {
    pub variant: Variant,
    pub rt: Runtime,
    pub world: WorldId,
    /// The dependency both consumers needed.
    pub key: DependencyKey,
    /// The Rust-built input, rooted by the host.
    pub form: Rooted,
    pub consumers: Vec<ConsumerReport>,
    /// The cell while both consumers waited.
    pub cell_while_waiting: CellStatus,
    /// [`Variant::ConsumerCancel`]: the cancelled consumer, and the cell
    /// once its cleanup run had ended.
    pub cancelled: Option<TaskId>,
    pub cell_after_cancel: Option<CellStatus>,
    /// Every producer request the consumers' runs claimed.
    pub requests: Vec<ProducerRequestInfo>,
    pub work: Vec<TaskId>,
    pub producer: TaskId,
    /// [`Variant::ProducerFailure`]: the producer task's failure.
    pub producer_failure: Option<VmError>,
    /// What the host published: the helper, or the failure value.
    pub published: PublishedAs,
    pub published_value: Rooted,
    /// The cell after all consumers ended.
    pub cell_after: CellStatus,
    pub collection: CollectionEvent,
    /// The workers' OS threads, by worker index.
    pub worker_threads: Vec<ThreadId>,
    /// The executor's event record.
    pub events: Vec<Event>,
    /// The host's observations.
    pub trace: Vec<Trace>,
}

/// Everything the companion scenario observed.
pub struct CompanionReport {
    pub rt: Runtime,
    pub world: WorldId,
    pub key: DependencyKey,
    pub form: Rooted,
    pub task: TaskId,
    /// Its returned value: the vector of the subtrees the helper selected.
    pub result: Rooted,
    /// What each fold step must select: the form's body, then its inner
    /// subtree, navigated by Rust.
    pub expected: [Rooted; 2],
    pub suspended_at: Option<SourceLocation>,
    pub status_after_collection: TaskStatus,
    pub requests: Vec<ProducerRequestInfo>,
    pub producer: TaskId,
    pub helper: Rooted,
    pub collection: CollectionEvent,
    pub worker_threads: Vec<ThreadId>,
    pub events: Vec<Event>,
    pub trace: Vec<Trace>,
}

/// A claimed producer request, as the host saw it.
#[derive(Clone, Copy, Debug)]
pub struct ProducerRequestInfo {
    pub task: TaskId,
    pub key: DependencyKey,
    pub producer: ProducerId,
}

/// The host effects of `task` in `trace`, in order.
pub fn effects_of(trace: &[Trace], task: TaskId) -> Vec<(WorkerId, ThreadId, u16, f64)> {
    trace
        .iter()
        .filter_map(|t| match *t {
            Trace::Effect {
                task: t,
                worker,
                thread,
                op,
                arg,
            } if t == task => Some((worker, thread, op, arg)),
            _ => None,
        })
        .collect()
}

/// The executor events of `task`, in order.
pub fn events_of(events: &[Event], task: TaskId) -> Vec<&Event> {
    events.iter().filter(|e| e.task == Some(task)).collect()
}

impl Report {
    /// The host effects of `task`, in order.
    pub fn effects_of(&self, task: TaskId) -> Vec<(WorkerId, ThreadId, u16, f64)> {
        effects_of(&self.trace, task)
    }

    /// The executor events of `task`, in order.
    pub fn events_of(&self, task: TaskId) -> Vec<&Event> {
        events_of(&self.events, task)
    }
}

// ---------------------------------------------------------------------------
// Host glue.

/// State shared by the workers' hosts, the driver and the scenario.
#[derive(Default)]
struct Shared {
    trace: Mutex<Vec<Trace>>,
    /// Tasks the scenario submitted, so the driver can take the producer
    /// requests their runs claimed.
    tasks: Mutex<HashMap<TaskId, Task>>,
    /// Requests claimed by runs, held until the host decides to produce.
    held: Mutex<Vec<(TaskId, ProducerRequest)>>,
    meet: Rendezvous,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn record(&self, t: Trace) {
        lock(&self.trace).push(t);
    }
}

/// A two-party rendezvous, bounded by [`TIMEOUT`].
#[derive(Default)]
struct Rendezvous {
    arrived: Mutex<usize>,
    cv: Condvar,
}

impl Rendezvous {
    fn arrive(&self) -> bool {
        let mut n = lock(&self.arrived);
        *n += 1;
        self.cv.notify_all();
        let (n, _) = self
            .cv
            .wait_timeout_while(n, TIMEOUT, |n| *n < 2)
            .unwrap_or_else(PoisonError::into_inner);
        *n >= 2
    }
}

/// A worker's leaf-effect host: records every effect; MEET also waits for
/// the other consumer (failing the task if it never arrives).
struct ScenarioHost {
    worker: WorkerId,
    shared: Arc<Shared>,
}

impl Host for ScenarioHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(arg) = call.arg else {
            return Err(format!("non-number argument {:?}", call.arg));
        };
        if !(PRE..=OUTER).contains(&call.op) {
            return Err(format!("unknown host operation {}", call.op));
        }
        self.shared.record(Trace::Effect {
            task: call.task,
            worker: self.worker,
            thread: thread::current().id(),
            op: call.op,
            arg,
        });
        if call.op == MEET && !self.shared.meet.arrive() {
            return Err("the other consumer never reached MEET".to_string());
        }
        Ok(Primitive::Number(arg))
    }
}

/// The input: `[DEFN, [W, H], body, {LINE 1, COLUMN 1}]` with
/// `body = [LET, [SQ, [MUL, W, H]], inner]` and `inner = [DO, SQ, SQ]`.
/// Returns the form and its body and inner subtrees.
pub fn build_form(rt: &Runtime) -> (Rooted, Rooted, Rooted) {
    rt.scope(|s| {
        let n = |x: f64| s.primitive(Primitive::Number(x));
        let inner = s.new_vector(&[n(DO), n(SQ), n(SQ)]).unwrap();
        let product = s.new_vector(&[n(MUL), n(W), n(H)]).unwrap();
        let bindings = s.new_vector(&[n(SQ), product]).unwrap();
        let body = s.new_vector(&[n(LET), bindings, inner]).unwrap();
        let params = s.new_vector(&[n(W), n(H)]).unwrap();
        let meta = s
            .new_map(&[(n(LINE), n(1.0)), (n(COLUMN), n(1.0))])
            .unwrap();
        let form = s.new_vector(&[n(DEFN), params, body, meta]).unwrap();
        (s.persist(form), s.persist(body), s.persist(inner))
    })
}

fn compile(source: &str) -> Result<Executable, String> {
    compile_function(source).map_err(|e| format!("compile error: {e:?}"))
}

fn released(e: &Event, task: TaskId, outcome: OutcomeKind) -> bool {
    e.task == Some(task) && e.kind == EventKind::Released(outcome)
}

/// What a stage hands back once its executor is shut down.
struct Finished {
    rt: Runtime,
    events: Vec<Event>,
    worker_threads: Vec<ThreadId>,
    trace: Vec<Trace>,
}

/// The runtime, executor, input and host glue every scenario shares.
struct Stage {
    rt: Runtime,
    world: WorldId,
    key: DependencyKey,
    form: Rooted,
    body: Rooted,
    inner: Rooted,
    producer_exe: Executable,
    shared: Arc<Shared>,
    exec: Executor,
}

impl Stage {
    fn start() -> Result<Stage, String> {
        let rt = Runtime::new();
        let world = rt.new_world();
        let key = DependencyKey::new(world, HELPER, CALLABLE);
        let (form, body, inner) = build_form(&rt);
        let producer_exe = compile(include_str!("producer.js"))?;

        let shared = Arc::new(Shared::default());
        let hosts = Arc::clone(&shared);
        let driver_shared = Arc::clone(&shared);
        let exec = ExecutorConfig::new()
            .workers(2)
            .host(move |worker| {
                Box::new(ScenarioHost {
                    worker,
                    shared: Arc::clone(&hosts),
                })
            })
            // The default VM driver, plus what clj-quickjs-23 requires of a
            // driver running `__rt.need`: take the producer requests the run
            // claimed and hand them to the host's scheduler (here: hold
            // them). It also traces how the run ended when that is not a
            // value: suspension, cancellation cleanup, failure.
            .driver(move |guard, cx| {
                let id = guard.task_id();
                let outcome = guard.run_with(cx.budget, &mut *cx.host);
                let task = lock(&driver_shared.tasks).get(&id).cloned();
                if let Some(task) = task {
                    match &outcome {
                        RunOutcome::Waiting(key) => driver_shared.record(Trace::Suspended {
                            task: id,
                            worker: cx.worker,
                            key: *key,
                            at: task.source_location(),
                        }),
                        RunOutcome::Cancelled => driver_shared.record(Trace::Cancelled {
                            task: id,
                            worker: cx.worker,
                        }),
                        RunOutcome::Failed(e) => driver_shared.record(Trace::Failed {
                            task: id,
                            worker: cx.worker,
                            at: e.diagnostic().source,
                            message: e.message().to_string(),
                        }),
                        RunOutcome::Complete(_) | RunOutcome::Yielded => {}
                    }
                    for request in task.take_producer_requests() {
                        driver_shared.record(Trace::Requested {
                            task: id,
                            worker: cx.worker,
                            key: request.key(),
                            producer: request.producer(),
                        });
                        lock(&driver_shared.held).push((id, request));
                    }
                }
                outcome
            })
            // A woken or cancelled task goes to the worker that did not run
            // it last: every resumed consumer migrates, and so does a
            // cancelled one's cleanup run.
            .placement(|req| match (req.reason, req.last_worker) {
                (EnqueueReason::Woken | EnqueueReason::Cancelled, Some(last)) => {
                    Placement::Worker(WorkerId::new((last.index() + 1) % req.workers))
                }
                _ => Placement::Any,
            })
            .start(&rt)
            .map_err(|e| format!("cannot start the executor: {e}"))?;
        Ok(Stage {
            rt,
            world,
            key,
            form,
            body,
            inner,
            producer_exe,
            shared,
            exec,
        })
    }

    fn number(&self, x: f64) -> Rooted {
        self.rt.root(Primitive::Number(x))
    }

    fn spawn(&self, exe: &Executable, args: &[&Rooted], what: &str) -> Result<Task, String> {
        self.rt
            .spawn_in(self.world, exe, args)
            .map_err(|e| format!("spawn {what}: {e:?}"))
    }

    fn submit(&self, task: Task, placement: Placement) -> Result<TaskId, String> {
        let id = task.id();
        lock(&self.shared.tasks).insert(id, task.clone());
        self.exec
            .submit_with(task, placement)
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    fn task(&self, id: TaskId) -> Task {
        lock(&self.shared.tasks)[&id].clone()
    }

    /// Wait until each of `ids` has a run released with `outcome`.
    fn wait_released(&self, ids: &[TaskId], outcome: OutcomeKind) -> Result<(), String> {
        let all = |events: &[Event]| {
            ids.iter()
                .all(|&id| events.iter().any(|e| released(e, id, outcome)))
        };
        if self.exec.wait_for(TIMEOUT, all) {
            Ok(())
        } else {
            Err(format!(
                "{ids:?} did not end a run {outcome:?}: {:?}",
                self.exec.events()
            ))
        }
    }

    /// Step 3: one unrelated task per worker, run to completion.
    fn run_work(&self) -> Result<Vec<TaskId>, String> {
        let work_exe = compile(include_str!("work.js"))?;
        let mut work = Vec::new();
        for (i, (n, _)) in WORK.iter().enumerate() {
            let task = self.spawn(&work_exe, &[&self.number(*n)], "work")?;
            work.push(self.submit(task, Placement::Worker(WorkerId::new(i)))?);
        }
        for (&id, (_, expected)) in work.iter().zip(WORK) {
            match self.exec.take_outcome(id, TIMEOUT) {
                Some(RunOutcome::Complete(v)) if v.view() == ValueView::Number(expected) => {
                    self.shared.record(Trace::Finished {
                        task: id,
                        value: v.view(),
                    });
                }
                other => return Err(format!("work task {id}: {other:?}")),
            }
        }
        Ok(work)
    }

    /// Step 4: a forced collection on a worker.
    fn collect(&self) -> Result<CollectionEvent, String> {
        let before = count_collections(&self.exec.events());
        self.exec.schedule_collection(Placement::Any);
        let mut collection = None;
        self.exec.wait_for(TIMEOUT, |events| {
            collection = events.iter().find_map(|e| match &e.kind {
                EventKind::Collected(c) if c.number > before => Some(c.clone()),
                _ => None,
            });
            collection.is_some()
        });
        let collection = collection.ok_or("the scheduled collection did not happen")?;
        self.shared.record(Trace::Collected {
            number: collection.number,
            live: collection.stats.live,
            reclaimed: collection.stats.reclaimed,
        });
        Ok(collection)
    }

    /// The one producer request the runs claimed, taken from the host.
    fn take_request(&self) -> Result<(Vec<ProducerRequestInfo>, ProducerRequest), String> {
        let held = std::mem::take(&mut *lock(&self.shared.held));
        let requests: Vec<ProducerRequestInfo> = held
            .iter()
            .map(|(task, r)| ProducerRequestInfo {
                task: *task,
                key: r.key(),
                producer: r.producer(),
            })
            .collect();
        let [(_, request)] = <[_; 1]>::try_from(held)
            .map_err(|held| format!("expected one producer request, got {}", held.len()))?;
        Ok((requests, request))
    }

    /// Step 5: run the producer task with `slot` and take its outcome.
    fn produce(&self, slot: f64) -> Result<(TaskId, RunOutcome), String> {
        let task = self.spawn(&self.producer_exe, &[&self.number(slot)], "producer")?;
        let id = self.submit(task, Placement::Any)?;
        let outcome = self
            .exec
            .take_outcome(id, TIMEOUT)
            .ok_or_else(|| format!("producer {id} did not finish"))?;
        if let RunOutcome::Complete(v) = &outcome {
            self.shared.record(Trace::Finished {
                task: id,
                value: v.view(),
            });
        }
        Ok((id, outcome))
    }

    /// Publish `completion` for the held request once and wake the tasks it
    /// made runnable; returns their ids.
    fn publish(
        &self,
        request: &ProducerRequest,
        completion: Completion<'_>,
        producer_task: TaskId,
        published: PublishedAs,
    ) -> Result<Vec<TaskId>, String> {
        let wakeups = self
            .rt
            .complete(request.key(), request.producer(), completion)
            .map_err(|e| format!("publish for {}: {e:?}", request.key()))?;
        let woke = wakeups.task_ids();
        // Record before waking, so the trace shows publication before any
        // resumed effect.
        self.shared.record(Trace::Published {
            key: request.key(),
            producer_task,
            published,
            woke: woke.clone(),
        });
        self.exec.wake(wakeups).map_err(|e| e.to_string())?;
        Ok(woke)
    }

    /// Shut the executor down and hand back the runtime, the event record,
    /// the workers' threads and the host trace.
    fn finish(self) -> Result<Finished, String> {
        let events = self.exec.events();
        let threads = worker_threads(&events);
        self.exec
            .shutdown(TIMEOUT)
            .map_err(|e| format!("shutdown: {e:?}"))?;
        let trace = std::mem::take(&mut *lock(&self.shared.trace));
        lock(&self.shared.tasks).clear();
        Ok(Finished {
            rt: self.rt,
            events,
            worker_threads: threads,
            trace,
        })
    }
}

/// Run the success scenario ([`Variant::Success`]).
pub fn run() -> Result<Report, String> {
    run_variant(Variant::Success)
}

/// Run the scenario with `variant`'s ending. `Err` names the step that did
/// not happen in time or went differently than the variant requires.
pub fn run_variant(variant: Variant) -> Result<Report, String> {
    let stage = Stage::start()?;
    let consumer_exe = compile(include_str!("consumer.js"))?;

    // 1-2. Two consumers share the one form root and run on both workers.
    let mut consumer_ids = Vec::new();
    for spec in CONSUMERS {
        let (tag, depth) = (stage.number(spec.tag), stage.number(spec.depth));
        let task = stage.spawn(&consumer_exe, &[&stage.form, &tag, &depth], "consumer")?;
        consumer_ids.push(stage.submit(task, Placement::Worker(spec.start))?);
    }
    stage.wait_released(&consumer_ids, OutcomeKind::Waiting(stage.key))?;
    let cell_while_waiting = stage.rt.cell_status(stage.key);
    let suspended_at: Vec<Option<SourceLocation>> = consumer_ids
        .iter()
        .map(|&id| stage.task(id).source_location())
        .collect();
    // The driver took the claimed request before either run was released.
    let claimant = lock(&stage.shared.held)
        .first()
        .map(|(task, _)| *task)
        .ok_or("no consumer claimed the producer request")?;

    // Cancel variant: cancel the claimant while it is parked, and wait for
    // its cleanup run.
    let (cancelled, cell_after_cancel) = if variant == Variant::ConsumerCancel {
        stage
            .shared
            .record(Trace::CancelRequested { task: claimant });
        // `true`: it was parked and is now queued for its cleanup run.
        let enqueued = stage
            .exec
            .cancel(&stage.task(claimant))
            .map_err(|e| e.to_string())?;
        if !enqueued {
            return Err(format!("{claimant} was not parked when cancelled"));
        }
        stage.wait_released(&[claimant], OutcomeKind::Cancelled)?;
        (Some(claimant), Some(stage.rt.cell_status(stage.key)))
    } else {
        (None, None)
    };

    // 3. Useful work on both workers while the helper is held pending.
    let work = stage.run_work()?;

    // 4. A forced collection on a worker while the consumers are parked.
    let collection = stage.collect()?;
    let status_after_collection: Vec<TaskStatus> = consumer_ids
        .iter()
        .map(|&id| stage.task(id).status())
        .collect();

    // 5. Run the producer task and publish its outcome once.
    let (requests, request) = stage.take_request()?;
    let slot = match variant {
        Variant::ProducerFailure => BAD_SLOT,
        Variant::Success | Variant::ConsumerCancel => SLOT,
    };
    let (producer, outcome) = stage.produce(slot)?;
    let (published, published_value, producer_failure) = match (variant, outcome) {
        (Variant::ProducerFailure, RunOutcome::Failed(error)) => {
            // The failure the host publishes: where the producer threw.
            let at = error
                .diagnostic()
                .source
                .ok_or("the producer failure has no source position")?;
            let value = stage.rt.scope(|s| {
                let n = |x: u32| s.primitive(Primitive::Number(f64::from(x)));
                let v = s.new_vector(&[n(at.line), n(at.column)]).unwrap();
                s.persist(v)
            });
            (PublishedAs::Failure, value, Some(error))
        }
        (Variant::Success | Variant::ConsumerCancel, RunOutcome::Complete(helper)) => {
            (PublishedAs::Helper, helper, None)
        }
        (_, other) => return Err(format!("producer {producer}: {other:?}")),
    };
    let completion = match published {
        PublishedAs::Helper => Completion::Ready(&published_value),
        PublishedAs::Failure => Completion::Failed(&published_value),
    };
    stage.publish(&request, completion, producer, published)?;

    // 6. The consumers resume (on the other worker) and end; a cancelled
    // one already has.
    let mut consumers = Vec::new();
    for (((spec, &id), expected), (at, status)) in CONSUMERS
        .iter()
        .zip(&consumer_ids)
        .zip([stage.body.clone(), stage.inner.clone()])
        .zip(suspended_at.into_iter().zip(status_after_collection))
    {
        let end = match stage.exec.take_outcome(id, TIMEOUT) {
            Some(RunOutcome::Complete(v)) => {
                stage.shared.record(Trace::Finished {
                    task: id,
                    value: v.view(),
                });
                ConsumerEnd::Returned(v)
            }
            Some(RunOutcome::Cancelled) if cancelled == Some(id) => ConsumerEnd::Cancelled,
            other => return Err(format!("consumer {id}: {other:?}")),
        };
        consumers.push(ConsumerReport {
            spec: *spec,
            task: id,
            end,
            expected,
            suspended_at: at,
            status_after_collection: status,
        });
    }
    let cell_after = stage.rt.cell_status(stage.key);

    let (world, key, form) = (stage.world, stage.key, stage.form.clone());
    let Finished {
        rt,
        events,
        worker_threads,
        trace,
    } = stage.finish()?;
    Ok(Report {
        variant,
        rt,
        world,
        key,
        form,
        consumers,
        cell_while_waiting,
        cancelled,
        cell_after_cancel,
        requests,
        work,
        producer,
        producer_failure,
        published,
        published_value,
        cell_after,
        collection,
        worker_threads,
        events,
        trace,
    })
}

/// Run the companion scenario: one `companion.js` task, pinned to worker 0,
/// suspends at the helper's need inside `__rt.reduce`'s callback; a worker
/// collects while it is parked; the producer task's helper is published;
/// the task resumes on worker 1 and its fold completes.
pub fn run_companion() -> Result<CompanionReport, String> {
    let stage = Stage::start()?;
    let companion_exe = compile(include_str!("companion.js"))?;

    let tag = stage.number(COMPANION_TAG);
    let task = stage.spawn(&companion_exe, &[&stage.form, &tag], "companion")?;
    let id = stage.submit(task, Placement::Worker(WorkerId::new(0)))?;
    stage.wait_released(&[id], OutcomeKind::Waiting(stage.key))?;
    let suspended_at = stage.task(id).source_location();

    let collection = stage.collect()?;
    let status_after_collection = stage.task(id).status();

    let (requests, request) = stage.take_request()?;
    let helper = match stage.produce(SLOT)? {
        (producer, RunOutcome::Complete(helper)) => {
            stage.publish(
                &request,
                Completion::Ready(&helper),
                producer,
                PublishedAs::Helper,
            )?;
            (producer, helper)
        }
        (producer, other) => return Err(format!("producer {producer}: {other:?}")),
    };
    let result = match stage.exec.take_outcome(id, TIMEOUT) {
        Some(RunOutcome::Complete(v)) => v,
        other => return Err(format!("companion {id}: {other:?}")),
    };
    stage.shared.record(Trace::Finished {
        task: id,
        value: result.view(),
    });

    let (world, key, form) = (stage.world, stage.key, stage.form.clone());
    let expected = [stage.body.clone(), stage.inner.clone()];
    let Finished {
        rt,
        events,
        worker_threads,
        trace,
    } = stage.finish()?;
    Ok(CompanionReport {
        rt,
        world,
        key,
        form,
        task: id,
        result,
        expected,
        suspended_at,
        status_after_collection,
        requests,
        producer: helper.0,
        helper: helper.1,
        collection,
        worker_threads,
        events,
        trace,
    })
}

fn count_collections(events: &[Event]) -> u64 {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Collected(c) => Some(c.number),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// The OS thread of each worker, by index, from its `WorkerStarted` event.
fn worker_threads(events: &[Event]) -> Vec<ThreadId> {
    let mut threads: Vec<(WorkerId, ThreadId)> = events
        .iter()
        .filter_map(|e| match (e.worker, &e.kind) {
            (Some(w), EventKind::WorkerStarted { thread }) => Some((w, *thread)),
            _ => None,
        })
        .collect();
    threads.sort_by_key(|(w, _)| *w);
    threads.into_iter().map(|(_, t)| t).collect()
}
