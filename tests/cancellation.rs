//! Consumer and cell cancellation through the embedding API (pellet
//! clj-quickjs-24) under real thread races. The individual handoff windows
//! are forced deterministically with test hooks in
//! `src/task/cancel_tests.rs`; here one barrier releases parking consumers,
//! a producer (completing, or the embedder cancelling the cell) and a
//! canceller of half the consumers together, many times over, and the
//! invariants are checked on every iteration. No sleeps; every wait is a
//! barrier or a bounded join.
//!
//! Needs are issued from Rust through the run guard, standing in for the
//! `__rt.need` VM operation of pellet clj-quickjs-23. Consumer tasks run
//! JavaScript source: the synthetic fixture `fixtures/js/cancel_consumer.js`
//! (not light-cljs output), compiled with `frontend::compile_script`.

use std::collections::HashMap;
use std::sync::Barrier;
use std::thread;

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{
    Budget, Cancel, CellStatus, Completion, DependencyId, DependencyKey, Host, HostCall, Need,
    NeedError, Primitive, ResultKind, RunError, RunOutcome, Runtime, Task, TaskId, TaskStatus,
    ValueView, Wake, WakeHandle,
};

const DEF: DependencyId = DependencyId::new(21);
const KIND: ResultKind = ResultKind::new(4);
const ITERATIONS: usize = 200;
const CONSUMERS: usize = 6;

/// The source fixture: a loop that completes with this value.
const FIXTURE_RESULT: f64 = 4950.0;

fn consumer(rt: &Runtime) -> Task {
    let exe = compile_script(
        "cancel_consumer",
        include_str!("fixtures/js/cancel_consumer.js"),
    )
    .unwrap();
    rt.spawn(&exe, &[]).unwrap()
}

/// Consumers subscribe, then (released by the barrier) park on their own
/// threads while the producer settles the cell and a canceller cancels the
/// even-numbered consumers. Afterwards:
///
/// * a consumer whose park observed the cancellation is finished and is
///   enqueued by nobody; every other consumer is enqueued exactly once, by
///   its own runner (park yielded), the settlement's wakeups, or its
///   canceller; never twice, never zero times;
/// * redelivering every wake handle changes nothing;
/// * each enqueued cancelled consumer's run ends `Cancelled`, and every
///   uncancelled consumer reads the outcome: the value on even iterations
///   (producer completion), `Need::Cancelled` on odd ones (the embedder
///   cancelled the cell), without itself being cancelled.
#[test]
fn consumer_cancellation_races_settlement_without_lost_or_double_enqueue() {
    for iteration in 0..ITERATIONS {
        let cancel_cell = iteration % 2 == 1;
        let rt = Runtime::new();
        let key = DependencyKey::new(rt.world(), DEF, KIND);
        let req = rt.claim(key).unwrap();
        let consumers: Vec<Task> = (0..CONSUMERS).map(|_| consumer(&rt)).collect();
        let cancelled = |id: TaskId| consumers.iter().position(|t| t.id() == id).unwrap() % 2 == 0;
        let barrier = Barrier::new(CONSUMERS + 2);

        let (parks, settled, cancels) = thread::scope(|s| {
            let parkers: Vec<_> = consumers
                .iter()
                .map(|task| {
                    let barrier = &barrier;
                    s.spawn(move || {
                        let mut guard = task.try_begin_run().unwrap();
                        let need = guard.need(DEF, KIND).unwrap();
                        assert!(matches!(need, Need::Waiting { produce: None, .. }));
                        let handle = guard.wake_handle();
                        barrier.wait();
                        (task.id(), handle, guard.park())
                    })
                })
                .collect();
            let producer = s.spawn(|| {
                barrier.wait();
                if cancel_cell {
                    rt.cancel_cell(key).unwrap()
                } else {
                    let value = rt.root(Primitive::Number(iteration as f64));
                    rt.complete(key, req.producer(), Completion::Ready(&value))
                        .unwrap()
                }
            });
            let canceller = s.spawn(|| {
                barrier.wait();
                consumers
                    .iter()
                    .filter(|t| cancelled(t.id()))
                    .map(|t| (t.id(), t.cancel()))
                    .collect::<Vec<_>>()
            });
            let parks: Vec<_> = parkers.into_iter().map(|h| h.join().unwrap()).collect();
            (parks, producer.join().unwrap(), canceller.join().unwrap())
        });

        let context = format!("iteration {iteration}");
        let mut enqueued: HashMap<TaskId, (usize, Task)> = HashMap::new();
        let mut enqueue = |task: Task| {
            enqueued.entry(task.id()).or_insert((0, task)).0 += 1;
        };
        let mut finished_in_park = Vec::new();
        for (id, _, outcome) in &parks {
            let task = consumers.iter().find(|t| t.id() == *id).unwrap();
            match outcome {
                RunOutcome::Waiting(k) => assert_eq!(*k, key, "{context}"),
                RunOutcome::Yielded => enqueue(task.clone()),
                RunOutcome::Cancelled => {
                    assert!(cancelled(*id), "{context}: only cancelled consumers");
                    finished_in_park.push(*id);
                }
                other => panic!("{context}: unexpected park outcome {other:?}"),
            }
        }
        let handles: Vec<WakeHandle> = parks.iter().map(|(_, h, _)| *h).collect();
        for task in settled.into_runnable() {
            enqueue(task);
        }
        for (id, cancel) in cancels {
            match cancel {
                Cancel::Runnable(task) => enqueue(task),
                Cancel::Requested => {}
                other => panic!("{context}: first cancellation of {id} gave {other:?}"),
            }
        }
        for task in &consumers {
            let times = enqueued.get(&task.id()).map_or(0, |(n, _)| *n);
            let expected = usize::from(!finished_in_park.contains(&task.id()));
            assert_eq!(times, expected, "{context}: {} enqueued", task.id());
        }
        for (task, h) in consumers.iter().zip(&handles) {
            assert!(
                matches!(task.wake(*h), Wake::Stale),
                "{context}: redelivery"
            );
        }

        for (_, task) in enqueued.into_values() {
            let mut guard = task.try_begin_run().unwrap();
            if cancelled(task.id()) {
                assert!(matches!(
                    guard.need(DEF, KIND),
                    Err(NeedError::Cancelled { .. })
                ));
                assert!(matches!(guard.park(), RunOutcome::Cancelled), "{context}");
            } else {
                match (cancel_cell, guard.need(DEF, KIND).unwrap()) {
                    (false, Need::Ready(v)) => {
                        assert_eq!(v.view(), ValueView::Number(iteration as f64))
                    }
                    (true, Need::Cancelled) => {}
                    (_, other) => panic!("{context}: read {other:?}"),
                }
                assert!(!guard.cancel_requested(), "{context}");
                // The uncancelled consumer goes on to run its source to the end.
                match guard.run() {
                    RunOutcome::Complete(v) => {
                        assert_eq!(v.view(), ValueView::Number(FIXTURE_RESULT), "{context}")
                    }
                    other => panic!("{context}: {other:?}"),
                }
            }
        }
        for task in &consumers {
            if cancelled(task.id()) {
                assert_eq!(task.status(), TaskStatus::Cancelled, "{context}");
                assert_eq!(task.try_begin_run().err(), Some(RunError::Finished));
            } else {
                assert_eq!(task.status(), TaskStatus::Finished, "{context}");
            }
            assert_eq!(task.dependency_log().waiting(), None, "{context}");
        }
        match rt.cell_status(key) {
            CellStatus::Ready(_) => assert!(!cancel_cell),
            CellStatus::Cancelled => assert!(cancel_cell),
            other => panic!("{context}: {other:?}"),
        }
    }
}

/// A parked consumer is cancelled from several threads at once: exactly one
/// canceller gets the task back to enqueue; the others see the request
/// already made. The producer it shares with another consumer is not
/// cancelled: the other consumer still gets the value.
#[test]
fn concurrent_cancellations_of_one_consumer_enqueue_it_once() {
    const CANCELLERS: usize = 4;
    for iteration in 0..ITERATIONS {
        let rt = Runtime::new();
        let key = DependencyKey::new(rt.world(), DEF, KIND);
        let (target, other) = (consumer(&rt), consumer(&rt));
        let mut req = None;
        for task in [&target, &other] {
            let mut guard = task.try_begin_run().unwrap();
            if let Need::Waiting { produce, .. } = guard.need(DEF, KIND).unwrap() {
                req = req.or(produce);
            }
            assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
        }
        let req = req.expect("one producer request");
        let barrier = Barrier::new(CANCELLERS);
        let results: Vec<Cancel> = thread::scope(|s| {
            let handles: Vec<_> = (0..CANCELLERS)
                .map(|_| {
                    s.spawn(|| {
                        barrier.wait();
                        target.cancel()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let runnable: Vec<Task> = results
            .into_iter()
            .filter_map(|c| match c {
                Cancel::Runnable(t) => Some(t),
                Cancel::AlreadyRequested => None,
                other => panic!("iteration {iteration}: {other:?}"),
            })
            .collect();
        assert_eq!(runnable.len(), 1, "iteration {iteration}");
        assert!(matches!(runnable[0].run(), Ok(RunOutcome::Cancelled)));
        assert_eq!(target.status(), TaskStatus::Cancelled);

        let value = rt.root(Primitive::Number(1.0));
        let woken = rt
            .complete(key, req.producer(), Completion::Ready(&value))
            .unwrap()
            .into_runnable();
        assert_eq!(woken.len(), 1, "iteration {iteration}");
        assert_eq!(woken[0].id(), other.id());
        let mut guard = woken[0].try_begin_run().unwrap();
        assert!(matches!(guard.need(DEF, KIND).unwrap(), Need::Ready(_)));
    }
}

/// A host that refuses every effect (the fixture performs none).
struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected effect {}", call.op))
    }
}

/// Two consumers running the same compiled source fixture, cut into
/// budgeted runs and interleaved. One yields mid-loop and is then cancelled
/// from another thread: its next run observes the request at run start,
/// executes nothing more, releases its VM state and ends `Cancelled`. The
/// other consumer of the same shared code completes the loop with the
/// fixture's value.
#[test]
fn a_yielded_source_consumer_is_cancelled_at_its_next_run() {
    let rt = Runtime::new();
    let (cancelled, other) = (consumer(&rt), consumer(&rt));
    let budget = Budget::instructions(25);
    for task in [&cancelled, &other] {
        assert!(matches!(
            task.run_with(budget, &mut NoEffects).unwrap(),
            RunOutcome::Yielded
        ));
    }
    assert!(
        cancelled.source_location().is_some(),
        "suspended mid-source"
    );
    let request = thread::scope(|s| s.spawn(|| cancelled.cancel()).join().unwrap());
    assert!(matches!(request, Cancel::Requested));
    assert_eq!(cancelled.status(), TaskStatus::Ready, "not observed yet");
    assert!(matches!(
        cancelled.run_with(budget, &mut NoEffects).unwrap(),
        RunOutcome::Cancelled
    ));
    assert_eq!(cancelled.status(), TaskStatus::Cancelled);
    assert_eq!(cancelled.source_location(), None, "VM state released");
    assert_eq!(
        cancelled.run_with(budget, &mut NoEffects).unwrap_err(),
        RunError::Finished
    );

    let mut runs = 1;
    let result = loop {
        runs += 1;
        match other.run_with(budget, &mut NoEffects).unwrap() {
            RunOutcome::Yielded => assert!(runs < 1000, "the loop terminates"),
            RunOutcome::Complete(v) => break v.view(),
            outcome => panic!("{outcome:?}"),
        }
    };
    assert!(runs > 2, "the fixture spans several runs");
    assert_eq!(result, ValueView::Number(FIXTURE_RESULT));
    assert!(!other.cancel_requested());
}
