//! Parking and wakeups through the embedding API (pellet clj-quickjs-7):
//! the task state machine under real thread races. The individual handoff
//! windows are forced deterministically with test hooks in
//! `src/task/handoff_tests.rs`; here barriers release consumers, producers
//! and redundant wakers together, many times over, and the invariants are
//! checked on every iteration: each consumer resumes exactly once per
//! suspension, duplicate wakes change nothing, and no task ever has two
//! execution owners. No sleeps; every wait is a barrier or a bounded join.
//!
//! Needs are issued from Rust through the run guard, standing in for the
//! `__rt.need` VM operation of pellet clj-quickjs-23. Consumer tasks run a
//! trivial hand-assembled program that is never executed: an early VM test
//! fixture, not JavaScript source support.

use std::collections::HashMap;
use std::sync::Barrier;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use big_boy_pants_js::{
    Assembler, Budget, CellStatus, Completion, DependencyId, DependencyKey, Host, HostCall, Need,
    Opcode, Primitive, ResultKind, RunError, RunOutcome, Runtime, Task, TaskId, TaskStatus,
    ValueView, Wake, WakeHandle,
};

const DEF: DependencyId = DependencyId::new(11);
const KIND: ResultKind = ResultKind::new(2);
const ITERATIONS: usize = 200;

fn consumer(rt: &Runtime) -> Task {
    let mut asm = Assembler::new();
    asm.constant(Primitive::Undefined).op(Opcode::Return);
    rt.spawn(&asm.finish("consumer", 0, 0).unwrap(), &[])
        .unwrap()
}

fn key(rt: &Runtime) -> DependencyKey {
    DependencyKey::new(rt.world(), DEF, KIND)
}

/// How one consumer's suspension ended, from the consumer's side.
#[derive(Debug, PartialEq)]
enum Side {
    /// Its need read the outcome: it never subscribed.
    ReadDirectly,
    /// Its park found the wake already delivered and returned `Yielded`.
    YieldedFromPark,
    /// It parked; only a wake can resume it.
    Parked,
}

/// Consumers subscribe and park on their own threads while the producer
/// completes (success or failure, alternating) on another, all released by
/// one barrier. Every consumer then resumes exactly once: either it read the
/// outcome directly, or its park yielded (wake remembered), or it parked and
/// the completion returned it as runnable, never two of these and never
/// none. The resumed runs all see the same published outcome.
#[test]
fn many_waiters_each_resume_exactly_once_on_success_and_failure() {
    const CONSUMERS: usize = 6;
    for iteration in 0..ITERATIONS {
        let rt = Runtime::new();
        let k = key(&rt);
        let req = rt.claim(k).unwrap();
        let tasks: Vec<Task> = (0..CONSUMERS).map(|_| consumer(&rt)).collect();
        let outcome = rt.scope(|s| {
            let n = s.primitive(Primitive::Number(iteration as f64));
            let v = s.new_vector(&[n]).unwrap();
            s.persist(v)
        });
        let fail = iteration % 2 == 1;
        let barrier = Barrier::new(CONSUMERS + 1);
        let (sides, wakeups) = thread::scope(|s| {
            let consumers: Vec<_> = tasks
                .iter()
                .map(|task| {
                    let barrier = &barrier;
                    s.spawn(move || {
                        let mut guard = task.try_begin_run().unwrap();
                        barrier.wait();
                        match guard.need(DEF, KIND).unwrap() {
                            Need::Waiting { produce: None, .. } => match guard.park() {
                                RunOutcome::Yielded => Side::YieldedFromPark,
                                RunOutcome::Waiting(waited) => {
                                    assert_eq!(waited, k);
                                    Side::Parked
                                }
                                other => panic!("{other:?}"),
                            },
                            Need::Ready(_) | Need::Failed(_) => Side::ReadDirectly,
                            other => panic!("{other:?}"),
                        }
                    })
                })
                .collect();
            barrier.wait();
            let completion = if fail {
                Completion::Failed(&outcome)
            } else {
                Completion::Ready(&outcome)
            };
            let wakeups = rt.complete(k, req.producer(), completion).unwrap();
            let sides: Vec<Side> = consumers.into_iter().map(|h| h.join().unwrap()).collect();
            (sides, wakeups)
        });

        let delivered: HashMap<TaskId, &Wake> = wakeups
            .deliveries()
            .iter()
            .map(|(h, w)| (h.task(), w))
            .collect();
        let handles: Vec<WakeHandle> = wakeups.deliveries().iter().map(|(h, _)| *h).collect();
        let mut queue = Vec::new();
        for (task, side) in tasks.iter().zip(&sides) {
            let wake = delivered.get(&task.id()).copied();
            match (side, wake) {
                (Side::ReadDirectly, None) => {}
                (Side::YieldedFromPark, Some(Wake::Remembered)) => queue.push(task.clone()),
                (Side::Parked, Some(Wake::Runnable(t))) => {
                    assert_eq!(t.id(), task.id());
                }
                other => panic!("{}: {other:?}", task.id()),
            }
        }
        queue.extend(wakeups.into_runnable());
        let expected_resumes = sides.iter().filter(|s| **s != Side::ReadDirectly).count();
        assert_eq!(queue.len(), expected_resumes, "one resume per suspension");

        // Redundant deliveries change nothing.
        for h in &handles {
            let task = tasks.iter().find(|t| t.id() == h.task()).unwrap();
            assert!(matches!(task.wake(*h), Wake::Stale));
        }
        for task in &queue {
            assert_eq!(task.status(), TaskStatus::Ready);
            let mut guard = task.try_begin_run().unwrap();
            let got = match guard.need(DEF, KIND).unwrap() {
                Need::Ready(v) if !fail => v,
                Need::Failed(e) if fail => e,
                other => panic!("fail={fail}: {other:?}"),
            };
            assert!(got.same_object(&outcome));
        }
        let terminal = match rt.cell_status(k) {
            CellStatus::Ready(v) | CellStatus::Failed(v) => v,
            other => panic!("{other:?}"),
        };
        assert!(terminal.same_object(&outcome));
    }
}

/// The completion's own wakes race redundant deliveries of the same handles
/// from other threads (as a buggy or duplicated notification path would
/// send them), all released by one barrier. Per suspension exactly one
/// delivery takes effect: one `Runnable` in total across the completion
/// and every duplicate; all others are `Stale`.
#[test]
fn duplicate_wakeups_racing_the_completion_take_effect_once() {
    const DUPLICATORS: usize = 3;
    for _ in 0..ITERATIONS {
        let rt = Runtime::new();
        let k = key(&rt);
        let task = consumer(&rt);
        let mut guard = task.try_begin_run().unwrap();
        let Need::Waiting {
            produce: Some(req), ..
        } = guard.need(DEF, KIND).unwrap()
        else {
            panic!("the first need claims");
        };
        let handle = guard.wake_handle();
        assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
        let value = rt.root(Primitive::Number(1.0));
        let barrier = Barrier::new(DUPLICATORS + 1);
        let runnable = AtomicUsize::new(0);
        let wakeups = thread::scope(|s| {
            for _ in 0..DUPLICATORS {
                let (barrier, task, runnable) = (&barrier, &task, &runnable);
                s.spawn(move || {
                    barrier.wait();
                    // A duplicate may arrive before the completion: the
                    // handle names a parked suspension, so it can win.
                    if let Wake::Runnable(_) = task.wake(handle) {
                        runnable.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
            barrier.wait();
            rt.complete(k, req.producer(), Completion::Ready(&value))
                .unwrap()
        });
        let from_completion = wakeups.into_runnable().len();
        assert_eq!(
            from_completion + runnable.into_inner(),
            1,
            "exactly one delivery makes the parked task runnable"
        );
        assert_eq!(task.status(), TaskStatus::Ready);
        assert!(matches!(task.wake(handle), Wake::Stale));
    }
}

/// After a wake makes a task runnable, several workers race to claim it,
/// released by one barrier. Exactly one claim succeeds; the others fail with
/// `AlreadyRunning` while it runs, and the parked task could not be claimed
/// by anyone before the wake.
#[test]
fn a_woken_task_has_exactly_one_execution_owner() {
    const WORKERS: usize = 4;
    for _ in 0..ITERATIONS {
        let rt = Runtime::new();
        let k = key(&rt);
        let task = consumer(&rt);
        let mut guard = task.try_begin_run().unwrap();
        let Need::Waiting {
            produce: Some(req), ..
        } = guard.need(DEF, KIND).unwrap()
        else {
            panic!("the first need claims");
        };
        assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
        assert_eq!(task.try_begin_run().err(), Some(RunError::Waiting));
        let value = rt.root(Primitive::Number(1.0));
        let mut runnable = rt
            .complete(k, req.producer(), Completion::Ready(&value))
            .unwrap()
            .into_runnable();
        let woken = runnable.pop().expect("the parked task is returned");
        let barrier = Barrier::new(WORKERS);
        let owners = AtomicUsize::new(0);
        let results: Vec<Result<(), RunError>> = thread::scope(|s| {
            let handles: Vec<_> = (0..WORKERS)
                .map(|_| {
                    let (barrier, task, owners) = (&barrier, woken.clone(), &owners);
                    s.spawn(move || {
                        barrier.wait();
                        let claim = task.try_begin_run().inspect(|_| {
                            owners.fetch_add(1, Ordering::SeqCst);
                        });
                        // Hold the claim until every worker has tried.
                        barrier.wait();
                        claim.map(|mut guard| {
                            assert!(matches!(guard.need(DEF, KIND).unwrap(), Need::Ready(_)));
                        })
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(owners.into_inner(), 1, "{results:?}");
        assert_eq!(
            results
                .iter()
                .filter(|r| **r == Err(RunError::AlreadyRunning))
                .count(),
            WORKERS - 1
        );
        assert_eq!(task.status(), TaskStatus::Ready);
    }
}

/// Counts host effects and records the OS thread of each.
#[derive(Default)]
struct Effects(Vec<thread::ThreadId>);

impl Host for Effects {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        self.0.push(thread::current().id());
        Ok(Primitive::Null)
    }
}

/// A parked task's saved frame is a collector root and its continuation
/// survives the park: the task runs part of its program (an effect, then a
/// new object held only in a local), needs a pending key and parks; a
/// collection succeeds while it is parked (the park left no mutator
/// registered) and keeps the object; after the completion the task resumes
/// on another thread at the saved instruction, without replaying the
/// effect, and returns that same object.
///
/// Early VM test: hand-assembled bytecode, not JavaScript source support.
/// The need is issued from Rust between runs, standing in for the VM
/// operation of pellet clj-quickjs-23.
#[test]
fn parked_frames_survive_collection_and_resume_on_another_worker() {
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.constant(Primitive::Number(1.0))
        .host_effect(7)
        .op(Opcode::Pop); // 0, 1, 2
    asm.op(Opcode::NewObject).set_local(0); // 3, 4
    asm.get_local(0).op(Opcode::Return); // 5, 6
    let task = rt
        .spawn(&asm.finish("park-mid-program", 0, 1).unwrap(), &[])
        .unwrap();
    let mut effects = Effects::default();
    assert!(matches!(
        task.run_with(Budget::instructions(5), &mut effects)
            .unwrap(),
        RunOutcome::Yielded
    ));
    assert_eq!(effects.0.len(), 1);

    let mut guard = task.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(req), ..
    } = guard.need(DEF, KIND).unwrap()
    else {
        panic!("the first need claims");
    };
    assert!(matches!(guard.park(), RunOutcome::Waiting(_)));
    let garbage = rt.scope(|s| {
        let o = s.new_object();
        s.downgrade(o)
    });
    assert_eq!(rt.heap_stats().objects, 2);
    let stats = rt
        .collect()
        .expect("a parked task is not a mutator: collection runs");
    assert_eq!(stats.reclaimed, 1);
    assert!(!garbage.is_live());
    assert_eq!(
        rt.heap_stats().objects,
        1,
        "the object in the parked frame survives"
    );

    let value = rt.root(Primitive::Number(2.0));
    let mut runnable = rt
        .complete(req.key(), req.producer(), Completion::Ready(&value))
        .unwrap()
        .into_runnable();
    let woken = runnable.pop().expect("the parked task is returned");
    let parker = thread::current().id();
    let (outcome, worker) = thread::scope(|s| {
        s.spawn(|| {
            let mut guard = woken.try_begin_run().unwrap();
            assert!(matches!(guard.need(DEF, KIND).unwrap(), Need::Ready(_)));
            (
                guard.run_with(Budget::UNLIMITED, &mut effects),
                thread::current().id(),
            )
        })
        .join()
        .unwrap()
    });
    assert_ne!(worker, parker, "resumed on another worker");
    let RunOutcome::Complete(result) = outcome else {
        panic!("expected Complete, got {outcome:?}");
    };
    assert_eq!(result.view(), ValueView::Object);
    assert_eq!(effects.0, [parker], "the effect is not replayed");
    assert_eq!(
        rt.heap_stats().objects,
        1,
        "no new object: the result is the one from the saved frame"
    );
    assert_eq!(task.status(), TaskStatus::Finished);
}
