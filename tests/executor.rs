//! The two-worker reference executor (pellet clj-quickjs-8): real OS worker
//! threads driving the public run API.
//!
//! Every wait is bounded: tests synchronize on the executor's event record
//! ([`Executor::wait_for`]), on terminal outcomes
//! ([`Executor::take_outcome`]), and on bounded latches/rendezvous inside
//! leaf host effects; never on sleeps. A rendezvous or latch that times out
//! fails its task, so a broken executor fails these tests instead of hanging.
//!
//! Programs: unrelated work and the endless task are the synthetic source
//! fixtures `fixtures/js/executor_work.js` and `fixtures/js/executor_spin.js`
//! (not light-cljs output). Tasks that perform host effects are
//! hand-assembled bytecode (the source subset has no host effects): early VM
//! test fixtures, not JavaScript source support. Needs are issued by the
//! executor's driver through the run guard, standing in for the `__rt.need`
//! VM operation of pellet clj-quickjs-23.

use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, ThreadId};
use std::time::Duration;

use clj_quickjs::exec::{
    EnqueueReason, Event, EventKind, ExecutorConfig, OutcomeKind, Placement, RunContext, WorkerId,
};
use clj_quickjs::frontend::compile_script;
use clj_quickjs::{
    Assembler, Budget, Cancel, Completion, DependencyId, DependencyKey, Host, HostCall, Need,
    NeedError, Opcode, Primitive, ResultKind, RunGuard, RunOutcome, Runtime, Task, TaskId,
    TaskStatus, ValueView,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const DEF: DependencyId = DependencyId::new(81);
const KIND: ResultKind = ResultKind::new(3);

/// Host operations of [`TestHost`].
const LOG: u16 = 1;
const MEET: u16 = 2;
const GATE: u16 = 3;

/// What `executor_work.js` completes with.
const WORK_RESULT: f64 = 2870.0;

// ---------------------------------------------------------------------------
// Bounded synchronization used inside host effects.

/// Opens once; waiting for it is bounded by [`TIMEOUT`].
#[derive(Default)]
struct Latch {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Latch {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }

    fn wait(&self) -> bool {
        let open = self.open.lock().unwrap();
        *self
            .cv
            .wait_timeout_while(open, TIMEOUT, |o| !*o)
            .unwrap()
            .0
    }
}

/// A two-party rendezvous; waiting is bounded by [`TIMEOUT`].
#[derive(Default)]
struct Rendezvous {
    arrived: Mutex<usize>,
    cv: Condvar,
}

impl Rendezvous {
    fn arrive(&self) -> bool {
        let mut n = self.arrived.lock().unwrap();
        *n += 1;
        self.cv.notify_all();
        *self
            .cv
            .wait_timeout_while(n, TIMEOUT, |n| *n < 2)
            .unwrap()
            .0
            >= 2
    }
}

/// One executed host effect.
#[derive(Clone, Debug)]
struct Effect {
    task: TaskId,
    /// `None` for the embedder's own thread (no executor).
    worker: Option<WorkerId>,
    thread: ThreadId,
    op: u16,
    arg: f64,
}

/// State shared by every worker's host.
#[derive(Clone, Default)]
struct Hosts {
    effects: Arc<Mutex<Vec<Effect>>>,
    meet: Arc<Rendezvous>,
    gate: Arc<Latch>,
}

impl Hosts {
    fn factory(&self) -> impl Fn(WorkerId) -> Box<dyn Host> + Send + Sync + 'static {
        let hosts = self.clone();
        move |worker| Box::new(hosts.host(Some(worker)))
    }

    fn host(&self, worker: Option<WorkerId>) -> TestHost {
        TestHost {
            worker,
            hosts: self.clone(),
        }
    }

    fn effects_of(&self, task: TaskId) -> Vec<Effect> {
        let effects = self.effects.lock().unwrap();
        effects.iter().filter(|e| e.task == task).cloned().collect()
    }
}

/// `LOG(x)` records and returns `x`; `MEET(x)` also waits for a second task
/// inside its effect; `GATE(x)` also waits for the test to open the gate.
struct TestHost {
    worker: Option<WorkerId>,
    hosts: Hosts,
}

impl Host for TestHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(arg) = call.arg else {
            return Err(format!("non-number argument {:?}", call.arg));
        };
        self.hosts.effects.lock().unwrap().push(Effect {
            task: call.task,
            worker: self.worker,
            thread: thread::current().id(),
            op: call.op,
            arg,
        });
        let ok = match call.op {
            LOG => true,
            MEET => self.hosts.meet.arrive(),
            GATE => self.hosts.gate.wait(),
            op => return Err(format!("unknown host op {op}")),
        };
        if ok {
            Ok(call.arg)
        } else {
            Err(format!("host op {} timed out", call.op))
        }
    }
}

// ---------------------------------------------------------------------------
// Programs.

/// Early VM test program: `return op(arg)`.
fn effect_task(rt: &Runtime, op: u16, arg: f64) -> Task {
    let mut asm = Assembler::new();
    asm.get_local(0).host_effect(op).op(Opcode::Return);
    let exe = asm.finish("effect", 1, 1).unwrap();
    rt.spawn(&exe, &[&rt.root(Primitive::Number(arg))]).unwrap()
}

/// Instructions of [`consumer`] before its need point.
const PREFIX: u64 = 3;

/// Early VM test program: `log(arg); <need point>; return log(arg + 100)`.
/// The need point is the budget boundary after [`PREFIX`] instructions, where
/// [`consumer_driver`] needs `DEF`/`KIND`.
fn consumer(rt: &Runtime, arg: f64) -> Task {
    let mut asm = Assembler::new();
    asm.get_local(0).host_effect(LOG).op(Opcode::Pop);
    asm.get_local(0)
        .constant(Primitive::Number(100.0))
        .op(Opcode::Add)
        .host_effect(LOG)
        .op(Opcode::Return);
    let exe = asm.finish("consumer", 1, 1).unwrap();
    rt.spawn(&exe, &[&rt.root(Primitive::Number(arg))]).unwrap()
}

fn source_task(rt: &Runtime, name: &str, source: &str) -> Task {
    rt.spawn(&compile_script(name, source).unwrap(), &[])
        .unwrap()
}

fn work(rt: &Runtime) -> Task {
    source_task(
        rt,
        "executor_work",
        include_str!("fixtures/js/executor_work.js"),
    )
}

fn spin(rt: &Runtime) -> Task {
    source_task(
        rt,
        "executor_spin",
        include_str!("fixtures/js/executor_spin.js"),
    )
}

/// The VM driver for every task except `consumers`, whose first run executes
/// the prefix and whose later runs need `DEF`/`KIND`: park while it is
/// pending, run the rest once it is ready. A cancelled consumer's need is
/// refused and its park performs terminal cleanup.
fn consumer_driver(
    consumers: HashSet<TaskId>,
) -> impl Fn(RunGuard<'_>, &mut RunContext<'_>) -> RunOutcome + Send + Sync + 'static {
    move |mut guard: RunGuard<'_>, cx: &mut RunContext<'_>| {
        if !consumers.contains(&guard.task_id()) {
            return guard.run_with(cx.budget, &mut *cx.host);
        }
        if guard.wake_handle().execution() == 1 {
            return guard.run_with(Budget::instructions(PREFIX), &mut *cx.host);
        }
        match guard.need(DEF, KIND) {
            Ok(Need::Waiting { produce: None, .. }) => guard.park(),
            Ok(Need::Ready(_)) => guard.run_with(cx.budget, &mut *cx.host),
            Err(NeedError::Cancelled { .. }) => guard.park(),
            other => panic!("unexpected need result {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Event helpers.

fn position(events: &[Event], task: TaskId, kind: impl Fn(&EventKind) -> bool) -> Option<usize> {
    events
        .iter()
        .position(|e| e.task == Some(task) && kind(&e.kind))
}

fn parked(events: &[Event], task: TaskId) -> bool {
    position(events, task, |k| {
        matches!(k, EventKind::Released(OutcomeKind::Waiting(_)))
    })
    .is_some()
}

fn dispatched(events: &[Event], task: TaskId) -> bool {
    position(events, task, |k| matches!(k, EventKind::Dispatched { .. })).is_some()
}

/// `(worker, execution)` of each dispatch of `task`, in order.
fn dispatches(events: &[Event], task: TaskId) -> Vec<(WorkerId, u64)> {
    events
        .iter()
        .filter(|e| e.task == Some(task))
        .filter_map(|e| match e.kind {
            EventKind::Dispatched { execution } => Some((e.worker.unwrap(), execution)),
            _ => None,
        })
        .collect()
}

fn releases(events: &[Event], task: TaskId) -> Vec<OutcomeKind> {
    events
        .iter()
        .filter(|e| e.task == Some(task))
        .filter_map(|e| match e.kind {
            EventKind::Released(kind) => Some(kind),
            _ => None,
        })
        .collect()
}

/// For every task, dispatches and releases alternate, each pair on one
/// worker: the record never shows a task owned twice at once.
fn assert_single_owner(events: &[Event]) {
    let tasks: HashSet<TaskId> = events.iter().filter_map(|e| e.task).collect();
    for task in tasks {
        let mut owner: Option<WorkerId> = None;
        for e in events.iter().filter(|e| e.task == Some(task)) {
            match e.kind {
                EventKind::Dispatched { .. } => {
                    assert_eq!(owner, None, "{task} dispatched twice at once: {events:#?}");
                    owner = e.worker;
                }
                EventKind::Released(_) | EventKind::Panicked => {
                    assert_eq!(owner, e.worker, "{task} released by a non-owner");
                    owner = None;
                }
                _ => {}
            }
        }
        assert_eq!(owner, None, "{task} still owned at the end of the record");
    }
}

fn complete_number(outcome: Option<RunOutcome>) -> f64 {
    match outcome {
        Some(RunOutcome::Complete(v)) => match v.view() {
            ValueView::Number(n) => n,
            other => panic!("expected a number, got {other:?}"),
        },
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn w(index: usize) -> WorkerId {
    WorkerId::new(index)
}

// ---------------------------------------------------------------------------
// Tests.

/// Two tasks meet inside a host effect, i.e. in the middle of executing VM
/// instructions with their execution claims held. The meeting can only
/// complete if two distinct OS workers are executing at the same time, so
/// no global lock serializes instruction execution.
#[test]
fn two_workers_execute_vm_instructions_at_the_same_time() {
    let rt = Runtime::new();
    let hosts = Hosts::default();
    let a = effect_task(&rt, MEET, 1.0);
    let b = effect_task(&rt, MEET, 2.0);
    let exec = ExecutorConfig::new()
        .host(hosts.factory())
        .start(&rt)
        .unwrap();
    assert_eq!(exec.workers(), 2);
    exec.submit(a.clone()).unwrap();
    exec.submit(b.clone()).unwrap();
    assert_eq!(complete_number(exec.take_outcome(a.id(), TIMEOUT)), 1.0);
    assert_eq!(complete_number(exec.take_outcome(b.id(), TIMEOUT)), 2.0);

    let (ea, eb) = (&hosts.effects_of(a.id())[0], &hosts.effects_of(b.id())[0]);
    assert_ne!(ea.worker, eb.worker);
    assert_ne!(ea.thread, eb.thread);
    let events = exec.events();
    // The effects ran on the workers' own recorded OS threads.
    for effect in [ea, eb] {
        let started = events.iter().find(|e| e.worker == effect.worker);
        let Some(Event {
            kind: EventKind::WorkerStarted { thread },
            ..
        }) = started
        else {
            panic!("no start event for {:?}", effect.worker)
        };
        assert_eq!(*thread, effect.thread);
    }
    // Both claims were held at once: each dispatch precedes the other's release.
    let at = |t: TaskId, f: fn(&EventKind) -> bool| position(&events, t, f).unwrap();
    let is_dispatch = |k: &EventKind| matches!(k, EventKind::Dispatched { .. });
    let is_release = |k: &EventKind| matches!(k, EventKind::Released(_));
    assert!(at(a.id(), is_dispatch) < at(b.id(), is_release));
    assert!(at(b.id(), is_dispatch) < at(a.id(), is_release));
    assert_single_owner(&events);
    exec.shutdown(TIMEOUT).unwrap();
}

/// Consumers park on a pending dependency (occupying no worker), an
/// unrelated source task completes its work meanwhile, and only then is the
/// result published. The placement hook sends each woken consumer to the
/// other worker: it resumes at its saved instruction, so its first effect
/// ran exactly once (before the park, on worker 0) and its second once
/// (after resuming, on worker 1).
#[test]
fn parked_consumers_resume_on_another_worker_without_replaying_effects() {
    let rt = Runtime::new();
    let key = DependencyKey::new(rt.world(), DEF, KIND);
    let producer = rt.claim(key).unwrap();
    let hosts = Hosts::default();
    let c1 = consumer(&rt, 1.0);
    let c2 = consumer(&rt, 2.0);
    let unrelated = work(&rt);
    let exec = ExecutorConfig::new()
        .budget(Budget::instructions(25))
        .host(hosts.factory())
        .driver(consumer_driver([c1.id(), c2.id()].into()))
        .placement(|r| match (r.reason, r.last_worker) {
            // A task that waited resumes on the worker after the one it parked on.
            (EnqueueReason::Woken, Some(last)) => {
                Placement::Worker(w((last.index() + 1) % r.workers))
            }
            // Budget yields stay put, keeping the trace deterministic.
            (EnqueueReason::Yielded, Some(last)) => Placement::Worker(last),
            _ => Placement::Any,
        })
        .start(&rt)
        .unwrap();
    exec.submit_with(c1.clone(), Placement::Worker(w(0)))
        .unwrap();
    exec.submit_with(c2.clone(), Placement::Worker(w(0)))
        .unwrap();
    assert!(
        exec.wait_for(TIMEOUT, |ev| parked(ev, c1.id()) && parked(ev, c2.id())),
        "{:#?}",
        exec.events()
    );
    assert_eq!(c1.status(), TaskStatus::Waiting);
    assert_eq!(c2.status(), TaskStatus::Waiting);

    // Useful work while both consumers wait.
    exec.submit(unrelated.clone()).unwrap();
    let done = exec.take_outcome(unrelated.id(), TIMEOUT);
    assert_eq!(complete_number(done), WORK_RESULT);
    assert_eq!(c1.status(), TaskStatus::Waiting);

    // Only now is the result published; its wakeups go to the executor.
    let value = rt.root(Primitive::Number(42.0));
    let wakeups = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(exec.wake(wakeups).unwrap(), 2);
    for (c, arg) in [(&c1, 1.0), (&c2, 2.0)] {
        assert_eq!(
            complete_number(exec.take_outcome(c.id(), TIMEOUT)),
            arg + 100.0
        );
    }

    let events = exec.events();
    assert_single_owner(&events);
    let work_done = position(&events, unrelated.id(), |k| {
        *k == EventKind::Released(OutcomeKind::Completed)
    })
    .unwrap();
    for (c, arg) in [(&c1, 1.0), (&c2, 2.0)] {
        let park = position(&events, c.id(), |k| {
            *k == EventKind::Released(OutcomeKind::Waiting(key))
        })
        .unwrap();
        let woken = position(&events, c.id(), |k| {
            matches!(
                k,
                EventKind::Enqueued {
                    reason: EnqueueReason::Woken,
                    ..
                }
            )
        })
        .unwrap();
        assert!(park < work_done && work_done < woken, "{events:#?}");
        // Prefix and park on worker 0; the resumed run on worker 1.
        assert_eq!(
            dispatches(&events, c.id()),
            [(w(0), 1), (w(0), 2), (w(1), 3)]
        );
        assert_eq!(
            releases(&events, c.id()),
            [
                OutcomeKind::Yielded,
                OutcomeKind::Waiting(key),
                OutcomeKind::Completed
            ]
        );
        let effects = hosts.effects_of(c.id());
        let seen: Vec<(Option<WorkerId>, u16, f64)> =
            effects.iter().map(|e| (e.worker, e.op, e.arg)).collect();
        assert_eq!(
            seen,
            [(Some(w(0)), LOG, arg), (Some(w(1)), LOG, arg + 100.0)]
        );
        assert_ne!(effects[0].thread, effects[1].thread);
    }
    let down = exec.shutdown(TIMEOUT).unwrap();
    assert!(down.pending.is_empty() && down.parked.is_empty());
}

/// A parked consumer and an endlessly running task are cancelled through the
/// executor. Each cancellation is observed by the task's own run (the parked
/// one is enqueued for it; the running one sees it at its next budget
/// boundary), which cleans up and ends `Cancelled`. The producer's later
/// completion does not resurrect the cancelled consumer and still resumes
/// the other one.
#[test]
fn cancellation_of_parked_and_running_tasks_is_observed_by_their_own_runs() {
    let rt = Runtime::new();
    let key = DependencyKey::new(rt.world(), DEF, KIND);
    let producer = rt.claim(key).unwrap();
    let hosts = Hosts::default();
    let c1 = consumer(&rt, 1.0);
    let c2 = consumer(&rt, 2.0);
    let endless = spin(&rt);
    let exec = ExecutorConfig::new()
        .budget(Budget::instructions(50))
        .host(hosts.factory())
        .driver(consumer_driver([c1.id(), c2.id()].into()))
        .start(&rt)
        .unwrap();
    exec.submit(c1.clone()).unwrap();
    exec.submit(c2.clone()).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| parked(ev, c1.id()) && parked(ev, c2.id())));
    exec.submit(endless.clone()).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| { releases(ev, endless.id()).len() >= 2 }));

    // Parked: the cancellation makes it runnable and the executor enqueues it.
    assert!(exec.cancel(&c1).unwrap());
    assert!(matches!(
        exec.take_outcome(c1.id(), TIMEOUT),
        Some(RunOutcome::Cancelled)
    ));
    assert_eq!(c1.status(), TaskStatus::Cancelled);
    assert!(!exec.cancel(&c1).unwrap(), "already finished");

    // Running (or queued between runs): observed at a budget boundary.
    assert!(!exec.cancel(&endless).unwrap());
    assert!(matches!(
        exec.take_outcome(endless.id(), TIMEOUT),
        Some(RunOutcome::Cancelled)
    ));
    assert_eq!(endless.status(), TaskStatus::Cancelled);

    // The producer is unaffected: its completion resumes only c2.
    let value = rt.root(Primitive::Number(7.0));
    let wakeups = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(wakeups.task_ids(), [c2.id()]);
    assert_eq!(exec.wake(wakeups).unwrap(), 1);
    assert_eq!(complete_number(exec.take_outcome(c2.id(), TIMEOUT)), 102.0);

    let events = exec.events();
    assert_single_owner(&events);
    assert!(
        position(&events, c1.id(), |k| matches!(
            k,
            EventKind::Enqueued {
                reason: EnqueueReason::Cancelled,
                ..
            }
        ))
        .is_some()
    );
    // Prefix, park, cleanup: the cancelled consumer never ran its second effect.
    assert_eq!(
        releases(&events, c1.id()),
        [
            OutcomeKind::Yielded,
            OutcomeKind::Waiting(key),
            OutcomeKind::Cancelled
        ]
    );
    assert_eq!(hosts.effects_of(c1.id()).len(), 1);
    let down = exec.shutdown(TIMEOUT).unwrap();
    assert!(down.pending.is_empty() && down.parked.is_empty());
}

/// Shutdown with work pending: both workers are inside gated runs, three
/// tasks are queued behind them and a consumer is parked. Shutdown lets the
/// in-flight runs finish and returns the queued tasks (never run, still
/// runnable) and the parked one, which the embedder then drives itself on
/// its own thread: nothing depends on the executor's threads.
#[test]
fn shutdown_returns_pending_and_parked_tasks_to_the_embedder() {
    let rt = Runtime::new();
    let key = DependencyKey::new(rt.world(), DEF, KIND);
    let producer = rt.claim(key).unwrap();
    let hosts = Hosts::default();
    let c = consumer(&rt, 5.0);
    let gates = [effect_task(&rt, GATE, 10.0), effect_task(&rt, GATE, 11.0)];
    let queued: Vec<Task> = (0..3).map(|_| work(&rt)).collect();
    let exec = ExecutorConfig::new()
        .host(hosts.factory())
        .driver(consumer_driver([c.id()].into()))
        .start(&rt)
        .unwrap();
    exec.submit(c.clone()).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| parked(ev, c.id())));
    for (i, gate) in gates.iter().enumerate() {
        exec.submit_with(gate.clone(), Placement::Worker(w(i)))
            .unwrap();
    }
    assert!(exec.wait_for(TIMEOUT, |ev| gates.iter().all(|g| dispatched(ev, g.id()))));
    for t in &queued {
        exec.submit(t.clone()).unwrap();
    }
    exec.begin_shutdown();
    hosts.gate.open();
    let mut down = exec.shutdown(TIMEOUT).unwrap();

    for (gate, arg) in gates.iter().zip([10.0, 11.0]) {
        assert_eq!(complete_number(down.outcomes.remove(&gate.id())), arg);
    }
    let pending: Vec<TaskId> = down.pending.iter().map(Task::id).collect();
    let expected: Vec<TaskId> = queued.iter().map(Task::id).collect();
    assert_eq!(pending, expected);
    for t in &down.pending {
        assert_eq!(t.status(), TaskStatus::Ready);
        assert!(!dispatched(&down.events, t.id()));
    }
    let parked_ids: Vec<TaskId> = down.parked.iter().map(Task::id).collect();
    assert_eq!(parked_ids, [c.id()]);
    let stopped = down
        .events
        .iter()
        .filter(|e| e.kind == EventKind::WorkerStopped)
        .count();
    assert_eq!(stopped, 2);
    assert_single_owner(&down.events);

    // The embedder finishes everything itself.
    for t in &down.pending {
        assert_eq!(complete_number(Some(t.run().unwrap())), WORK_RESULT);
    }
    let value = rt.root(Primitive::Number(0.0));
    let wakeups = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    let runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    let mut guard = runnable[0].try_begin_run().unwrap();
    assert!(matches!(guard.need(DEF, KIND), Ok(Need::Ready(_))));
    let mut host = hosts.host(None);
    let outcome = guard.run_with(Budget::UNLIMITED, &mut host);
    assert_eq!(complete_number(Some(outcome)), 105.0);
    let seen: Vec<_> = hosts
        .effects_of(c.id())
        .iter()
        .map(|e| (e.worker, e.arg))
        .collect();
    // The prefix effect ran once on a worker; the rest on the embedder's thread.
    assert_eq!(seen.len(), 2);
    assert!(seen[0].0.is_some() && seen[0].1 == 5.0, "{seen:?}");
    assert_eq!(seen[1], (None, 105.0));
}

/// Shutdown is bounded: a worker blocked inside a run makes it time out,
/// naming the busy worker and task, and hands the executor back; once the
/// run can finish, a retry succeeds and returns the queued task unrun.
#[test]
fn shutdown_times_out_while_a_worker_is_busy_and_can_be_retried() {
    let rt = Runtime::new();
    let hosts = Hosts::default();
    let gate = effect_task(&rt, GATE, 1.0);
    let endless = spin(&rt);
    let exec = ExecutorConfig::new()
        .workers(1)
        .budget(Budget::instructions(50))
        .host(hosts.factory())
        .start(&rt)
        .unwrap();
    exec.submit(gate.clone()).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| dispatched(ev, gate.id())));
    exec.submit(endless.clone()).unwrap();

    let timeout = exec.shutdown(Duration::from_millis(50)).unwrap_err();
    assert_eq!(timeout.busy, [(w(0), gate.id())]);
    hosts.gate.open();
    let mut down = timeout.executor.shutdown(TIMEOUT).unwrap();
    assert_eq!(complete_number(down.outcomes.remove(&gate.id())), 1.0);
    let pending: Vec<TaskId> = down.pending.iter().map(Task::id).collect();
    assert_eq!(pending, [endless.id()]);
    assert!(!dispatched(&down.events, endless.id()));
}

/// An endless task in flight does not hold shutdown up: its run ends at the
/// budget, and the task comes back runnable with its VM state, for the
/// embedder to cancel (or run) by hand.
#[test]
fn an_endless_task_in_flight_is_returned_at_shutdown() {
    let rt = Runtime::new();
    let endless = spin(&rt);
    let exec = ExecutorConfig::new()
        .budget(Budget::instructions(50))
        .start(&rt)
        .unwrap();
    exec.submit(endless.clone()).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| !releases(ev, endless.id()).is_empty()));
    let down = exec.shutdown(TIMEOUT).unwrap();
    let pending: Vec<TaskId> = down.pending.iter().map(Task::id).collect();
    assert_eq!(pending, [endless.id()]);
    assert_eq!(endless.status(), TaskStatus::Ready);
    assert!(matches!(endless.cancel(), Cancel::Requested));
    assert!(matches!(endless.run(), Ok(RunOutcome::Cancelled)));
}

/// The same embedding API without any executor: the test thread runs a
/// consumer's prefix and park and an unrelated task; after the completion
/// the consumer resumes on another plain thread. No pool, no executor.
#[test]
fn manual_embedding_needs_no_executor() {
    let rt = Runtime::new();
    let key = DependencyKey::new(rt.world(), DEF, KIND);
    let producer = rt.claim(key).unwrap();
    let hosts = Hosts::default();
    let c = consumer(&rt, 3.0);
    let unrelated = work(&rt);
    let mut host = hosts.host(None);

    let first = c.run_with(Budget::instructions(PREFIX), &mut host).unwrap();
    assert!(matches!(first, RunOutcome::Yielded), "{first:?}");
    let mut guard = c.try_begin_run().unwrap();
    assert!(matches!(
        guard.need(DEF, KIND),
        Ok(Need::Waiting { produce: None, .. })
    ));
    assert!(matches!(guard.park(), RunOutcome::Waiting(k) if k == key));
    let done = unrelated.run_with(Budget::UNLIMITED, &mut host).unwrap();
    assert_eq!(complete_number(Some(done)), WORK_RESULT);

    let value = rt.root(Primitive::Number(1.0));
    let runnable = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap()
        .into_runnable();
    assert_eq!(runnable.len(), 1);
    let resumed = thread::scope(|s| {
        s.spawn(|| {
            let mut host = hosts.host(None);
            let mut guard = runnable[0].try_begin_run().unwrap();
            assert!(matches!(guard.need(DEF, KIND), Ok(Need::Ready(_))));
            guard.run_with(Budget::UNLIMITED, &mut host)
        })
        .join()
        .unwrap()
    });
    assert_eq!(complete_number(Some(resumed)), 103.0);
    let effects = hosts.effects_of(c.id());
    let args: Vec<f64> = effects.iter().map(|e| e.arg).collect();
    assert_eq!(args, [3.0, 103.0]);
    assert_ne!(effects[0].thread, effects[1].thread);
}

/// Task ids restart at 1 in every runtime, so an executor serves only the
/// runtime it was started for: a task of another runtime, even one with a
/// colliding id, is handed back at submit, wake and cancel, untouched.
#[test]
fn foreign_runtime_tasks_are_handed_back_untouched() {
    let rt = Runtime::new();
    let other = Runtime::new();
    let own = work(&rt);
    let foreign = work(&other);
    assert_eq!(own.id(), foreign.id(), "ids collide across runtimes");
    let exec = ExecutorConfig::new().start(&rt).unwrap();

    let back = exec.submit(foreign.clone()).unwrap_err();
    assert_eq!(back.0.len(), 1);
    assert!(exec.submit_with(foreign.clone(), Placement::Any).is_err());
    assert!(exec.cancel(&foreign).is_err());
    assert!(!foreign.cancel_requested());
    exec.submit(own.clone()).unwrap();
    assert_eq!(
        complete_number(exec.take_outcome(own.id(), TIMEOUT)),
        WORK_RESULT
    );

    // A parked task of the other runtime: its wakeups are handed back.
    let key = DependencyKey::new(other.world(), DEF, KIND);
    let producer = other.claim(key).unwrap();
    let parked = work(&other);
    let mut guard = parked.try_begin_run().unwrap();
    assert!(matches!(guard.need(DEF, KIND), Ok(Need::Waiting { .. })));
    assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
    let value = other.root(Primitive::Number(0.0));
    let wakeups = other
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    let back = exec.wake(wakeups).unwrap_err();
    let ids: Vec<TaskId> = back.0.iter().map(Task::id).collect();
    assert_eq!(ids, [parked.id()]);

    let down = exec.shutdown(TIMEOUT).unwrap();
    assert!(down.pending.is_empty() && down.parked.is_empty());
    // Nothing of the other runtime was enqueued (its parked task's id
    // collides with no task of this runtime).
    assert!(down.events.iter().all(|e| e.task != Some(parked.id())));
    // The handed-back tasks still run by hand in their own runtime.
    assert_eq!(complete_number(Some(foreign.run().unwrap())), WORK_RESULT);
    assert_eq!(complete_number(Some(back.0[0].run().unwrap())), WORK_RESULT);
}

/// A `wait_for` predicate runs with no executor lock held, so it may query
/// the executor and submit work.
#[test]
fn a_wait_predicate_may_call_the_executor() {
    let rt = Runtime::new();
    let task = work(&rt);
    let exec = ExecutorConfig::new().workers(1).start(&rt).unwrap();
    assert!(exec.wait_for(TIMEOUT, |_| !exec.events().is_empty()));
    let mut submitted = false;
    let done = exec.wait_for(TIMEOUT, |events| {
        if !submitted {
            exec.submit(task.clone()).unwrap();
            submitted = true;
        }
        releases(events, task.id()).contains(&OutcomeKind::Completed)
    });
    assert!(done, "{:#?}", exec.events());
    exec.shutdown(TIMEOUT).unwrap();
}

/// The wake of a park can arrive before the parking dispatch has recorded
/// its release (here the driver is held right after the park). The woken
/// entry stays ineligible until that dispatch has finished, so the record
/// shows the release before the next dispatch even with an idle worker
/// ready to take it.
#[test]
fn a_wake_during_the_parking_release_waits_for_it() {
    let rt = Runtime::new();
    let key = DependencyKey::new(rt.world(), DEF, KIND);
    let producer = rt.claim(key).unwrap();
    let hosts = Hosts::default();
    let c = consumer(&rt, 4.0);
    let (parked_latch, release_latch) = (Arc::new(Latch::default()), Arc::new(Latch::default()));
    let (parked_signal, release_wait) = (Arc::clone(&parked_latch), Arc::clone(&release_latch));
    let exec = ExecutorConfig::new()
        .host(hosts.factory())
        .driver(move |mut guard: RunGuard<'_>, cx: &mut RunContext<'_>| {
            match guard.need(DEF, KIND) {
                Ok(Need::Waiting { .. }) => {
                    let outcome = guard.park();
                    parked_signal.open();
                    assert!(release_wait.wait(), "release latch never opened");
                    outcome
                }
                Ok(Need::Ready(_)) => guard.run_with(cx.budget, &mut *cx.host),
                other => panic!("unexpected need result {other:?}"),
            }
        })
        .start(&rt)
        .unwrap();
    exec.submit_with(c.clone(), Placement::Worker(w(0)))
        .unwrap();
    assert!(parked_latch.wait());
    assert_eq!(c.status(), TaskStatus::Waiting);
    let value = rt.root(Primitive::Number(0.0));
    let wakeups = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(exec.wake(wakeups).unwrap(), 1);
    // Probe while the parking dispatch is held: worker 1 is idle and the
    // task is runnable, yet its entry must not be dispatched. (This cannot
    // fail when the executor is correct; the bound only limits how long a
    // regression has to show itself.)
    let early = exec.wait_for(Duration::from_millis(200), |ev| {
        dispatches(ev, c.id()).len() > 1
    });
    assert!(!early, "{:#?}", exec.events());
    release_latch.open();
    assert_eq!(complete_number(exec.take_outcome(c.id(), TIMEOUT)), 104.0);

    let events = exec.events();
    assert_single_owner(&events);
    assert_eq!(
        releases(&events, c.id()),
        [OutcomeKind::Waiting(key), OutcomeKind::Completed]
    );
    let waiting = position(&events, c.id(), |k| {
        *k == EventKind::Released(OutcomeKind::Waiting(key))
    })
    .unwrap();
    let resumed = position(&events, c.id(), |k| {
        *k == EventKind::Dispatched { execution: 2 }
    })
    .unwrap();
    assert!(waiting < resumed, "{events:#?}");
    exec.shutdown(TIMEOUT).unwrap();
}

/// A driver that panics while holding a run guard poisons that task only:
/// the worker records `Panicked`, keeps running other tasks, and shuts down.
#[test]
fn a_panicking_driver_does_not_stop_its_worker() {
    let rt = Runtime::new();
    let doomed = work(&rt);
    let healthy = work(&rt);
    let doomed_id = doomed.id();
    let exec = ExecutorConfig::new()
        .workers(1)
        .driver(move |guard: RunGuard<'_>, cx: &mut RunContext<'_>| {
            if guard.task_id() == doomed_id {
                panic!("driver panic for the test (expected)");
            }
            guard.run_with(cx.budget, &mut *cx.host)
        })
        .start(&rt)
        .unwrap();
    exec.submit(doomed.clone()).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| {
        position(ev, doomed.id(), |k| *k == EventKind::Panicked).is_some()
    }));
    assert_eq!(doomed.status(), TaskStatus::Finished, "poisoned");
    exec.submit(healthy.clone()).unwrap();
    assert_eq!(
        complete_number(exec.take_outcome(healthy.id(), TIMEOUT)),
        WORK_RESULT
    );
    let down = exec.shutdown(TIMEOUT).unwrap();
    assert_single_owner(&down.events);
    assert!(down.pending.is_empty() && down.outcomes.is_empty());
    let workers: HashSet<Option<WorkerId>> = down
        .events
        .iter()
        .filter(|e| e.task.is_some() && matches!(e.kind, EventKind::Dispatched { .. }))
        .map(|e| e.worker)
        .collect();
    assert_eq!(workers, HashSet::from([Some(w(0))]));
}

fn complete_string(outcome: Option<RunOutcome>) -> String {
    match outcome {
        Some(RunOutcome::Complete(v)) => v
            .string()
            .unwrap_or_else(|| panic!("expected a string, got {:?}", v.view()))
            .to_string(),
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// The realm intrinsics are shared by tasks on both workers
/// (clj-quickjs-117). Two source tasks (synthetic fixtures) each make
/// objects (the first `{}` of either builds the realm) and meet inside a
/// host effect, so both are mid-run on distinct workers at once. Then one
/// tries to write `Object.prototype`, which throws the documented
/// catchable `TypeError`, and completes; only then does the test open the
/// gate the other waits at, and the other still sees `Object.prototype`
/// unchanged.
#[test]
fn two_workers_share_the_realm_intrinsics() {
    let rt = Runtime::new();
    let hosts = Hosts::default();
    let writer = source_task(
        &rt,
        "writer",
        "let o = {x: 1}; let before = o.toString(); \
         __rt.effect(2, 1); \
         let r = 'none'; \
         try { Object.prototype.x = 2; } catch (e) { r = 'caught'; } \
         try { Object.prototype.toString = 3; } catch (e) { r = r + '+caught'; } \
         before + ':' + ({}).toString() + ':' + r + ':' + typeof ({}).x",
    );
    let reader = source_task(
        &rt,
        "reader",
        "let p = {y: 2}; let before = p.toString(); \
         __rt.effect(2, 2); \
         __rt.effect(3, 2); \
         before + ':' + ({}).toString() + ':' + typeof ({}).x + ':' + \
         (p.toString === Object.prototype.toString)",
    );
    let exec = ExecutorConfig::new()
        .host(hosts.factory())
        .start(&rt)
        .unwrap();
    exec.submit(writer.clone()).unwrap();
    exec.submit(reader.clone()).unwrap();
    assert_eq!(
        complete_string(exec.take_outcome(writer.id(), TIMEOUT)),
        "[object Object]:[object Object]:caught+caught:undefined"
    );
    hosts.gate.open();
    assert_eq!(
        complete_string(exec.take_outcome(reader.id(), TIMEOUT)),
        "[object Object]:[object Object]:undefined:true"
    );
    // They met mid-run on two workers.
    let (ew, er) = (
        &hosts.effects_of(writer.id())[0],
        &hosts.effects_of(reader.id())[0],
    );
    assert_eq!((ew.op, er.op), (MEET, MEET));
    assert_ne!(ew.worker, er.worker);
    assert_single_owner(&exec.events());
    exec.shutdown(TIMEOUT).unwrap();
}
