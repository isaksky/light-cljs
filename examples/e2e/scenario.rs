//! The end-to-end synthetic compiler-host success scenario
//! (`docs/design.md` §13; pellet clj-quickjs-13), shared by
//! `cargo run --example e2e` and `tests/e2e.rs`.
//!
//! This file is example host/driver glue built only on the public embedding
//! API: the runtime, the reference executor (`exec`), dependency cells and
//! JavaScript source compiled by `frontend`. The program's control flow is
//! the JavaScript of `consumer.js`, `producer.js` and `work.js` (synthetic
//! fixtures, not light-cljs output) executed by the VM; the host below only
//! provides leaf effects, schedules tasks and publishes results.
//!
//! # The scenario
//!
//! 1. Rust builds and roots one nested persistent form-like input.
//! 2. Two consumer tasks of `consumer.js` receive the same form root and run
//!    on two executor workers, pinned one per worker. Each runs its pre-wait
//!    JavaScript (PRE effect, then a MEET effect that returns only when the
//!    other consumer is inside its own MEET: both execute at once) and
//!    reaches `__rt.need(7, 2)`, the CALLABLE result of definition 7 in the
//!    scenario's world. The first need claims the cell's single producer
//!    request (the driver takes it with `Task::take_producer_requests`); the
//!    host holds it, so both consumers park (`Waiting`) and leave their
//!    workers.
//! 3. Each worker runs one unrelated `work.js` task to completion while the
//!    consumers wait.
//! 4. A worker requests a collection (`Executor::schedule_collection`)
//!    while both consumers are parked.
//! 5. Only then the host runs the producer, a VM task of `producer.js`,
//!    which returns a helper function compiled by this runtime, and
//!    publishes it as the cell's result (`Runtime::complete`): the one
//!    completion wakes both consumers.
//! 6. The placement hook sends each woken consumer to the *other* worker.
//!    Each continues after its `need` (no pre-wait effect runs again), calls
//!    the shared helper and returns a subtree of the input with its
//!    original identity.
//!
//! Every wait of the host is bounded and synchronizes on the executor's
//! event record, terminal outcomes or the MEET rendezvous; nothing sleeps.
//! A timeout makes [`run`] return an error (or a MEET effect fail its task),
//! never hang.

// Shared by the example and the integration test; each uses a different
// part of the report.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};
use std::time::Duration;

use clj_quickjs::exec::{
    CollectionEvent, EnqueueReason, Event, EventKind, ExecutorConfig, OutcomeKind, Placement,
    WorkerId,
};
use clj_quickjs::frontend::compile_function;
use clj_quickjs::{
    CellStatus, Completion, DependencyId, DependencyKey, Executable, Host, HostCall, Primitive,
    ProducerId, ResultKind, Rooted, RunOutcome, Runtime, SourceLocation, Task, TaskId, TaskStatus,
    ValueView, WorldId,
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

/// Bound of every wait of the host, and of the MEET rendezvous.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The slot `producer.js`'s helper descends through: the body of each form.
pub const SLOT: f64 = 2.0;
/// `work.js` arguments, one task per worker, and their results.
pub const WORK: [(f64, f64); 2] = [(20.0, 2870.0), (30.0, 9455.0)];

/// What each consumer's REPORT effect must see: three noted values in its
/// task-owned builder (`seen`), then `scratch[0] * 10 + scratch[1]` (head
/// symbol 1, arity 2), all kept across the wait and the collection.
pub const EXPECTED_REPORT: f64 = 312.0;

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
    /// The host observed the forced collection while the consumers waited.
    Collected {
        number: u64,
        live: usize,
        reclaimed: usize,
    },
    /// The host published the producer task's result for `key`, waking
    /// `woke`.
    Published {
        key: DependencyKey,
        producer_task: TaskId,
        woke: Vec<TaskId>,
    },
    /// A task's terminal outcome, taken by the host.
    Finished { task: TaskId, value: ValueView },
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
            } => {
                let name = match *op {
                    PRE => "PRE",
                    MEET => "MEET",
                    POST => "POST",
                    REPORT => "REPORT",
                    _ => "?",
                };
                write!(f, "{task} on {worker} ({thread:?}): effect {name}({arg})")
            }
            Trace::Suspended {
                task,
                worker,
                key,
                at,
            } => {
                write!(f, "{task} on {worker}: suspended on {key}")?;
                match at {
                    Some(at) => write!(f, " at consumer.js:{}:{}", at.line, at.column),
                    None => Ok(()),
                }
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
            Trace::Collected {
                number,
                live,
                reclaimed,
            } => write!(
                f,
                "collection #{number} while consumers wait: {live} live, {reclaimed} reclaimed"
            ),
            Trace::Published {
                key,
                producer_task,
                woke,
            } => write!(
                f,
                "published {producer_task}'s helper for {key}; woke {}",
                woke.iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Trace::Finished { task, value } => write!(f, "{task} finished: {value:?}"),
        }
    }
}

/// A consumer after the scenario.
pub struct ConsumerReport {
    pub spec: ConsumerSpec,
    pub task: TaskId,
    /// Its returned value.
    pub result: Rooted,
    /// The subtree of the input it should return, navigated by Rust.
    pub expected: Rooted,
    /// The source location it was suspended at, while parked.
    pub suspended_at: Option<SourceLocation>,
    /// Its status when the collection had completed.
    pub status_after_collection: TaskStatus,
}

/// Everything the scenario observed.
pub struct Report {
    pub rt: Runtime,
    pub world: WorldId,
    /// The dependency both consumers needed.
    pub key: DependencyKey,
    /// The Rust-built input, rooted by the host.
    pub form: Rooted,
    pub consumers: Vec<ConsumerReport>,
    /// The cell while both consumers waited.
    pub cell_while_waiting: CellStatus,
    /// Every producer request the consumers' runs claimed.
    pub requests: Vec<ProducerRequestInfo>,
    pub work: Vec<TaskId>,
    pub producer: TaskId,
    pub collection: CollectionEvent,
    /// The workers' OS threads, by worker index.
    pub worker_threads: Vec<ThreadId>,
    /// The executor's event record.
    pub events: Vec<Event>,
    /// The host's observations.
    pub trace: Vec<Trace>,
}

/// A claimed producer request, as the host saw it.
#[derive(Clone, Copy, Debug)]
pub struct ProducerRequestInfo {
    pub task: TaskId,
    pub key: DependencyKey,
    pub producer: ProducerId,
}

impl Report {
    /// The host effects of `task`, in order.
    pub fn effects_of(&self, task: TaskId) -> Vec<(WorkerId, ThreadId, u16, f64)> {
        self.trace
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
    pub fn events_of(&self, task: TaskId) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| e.task == Some(task))
            .collect()
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
    held: Mutex<Vec<(TaskId, clj_quickjs::ProducerRequest)>>,
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
        if !(PRE..=REPORT).contains(&call.op) {
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

/// Run the scenario. `Err` names the step that did not happen in time or
/// went differently than the scenario requires.
pub fn run() -> Result<Report, String> {
    let rt = Runtime::new();
    let world = rt.new_world();
    let key = DependencyKey::new(world, HELPER, CALLABLE);
    let (form, body, inner) = build_form(&rt);

    let consumer_exe = compile(include_str!("consumer.js"))?;
    let producer_exe = compile(include_str!("producer.js"))?;
    let work_exe = compile(include_str!("work.js"))?;

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
        // claimed and hand them to the host's scheduler (here: hold them).
        .driver(move |guard, cx| {
            let id = guard.task_id();
            let outcome = guard.run_with(cx.budget, &mut *cx.host);
            let task = lock(&driver_shared.tasks).get(&id).cloned();
            if let Some(task) = task {
                if let RunOutcome::Waiting(key) = outcome {
                    driver_shared.record(Trace::Suspended {
                        task: id,
                        worker: cx.worker,
                        key,
                        at: task.source_location(),
                    });
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
        // A woken task goes to the worker that did not run it last: every
        // resumed consumer migrates.
        .placement(|req| match (req.reason, req.last_worker) {
            (EnqueueReason::Woken, Some(last)) => {
                Placement::Worker(WorkerId::new((last.index() + 1) % req.workers))
            }
            _ => Placement::Any,
        })
        .start(&rt)
        .map_err(|e| format!("cannot start the executor: {e}"))?;

    let submit = |task: Task, placement: Placement| -> Result<TaskId, String> {
        let id = task.id();
        lock(&shared.tasks).insert(id, task.clone());
        exec.submit_with(task, placement)
            .map_err(|e| e.to_string())?;
        Ok(id)
    };
    let number = |x: f64| rt.root(Primitive::Number(x));

    // 1-2. Two consumers share the one form root and run on both workers.
    let mut consumer_ids = Vec::new();
    for spec in CONSUMERS {
        let (tag, depth) = (number(spec.tag), number(spec.depth));
        let task = rt
            .spawn_in(world, &consumer_exe, &[&form, &tag, &depth])
            .map_err(|e| format!("spawn consumer: {e:?}"))?;
        consumer_ids.push(submit(task, Placement::Worker(spec.start))?);
    }
    let both_waiting = |events: &[Event]| {
        consumer_ids.iter().all(|&id| {
            events
                .iter()
                .any(|e| released(e, id, OutcomeKind::Waiting(key)))
        })
    };
    if !exec.wait_for(TIMEOUT, both_waiting) {
        return Err(format!(
            "both consumers did not suspend on {key}: {:?}",
            exec.events()
        ));
    }
    let cell_while_waiting = rt.cell_status(key);
    let suspended_at: Vec<Option<SourceLocation>> = consumer_ids
        .iter()
        .map(|id| lock(&shared.tasks)[id].source_location())
        .collect();

    // 3. Useful work on both workers while the helper is held pending.
    let mut work = Vec::new();
    for (i, (n, _)) in WORK.iter().enumerate() {
        let task = rt
            .spawn_in(world, &work_exe, &[&number(*n)])
            .map_err(|e| format!("spawn work: {e:?}"))?;
        work.push(submit(task, Placement::Worker(WorkerId::new(i)))?);
    }
    for (&id, (_, expected)) in work.iter().zip(WORK) {
        match exec.take_outcome(id, TIMEOUT) {
            Some(RunOutcome::Complete(v)) if v.view() == ValueView::Number(expected) => {
                shared.record(Trace::Finished {
                    task: id,
                    value: v.view(),
                });
            }
            other => return Err(format!("work task {id}: {other:?}")),
        }
    }

    // 4. A forced collection on a worker while both consumers are parked.
    let collections_before = count_collections(&exec.events());
    exec.schedule_collection(Placement::Any);
    let mut collection = None;
    exec.wait_for(TIMEOUT, |events| {
        collection = events.iter().find_map(|e| match &e.kind {
            EventKind::Collected(c) if c.number > collections_before => Some(c.clone()),
            _ => None,
        });
        collection.is_some()
    });
    let collection = collection.ok_or("the scheduled collection did not happen")?;
    shared.record(Trace::Collected {
        number: collection.number,
        live: collection.stats.live,
        reclaimed: collection.stats.reclaimed,
    });
    let status_after_collection: Vec<TaskStatus> = consumer_ids
        .iter()
        .map(|id| lock(&shared.tasks)[id].status())
        .collect();

    // 5. Produce the helper with a VM task and publish it once.
    let held = std::mem::take(&mut *lock(&shared.held));
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
    let producer_task = rt
        .spawn_in(world, &producer_exe, &[&number(SLOT)])
        .map_err(|e| format!("spawn producer: {e:?}"))?;
    let producer = submit(producer_task, Placement::Any)?;
    let helper = match exec.take_outcome(producer, TIMEOUT) {
        Some(RunOutcome::Complete(helper)) => helper,
        other => return Err(format!("producer {producer}: {other:?}")),
    };
    shared.record(Trace::Finished {
        task: producer,
        value: helper.view(),
    });
    let wakeups = rt
        .complete(
            request.key(),
            request.producer(),
            Completion::Ready(&helper),
        )
        .map_err(|e| format!("publish the helper: {e:?}"))?;
    let woke = wakeups.task_ids();
    // Record before waking, so the trace shows publication before any
    // resumed effect.
    shared.record(Trace::Published {
        key,
        producer_task: producer,
        woke,
    });
    exec.wake(wakeups).map_err(|e| e.to_string())?;

    // 6. Both consumers resume (on the other worker) and return.
    let mut consumers = Vec::new();
    for (((spec, &id), expected), (at, status)) in CONSUMERS
        .iter()
        .zip(&consumer_ids)
        .zip([body, inner])
        .zip(suspended_at.into_iter().zip(status_after_collection))
    {
        let result = match exec.take_outcome(id, TIMEOUT) {
            Some(RunOutcome::Complete(v)) => v,
            other => return Err(format!("consumer {id}: {other:?}")),
        };
        shared.record(Trace::Finished {
            task: id,
            value: result.view(),
        });
        consumers.push(ConsumerReport {
            spec: *spec,
            task: id,
            result,
            expected,
            suspended_at: at,
            status_after_collection: status,
        });
    }

    let events = exec.events();
    let worker_threads = worker_threads(&events);
    exec.shutdown(TIMEOUT)
        .map_err(|e| format!("shutdown: {e:?}"))?;
    let trace = std::mem::take(&mut *lock(&shared.trace));
    lock(&shared.tasks).clear();
    Ok(Report {
        rt,
        world,
        key,
        form,
        consumers,
        cell_while_waiting,
        requests,
        work,
        producer,
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
