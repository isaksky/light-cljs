//! Stop-the-world collection under the two-worker executor (pellet
//! clj-quickjs-25): registered workers, VM safepoints, native scopes on
//! workers and other threads, idle workers, and the roots the executor's
//! tasks hold (running, queued, parked, published results).
//!
//! Every wait is bounded and synchronizes on an event, latch or flag, never
//! on a sleep. Programs are hand-assembled bytecode (the source subset has
//! no host effects): early VM test fixtures, not JavaScript source support.
//! Needs are issued by the executor's driver through the run guard,
//! standing in for the VM's need operation (pellet clj-quickjs-23).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use clj_quickjs::exec::{CollectionEvent, Event, EventKind, ExecutorConfig, Placement, WorkerId};
use clj_quickjs::{
    Assembler, Budget, CollectError, Completion, DependencyId, DependencyKey, Host, HostCall,
    MutatorState, Need, Opcode, Primitive, ResultKind, Rooted, RunGuard, RunOutcome, Runtime, Task,
    TaskId, WeakHandle,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const DEF: DependencyId = DependencyId::new(25);
const PUBLISHED: DependencyId = DependencyId::new(26);
const KIND: ResultKind = ResultKind::new(1);
/// Instructions of [`parker`] before its need point.
const PREFIX: u64 = 2;

fn w(index: usize) -> WorkerId {
    WorkerId::new(index)
}

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

/// Bounded wait for a condition another thread makes true; used only for
/// `Runtime::collection_requested`, which has no notification.
fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::yield_now();
    }
}

/// The spinning task's host: opens `entered` on the first effect, then
/// answers "continue" until `stop` is set (failing after a bound).
#[derive(Clone, Default)]
struct Spin {
    entered: Arc<Latch>,
    stop: Arc<AtomicBool>,
}

struct SpinHost {
    spin: Spin,
    deadline: Instant,
}

impl Host for SpinHost {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        self.spin.entered.open();
        if Instant::now() > self.deadline {
            return Err("spin ran past its deadline".to_string());
        }
        Ok(Primitive::Bool(!self.spin.stop.load(Ordering::Acquire)))
    }
}

/// An object rooted for the caller, and a non-rooting handle to it.
fn object(rt: &Runtime) -> (Rooted, WeakHandle) {
    let rooted = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    let weak = rooted.downgrade();
    (rooted, weak)
}

/// `function(kept) { do { new Object(); } while (spin()); return kept; }`:
/// allocates garbage at every iteration; `kept` is only in its frame.
fn spinner(rt: &Runtime, kept: &Rooted) -> Task {
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.bind(top);
    asm.op(Opcode::NewObject).op(Opcode::Pop);
    asm.constant(Primitive::Null).host_effect(0);
    asm.jump_if_false(done).jump(top);
    asm.bind(done);
    asm.get_local(0).op(Opcode::Return);
    let exe = asm.finish("spinner", 1, 1).unwrap();
    rt.spawn(&exe, &[kept]).unwrap()
}

/// `function(kept) { kept; <need point>; return kept; }`: the need point is
/// the budget boundary after [`PREFIX`] instructions, where [`driver`]
/// needs `DEF`.
fn parker(rt: &Runtime, kept: &Rooted) -> Task {
    let mut asm = Assembler::new();
    asm.get_local(0).op(Opcode::Pop);
    asm.get_local(0).op(Opcode::Return);
    let exe = asm.finish("parker", 1, 1).unwrap();
    rt.spawn(&exe, &[kept]).unwrap()
}

/// `function(kept) { return kept; }`
fn identity(rt: &Runtime, kept: &Rooted) -> Task {
    let mut asm = Assembler::new();
    asm.get_local(0).op(Opcode::Return);
    let exe = asm.finish("identity", 1, 1).unwrap();
    rt.spawn(&exe, &[kept]).unwrap()
}

/// Spinners run with an unlimited budget (so they stay running); parkers
/// run their prefix, then need `DEF` and park while it is pending; every
/// other task just runs.
fn executor(
    rt: &Runtime,
    spin: &Spin,
    spinners: Vec<TaskId>,
    parkers: Vec<TaskId>,
) -> clj_quickjs::Executor {
    let spin = spin.clone();
    ExecutorConfig::new()
        .host(move |_| {
            Box::new(SpinHost {
                spin: spin.clone(),
                deadline: Instant::now() + TIMEOUT,
            })
        })
        .driver(move |mut guard: RunGuard<'_>, cx| {
            let id = guard.task_id();
            if spinners.contains(&id) {
                return guard.run_with(Budget::UNLIMITED, &mut *cx.host);
            }
            if !parkers.contains(&id) {
                return guard.run_with(cx.budget, &mut *cx.host);
            }
            if guard.wake_handle().execution() == 1 {
                return guard.run_with(Budget::instructions(PREFIX), &mut *cx.host);
            }
            match guard.need(DEF, KIND) {
                Ok(Need::Waiting { produce: None, .. }) => guard.park(),
                Ok(Need::Ready(_)) => guard.run_with(cx.budget, &mut *cx.host),
                other => panic!("unexpected need result {other:?}"),
            }
        })
        .start(rt)
        .unwrap()
}

fn parked(events: &[Event], task: TaskId) -> bool {
    events.iter().any(|e| {
        e.task == Some(task)
            && matches!(
                e.kind,
                EventKind::Released(clj_quickjs::exec::OutcomeKind::Waiting(_))
            )
    })
}

fn collections(events: &[Event]) -> Vec<CollectionEvent> {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Collected(c) => Some(c.clone()),
            _ => None,
        })
        .collect()
}

/// Wait for the recorded collection number `number` and return it.
fn collection(exec: &clj_quickjs::Executor, number: u64) -> CollectionEvent {
    let mut found = None;
    assert!(
        exec.wait_for(TIMEOUT, |ev| {
            found = collections(ev).into_iter().find(|c| c.number == number);
            found.is_some()
        }),
        "collection {number} not recorded: {:#?}",
        exec.events()
    );
    found.unwrap()
}

fn complete_value(outcome: Option<RunOutcome>) -> Rooted {
    match outcome {
        Some(RunOutcome::Complete(value)) => value,
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// Forced collection with a task running on worker 0, a task parked on a
/// dependency (held only by the executor), a task queued behind the running
/// one, a published dependency result, and a native scope released on
/// another thread; requested by worker 1. Every one of those values keeps
/// its identity, garbage is reclaimed, the event names the requester and
/// how each participant stopped, and nothing deadlocks.
#[test]
fn a_worker_requested_collection_sees_running_queued_parked_and_native_roots() {
    let rt = Runtime::new();
    let key = DependencyKey::new(rt.world(), DEF, KIND);
    let producer = rt.claim(key).unwrap();
    let spin = Spin::default();

    let (running_arg, running) = object(&rt);
    let (parked_arg, parked_obj) = object(&rt);
    let (queued_arg, queued) = object(&rt);
    let spin_task = spinner(&rt, &running_arg);
    let park_task = parker(&rt, &parked_arg);
    let queued_task = identity(&rt, &queued_arg);
    let (spin_id, park_id, queued_id) = (spin_task.id(), park_task.id(), queued_task.id());
    // From here on only task frames hold these objects.
    drop((running_arg, parked_arg, queued_arg));
    let exec = executor(&rt, &spin, vec![spin_id], vec![park_id]);

    // Parked, and held only by the executor.
    exec.submit(park_task).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| parked(ev, park_id)));
    // Running on worker 0, endlessly until stopped.
    exec.submit_with(spin_task, Placement::Worker(w(0)))
        .unwrap();
    assert!(spin.entered.wait());
    // Queued behind it, held only by the ready queue.
    exec.submit_with(queued_task, Placement::Worker(w(0)))
        .unwrap();

    // A published result held only by its cell.
    let (published_value, published) = {
        let v = rt.scope(|s| {
            let v = s.new_vector(&[]).unwrap();
            s.persist(v)
        });
        let weak = v.downgrade();
        (v, weak)
    };
    let pkey = DependencyKey::new(rt.world(), PUBLISHED, KIND);
    let pproducer = rt.claim(pkey).unwrap();
    let none = rt
        .complete(
            pkey,
            pproducer.producer(),
            Completion::Ready(&published_value),
        )
        .unwrap();
    assert!(none.task_ids().is_empty(), "nobody waits on it");
    drop(published_value);

    // Native access on another thread: a scope local, the scope released
    // while the collection runs.
    let release = Arc::new(Latch::default());
    let (local_tx, local_rx) = std::sync::mpsc::channel();
    let native = {
        let (rt, release) = (rt.clone(), Arc::clone(&release));
        thread::spawn(move || {
            rt.scope(|s| {
                let local = s.new_object();
                local_tx.send(s.downgrade(local)).unwrap();
                assert!(s.released(move || release.wait()));
                // The local is still valid after the release.
                assert!(s.downgrade(local).is_live());
            });
        })
    };
    let native_thread = native.thread().id();
    let scope_local = local_rx.recv_timeout(TIMEOUT).unwrap();

    let (garbage_root, garbage) = object(&rt);
    drop(garbage_root);

    exec.schedule_collection(Placement::Worker(w(1)));
    let c = collection(&exec, 1);
    assert_eq!(c.requester, Some(w(1)), "{c:#?}");
    assert!(c.waited, "worker 0 was running: {c:#?}");
    assert_eq!(c.state_of(w(0)), Some(MutatorState::Safepoint), "{c:#?}");
    // The scope thread was in heap access when the request was made, so the
    // collection ran only once it had released its scope.
    assert_eq!(
        c.state_of_thread(native_thread),
        Some(MutatorState::Released),
        "{c:#?}"
    );
    assert!(c.stats.reclaimed >= 1, "{c:#?}");
    assert!(!garbage.is_live());
    for (what, weak) in [
        ("running frame", &running),
        ("parked frame", &parked_obj),
        ("queued frame", &queued),
        ("published result", &published),
        ("native scope local", &scope_local),
    ] {
        assert!(weak.is_live(), "{what} was reclaimed");
    }

    // Everything resumes: the scope closes, the spinner and the queued task
    // finish on worker 0, and the parked task completes once woken.
    release.open();
    native.join().unwrap();
    spin.stop.store(true, Ordering::Release);
    let spun = complete_value(exec.take_outcome(spin_id, TIMEOUT));
    assert!(spun.same_object(&running.upgrade().unwrap()));
    let echoed = complete_value(exec.take_outcome(queued_id, TIMEOUT));
    assert!(echoed.same_object(&queued.upgrade().unwrap()));
    let value = rt.root(Primitive::Number(1.0));
    let wakeups = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(exec.wake(wakeups).unwrap(), 1);
    let resumed = complete_value(exec.take_outcome(park_id, TIMEOUT));
    assert!(resumed.same_object(&parked_obj.upgrade().unwrap()));

    // Once the results are dropped only the published cell keeps anything.
    drop((spun, echoed, resumed));
    rt.request_collection().unwrap();
    assert!(!running.is_live() && !parked_obj.is_live() && !queued.is_live());
    assert!(!scope_local.is_live());
    assert!(published.is_live());
    let down = exec.shutdown(TIMEOUT).unwrap();
    assert_eq!(collections(&down.events).len(), 2);
}

/// A worker blocked inside a native scope (no-GC access) holds a request
/// until it closes the scope; the other worker, idle in the ready-queue
/// wait, never holds one up.
#[test]
fn a_native_scope_on_a_worker_delays_collection_and_an_idle_worker_does_not() {
    let rt = Runtime::new();
    let spin = Spin::default();
    let exec = executor(&rt, &spin, vec![], vec![]);
    // Worker 1 does some work first, then idles.
    let (arg, _) = object(&rt);
    let task = identity(&rt, &arg);
    let id = task.id();
    exec.submit_with(task, Placement::Worker(w(1))).unwrap();
    drop(complete_value(exec.take_outcome(id, TIMEOUT)));
    drop(arg);

    // Both workers registered and idle: a collection that does not wait
    // succeeds at once.
    let started = |ev: &[Event]| {
        ev.iter()
            .filter(|e| matches!(e.kind, EventKind::WorkerStarted { .. }))
            .count()
            == 2
    };
    assert!(exec.wait_for(TIMEOUT, started));
    rt.collect().unwrap();
    let c = collection(&exec, 1);
    assert_eq!((c.requester, c.waited), (None, false), "{c:#?}");
    assert_eq!(c.state_of(w(0)), Some(MutatorState::Idle));
    assert_eq!(c.state_of(w(1)), Some(MutatorState::Idle));

    // Worker 0 opens a scope and blocks in it.
    let gate = Arc::new(Latch::default());
    let (local_tx, local_rx) = std::sync::mpsc::channel();
    let job = {
        let (rt, gate) = (rt.clone(), Arc::clone(&gate));
        exec.submit_native(Placement::Worker(w(0)), move |_| {
            rt.scope(|s| {
                let local = s.new_object();
                local_tx.send(s.downgrade(local)).unwrap();
                assert!(gate.wait());
            })
        })
    };
    let local = local_rx.recv_timeout(TIMEOUT).unwrap();

    assert_eq!(rt.collect(), Err(CollectError::MutatorActive));
    let requester = {
        let rt = rt.clone();
        thread::spawn(move || rt.request_collection())
    };
    wait_until("the request", || rt.collection_requested());
    // Pending, and held up only by worker 0's scope: no collection ran.
    assert_eq!(rt.heap_stats().collections, 1);
    assert!(!requester.is_finished());
    assert!(local.is_live());

    gate.open();
    let stats = requester.join().unwrap().unwrap();
    let c = collection(&exec, 2);
    assert_eq!(c.stats, stats);
    assert_eq!(c.requester, None);
    assert!(c.waited);
    assert_eq!(c.state_of(w(1)), Some(MutatorState::Idle), "{c:#?}");
    // Worker 0 was stopped only once its scope had closed.
    assert_eq!(c.state_of(w(0)), Some(MutatorState::Idle), "{c:#?}");
    assert!(!local.is_live(), "the scope closed before the collection");
    let finished = EventKind::NativeFinished {
        job,
        panicked: false,
    };
    assert!(exec.wait_for(TIMEOUT, |ev| ev.iter().any(|e| e.kind == finished)));
    exec.shutdown(TIMEOUT).unwrap();
}

/// A value reachable only from a parked task survives collections while
/// the task waits, and is reclaimed once the task has terminated and its
/// result has been dropped.
#[test]
fn values_only_a_parked_task_holds_survive_until_it_ends_and_is_unrooted() {
    let rt = Runtime::new();
    let key = DependencyKey::new(rt.world(), DEF, KIND);
    let producer = rt.claim(key).unwrap();
    let (arg, kept) = object(&rt);
    let task = parker(&rt, &arg);
    let id = task.id();
    drop(arg);
    let exec = executor(&rt, &Spin::default(), vec![], vec![id]);
    exec.submit(task).unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| parked(ev, id)));

    exec.schedule_collection(Placement::Any);
    let c = collection(&exec, 1);
    assert!(c.requester.is_some());
    assert!(kept.is_live(), "reclaimed while parked");
    rt.request_collection().unwrap();
    assert!(kept.is_live(), "reclaimed while parked");

    let value = rt.root(Primitive::Number(1.0));
    let wakeups = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(exec.wake(wakeups).unwrap(), 1);
    let result = complete_value(exec.take_outcome(id, TIMEOUT));
    assert!(result.same_object(&kept.upgrade().unwrap()));
    rt.request_collection().unwrap();
    assert!(kept.is_live(), "held by the result");
    drop(result);
    rt.request_collection().unwrap();
    assert!(!kept.is_live(), "the task ended and nothing roots it");
    let down = exec.shutdown(TIMEOUT).unwrap();
    assert!(down.parked.is_empty() && down.pending.is_empty());
}
