//! The wake/park/cancel matrix: every terminal action on a dependency
//! (publish `Ready`, publish `Failed`, cancel the cell, cancel the consumer,
//! duplicate or spurious wakes, publication and cancellation together) in
//! each window of the consumer's suspension:
//!
//! * **before registration**: the consumer's VM run is inside a gate just
//!   before its `__rt.need`;
//! * **subscribed, not parked**: the driver's [`RunGuard::need`] subscribed
//!   and the run is gated before [`RunGuard::park`] (the window a VM `need`
//!   closes inside one instruction);
//! * **parked**: the run ended `Waiting` and the task occupies no worker.
//!
//! Each cell asserts one exact outcome, the exact sequence of releases and
//! enqueues, the wake delivery kinds, effect counts (no replay) and the
//! cell's final state. Woken tasks migrate (placement hook).
//!
//! [`RunGuard::need`]: big_boy_pants_js::RunGuard::need
//! [`RunGuard::park`]: big_boy_pants_js::RunGuard::park

use std::sync::Barrier;
use std::thread;

use big_boy_pants_js::exec::{EnqueueReason, OutcomeKind, Placement};
use big_boy_pants_js::{
    Cancel, CellStatus, Completion, DependencyId, DependencyKey, ResultKind, Rooted, Task,
    TaskStatus, VmErrorKind, Wake,
};

use crate::harness::{
    Config, Harness, NOTE, POST, PRE, RustNeed, dispatches, enqueues, releases, scenario, w,
};
use crate::{blocker_exe, consumer_exe, done_exe};

const DEF_N: u64 = 41;
const KIND_N: u32 = 1;
const DEF: DependencyId = DependencyId::new(DEF_N);
const KIND: ResultKind = ResultKind::new(KIND_N);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// The producer publishes a value.
    Ready,
    /// The producer publishes an error.
    Failed,
    /// The embedder cancels the cell.
    CancelCell,
    /// The embedder cancels the consumer task.
    CancelConsumer,
    /// Publish, then deliver every wake handle again (and, while
    /// subscribed, deliver the run's handle before the completion).
    Duplicate,
    /// Publish and cancel the consumer in the same window.
    ReadyAndCancel,
}

const ACTIONS: [Action; 6] = [
    Action::Ready,
    Action::Failed,
    Action::CancelCell,
    Action::CancelConsumer,
    Action::Duplicate,
    Action::ReadyAndCancel,
];

fn key(h: &Harness) -> DependencyKey {
    DependencyKey::new(h.rt().world(), DEF, KIND)
}

fn spawn_consumer(h: &Harness, tag: f64) -> Task {
    h.spawn(
        h.rt().world(),
        &consumer_exe(),
        &[tag, DEF_N as f64, f64::from(KIND_N)],
    )
}

fn assert_pending(
    h: &Harness,
    key: DependencyKey,
    producer: bool,
    waiters: &[big_boy_pants_js::TaskId],
) {
    match h.rt().cell_status(key) {
        CellStatus::Pending {
            producer: p,
            waiters: ws,
        } => {
            assert_eq!(p.is_some(), producer, "producer claim of {key}");
            assert_eq!(ws, waiters, "waiters of {key}");
        }
        other => h.fail(format!("{key} should be pending, is {other:?}")),
    }
}

/// After a consumer was cancelled with its cell still pending: another
/// consumer is not affected, and the cell's producer (if one was claimed)
/// can still complete it for that consumer.
fn second_consumer_still_served(h: &Harness, request: Option<big_boy_pants_js::ProducerRequest>) {
    let key = key(h);
    let other = spawn_consumer(h, 90.0);
    let id = h.submit(&other, Placement::Any);
    h.wait_parked(id);
    let request = request.unwrap_or_else(|| h.take_request(key));
    let value = h.vector(&[9.0]);
    let d = h.publish(&request, Completion::Ready(&value));
    assert_eq!(d.runnable, [id]);
    assert!(h.complete_value(id).same_object(&value));
}

// ---------------------------------------------------------------------------
// Window 1: before registration (VM path, gate before `__rt.need`).

fn before_registration(action: Action) {
    scenario(
        &format!("before registration / {action:?}"),
        Config::default(),
        move |h| {
            let key = key(h);
            h.arm(1);
            let task = spawn_consumer(h, 1.0);
            let id = h.submit(&task, Placement::Worker(w(0)));
            h.wait_gated(id, 1);
            // The host produces before any need: its claim is the cell's one
            // producer, so the consumer's need never claims one.
            let request = h.rt().claim(key).unwrap();
            let value = h.vector(&[1.0, 2.0]);
            let error = h.number(-7.0);
            let mut cancelled_consumer = false;
            match action {
                Action::Ready | Action::Duplicate => {
                    let d = h.publish(&request, Completion::Ready(&value));
                    assert!(d.handles.is_empty(), "nobody subscribed yet: {d}");
                }
                Action::Failed => {
                    h.publish(&request, Completion::Failed(&error));
                }
                Action::CancelCell => {
                    let d = h.deliver("cancel cell".into(), h.rt().cancel_cell(key).unwrap());
                    assert!(d.handles.is_empty());
                }
                Action::CancelConsumer => {
                    assert!(!h.cancel(&task), "a running task is not enqueued");
                    cancelled_consumer = true;
                }
                Action::ReadyAndCancel => {
                    h.publish(&request, Completion::Ready(&value));
                    assert!(!h.cancel(&task));
                    cancelled_consumer = true;
                }
            }
            h.open(1);
            let events_of = |h: &Harness| (releases(&h.events(), id), enqueues(&h.events(), id));
            if cancelled_consumer {
                // The need is an observation point: the run ends Cancelled
                // there, whether or not the result is ready.
                assert!(matches!(
                    h.outcome(id),
                    big_boy_pants_js::RunOutcome::Cancelled
                ));
                assert!(h.effects(id, POST).is_empty());
                assert_eq!(task.status(), TaskStatus::Cancelled);
                if action == Action::CancelConsumer {
                    assert_pending(h, key, true, &[]);
                    second_consumer_still_served(h, Some(request));
                }
            } else {
                match action {
                    Action::Ready | Action::Duplicate => {
                        assert!(h.complete_value(id).same_object(&value));
                        assert_eq!(h.effects(id, POST).len(), 1);
                    }
                    Action::Failed => {
                        assert_eq!(h.failure(id), VmErrorKind::DependencyFailed(key));
                    }
                    Action::CancelCell => {
                        assert_eq!(h.failure(id), VmErrorKind::DependencyCancelled(key));
                    }
                    _ => unreachable!(),
                }
                // A ready read: recorded as an edge, never an unresolved wait.
                let log = task.dependency_log();
                assert!(log.depends_on(key));
                assert_eq!(log.waiting(), None);
            }
            let (rel, enq) = events_of(h);
            assert_eq!(rel.len(), 1, "one run, no suspension: {rel:?}");
            assert_eq!(enq, [EnqueueReason::Submitted]);
            assert_eq!(h.effects(id, PRE).len(), 1);
            assert_eq!(h.claimed(), 0, "the host claim is the only producer");
        },
    );
}

#[test]
fn matrix_before_registration() {
    for action in ACTIONS {
        before_registration(action);
    }
}

// ---------------------------------------------------------------------------
// Window 2: subscribed, not yet parked (driver need, gate before park).

fn subscribed_before_park(action: Action) {
    scenario(
        &format!("subscribed before park / {action:?}"),
        Config::default(),
        move |h| {
            let key = key(h);
            let gate = 2;
            h.arm(gate);
            let task = h.spawn(h.rt().world(), &done_exe(), &[5.0]);
            let plan = RustNeed {
                def: DEF,
                kind: KIND,
                park_gate: Some(gate),
            };
            let id = h.submit_rust_need(&task, plan, Placement::Worker(w(0)));
            h.wait_gated(id, gate);
            let request = h.take_request(key);
            let [handle] = <[_; 1]>::try_from(h.subscriptions(id)).unwrap();
            assert_eq!(handle.execution(), 1);
            assert_pending(h, key, true, &[id]);
            let value = h.vector(&[3.0]);
            let error = h.number(-1.0);
            match action {
                Action::Ready => {
                    let d = h.publish(&request, Completion::Ready(&value));
                    assert_eq!(d.remembered, [id], "{d}");
                }
                Action::Failed => {
                    let d = h.publish(&request, Completion::Failed(&error));
                    assert_eq!(d.remembered, [id], "{d}");
                }
                Action::CancelCell => {
                    let d = h.deliver("cancel cell".into(), h.rt().cancel_cell(key).unwrap());
                    assert_eq!(d.remembered, [id], "{d}");
                }
                Action::CancelConsumer => assert!(!h.cancel(&task)),
                Action::Duplicate => {
                    // A spurious wake of the subscribing run, then its
                    // duplicate: remembered once, stale after.
                    assert!(matches!(task.wake(handle), Wake::Remembered));
                    assert!(matches!(task.wake(handle), Wake::Stale));
                }
                Action::ReadyAndCancel => {
                    let d = h.publish(&request, Completion::Ready(&value));
                    assert_eq!(d.remembered, [id]);
                    assert!(!h.cancel(&task));
                }
            }
            h.open(gate);
            let events = |h: &Harness| (releases(&h.events(), id), enqueues(&h.events(), id));
            use EnqueueReason::{Submitted, Woken, Yielded};
            match action {
                Action::Ready | Action::Failed | Action::CancelCell => {
                    // The remembered wake turns the park into a yield; the
                    // requeued run reads the outcome. No wake is lost and the
                    // task is enqueued exactly once more.
                    let _ = h.complete_value(id);
                    let (rel, enq) = events(h);
                    assert_eq!(rel, [OutcomeKind::Yielded, OutcomeKind::Completed]);
                    assert_eq!(enq, [Submitted, Yielded]);
                    let what = match action {
                        Action::Ready => "ready",
                        Action::Failed => "failed",
                        _ => "cancelled",
                    };
                    assert!(h.notes().iter().any(|n| matches!(n,
                        crate::harness::Note::Read { task, what: w, .. } if *task == id && *w == what)));
                    if action == Action::Ready {
                        assert!(h.reads(id)[0].same_object(&value));
                    }
                    let [(first, 1), (second, 2)] =
                        <[_; 2]>::try_from(dispatches(&h.events(), id)).unwrap()
                    else {
                        h.fail("execution numbers 1 and 2")
                    };
                    assert_ne!(first, second, "the requeued run migrates");
                    assert_eq!(h.effects(id, NOTE).len(), 1, "the VM part runs once");
                }
                Action::CancelConsumer | Action::ReadyAndCancel => {
                    // Cancellation takes precedence over parking and over a
                    // remembered wake: terminal cleanup withdraws the waiter.
                    assert!(matches!(
                        h.outcome(id),
                        big_boy_pants_js::RunOutcome::Cancelled
                    ));
                    let (rel, enq) = events(h);
                    assert_eq!(rel, [OutcomeKind::Cancelled]);
                    assert_eq!(enq, [Submitted]);
                    assert!(h.effects(id, NOTE).is_empty());
                    assert_eq!(task.dependency_log().waiting(), None);
                    if action == Action::CancelConsumer {
                        assert_pending(h, key, true, &[]);
                        second_consumer_still_served(h, Some(request));
                    }
                }
                Action::Duplicate => {
                    // The spurious resume needs again, finds the cell pending
                    // and parks with a fresh handle; the old handle is stale.
                    // Only then does the real completion wake it, once.
                    let k = h.wait_parked(id);
                    assert_eq!(k, key);
                    let subs = h.subscriptions(id);
                    assert_eq!(subs.len(), 2);
                    assert_eq!(subs[1].execution(), 2);
                    assert!(matches!(task.wake(handle), Wake::Stale));
                    let d = h.publish(&request, Completion::Ready(&value));
                    assert_eq!(d.runnable, [id]);
                    for handle in &d.handles {
                        assert!(matches!(task.wake(*handle), Wake::Stale));
                    }
                    let _ = h.complete_value(id);
                    let (rel, enq) = events(h);
                    assert_eq!(
                        rel,
                        [
                            OutcomeKind::Yielded,
                            OutcomeKind::Waiting(key),
                            OutcomeKind::Completed
                        ]
                    );
                    assert_eq!(enq, [Submitted, Yielded, Woken]);
                    assert_eq!(h.claimed(), 1, "the second need claimed nothing");
                }
            }
            assert!(task.dependency_log().depends_on(key));
        },
    );
}

#[test]
fn matrix_subscribed_before_park() {
    for action in ACTIONS {
        subscribed_before_park(action);
    }
}

// ---------------------------------------------------------------------------
// Window 3: parked (VM path, the run ended `Waiting`).

fn parked(action: Action) {
    scenario(
        &format!("parked / {action:?}"),
        Config::default(),
        move |h| {
            let key = key(h);
            let task = spawn_consumer(h, 3.0);
            let id = h.submit(&task, Placement::Worker(w(0)));
            assert_eq!(h.wait_parked(id), key);
            assert_eq!(task.status(), TaskStatus::Waiting);
            assert_pending(h, key, true, &[id]);
            assert_eq!(task.dependency_log().waiting(), Some(key));
            let request = h.take_request(key);
            // A worker collects while the task is parked (before the
            // blockers, whose gated effects would delay it).
            h.collect();
            assert_eq!(task.status(), TaskStatus::Waiting);
            let value = h.vector(&[4.0, 5.0]);
            let error = h.number(-3.0);
            // Hold both workers so the woken task stays queued while the test
            // acts: the outcome is then decided by the actions alone.
            let busy = h.occupy(&blocker_exe(), 100);
            let mut handles = Vec::new();
            let enqueued_by_cancel = match action {
                Action::Ready | Action::Duplicate => {
                    let d = h.publish(&request, Completion::Ready(&value));
                    assert_eq!(d.runnable, [id]);
                    handles = d.handles;
                    false
                }
                Action::Failed => {
                    assert_eq!(
                        h.publish(&request, Completion::Failed(&error)).runnable,
                        [id]
                    );
                    false
                }
                Action::CancelCell => {
                    let d = h.deliver("cancel cell".into(), h.rt().cancel_cell(key).unwrap());
                    assert_eq!(d.runnable, [id]);
                    false
                }
                Action::CancelConsumer => {
                    assert!(
                        h.cancel(&task),
                        "a parked task is enqueued by its canceller"
                    );
                    assert!(matches!(task.cancel(), Cancel::AlreadyRequested));
                    true
                }
                Action::ReadyAndCancel => {
                    let d = h.publish(&request, Completion::Ready(&value));
                    assert_eq!(d.runnable, [id]);
                    assert!(!h.cancel(&task), "already runnable: the wake enqueued it");
                    false
                }
            };
            if action == Action::Duplicate {
                for handle in &handles {
                    assert!(matches!(task.wake(*handle), Wake::Stale), "duplicate wake");
                }
            }
            busy.release(h);
            use EnqueueReason::{Cancelled, Submitted, Woken};
            let resumed_by = if enqueued_by_cancel { Cancelled } else { Woken };
            match action {
                Action::Ready | Action::Duplicate => {
                    assert!(h.complete_value(id).same_object(&value));
                    assert_eq!(h.effects(id, POST).len(), 1);
                }
                Action::Failed => assert_eq!(h.failure(id), VmErrorKind::DependencyFailed(key)),
                Action::CancelCell => {
                    assert_eq!(h.failure(id), VmErrorKind::DependencyCancelled(key))
                }
                Action::CancelConsumer | Action::ReadyAndCancel => {
                    // Woken or not, the next run starts by observing the
                    // cancellation: no code after the need runs.
                    assert!(matches!(
                        h.outcome(id),
                        big_boy_pants_js::RunOutcome::Cancelled
                    ));
                    assert!(h.effects(id, POST).is_empty());
                    assert_eq!(task.dependency_log().waiting(), None);
                }
            }
            let events = h.events();
            assert_eq!(enqueues(&events, id), [Submitted, resumed_by]);
            let runs = dispatches(&events, id);
            assert_eq!(runs.len(), 2);
            assert_ne!(runs[0].0, runs[1].0, "the resumed run migrates");
            assert_eq!(releases(&events, id)[0], OutcomeKind::Waiting(key));
            assert_eq!(h.effects(id, PRE).len(), 1, "no replay of pre-wait effects");
            // Terminal stability: no wake, cancellation or completion changes
            // the finished task or the settled cell.
            for handle in &handles {
                assert!(matches!(task.wake(*handle), Wake::Stale));
            }
            assert!(matches!(
                task.cancel(),
                Cancel::Finished | Cancel::AlreadyRequested
            ));
            if action == Action::CancelConsumer {
                assert_pending(h, key, true, &[]);
                second_consumer_still_served(h, Some(request));
            } else {
                assert!(
                    h.rt().cancel_cell(key).is_err(),
                    "a settled cell stays settled"
                );
            }
        },
    );
}

#[test]
fn matrix_parked() {
    for action in ACTIONS {
        parked(action);
    }
}

/// A completion and a consumer cancellation of the same parked execution,
/// released together from two threads (barrier) while both workers are
/// held: whichever CAS wins, exactly one of them enqueues the task, and its
/// run observes the cancellation. Repeated to vary which side wins.
#[test]
fn completion_racing_consumer_cancellation_enqueues_once() {
    for round in 0..8 {
        scenario(
            &format!("completion vs cancellation, round {round}"),
            Config::default(),
            move |h| {
                let key = key(h);
                let task = spawn_consumer(h, 7.0);
                let id = h.submit(&task, Placement::Any);
                h.wait_parked(id);
                let request = h.take_request(key);
                let value: Rooted = h.vector(&[7.0]);
                let busy = h.occupy(&blocker_exe(), 100);
                let barrier = Barrier::new(2);
                let (woken, cancelled) = thread::scope(|s| {
                    let completer = s.spawn(|| {
                        barrier.wait();
                        h.publish(&request, Completion::Ready(&value))
                            .runnable
                            .len()
                    });
                    let canceller = s.spawn(|| {
                        barrier.wait();
                        h.cancel(&task)
                    });
                    (completer.join().unwrap(), canceller.join().unwrap())
                });
                assert_eq!(
                    woken + usize::from(cancelled),
                    1,
                    "exactly one side enqueues (woken {woken}, cancel {cancelled})"
                );
                busy.release(h);
                assert!(matches!(
                    h.outcome(id),
                    big_boy_pants_js::RunOutcome::Cancelled
                ));
                assert_eq!(enqueues(&h.events(), id).len(), 2);
                assert!(h.effects(id, POST).is_empty());
                assert!(
                    matches!(h.rt().cell_status(key), CellStatus::Ready(v) if v.same_object(&value))
                );
            },
        );
    }
}
