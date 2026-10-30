//! Deterministic tests of the consumer/completion handoff (pellet
//! clj-quickjs-7): each window of `docs/design.md` §8 is forced with a test
//! hook ([`HookPoint`]) or a channel handshake, never a sleep, and every
//! handshake has a bounded timeout that fails the test instead of hanging.
//!
//! Needs are issued from Rust through the run guard, standing in for the
//! `__rt.need` VM operation of pellet clj-quickjs-23. Consumer tasks run a
//! trivial hand-assembled program that is never executed: an early VM test
//! fixture, not JavaScript source support.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::dep::{CellStatus, Completion, HookPoint, Wakeups};
use crate::ids::{DependencyId, ResultKind};
use crate::value::Primitive;
use crate::vm::{Assembler, Opcode};
use crate::{ProducerRequest, Runtime};

pub(super) const DEF: DependencyId = DependencyId::new(3);
pub(super) const KIND: ResultKind = ResultKind::new(1);
/// Bound for every cross-thread handshake.
pub(super) const HANDSHAKE: Duration = Duration::from_secs(10);

pub(super) fn consumer(rt: &Runtime) -> Task {
    let mut asm = Assembler::new();
    asm.constant(Primitive::Undefined).op(Opcode::Return);
    rt.spawn(&asm.finish("consumer", 0, 0).unwrap(), &[])
        .unwrap()
}

pub(super) fn key(rt: &Runtime) -> DependencyKey {
    DependencyKey::new(rt.world(), DEF, KIND)
}

pub(super) fn hooks(task: &Task) -> &crate::dep::Hooks {
    &task.0.deps.hooks
}

pub(super) fn number(n: Need) -> f64 {
    match n {
        Need::Ready(v) => match v.view() {
            crate::ValueView::Number(n) => n,
            other => panic!("expected a number, got {other:?}"),
        },
        other => panic!("expected Ready, got {other:?}"),
    }
}

pub(super) fn complete(rt: &Runtime, req: &ProducerRequest, n: f64) -> Wakeups {
    let value = rt.root(Primitive::Number(n));
    rt.complete(req.key(), req.producer(), Completion::Ready(&value))
        .unwrap()
}

/// Window 1: completion before registration. The need reads the outcome
/// through the atomic fast path (the slow-path hook would fail the test),
/// registers nothing, and the task never parks.
#[test]
fn completion_before_registration_is_read_on_the_fast_path() {
    let rt = Runtime::new();
    let req = rt.claim(key(&rt)).unwrap();
    assert!(complete(&rt, &req, 1.0).deliveries().is_empty());
    let task = consumer(&rt);
    hooks(&task).always(HookPoint::NeedAfterPendingRead, || {
        panic!("a completed cell is read without the cell lock")
    });
    let mut guard = task.try_begin_run().unwrap();
    assert_eq!(number(guard.need(DEF, KIND).unwrap()), 1.0);
    assert!(
        matches!(guard.park(), RunOutcome::Yielded),
        "nothing to wait for"
    );
    assert_eq!(task.status(), TaskStatus::Ready);
    let log = task.dependency_log();
    assert_eq!((log.edges(), log.waiting()), (&[key(&rt)][..], None));
}

/// A reader that counts the edges it has read gets only the new ones, in
/// the order the full log has them; a repeated need adds no edge.
#[test]
fn edges_since_returns_only_edges_recorded_after_the_count() {
    let rt = Runtime::new();
    let other = DependencyKey::new(rt.world(), DependencyId::new(4), KIND);
    for key in [key(&rt), other] {
        let req = rt.claim(key).unwrap();
        complete(&rt, &req, 1.0);
    }
    let task = consumer(&rt);
    let mut guard = task.try_begin_run().unwrap();
    guard.need(DEF, KIND).unwrap();
    assert_eq!(task.dependency_edges_since(0), [key(&rt)]);
    guard.need(DEF, KIND).unwrap();
    guard.need(DependencyId::new(4), KIND).unwrap();
    assert_eq!(task.dependency_edges_since(1), [other]);
    assert_eq!(task.dependency_edges_since(2), []);
    assert_eq!(task.dependency_edges_since(5), []);
    assert_eq!(
        task.dependency_edges_since(0),
        task.dependency_log().edges()
    );
}

/// Window 2: the completion lands after the need observed `Pending` on the
/// fast path but before it took the cell lock to subscribe. The recheck
/// under the lock reads the outcome: the task is not registered, the
/// completion wakes nobody, and the need returns the value.
#[test]
fn completion_between_pending_read_and_subscription_is_read_by_the_recheck() {
    let rt = Runtime::new();
    let req = rt.claim(key(&rt)).unwrap();
    let task = consumer(&rt);
    let woken = Arc::new(Mutex::new(None));
    {
        let (rt, woken) = (rt.clone(), Arc::clone(&woken));
        hooks(&task).once(HookPoint::NeedAfterPendingRead, move || {
            let value = rt.root(Primitive::Number(2.0));
            let w = rt
                .complete(req.key(), req.producer(), Completion::Ready(&value))
                .unwrap();
            *woken.lock().unwrap() = Some(w.task_ids());
        });
    }
    let mut guard = task.try_begin_run().unwrap();
    assert_eq!(number(guard.need(DEF, KIND).unwrap()), 2.0);
    assert_eq!(
        woken.lock().unwrap().take(),
        Some(vec![]),
        "the completion ran inside the window and found no waiter"
    );
    assert!(matches!(guard.park(), RunOutcome::Yielded));
    assert_eq!(task.dependency_log().waiting(), None);
    assert_eq!(task.status(), TaskStatus::Ready);
}

/// Window 3: completion after subscription, before the consumer parks. The
/// wake is remembered (nothing to enqueue), the park returns `Yielded`
/// with the task runnable, and the next run reads the value.
#[test]
fn completion_after_subscription_before_park_is_remembered() {
    let rt = Runtime::new();
    let task = consumer(&rt);
    let mut guard = task.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    let handle = guard.wake_handle();
    let wakeups = complete(&rt, &req, 3.0);
    assert!(matches!(
        wakeups.deliveries(),
        [(h, Wake::Remembered)] if *h == handle
    ));
    assert!(wakeups.into_runnable().is_empty(), "the runner requeues it");
    assert_eq!(task.status(), TaskStatus::Running);
    assert!(matches!(guard.park(), RunOutcome::Yielded));
    assert_eq!(task.status(), TaskStatus::Ready);
    assert!(matches!(task.wake(handle), Wake::Stale), "duplicate");
    let mut guard = task.try_begin_run().unwrap();
    assert_eq!(number(guard.need(DEF, KIND).unwrap()), 3.0);
    assert_eq!(guard.wake_handle().execution(), 2);
}

/// Window 3, inside the park: the park has read `Executing` without a
/// notification, and the completion's wake sets `NOTIFIED` before the park's
/// CAS. The CAS fails, the park rereads, and the run yields instead of
/// parking with nobody left to wake it.
#[test]
fn completion_between_park_read_and_park_cas_is_not_lost() {
    let rt = Runtime::new();
    let task = consumer(&rt);
    let mut guard = task.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    let remembered = Arc::new(Mutex::new(false));
    {
        let (rt, remembered) = (rt.clone(), Arc::clone(&remembered));
        hooks(&task).once(HookPoint::ParkBeforeCas, move || {
            let w = complete(&rt, &req, 4.0);
            *remembered.lock().unwrap() = matches!(w.deliveries(), [(_, Wake::Remembered)]);
        });
    }
    assert!(matches!(guard.park(), RunOutcome::Yielded));
    assert!(
        *remembered.lock().unwrap(),
        "the hook completed inside park"
    );
    assert_eq!(task.status(), TaskStatus::Ready);
    assert_eq!(
        number(task.try_begin_run().unwrap().need(DEF, KIND).unwrap()),
        4.0
    );
}

/// Window 4: completion after the park. The parked task cannot be run; the
/// completion's wake makes it runnable and returns it to enqueue, once.
#[test]
fn completion_after_park_returns_the_task_to_enqueue_once() {
    let rt = Runtime::new();
    let task = consumer(&rt);
    let mut guard = task.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    let handle = guard.wake_handle();
    assert!(matches!(guard.park(), RunOutcome::Waiting(k) if k == key(&rt)));
    assert_eq!(task.status(), TaskStatus::Waiting);
    assert_eq!(task.try_begin_run().err(), Some(RunError::Waiting));
    let runnable = complete(&rt, &req, 5.0).into_runnable();
    assert_eq!(runnable.len(), 1);
    assert_eq!(runnable[0].id(), task.id());
    assert_eq!(task.status(), TaskStatus::Ready);
    assert!(matches!(task.wake(handle), Wake::Stale), "duplicate");
    let mut guard = runnable[0].try_begin_run().unwrap();
    assert_eq!(number(guard.need(DEF, KIND).unwrap()), 5.0);
    assert_eq!(task.dependency_log().waiting(), None);
}

/// Window 4, inside the wake: the completion's wake has read `Executing`
/// and the consumer (on its own thread, holding its guard) parks before the
/// wake's CAS. The CAS fails, the wake rereads `Parked`, and returns the
/// task to enqueue; the consumer's park reported `Waiting`.
#[test]
fn park_between_wake_read_and_wake_cas_is_woken() {
    let rt = Runtime::new();
    let task = consumer(&rt);
    let (subscribed_tx, subscribed_rx) = mpsc::channel();
    let (park_tx, park_rx) = mpsc::channel::<()>();
    let (parked_tx, parked_rx) = mpsc::channel();
    let parked_rx = Mutex::new(parked_rx);
    let park_tx = Mutex::new(park_tx);
    let consumer_thread = {
        let task = task.clone();
        std::thread::spawn(move || {
            let mut guard = task.try_begin_run().unwrap();
            let need = guard.need(DEF, KIND).unwrap();
            subscribed_tx.send(need).unwrap();
            park_rx.recv_timeout(HANDSHAKE).expect("told to park");
            let outcome = guard.park();
            parked_tx.send(()).unwrap();
            outcome
        })
    };
    let Need::Waiting {
        produce: Some(req), ..
    } = subscribed_rx.recv_timeout(HANDSHAKE).unwrap()
    else {
        panic!("the first need claims");
    };
    hooks(&task).once(HookPoint::WakeBeforeCas, move || {
        park_tx.lock().unwrap().send(()).unwrap();
        parked_rx
            .lock()
            .unwrap()
            .recv_timeout(HANDSHAKE)
            .expect("consumer parked");
    });
    let wakeups = complete(&rt, &req, 6.0);
    assert!(matches!(
        consumer_thread.join().unwrap(),
        RunOutcome::Waiting(_)
    ));
    let runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1, "the wake found the task parked");
    assert_eq!(task.status(), TaskStatus::Ready);
    assert_eq!(
        number(task.try_begin_run().unwrap().need(DEF, KIND).unwrap()),
        6.0
    );
}

/// Many waiters in every handoff state, woken by one producer failure.
/// A deterministic scheduler loop counts resumptions: every consumer resumes
/// exactly once per suspension and sees the same published error, and
/// redelivering every handle afterwards changes nothing.
#[test]
fn producer_failure_resumes_waiters_in_every_state_exactly_once() {
    let rt = Runtime::new();
    let k = key(&rt);
    let parked: Vec<Task> = (0..3).map(|_| consumer(&rt)).collect();
    let executing = consumer(&rt);
    let unparked = consumer(&rt);
    let mut req = None;
    for task in &parked {
        let mut g = task.try_begin_run().unwrap();
        if let Need::Waiting { produce, .. } = g.need(DEF, KIND).unwrap() {
            req = req.or(produce);
        }
        assert!(matches!(g.park(), RunOutcome::Waiting(_)));
    }
    // Subscribed in run 1, which ended without parking (guard dropped).
    let _ = unparked.try_begin_run().unwrap().need(DEF, KIND).unwrap();
    let mut executing_guard = executing.try_begin_run().unwrap();
    let _ = executing_guard.need(DEF, KIND).unwrap();
    let req = req.expect("one producer request");

    let error = rt.scope(|s| {
        let one = s.primitive(Primitive::Number(1.0));
        let v = s.new_vector(&[one]).unwrap();
        s.persist(v)
    });
    let wakeups = rt
        .complete(k, req.producer(), Completion::Failed(&error))
        .unwrap();
    let handles: Vec<WakeHandle> = wakeups.deliveries().iter().map(|(h, _)| *h).collect();
    let kinds: Vec<&str> = wakeups
        .deliveries()
        .iter()
        .map(|(_, w)| match w {
            Wake::Runnable(_) => "runnable",
            Wake::Remembered => "remembered",
            Wake::Stale => "stale",
        })
        .collect();
    assert_eq!(
        kinds,
        ["runnable", "runnable", "runnable", "stale", "remembered"],
        "subscription order: parked x3, unparked (already runnable), executing"
    );

    // The scheduler queue: what the completion returned, the run whose park
    // found its wake already delivered, and the task that never parked
    // (its runner kept it).
    let mut queue: Vec<Task> = wakeups.into_runnable();
    assert!(matches!(executing_guard.park(), RunOutcome::Yielded));
    queue.push(executing.clone());
    queue.push(unparked.clone());
    for h in &handles {
        assert!(matches!(rt_task(&queue, h.task()).wake(*h), Wake::Stale));
    }
    let mut resumed = std::collections::HashMap::new();
    while let Some(task) = queue.pop() {
        *resumed.entry(task.id()).or_insert(0) += 1;
        let mut g = task.try_begin_run().unwrap();
        match g.need(DEF, KIND).unwrap() {
            Need::Failed(e) => assert!(e.same_object(&error)),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
    assert_eq!(resumed.len(), 5);
    assert!(resumed.values().all(|&n| n == 1), "{resumed:?}");
    assert!(matches!(rt.cell_status(k), CellStatus::Failed(_)));
}

fn rt_task(tasks: &[Task], id: TaskId) -> &Task {
    tasks.iter().find(|t| t.id() == id).unwrap()
}

/// A task that subscribed in one run, ended it without parking, and then
/// subscribes again in a later run and parks: the later need replaces the
/// cell's stale handle, so the completion still wakes the parked run.
#[test]
fn resubscription_in_a_later_run_refreshes_the_wake_handle() {
    let rt = Runtime::new();
    let task = consumer(&rt);
    let req = match task.try_begin_run().unwrap().need(DEF, KIND).unwrap() {
        Need::Waiting {
            produce: Some(req), ..
        } => req,
        other => panic!("{other:?}"),
    };
    let mut guard = task.try_begin_run().unwrap();
    assert!(matches!(
        guard.need(DEF, KIND).unwrap(),
        Need::Waiting { produce: None, .. }
    ));
    assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
    let wakeups = complete(&rt, &req, 7.0);
    assert!(matches!(
        wakeups.deliveries(),
        [(h, Wake::Runnable(_))] if h.execution() == 2
    ));
}

/// Producer cancellation wakes like success and failure: the parked
/// consumer is returned to enqueue, the consumer still in its subscribing
/// run has the wake remembered (its park yields), duplicates are stale, and
/// both resumed needs read `Cancelled` with their waits resolved. (Consumer
/// cancellation and embedder cell cancellation: `cancel_tests.rs`.)
#[test]
fn cancelled_completion_wakes_parked_and_parking_consumers() {
    let rt = Runtime::new();
    let (parked, parking) = (consumer(&rt), consumer(&rt));
    let mut g = parked.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = g.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    assert!(matches!(g.park(), RunOutcome::Waiting(_)));
    let mut parking_guard = parking.try_begin_run().unwrap();
    let _ = parking_guard.need(DEF, KIND).unwrap();

    let wakeups = rt
        .complete(req.key(), req.producer(), Completion::Cancelled)
        .unwrap();
    let handles: Vec<WakeHandle> = wakeups.deliveries().iter().map(|(h, _)| *h).collect();
    assert!(matches!(
        wakeups.deliveries(),
        [(_, Wake::Runnable(_)), (_, Wake::Remembered)]
    ));
    let runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    assert_eq!(runnable[0].id(), parked.id());
    assert!(matches!(parking_guard.park(), RunOutcome::Yielded));
    for (task, h) in [&parked, &parking].into_iter().zip(&handles) {
        assert!(matches!(task.wake(*h), Wake::Stale), "duplicate");
        let mut g = task.try_begin_run().unwrap();
        assert!(matches!(g.need(DEF, KIND).unwrap(), Need::Cancelled));
        assert_eq!(task.dependency_log().waiting(), None);
    }
}

/// Starting a run never waits for the task body: while a diagnostic reader
/// holds it (here this thread, standing in for `Task::source_location` on
/// another), the claim is undone and `AlreadyRunning` is reported; the task
/// stays runnable and starts normally once the body is free.
#[test]
fn beginning_a_run_does_not_wait_for_a_body_reader() {
    let rt = Runtime::new();
    let task = consumer(&rt);
    let body = task.0.body.lock().unwrap();
    assert_eq!(task.try_begin_run().err(), Some(RunError::AlreadyRunning));
    assert_eq!(task.status(), TaskStatus::Ready, "the claim was undone");
    drop(body);
    let guard = task.try_begin_run().unwrap();
    assert_eq!(guard.wake_handle().execution(), 2, "a fresh run number");
}

/// A wake handle names its runtime: task ids and run numbers restart in
/// every runtime, but another runtime's handle (even one whose runtime and
/// task are gone) never wakes a task.
#[test]
fn foreign_runtime_wake_handles_are_stale() {
    let foreign = {
        let other = Runtime::new();
        let task = consumer(&other);
        let guard = task.try_begin_run().unwrap();
        guard.wake_handle()
    };
    let rt = Runtime::new();
    let task = consumer(&rt);
    let mut guard = task.try_begin_run().unwrap();
    let _ = guard.need(DEF, KIND).unwrap();
    let own = guard.wake_handle();
    assert_eq!(
        (own.task(), own.execution()),
        (foreign.task(), foreign.execution()),
        "same task id and run number in both runtimes"
    );
    assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
    assert!(matches!(task.wake(foreign), Wake::Stale));
    assert_eq!(task.status(), TaskStatus::Waiting);
    assert!(matches!(task.wake(own), Wake::Runnable(_)));
}
