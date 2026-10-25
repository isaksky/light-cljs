//! Dependency cells through the embedding API (pellet clj-quickjs-6):
//! single-assignment outcomes, read-or-subscribe, one producer per key,
//! world/version identity, the per-task dependency log, publication and
//! collection of results.
//!
//! The needs here are issued from Rust through a task's run guard
//! (`RunGuard::need`), standing in for the `__rt.need` VM operation of
//! pellet clj-quickjs-23. Consumer tasks run a trivial hand-assembled program
//! that is never executed: an early VM test fixture, not JavaScript source
//! support.

use std::sync::{Barrier, Mutex};
use std::thread;

use big_boy_pants_js::{
    Assembler, CellStatus, ClaimError, CompleteError, Completion, DependencyId, DependencyKey,
    HandleError, Need, NeedError, Opcode, PathStep, Primitive, ProducerId, PublishError,
    ResultKind, Rooted, RunOutcome, Runtime, Task, TaskId, ValueView, WorldId,
};

const DEF_HELPER: DependencyId = DependencyId::new(7);
const RESULT_CALLABLE: ResultKind = ResultKind::new(1);
const RESULT_INITIALIZED: ResultKind = ResultKind::new(2);

fn consumer(rt: &Runtime, world: WorldId) -> Task {
    let mut asm = Assembler::new();
    asm.constant(Primitive::Undefined).op(Opcode::Return);
    let exe = asm.finish("consumer", 0, 0).unwrap();
    rt.spawn_in(world, &exe, &[]).unwrap()
}

fn key(world: WorldId, kind: ResultKind) -> DependencyKey {
    DependencyKey::new(world, DEF_HELPER, kind)
}

/// Complete `key` and return the drained wake handles' tasks, in
/// subscription order (what the wakes did is covered in `tests/wakeup.rs`).
fn complete(
    rt: &Runtime,
    key: DependencyKey,
    producer: ProducerId,
    completion: Completion<'_>,
) -> Result<Vec<TaskId>, CompleteError> {
    rt.complete(key, producer, completion).map(|w| w.task_ids())
}

/// One need by `task` for (`DEF_HELPER`, `kind`), through a fresh run guard.
fn need(task: &Task, kind: ResultKind) -> Need {
    task.try_begin_run()
        .unwrap()
        .need(DEF_HELPER, kind)
        .unwrap()
}

fn ready(n: Need) -> Rooted {
    match n {
        Need::Ready(v) => v,
        other => panic!("expected Ready, got {other:?}"),
    }
}

/// A shareable vector `[1, [2]]`.
fn nested_vector(rt: &Runtime) -> Rooted {
    rt.scope(|s| {
        let one = s.primitive(Primitive::Number(1.0));
        let two = s.primitive(Primitive::Number(2.0));
        let inner = s.new_vector(&[two]).unwrap();
        let outer = s.new_vector(&[one, inner]).unwrap();
        s.persist(outer)
    })
}

#[test]
fn an_already_ready_read_returns_the_shared_value_and_records_the_edge() {
    let rt = Runtime::new();
    let k = key(rt.world(), RESULT_CALLABLE);
    let req = rt.claim(k).unwrap();
    assert_eq!(req.key(), k);
    let value = nested_vector(&rt);
    assert_eq!(
        complete(&rt, k, req.producer(), Completion::Ready(&value)),
        Ok(vec![])
    );

    let task = consumer(&rt, rt.world());
    let got = ready(need(&task, RESULT_CALLABLE));
    assert!(
        got.same_object(&value),
        "readers get the published identity"
    );
    let log = task.dependency_log();
    assert_eq!(log.edges(), [k], "a ready read is a recorded dependency");
    assert!(log.depends_on(k));
    assert_eq!(log.waiting(), None, "a ready read is not a wait");
    assert!(matches!(rt.cell_status(k), CellStatus::Ready(v) if v.same_object(&value)));
}

#[test]
fn a_pending_subscription_is_woken_by_completion_and_then_reads_the_value() {
    let rt = Runtime::new();
    let k = key(rt.world(), RESULT_CALLABLE);
    let task = consumer(&rt, rt.world());
    let Need::Waiting {
        key: waited,
        produce: Some(req),
    } = need(&task, RESULT_CALLABLE)
    else {
        panic!("the first need of a pending cell claims its producer");
    };
    assert_eq!((waited, req.key()), (k, k));
    let log = task.dependency_log();
    assert_eq!((log.edges(), log.waiting()), (&[k][..], Some(k)));
    assert!(matches!(
        rt.cell_status(k),
        CellStatus::Pending { producer: Some(p), waiters } if p == req.producer() && waiters == [task.id()]
    ));

    let value = rt.root(Primitive::Number(42.0));
    let woken = complete(&rt, k, req.producer(), Completion::Ready(&value)).unwrap();
    assert_eq!(woken, [task.id()], "completion returns the wake handle");

    assert_eq!(
        ready(need(&task, RESULT_CALLABLE)).view(),
        ValueView::Number(42.0)
    );
    let log = task.dependency_log();
    assert_eq!(log.edges(), [k], "the resumed read is the same edge");
    assert_eq!(log.waiting(), None, "the wait is resolved");
}

#[test]
fn producer_failure_wakes_every_waiter_and_stays_failed() {
    let rt = Runtime::new();
    let k = key(rt.world(), RESULT_INITIALIZED);
    let (a, b) = (consumer(&rt, rt.world()), consumer(&rt, rt.world()));
    let Need::Waiting {
        produce: Some(req), ..
    } = need(&a, RESULT_INITIALIZED)
    else {
        panic!("a claims the producer");
    };
    assert!(matches!(
        need(&b, RESULT_INITIALIZED),
        Need::Waiting { produce: None, .. }
    ));
    let error = nested_vector(&rt);
    let woken = complete(&rt, k, req.producer(), Completion::Failed(&error)).unwrap();
    assert_eq!(
        woken,
        [a.id(), b.id()],
        "failure wakes waiters like success"
    );
    for task in [&a, &b] {
        match need(task, RESULT_INITIALIZED) {
            Need::Failed(e) => assert!(e.same_object(&error)),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(task.dependency_log().waiting(), None);
    }
    let late = consumer(&rt, rt.world());
    assert!(matches!(need(&late, RESULT_INITIALIZED), Need::Failed(_)));
    assert_eq!(late.dependency_log().edges(), [k]);
}

#[test]
fn cancellation_is_terminal_and_cannot_be_reset_or_replaced() {
    let rt = Runtime::new();
    let k = key(rt.world(), RESULT_CALLABLE);
    let task = consumer(&rt, rt.world());
    let Need::Waiting {
        produce: Some(req), ..
    } = need(&task, RESULT_CALLABLE)
    else {
        panic!("first need claims");
    };
    assert_eq!(
        complete(&rt, k, req.producer(), Completion::Cancelled),
        Ok(vec![task.id()])
    );
    assert!(matches!(need(&task, RESULT_CALLABLE), Need::Cancelled));
    let value = rt.root(Primitive::Number(1.0));
    assert_eq!(
        complete(&rt, k, req.producer(), Completion::Ready(&value)),
        Err(CompleteError::AlreadyComplete { key: k })
    );
    assert_eq!(rt.claim(k), Err(ClaimError::AlreadyComplete { key: k }));
    assert!(matches!(rt.cell_status(k), CellStatus::Cancelled));
}

#[test]
fn a_second_completion_is_rejected_and_the_first_outcome_stays() {
    let rt = Runtime::new();
    let k = key(rt.world(), RESULT_CALLABLE);
    let req = rt.claim(k).unwrap();
    let first = nested_vector(&rt);
    let second = nested_vector(&rt);
    complete(&rt, k, req.producer(), Completion::Ready(&first)).unwrap();
    for completion in [
        Completion::Ready(&second),
        Completion::Ready(&first),
        Completion::Failed(&second),
        Completion::Cancelled,
    ] {
        assert_eq!(
            complete(&rt, k, req.producer(), completion),
            Err(CompleteError::AlreadyComplete { key: k })
        );
    }
    let task = consumer(&rt, rt.world());
    assert!(ready(need(&task, RESULT_CALLABLE)).same_object(&first));
}

#[test]
fn many_needs_of_one_key_emit_exactly_one_producer_request() {
    let rt = Runtime::new();
    let k = key(rt.world(), RESULT_CALLABLE);
    let tasks: Vec<Task> = (0..4).map(|_| consumer(&rt, rt.world())).collect();
    let mut requests = Vec::new();
    for task in &tasks {
        let Need::Waiting { produce, .. } = need(task, RESULT_CALLABLE) else {
            panic!("pending");
        };
        requests.extend(produce);
    }
    // A repeated need (e.g. a spurious resume) neither re-claims nor
    // registers the task twice.
    assert!(matches!(
        need(&tasks[0], RESULT_CALLABLE),
        Need::Waiting { produce: None, .. }
    ));
    assert_eq!(requests.len(), 1, "one producer request per key");
    let claimed = requests[0].producer();
    assert_eq!(
        rt.claim(k),
        Err(ClaimError::AlreadyClaimed { key: k, claimed })
    );
    let ids: Vec<TaskId> = tasks.iter().map(Task::id).collect();
    assert_eq!(complete(&rt, k, claimed, Completion::Cancelled), Ok(ids));
}

#[test]
fn worlds_and_result_kinds_name_distinct_cells() {
    let rt = Runtime::new();
    let (w1, w2) = (rt.world(), rt.new_world());
    assert_ne!(w1, w2);
    let t1 = consumer(&rt, w1);
    let t2 = consumer(&rt, w2);
    let t1_init = consumer(&rt, w1);
    let req = |task: &Task, kind| match need(task, kind) {
        Need::Waiting {
            produce: Some(req), ..
        } => req,
        other => panic!("each distinct cell gets its own producer: {other:?}"),
    };
    let r1 = req(&t1, RESULT_CALLABLE);
    let r2 = req(&t2, RESULT_CALLABLE);
    let r1_init = req(&t1_init, RESULT_INITIALIZED);
    assert_eq!(r1.key(), key(w1, RESULT_CALLABLE));
    assert_eq!(r2.key(), key(w2, RESULT_CALLABLE));
    assert_eq!(r1_init.key(), key(w1, RESULT_INITIALIZED));

    // World 2's producer cannot substitute its result into world 1's cell.
    let v2 = rt.root(Primitive::Number(2.0));
    assert_eq!(
        complete(&rt, r1.key(), r2.producer(), Completion::Ready(&v2)),
        Err(CompleteError::NotProducer {
            key: r1.key(),
            producer: r2.producer(),
            claimed: Some(r1.producer()),
        })
    );
    assert_eq!(
        complete(&rt, r2.key(), r2.producer(), Completion::Ready(&v2)),
        Ok(vec![t2.id()]),
        "only world 2's waiter wakes"
    );
    assert_eq!(
        ready(need(&t2, RESULT_CALLABLE)).view(),
        ValueView::Number(2.0)
    );
    assert!(
        matches!(
            need(&t1, RESULT_CALLABLE),
            Need::Waiting { produce: None, .. }
        ),
        "world 1 is still pending"
    );
    assert!(matches!(
        rt.cell_status(r1_init.key()),
        CellStatus::Pending { waiters, .. } if waiters == [t1_init.id()]
    ));
    // A producer cannot complete a cell that does not exist either.
    let w3 = key(rt.new_world(), RESULT_CALLABLE);
    assert!(matches!(
        complete(&rt, w3, r1.producer(), Completion::Cancelled),
        Err(CompleteError::NotProducer { claimed: None, .. })
    ));
    assert!(matches!(rt.cell_status(w3), CellStatus::Absent));
}

#[test]
fn a_task_suspended_on_one_key_cannot_demand_another() {
    let rt = Runtime::new();
    let task = consumer(&rt, rt.world());
    let _ = need(&task, RESULT_CALLABLE);
    let mut guard = task.try_begin_run().unwrap();
    assert_eq!(
        guard.need(DEF_HELPER, RESULT_INITIALIZED).unwrap_err(),
        NeedError::AlreadyWaiting {
            waiting: key(rt.world(), RESULT_CALLABLE),
            requested: key(rt.world(), RESULT_INITIALIZED),
        }
    );
    drop(guard);
    assert!(matches!(
        rt.cell_status(key(rt.world(), RESULT_INITIALIZED)),
        CellStatus::Absent
    ));
    assert_eq!(task.dependency_log().edges().len(), 1);
}

/// A scheduler refusing a wait (pellet clj-quickjs-26: a wait cycle) abandons
/// it: the task leaves the cell's waiters and has no unresolved wait, the
/// recorded edge stays, the producer claim stays, and the task may demand
/// another key.
#[test]
fn an_abandoned_wait_leaves_the_cell_but_keeps_the_recorded_edge() {
    let rt = Runtime::new();
    let task = consumer(&rt, rt.world());
    let other = consumer(&rt, rt.world());
    let callable = key(rt.world(), RESULT_CALLABLE);
    let mut guard = task.try_begin_run().unwrap();
    let Need::Waiting {
        produce: Some(request),
        ..
    } = guard.need(DEF_HELPER, RESULT_CALLABLE).unwrap()
    else {
        panic!("the first need claims the producer");
    };
    assert!(matches!(
        need(&other, RESULT_CALLABLE),
        Need::Waiting { produce: None, .. }
    ));
    guard.abandon_wait();
    assert!(matches!(
        guard.need(DEF_HELPER, RESULT_INITIALIZED).unwrap(),
        Need::Waiting { .. }
    ));
    guard.abandon_wait();
    assert!(
        matches!(guard.park(), RunOutcome::Yielded),
        "nothing to park on"
    );
    let log = task.dependency_log();
    assert_eq!(
        (log.edges(), log.waiting()),
        (&[callable, key(rt.world(), RESULT_INITIALIZED)][..], None)
    );
    assert!(matches!(
        rt.cell_status(callable),
        CellStatus::Pending { producer: Some(p), waiters } if p == request.producer() && waiters == [other.id()]
    ));
    let value = rt.root(Primitive::Number(1.0));
    assert_eq!(
        complete(&rt, callable, request.producer(), Completion::Ready(&value)),
        Ok(vec![other.id()])
    );
}

#[test]
fn published_results_and_errors_survive_collection_while_the_cell_holds_them() {
    let rt = Runtime::new();
    let (k_ok, k_err) = (
        key(rt.world(), RESULT_CALLABLE),
        key(rt.world(), RESULT_INITIALIZED),
    );
    let (value, error) = (nested_vector(&rt), nested_vector(&rt));
    let (weak_value, weak_error) = (value.downgrade(), error.downgrade());
    let req_ok = rt.claim(k_ok).unwrap();
    let req_err = rt.claim(k_err).unwrap();
    complete(&rt, k_ok, req_ok.producer(), Completion::Ready(&value)).unwrap();
    complete(&rt, k_err, req_err.producer(), Completion::Failed(&error)).unwrap();
    drop((value, error));
    // An unrelated unrooted object is reclaimed, so the collection is real.
    let weak_garbage = rt.scope(|s| {
        let o = s.new_object();
        s.downgrade(o)
    });
    let stats = rt.collect().unwrap();
    assert!(stats.reclaimed >= 1);
    assert!(!weak_garbage.is_live());
    assert!(weak_value.is_live() && weak_error.is_live());

    let task = consumer(&rt, rt.world());
    let got = ready(need(&task, RESULT_CALLABLE));
    assert!(got.same_object(&weak_value.upgrade().unwrap()));
    rt.scope(|s| {
        let v = s.local(&got).unwrap();
        let inner = s.nth(v, 1).unwrap();
        assert_eq!(s.view(s.nth(inner, 0).unwrap()), ValueView::Number(2.0));
    });
    drop(got);
    rt.collect().unwrap();
    assert!(
        weak_value.is_live(),
        "the cell, not the reader, keeps it alive"
    );
    let mut guard = task.try_begin_run().unwrap();
    let Need::Failed(e) = guard.need(DEF_HELPER, RESULT_INITIALIZED).unwrap() else {
        panic!("expected Failed");
    };
    assert!(e.same_object(&weak_error.upgrade().unwrap()));
}

#[test]
fn non_shareable_values_are_rejected_at_completion_and_the_cell_stays_pending() {
    let rt = Runtime::new();
    let k = key(rt.world(), RESULT_CALLABLE);
    let task = consumer(&rt, rt.world());
    let Need::Waiting {
        produce: Some(req), ..
    } = need(&task, RESULT_CALLABLE)
    else {
        panic!("first need claims");
    };
    let (plain, holder) = rt.scope(|s| {
        let o = s.new_object();
        let one = s.primitive(Primitive::Number(1.0));
        let v = s.new_vector(&[one, o]).unwrap();
        (s.persist(o), s.persist(v))
    });
    for completion in [Completion::Ready(&holder), Completion::Failed(&plain)] {
        match complete(&rt, k, req.producer(), completion) {
            Err(CompleteError::NotShareable(PublishError::NotShareable { found, .. })) => {
                assert_eq!(found, "object")
            }
            other => panic!("expected NotShareable, got {other:?}"),
        }
    }
    let Err(CompleteError::NotShareable(PublishError::NotShareable { path, .. })) =
        complete(&rt, k, req.producer(), Completion::Ready(&holder))
    else {
        panic!("rejected again");
    };
    assert_eq!(path.steps(), [PathStep::Index(1)]);
    assert!(matches!(
        rt.cell_status(k),
        CellStatus::Pending { waiters, .. } if waiters == [task.id()]
    ));
    let ok = rt.root(Primitive::Bool(true));
    assert_eq!(
        complete(&rt, k, req.producer(), Completion::Ready(&ok)),
        Ok(vec![task.id()]),
        "a rejected completion keeps the waiters for the real one"
    );
}

#[test]
fn handles_of_another_runtime_are_rejected_at_completion() {
    let (rt, other) = (Runtime::new(), Runtime::new());
    let k = key(rt.world(), RESULT_CALLABLE);
    let req = rt.claim(k).unwrap();
    let foreign = other.root(Primitive::Number(1.0));
    assert_eq!(
        complete(&rt, k, req.producer(), Completion::Ready(&foreign)),
        Err(CompleteError::Handle(HandleError::ForeignRuntime))
    );
    assert!(matches!(rt.cell_status(k), CellStatus::Pending { .. }));
}

/// Needs racing one completion from other threads, released together by a
/// barrier, many times over. Every consumer observes the outcome exactly
/// once: either its need read the terminal value, or it registered and is in
/// the completion's wake list, never both and never neither. A check-then-
/// register window would show up as a consumer that saw `Pending` but is
/// missing from the wake list. (Deterministic forcing of each window is
/// pellet clj-quickjs-7; this test only needs no blocking beyond the barrier.)
#[test]
fn needs_racing_completion_are_never_lost_or_woken_twice() {
    const CONSUMERS: usize = 4;
    for _ in 0..200 {
        let rt = Runtime::new();
        let k = key(rt.world(), RESULT_CALLABLE);
        let req = rt.claim(k).unwrap();
        let tasks: Vec<Task> = (0..CONSUMERS).map(|_| consumer(&rt, rt.world())).collect();
        let value = rt.root(Primitive::Number(9.0));
        let barrier = Barrier::new(CONSUMERS + 1);
        let saw_ready = Mutex::new(Vec::new());
        let woken = thread::scope(|s| {
            for task in &tasks {
                let (barrier, saw_ready) = (&barrier, &saw_ready);
                s.spawn(move || {
                    let mut guard = task.try_begin_run().unwrap();
                    barrier.wait();
                    match guard.need(DEF_HELPER, RESULT_CALLABLE).unwrap() {
                        Need::Ready(v) => {
                            assert_eq!(v.view(), ValueView::Number(9.0));
                            saw_ready.lock().unwrap().push(task.id());
                        }
                        Need::Waiting { produce: None, .. } => {}
                        other => panic!("unexpected {other:?}"),
                    }
                });
            }
            barrier.wait();
            complete(&rt, k, req.producer(), Completion::Ready(&value)).unwrap()
        });
        let saw_ready = saw_ready.into_inner().unwrap();
        for task in &tasks {
            let ready = saw_ready.contains(&task.id());
            let wakes = woken.iter().filter(|&&id| id == task.id()).count();
            assert_eq!(
                (ready as usize) + wakes,
                1,
                "{} ready={ready} wakes={wakes}",
                task.id()
            );
            assert_eq!(task.dependency_log().edges(), [k]);
            assert_eq!(task.dependency_log().waiting().is_some(), !ready);
        }
    }
}

#[test]
fn concurrent_first_needs_claim_exactly_one_producer() {
    const CONSUMERS: usize = 4;
    for _ in 0..200 {
        let rt = Runtime::new();
        let tasks: Vec<Task> = (0..CONSUMERS).map(|_| consumer(&rt, rt.world())).collect();
        let barrier = Barrier::new(CONSUMERS);
        let claims = thread::scope(|s| {
            let handles: Vec<_> = tasks
                .iter()
                .map(|task| {
                    let barrier = &barrier;
                    s.spawn(move || {
                        let mut guard = task.try_begin_run().unwrap();
                        barrier.wait();
                        match guard.need(DEF_HELPER, RESULT_CALLABLE).unwrap() {
                            Need::Waiting { produce, .. } => produce,
                            other => panic!("unexpected {other:?}"),
                        }
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(claims.len(), 1);
        let CellStatus::Pending { producer, waiters } = rt.cell_status(claims[0].key()) else {
            panic!("pending");
        };
        assert_eq!(producer, Some(claims[0].producer()));
        assert_eq!(waiters.len(), CONSUMERS);
    }
}
