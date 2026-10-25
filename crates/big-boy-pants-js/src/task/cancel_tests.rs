//! Deterministic tests of consumer and cell cancellation (pellet
//! clj-quickjs-24) in every handoff window of `docs/design.md` §8:
//! registration, parking, wake, and the running task's observation points.
//! Each window is forced with a one-shot test hook ([`HookPoint`]) that runs
//! the racing operation at exactly that point, or with a barrier and a
//! bounded timeout; never a sleep.
//!
//! In each window the invariants are the same: the cancelled consumer is
//! enqueued at most once (by its canceller or by a completion's wake, never
//! both), no stale wake makes it runnable again, its next run observes the
//! cancellation and performs terminal cleanup, and another consumer of the
//! same cell still receives the producer's result.
//!
//! Needs are issued from Rust through the run guard, standing in for the
//! `__rt.need` VM operation of pellet clj-quickjs-23. Consumer tasks run
//! hand-assembled programs: early VM test fixtures, not JavaScript source
//! support.

use std::sync::{Arc, Barrier, Mutex};

use super::handoff_tests::{DEF, KIND, complete, consumer, hooks, key, number};
use super::*;
use crate::dep::{CellStatus, ClaimError, CompleteError, Completion, HookPoint};
use crate::host::HostCall;
use crate::value::Primitive;
use crate::vm::{Assembler, Opcode};
use crate::{ProducerRequest, Runtime};

/// Subscribe `task` in a fresh run (claiming the producer if first) and
/// park it; returns the run's wake handle and any producer request.
fn park(task: &Task) -> (WakeHandle, Option<ProducerRequest>) {
    let mut guard = task.try_begin_run().unwrap();
    let Need::Waiting { produce, .. } = guard.need(DEF, KIND).unwrap() else {
        panic!("the cell is pending");
    };
    let handle = guard.wake_handle();
    assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
    (handle, produce)
}

/// Run `task` once as its scheduler would: a cancelled task's run observes
/// the cancellation at run start and returns `Cancelled`.
fn run_cancelled(task: &Task) {
    let guard = task.try_begin_run().unwrap();
    assert!(guard.cancel_requested());
    assert!(matches!(guard.run(), RunOutcome::Cancelled));
    assert_terminally_cancelled(task);
}

/// A terminally cancelled task: finished, not runnable, never waiting, and
/// no wake handle of any of its runs revives it.
fn assert_terminally_cancelled(task: &Task) {
    assert_eq!(task.status(), TaskStatus::Cancelled);
    assert_eq!(task.try_begin_run().err(), Some(RunError::Finished));
    assert_eq!(task.dependency_log().waiting(), None);
    assert!(matches!(task.cancel(), Cancel::Finished));
    let last = task.0.exec.snapshot().execution;
    for execution in 1..=last + 1 {
        let handle = WakeHandle::new(task.0.deps.runtime, task.id(), execution);
        assert!(matches!(task.wake(handle), Wake::Stale), "run {execution}");
    }
}

fn waiters(rt: &Runtime) -> Vec<TaskId> {
    match rt.cell_status(key(rt)) {
        CellStatus::Pending { waiters, .. } => waiters,
        other => panic!("expected Pending, got {other:?}"),
    }
}

/// `other` parks on the cell, `req` completes it, and `other` reads the
/// value: the producer survived the cancellation of a fellow consumer.
fn other_consumer_still_gets(rt: &Runtime, other: &Task, req: &ProducerRequest, n: f64) {
    let (handle, produce) = park(other);
    assert!(produce.is_none(), "the producer was already claimed");
    let wakeups = complete(rt, req, n);
    assert!(
        matches!(wakeups.deliveries(), [(h, Wake::Runnable(_))] if *h == handle),
        "only the live consumer is woken: {:?}",
        wakeups.deliveries()
    );
    let runnable = wakeups.into_runnable();
    let mut guard = runnable[0].try_begin_run().unwrap();
    assert_eq!(number(guard.need(DEF, KIND).unwrap()), n);
}

/// Registration window, before the need: the cancelled consumer's need is
/// refused (`NeedError::Cancelled`, distinct from the cell outcome
/// `Need::Cancelled`), it registers nothing and claims no producer, and the
/// run ends cancelled at park.
#[test]
fn cancel_before_registration_refuses_the_need() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    assert!(matches!(cancelled.cancel(), Cancel::Requested));
    assert!(matches!(cancelled.cancel(), Cancel::AlreadyRequested));
    assert_eq!(cancelled.status(), TaskStatus::Ready, "not observed yet");
    assert!(cancelled.cancel_requested());
    let mut guard = cancelled.try_begin_run().unwrap();
    assert_eq!(
        guard.need(DEF, KIND).unwrap_err(),
        NeedError::Cancelled {
            requested: key(&rt)
        }
    );
    assert!(matches!(rt.cell_status(key(&rt)), CellStatus::Absent));
    assert!(matches!(guard.park(), RunOutcome::Cancelled));
    assert_terminally_cancelled(&cancelled);
    assert!(cancelled.dependency_log().edges().is_empty());

    let req = rt.claim(key(&rt)).unwrap();
    other_consumer_still_gets(&rt, &other, &req, 1.0);
}

/// Registration window, inside the need: the cancellation lands after the
/// fast path read `Pending` and before the subscription. The need still
/// subscribes and returns the producer request it claimed (dropping it
/// would strand the other consumers); the park observes the cancellation,
/// removes the waiter entry and leaves the producer claimed.
#[test]
fn cancel_during_registration_keeps_the_claimed_producer() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    {
        let task = cancelled.clone();
        hooks(&cancelled).once(HookPoint::NeedAfterPendingRead, move || {
            assert!(matches!(task.cancel(), Cancel::Requested));
        });
    }
    let mut guard = cancelled.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims, cancelled or not");
    };
    assert_eq!(waiters(&rt), [cancelled.id()]);
    assert!(guard.cancel_requested());
    assert!(matches!(guard.park(), RunOutcome::Cancelled));
    assert_terminally_cancelled(&cancelled);
    assert!(waiters(&rt).is_empty(), "the waiter entry was removed");
    assert!(matches!(
        rt.cell_status(key(&rt)),
        CellStatus::Pending { producer: Some(p), .. } if p == req.producer()
    ));
    assert_eq!(cancelled.dependency_log().edges(), [key(&rt)]);

    other_consumer_still_gets(&rt, &other, &req, 2.0);
}

/// Parking window, before the park: subscribed, then cancelled while still
/// executing. The park observes it, even ahead of a remembered wake from a
/// completion that also landed in the window.
#[test]
fn cancel_after_subscription_is_observed_by_the_park() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    let mut guard = cancelled.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    assert!(matches!(cancelled.cancel(), Cancel::Requested));
    assert_eq!(cancelled.status(), TaskStatus::Running);
    assert!(matches!(guard.park(), RunOutcome::Cancelled));
    assert_terminally_cancelled(&cancelled);
    other_consumer_still_gets(&rt, &other, &req, 3.0);

    // The same window with the completion's wake remembered first.
    let rt = Runtime::new();
    let cancelled = consumer(&rt);
    let mut guard = cancelled.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    let wakeups = complete(&rt, &req, 3.5);
    assert!(matches!(wakeups.deliveries(), [(_, Wake::Remembered)]));
    assert!(matches!(cancelled.cancel(), Cancel::Requested));
    assert!(
        matches!(guard.park(), RunOutcome::Cancelled),
        "cancellation wins over the remembered wake"
    );
    assert_terminally_cancelled(&cancelled);
}

/// Parking window, inside the park: the park has read `Executing` and the
/// cancellation sets its bit before the park's CAS. The CAS fails, the park
/// rereads, and the run ends cancelled instead of parking with only a
/// cancellation left to wake it.
#[test]
fn cancel_between_park_read_and_park_cas_is_observed() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    let mut guard = cancelled.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    let fired = Arc::new(Mutex::new(false));
    {
        let (task, fired) = (cancelled.clone(), Arc::clone(&fired));
        hooks(&cancelled).once(HookPoint::ParkBeforeCas, move || {
            assert!(matches!(task.cancel(), Cancel::Requested));
            *fired.lock().unwrap() = true;
        });
    }
    assert!(matches!(guard.park(), RunOutcome::Cancelled));
    assert!(*fired.lock().unwrap(), "the hook cancelled inside the park");
    assert_terminally_cancelled(&cancelled);
    assert!(waiters(&rt).is_empty());
    other_consumer_still_gets(&rt, &other, &req, 4.0);
}

/// Parked window: the cancellation makes the parked task runnable and
/// returns it to enqueue. Its old wake handle is stale from then on, so the
/// completion (still finding its waiter entry, since the cleanup has not
/// run) cannot enqueue it a second time; its run cleans up, and the other
/// consumer gets the value.
#[test]
fn cancel_after_park_enqueues_once_and_stale_wakes_cannot_resurrect() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    let (handle, req) = park(&cancelled);
    let req = req.expect("the first need claims");
    let (other_handle, _) = park(&other);
    let Cancel::Runnable(enqueued) = cancelled.cancel() else {
        panic!("a parked task is returned to enqueue");
    };
    assert_eq!(enqueued.id(), cancelled.id());
    assert_eq!(cancelled.status(), TaskStatus::Ready);
    assert!(matches!(cancelled.wake(handle), Wake::Stale));
    assert!(matches!(cancelled.cancel(), Cancel::AlreadyRequested));

    let wakeups = complete(&rt, &req, 5.0);
    assert!(
        matches!(
            wakeups.deliveries(),
            [(h1, Wake::Stale), (h2, Wake::Runnable(_))] if *h1 == handle && *h2 == other_handle
        ),
        "{:?}",
        wakeups.deliveries()
    );
    let runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    run_cancelled(&enqueued);
    let mut guard = runnable[0].try_begin_run().unwrap();
    assert_eq!(number(guard.need(DEF, KIND).unwrap()), 5.0);
}

/// Parked window, cleanup before the completion: once the cancelled task's
/// run has cleaned up, its waiter entry is gone and the completion wakes
/// only the other consumer.
#[test]
fn cancelled_parked_consumer_leaves_the_cell_when_its_run_cleans_up() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    let (_, req) = park(&cancelled);
    let req = req.expect("the first need claims");
    let Cancel::Runnable(enqueued) = cancelled.cancel() else {
        panic!("a parked task is returned to enqueue");
    };
    assert_eq!(
        waiters(&rt),
        [cancelled.id()],
        "invalidated, not yet removed"
    );
    run_cancelled(&enqueued);
    assert!(waiters(&rt).is_empty());
    other_consumer_still_gets(&rt, &other, &req, 6.0);
}

/// Wake window, cancellation inside the wake: the completion's wake has
/// read `Parked`, and the cancellation makes the task runnable before the
/// wake's CAS. The CAS fails, the wake rereads `CANCELLED` and is stale:
/// the canceller alone enqueues it.
#[test]
fn cancel_between_wake_read_and_wake_cas_enqueues_once() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    let (_, req) = park(&cancelled);
    let req = req.expect("the first need claims");
    let (other_handle, _) = park(&other);
    let by_cancel = Arc::new(Mutex::new(None));
    {
        let (task, by_cancel) = (cancelled.clone(), Arc::clone(&by_cancel));
        hooks(&cancelled).once(HookPoint::WakeBeforeCas, move || {
            *by_cancel.lock().unwrap() = Some(task.cancel());
        });
    }
    let wakeups = complete(&rt, &req, 7.0);
    let Some(Cancel::Runnable(enqueued)) = by_cancel.lock().unwrap().take() else {
        panic!("the cancellation won the parked word");
    };
    assert!(
        matches!(
            wakeups.deliveries(),
            [(_, Wake::Stale), (h, Wake::Runnable(_))] if *h == other_handle
        ),
        "{:?}",
        wakeups.deliveries()
    );
    let runnable = wakeups.into_runnable();
    assert_eq!(
        runnable.len(),
        1,
        "the cancelled task is not enqueued twice"
    );
    run_cancelled(&enqueued);
    assert_eq!(
        number(
            runnable[0]
                .try_begin_run()
                .unwrap()
                .need(DEF, KIND)
                .unwrap()
        ),
        7.0
    );
}

/// Wake window, wake inside the cancellation: the cancellation has read
/// `Parked`, and the completion's wake makes the task runnable before the
/// cancellation's CAS. The cancellation rereads `Runnable`, only sets its
/// bit and enqueues nothing: the wake alone enqueues it. The woken run then
/// observes the cancellation instead of reading the value.
#[test]
fn wake_between_cancel_read_and_cancel_cas_enqueues_once() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    let (handle, req) = park(&cancelled);
    let req = req.expect("the first need claims");
    let woken = Arc::new(Mutex::new(None));
    {
        let (rt, woken) = (rt.clone(), Arc::clone(&woken));
        hooks(&cancelled).once(HookPoint::CancelBeforeCas, move || {
            *woken.lock().unwrap() = Some(complete(&rt, &req, 8.0));
        });
    }
    assert!(matches!(cancelled.cancel(), Cancel::Requested));
    let wakeups = woken.lock().unwrap().take().expect("the hook completed");
    assert!(matches!(
        wakeups.deliveries(),
        [(h, Wake::Runnable(_))] if *h == handle
    ));
    let runnable = wakeups.into_runnable();
    let mut guard = runnable[0].try_begin_run().unwrap();
    assert!(guard.cancel_requested(), "observed when it next runs");
    assert!(matches!(
        guard.need(DEF, KIND),
        Err(NeedError::Cancelled { .. })
    ));
    assert!(matches!(guard.park(), RunOutcome::Cancelled));
    assert_terminally_cancelled(&cancelled);
    // The cell completed; another consumer reads the value directly.
    let mut guard = other.try_begin_run().unwrap();
    assert_eq!(number(guard.need(DEF, KIND).unwrap()), 8.0);
}

/// Host whose effects cancel a task (its own run's task) and count calls.
struct Canceller {
    task: Option<Task>,
    calls: usize,
}

impl crate::Host for Canceller {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        self.calls += 1;
        if let Some(task) = self.task.take() {
            assert!(matches!(task.cancel(), Cancel::Requested));
        }
        Ok(Primitive::Number(self.calls as f64))
    }
}

/// `effect(1); local0 = 2; return local0;`: five instructions, the effect
/// second. Early VM test bytecode.
fn effect_then_return() -> Executable {
    let mut asm = Assembler::new();
    asm.constant(Primitive::Number(1.0)).host_effect(0); // 0, 1
    asm.constant(Primitive::Number(2.0)).set_local(0); // 2, 3
    asm.get_local(0).op(Opcode::Return); // 4, 5
    asm.finish("effect-then-return", 0, 1).unwrap()
}

/// Running window: a cancellation requested while the task executes (here
/// by a host effect of its own run) is observed at the budget boundary: the
/// run ends `Cancelled` instead of `Yielded`, and nothing after the
/// boundary executes. A run that returns before reaching an observation
/// point reports its outcome; the request is moot.
#[test]
fn cancel_while_running_is_observed_at_the_budget_boundary() {
    let rt = Runtime::new();
    let task = rt.spawn(&effect_then_return(), &[]).unwrap();
    let mut host = Canceller {
        task: Some(task.clone()),
        calls: 0,
    };
    let outcome = task.run_with(Budget::instructions(3), &mut host).unwrap();
    assert!(matches!(outcome, RunOutcome::Cancelled), "{outcome:?}");
    assert_eq!(host.calls, 1);
    assert_terminally_cancelled(&task);
    assert!(task.saved_state().is_none(), "VM state released");

    let task = rt.spawn(&effect_then_return(), &[]).unwrap();
    let mut host = Canceller {
        task: Some(task.clone()),
        calls: 0,
    };
    let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
    assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
    assert_eq!(task.status(), TaskStatus::Finished);
    assert!(!task.cancel_requested(), "the completed run dropped it");
    assert!(matches!(task.cancel(), Cancel::Finished));
}

/// Running window, between runs: a run that yields (budget) before the
/// request leaves the task runnable with its state saved; the request then
/// makes the next run end at run start without executing anything.
#[test]
fn cancel_between_runs_is_observed_at_run_start() {
    let rt = Runtime::new();
    let task = rt.spawn(&effect_then_return(), &[]).unwrap();
    let mut host = Canceller {
        task: None,
        calls: 0,
    };
    let outcome = task.run_with(Budget::instructions(1), &mut host).unwrap();
    assert!(matches!(outcome, RunOutcome::Yielded));
    assert!(matches!(task.cancel(), Cancel::Requested));
    let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
    assert!(matches!(outcome, RunOutcome::Cancelled));
    assert_eq!(host.calls, 0, "nothing executed after the request");
    assert_terminally_cancelled(&task);
}

/// Roots held by a cancelled consumer's suspended state (its saved locals)
/// stay roots while it is parked and after the cancellation request (the
/// task's own run has not cleaned up yet), and are released and reclaimable
/// after terminal cleanup. The cell never held them.
#[test]
fn cancelled_consumer_roots_are_reclaimable_after_terminal_cleanup() {
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.get_local(0).op(Opcode::Return);
    let exe = asm.finish("hold", 1, 1).unwrap();
    let obj = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    let weak = obj.downgrade();
    let task = rt.spawn(&exe, &[&obj]).unwrap();
    drop(obj);
    let (_, req) = park(&task);
    let _req = req.expect("the first need claims");
    assert_eq!(rt.collect().unwrap().reclaimed, 0);
    assert!(weak.is_live(), "a parked task's frame is a root");
    let Cancel::Runnable(enqueued) = task.cancel() else {
        panic!("a parked task is returned to enqueue");
    };
    assert_eq!(rt.collect().unwrap().reclaimed, 0);
    assert!(weak.is_live(), "roots stay until terminal cleanup");
    run_cancelled(&enqueued);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert!(!weak.is_live(), "released by terminal cleanup");
    drop(enqueued);
    assert_eq!(task.status(), TaskStatus::Cancelled);
}

/// Terminal cleanup also releases the cancelled task's open transient
/// builders, even one retained from outside: its draft (and the object in
/// it) becomes reclaimable while the builder object itself stays alive
/// through the external root, and any later use reports `OwnerFinished`.
#[test]
fn cancellation_releases_open_builder_drafts() {
    // let b = vector_builder(); conj!(b, {}); /* yield */ return 1;
    let mut asm = Assembler::new();
    asm.op(Opcode::NewVectorBuilder).set_local(0);
    asm.get_local(0)
        .op(Opcode::NewObject)
        .op(Opcode::BuilderConj)
        .op(Opcode::Pop);
    asm.constant(Primitive::Number(1.0)).op(Opcode::Return);
    let rt = Runtime::new();
    let owner = rt.spawn(&asm.finish("owner", 0, 1).unwrap(), &[]).unwrap();
    let first = owner.run_with(Budget::instructions(6), &mut crate::host::NoHost);
    assert!(matches!(first, Ok(RunOutcome::Yielded)), "{first:?}");
    let (_, _, locals) = owner.saved_state().unwrap();
    let builder = crate::Rooted::new(rt.heap(), locals[0].clone());
    assert_eq!(rt.collect().unwrap().reclaimed, 0, "the draft is live");

    assert!(matches!(owner.cancel(), Cancel::Requested));
    assert!(matches!(owner.run().unwrap(), RunOutcome::Cancelled));
    let stats = rt.collect().unwrap();
    assert_eq!(
        (stats.live, stats.reclaimed),
        (1, 1),
        "only the draft's object"
    );
    assert!(builder.downgrade().is_live());

    let mut asm = Assembler::new();
    asm.get_local(0).op(Opcode::Freeze).op(Opcode::Return);
    let other = rt
        .spawn(&asm.finish("other", 1, 1).unwrap(), &[&builder])
        .unwrap();
    let RunOutcome::Failed(err) = other.run().unwrap() else {
        panic!("expected Failed");
    };
    let expected = crate::BuilderError::OwnerFinished { owner: owner.id() };
    assert_eq!(err.kind(), crate::VmErrorKind::Builder(expected));
}

/// Cell cancellation by the embedder (no producer id): every waiter, in
/// every handoff state, is woken exactly once with the terminal outcome,
/// redelivery is stale, and each resumed need reads `Need::Cancelled`
/// while the consumer itself stays live (cell cancellation is not consumer
/// cancellation). A consumer that was itself cancelled is not resurrected.
/// The outcome is terminal: the producer's completion, a claim and a second
/// cancellation are all rejected.
#[test]
fn cell_cancellation_wakes_every_waiter_once_and_is_terminal() {
    let rt = Runtime::new();
    let k = key(&rt);
    let parked: Vec<Task> = (0..2).map(|_| consumer(&rt)).collect();
    let mut req = None;
    for task in &parked {
        req = req.or(park(task).1);
    }
    let req = req.expect("one producer request");
    // Subscribed in a run that ended without parking (guard dropped).
    let unparked = consumer(&rt);
    let _ = unparked.try_begin_run().unwrap().need(DEF, KIND).unwrap();
    // Parked, then cancelled as a consumer.
    let gone = consumer(&rt);
    park(&gone);
    let Cancel::Runnable(gone_enqueued) = gone.cancel() else {
        panic!("a parked task is returned to enqueue");
    };
    // Still in its subscribing run.
    let executing = consumer(&rt);
    let mut executing_guard = executing.try_begin_run().unwrap();
    let _ = executing_guard.need(DEF, KIND).unwrap();

    let wakeups = rt.cancel_cell(k).unwrap();
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
        ["runnable", "runnable", "stale", "stale", "remembered"],
        "subscription order: parked x2, unparked (runnable already), \
         consumer-cancelled (never woken), executing"
    );
    let handles: Vec<WakeHandle> = wakeups.deliveries().iter().map(|(h, _)| *h).collect();
    let mut queue = wakeups.into_runnable();
    assert!(matches!(executing_guard.park(), RunOutcome::Yielded));
    queue.push(executing.clone());
    queue.push(unparked.clone());
    for h in &handles {
        let task = [&parked[0], &parked[1], &unparked, &gone, &executing]
            .into_iter()
            .find(|t| t.id() == h.task())
            .unwrap();
        assert!(matches!(task.wake(*h), Wake::Stale), "redelivery");
    }
    let mut resumed = std::collections::HashMap::new();
    while let Some(task) = queue.pop() {
        *resumed.entry(task.id()).or_insert(0) += 1;
        let mut guard = task.try_begin_run().unwrap();
        assert!(matches!(guard.need(DEF, KIND).unwrap(), Need::Cancelled));
        assert!(!guard.cancel_requested(), "the consumer is not cancelled");
        assert_eq!(task.dependency_log().waiting(), None);
    }
    assert_eq!(resumed.len(), 4);
    assert!(resumed.values().all(|&n| n == 1), "{resumed:?}");
    run_cancelled(&gone_enqueued);

    assert!(matches!(rt.cell_status(k), CellStatus::Cancelled));
    let value = rt.root(Primitive::Number(9.0));
    assert_eq!(
        rt.complete(k, req.producer(), Completion::Ready(&value))
            .unwrap_err(),
        CompleteError::AlreadyComplete { key: k }
    );
    assert_eq!(
        rt.claim(k).unwrap_err(),
        ClaimError::AlreadyComplete { key: k }
    );
    assert_eq!(
        rt.cancel_cell(k).unwrap_err(),
        CompleteError::AlreadyComplete { key: k }
    );
    assert!(matches!(rt.cell_status(k), CellStatus::Cancelled));
}

/// The embedder may cancel a cell nobody has claimed or even created: the
/// cell is created cancelled, so later needs read `Cancelled` instead of
/// claiming a producer, and a claim is refused.
#[test]
fn cell_cancellation_needs_no_producer() {
    let rt = Runtime::new();
    let k = key(&rt);
    assert!(rt.cancel_cell(k).unwrap().deliveries().is_empty());
    let task = consumer(&rt);
    let mut guard = task.try_begin_run().unwrap();
    assert!(matches!(guard.need(DEF, KIND).unwrap(), Need::Cancelled));
    assert_eq!(
        rt.claim(k).unwrap_err(),
        ClaimError::AlreadyComplete { key: k }
    );
}

/// Producer completion and embedder cancellation race after both passed
/// their unlocked checks (a barrier in the hook holds both until then): the
/// recheck under the cell lock lets exactly one install; the other is
/// `AlreadyComplete`, and the parked waiter is woken exactly once.
#[test]
fn producer_completion_and_cell_cancellation_settle_once() {
    let rt = Runtime::new();
    let task = consumer(&rt);
    let (_, req) = park(&task);
    let req = req.expect("the first need claims");
    let barrier = Arc::new(Barrier::new(2));
    {
        let barrier = Arc::clone(&barrier);
        hooks(&task).always(HookPoint::CompleteBeforeInstall, move || {
            barrier.wait();
        });
    }
    let (completed, cancelled) = std::thread::scope(|s| {
        let producer = s.spawn(|| {
            let value = rt.root(Primitive::Number(10.0));
            rt.complete(req.key(), req.producer(), Completion::Ready(&value))
        });
        let canceller = s.spawn(|| rt.cancel_cell(key(&rt)));
        (producer.join().unwrap(), canceller.join().unwrap())
    });
    let (won, lost) = match (completed, cancelled) {
        (Ok(w), Err(e)) => ((w, "ready"), e),
        (Err(e), Ok(w)) => ((w, "cancelled"), e),
        other => panic!("exactly one must win: {other:?}"),
    };
    assert_eq!(lost, CompleteError::AlreadyComplete { key: key(&rt) });
    let (wakeups, winner) = won;
    let runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1, "woken exactly once");
    let mut guard = runnable[0].try_begin_run().unwrap();
    match (winner, guard.need(DEF, KIND).unwrap()) {
        ("ready", Need::Ready(v)) => assert_eq!(v.view(), crate::ValueView::Number(10.0)),
        ("cancelled", Need::Cancelled) => {}
        (w, other) => panic!("{w} won but the need read {other:?}"),
    }
}
