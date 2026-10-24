//! The end-to-end synthetic compiler-host success scenario (pellet
//! clj-quickjs-13, `docs/design.md` §13), asserted.
//!
//! Each test runs the scenario of `examples/e2e/scenario.rs` (the same code
//! `cargo run --example e2e` runs; its module docs list the steps) and
//! checks one group of claims against the executor's event record, the
//! host's trace and the returned handles. The JavaScript is the synthetic
//! fixtures `examples/e2e/*.js` (not light-cljs output), compiled and run by
//! this crate. The scenario synchronizes on events, terminal outcomes and a
//! bounded rendezvous inside a host effect, never on sleeps; any step that
//! does not happen in time fails the test.

#[path = "../examples/e2e/scenario.rs"]
mod scenario;

use clj_quickjs::exec::{EnqueueReason, Event, EventKind, OutcomeKind, Placement, WorkerId};
use clj_quickjs::{CellStatus, MutatorState, TaskId, TaskStatus, ValueView};

use scenario::{
    CONSUMERS, ConsumerReport, EXPECTED_REPORT, MEET, POST, PRE, REPORT, Report, Trace, WORK,
};

fn run() -> Report {
    scenario::run().unwrap_or_else(|e| panic!("scenario: {e}"))
}

/// Line and column of `__rt.need(7, 2)` in `examples/e2e/consumer.js`.
const NEED_AT: (u32, u32) = (23, 18);

fn other(worker: WorkerId) -> WorkerId {
    WorkerId::new(1 - worker.index())
}

/// Sequence numbers of the events matching `pred`.
fn seqs(events: &[Event], pred: impl Fn(&Event) -> bool) -> Vec<usize> {
    events.iter().filter(|e| pred(e)).map(|e| e.seq).collect()
}

/// The one event matching `pred`.
fn one(events: &[Event], pred: impl Fn(&Event) -> bool) -> &Event {
    let found: Vec<&Event> = events.iter().filter(|e| pred(e)).collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found[0]
}

fn is(task: TaskId, kind: EventKind) -> impl Fn(&Event) -> bool {
    move |e| e.task == Some(task) && e.kind == kind
}

fn dispatched(task: TaskId) -> impl Fn(&Event) -> bool {
    move |e| e.task == Some(task) && matches!(e.kind, EventKind::Dispatched { .. })
}

/// Position of the first trace entry matching `pred`.
fn position(trace: &[Trace], pred: impl Fn(&Trace) -> bool) -> usize {
    trace
        .iter()
        .position(pred)
        .unwrap_or_else(|| panic!("not in trace: {trace:#?}"))
}

fn effect(task: TaskId, op: u16) -> impl Fn(&Trace) -> bool {
    move |t| matches!(*t, Trace::Effect { task: t, op: o, .. } if t == task && o == op)
}

fn ids(report: &Report) -> Vec<TaskId> {
    report.consumers.iter().map(|c| c.task).collect()
}

/// Claims 1, 2 and 4 of §13: the two consumers execute at the same time on
/// two OS workers, both read the one Rust-built input (no per-worker
/// import), and one producer request serves both.
#[test]
fn consumers_overlap_on_distinct_workers_and_share_one_producer() {
    let report = run();
    let ev = &report.events;

    // Two workers on two OS threads.
    assert_eq!(report.worker_threads.len(), 2);
    assert_ne!(report.worker_threads[0], report.worker_threads[1]);

    // Each consumer's first run is on its own worker, and the two runs
    // overlap in the record: each was dispatched before the other released.
    assert_eq!(report.consumers.len(), 2);
    let (a, b) = (&report.consumers[0], &report.consumers[1]);
    for c in [a, b] {
        let first = one(ev, is(c.task, EventKind::Dispatched { execution: 1 }));
        assert_eq!(first.worker, Some(c.spec.start));
    }
    let wait = |c: &ConsumerReport| {
        one(
            ev,
            is(
                c.task,
                EventKind::Released(OutcomeKind::Waiting(report.key)),
            ),
        )
        .seq
    };
    let start =
        |c: &ConsumerReport| one(ev, is(c.task, EventKind::Dispatched { execution: 1 })).seq;
    assert!(start(a) < wait(b) && start(b) < wait(a), "{ev:#?}");

    // Stronger than the record: MEET returns only once both consumers are
    // inside their MEET effects, on their two worker threads at once, and
    // both did return (neither task failed), before either suspended.
    let mut meet_threads = Vec::new();
    for c in [a, b] {
        let meet: Vec<_> = report
            .effects_of(c.task)
            .into_iter()
            .filter(|e| e.2 == MEET)
            .collect();
        assert_eq!(meet.len(), 1);
        assert_eq!(meet[0].0, c.spec.start);
        assert_eq!(meet[0].1, report.worker_threads[c.spec.start.index()]);
        meet_threads.push(meet[0].1);
    }
    assert_ne!(meet_threads[0], meet_threads[1]);
    let last_meet = position(&report.trace, effect(a.task, MEET))
        .max(position(&report.trace, effect(b.task, MEET)));
    let first_suspend = position(&report.trace, |t| matches!(t, Trace::Suspended { .. }));
    assert!(last_meet < first_suspend);

    // One producer request for both consumers: the first need claimed it,
    // the other subscribed to the same cell.
    assert_eq!(report.requests.len(), 1, "{:?}", report.requests);
    let request = report.requests[0];
    assert_eq!(request.key, report.key);
    assert!(ids(&report).contains(&request.task));
    let CellStatus::Pending {
        producer: Some(producer),
        waiters,
    } = &report.cell_while_waiting
    else {
        panic!("cell while waiting: {:?}", report.cell_while_waiting);
    };
    assert_eq!(*producer, request.producer);
    let mut waiters = waiters.clone();
    waiters.sort();
    assert_eq!(waiters, ids(&report));

    // The key carries the scenario's world, definition 7 and kind 2.
    assert_eq!(report.key.world(), report.world);
    assert_eq!(report.key.dependency(), scenario::HELPER);
    assert_eq!(report.key.kind(), scenario::CALLABLE);
}

/// Claim 2 and the final result: each consumer returns a subtree of the one
/// input with its original identity, which a structurally equal copy does
/// not have (identity, not equality).
#[test]
fn returned_subtrees_keep_the_input_identity() {
    let report = run();
    let (copy, copy_body, copy_inner) = scenario::build_form(&report.rt);
    for (c, copied) in report.consumers.iter().zip([&copy_body, &copy_inner]) {
        assert_eq!(c.result.view(), ValueView::Object);
        assert!(
            c.result.same_object(&c.expected),
            "{} returned another object",
            c.task
        );
        assert!(!c.result.same_object(&report.form));
        assert!(!c.result.same_object(copied));
        report.rt.scope(|s| {
            let (r, k) = (s.local(&c.result).unwrap(), s.local(copied).unwrap());
            assert!(s.value_equals(r, k), "the copy is equal, not identical");
        });
    }
    // The expected subtrees were navigated from the one input by Rust.
    report.rt.scope(|s| {
        let form = s.local(&report.form).unwrap();
        let body = s.nth(form, 2).unwrap();
        let inner = s.nth(body, 2).unwrap();
        let expected: Vec<_> = report
            .consumers
            .iter()
            .map(|c| s.local(&c.expected).unwrap())
            .collect();
        assert!(s.same_object(expected[0], body));
        assert!(s.same_object(expected[1], inner));
        let copy = s.local(&copy).unwrap();
        assert!(s.value_equals(form, copy) && !s.same_object(form, copy));
    });
    // Results stay valid roots after the executor is gone.
    report.rt.collect().unwrap();
    for c in &report.consumers {
        assert!(c.result.same_object(&c.expected));
    }
}

/// Claim 3: a suspended consumer occupies no worker. While both wait (the
/// helper held pending), each worker runs an unrelated task to completion.
#[test]
fn waiting_consumers_leave_their_workers_free_for_useful_work() {
    let report = run();
    let ev = &report.events;
    let waits: Vec<usize> = ids(&report)
        .into_iter()
        .map(|t| {
            one(
                ev,
                is(t, EventKind::Released(OutcomeKind::Waiting(report.key))),
            )
            .seq
        })
        .collect();
    let woken: Vec<usize> = ids(&report)
        .into_iter()
        .map(|t| {
            one(ev, |e| {
                e.task == Some(t)
                    && matches!(
                        e.kind,
                        EventKind::Enqueued {
                            reason: EnqueueReason::Woken,
                            ..
                        }
                    )
            })
            .seq
        })
        .collect();
    let last_wait = *waits.iter().max().unwrap();
    let first_wake = *woken.iter().min().unwrap();

    for (i, &w) in report.work.iter().enumerate() {
        let start = one(ev, dispatched(w));
        let done = one(ev, is(w, EventKind::Released(OutcomeKind::Completed)));
        assert_eq!(start.worker, Some(WorkerId::new(i)));
        assert_eq!(done.worker, Some(WorkerId::new(i)));
        assert!(last_wait < start.seq && done.seq < first_wake, "{ev:#?}");
        assert!(report.trace.iter().any(|t| matches!(
            *t,
            Trace::Finished { task, value } if task == w && value == ValueView::Number(WORK[i].1)
        )));
    }
    // Both workers had run a consumer before doing that work.
    let starts: Vec<_> = CONSUMERS.iter().map(|c| c.start).collect();
    assert_eq!(starts, [WorkerId::new(0), WorkerId::new(1)]);

    // Nothing of a consumer was dispatched between its wait and its wake.
    for (c, (wait, wake)) in report.consumers.iter().zip(waits.iter().zip(&woken)) {
        assert!(
            ev.iter()
                .filter(|e| e.task == Some(c.task))
                .all(|e| e.seq <= *wait || e.seq >= *wake)
        );
    }
}

/// Claim 7: a collection requested by a worker while both consumers are
/// parked reclaims garbage, and the values only their suspended frames
/// hold (the task-owned builder `seen`, the vector `scratch`, the live
/// locals) are intact when they resume.
#[test]
fn collection_while_suspended_keeps_task_values_live() {
    let report = run();
    let ev = &report.events;
    let c = &report.collection;
    assert!(c.stats.reclaimed > 0, "{c:?}");
    let requester = c.requester.expect("a worker requested it");
    // The other worker was idle (registered, no heap access): no worker is
    // held by a waiting consumer.
    assert_eq!(c.state_of(other(requester)), Some(MutatorState::Idle));

    let collected = one(
        ev,
        |e| matches!(&e.kind, EventKind::Collected(x) if x.number == c.number),
    );
    for t in ids(&report) {
        let wait = one(
            ev,
            is(t, EventKind::Released(OutcomeKind::Waiting(report.key))),
        );
        let resumed = one(ev, is(t, EventKind::Dispatched { execution: 2 }));
        assert!(wait.seq < collected.seq && collected.seq < resumed.seq);
    }
    let producer = one(ev, |e| {
        e.task == Some(report.producer) && matches!(e.kind, EventKind::Enqueued { .. })
    });
    assert!(collected.seq < producer.seq, "the helper did not exist yet");

    for c in &report.consumers {
        assert_eq!(c.status_after_collection, TaskStatus::Waiting);
        // Source context of the suspension: the `__rt.need` of consumer.js.
        let at = c.suspended_at.expect("suspended at the need");
        assert_eq!((at.line, at.column), NEED_AT);
        // After resuming: `scratch[2]` is the tag, and REPORT combines the
        // builder count with `scratch[0]` and `scratch[1]`.
        let effects = report.effects_of(c.task);
        let post = effects.iter().find(|e| e.2 == POST).unwrap();
        assert_eq!(post.3, c.spec.tag);
        let rep = effects.iter().find(|e| e.2 == REPORT).unwrap();
        assert_eq!(rep.3, EXPECTED_REPORT);
    }
}

/// Claims 5 and 6: each woken consumer is placed on, and resumes on, the
/// other worker; it continues after the need, so the effects before it ran
/// exactly once, and the task was dispatched exactly twice.
#[test]
fn resumed_consumers_migrate_and_replay_nothing() {
    let report = run();
    for c in &report.consumers {
        let (first, second) = (c.spec.start, other(c.spec.start));
        let kinds: Vec<(Option<WorkerId>, EventKind)> = report
            .events_of(c.task)
            .into_iter()
            .map(|e| (e.worker, e.kind.clone()))
            .collect();
        assert_eq!(
            kinds,
            [
                (
                    None,
                    EventKind::Enqueued {
                        reason: EnqueueReason::Submitted,
                        placement: Placement::Worker(first),
                    }
                ),
                (Some(first), EventKind::Dispatched { execution: 1 }),
                (
                    Some(first),
                    EventKind::Released(OutcomeKind::Waiting(report.key))
                ),
                (
                    None,
                    EventKind::Enqueued {
                        reason: EnqueueReason::Woken,
                        placement: Placement::Worker(second),
                    }
                ),
                (Some(second), EventKind::Dispatched { execution: 2 }),
                (Some(second), EventKind::Released(OutcomeKind::Completed)),
            ]
        );

        let effects = report.effects_of(c.task);
        let ops: Vec<(u16, WorkerId)> = effects.iter().map(|e| (e.2, e.0)).collect();
        assert_eq!(
            ops,
            [
                (PRE, first),
                (MEET, first),
                (POST, second),
                (REPORT, second)
            ],
            "pre-wait effects ran once, on the first worker"
        );
        assert_ne!(effects[0].1, effects[2].1, "resumed on another OS thread");
    }
}

/// The scenario's order, in the host trace (one lock, a true order across
/// threads) and in the executor record: pre-wait effects, suspension and
/// the one held request, useful work, collection, production, one
/// publication waking both, resumed effects, results.
#[test]
fn events_follow_the_scenario_order() {
    let report = run();
    let t = &report.trace;
    let consumers = ids(&report);

    let pre_wait: Vec<usize> = consumers
        .iter()
        .flat_map(|&c| [position(t, effect(c, PRE)), position(t, effect(c, MEET))])
        .collect();
    let suspended: Vec<usize> = consumers
        .iter()
        .map(|&c| position(t, |x| matches!(*x, Trace::Suspended { task, key, .. } if task == c && key == report.key)))
        .collect();
    let requested = position(t, |x| matches!(x, Trace::Requested { .. }));
    let work: Vec<usize> = report
        .work
        .iter()
        .map(|&w| {
            position(
                t,
                |x| matches!(*x, Trace::Finished { task, .. } if task == w),
            )
        })
        .collect();
    let collected = position(t, |x| matches!(x, Trace::Collected { .. }));
    let produced = position(
        t,
        |x| matches!(*x, Trace::Finished { task, value: ValueView::Object } if task == report.producer),
    );
    let published = position(t, |x| matches!(x, Trace::Published { .. }));
    let resumed: Vec<[usize; 2]> = consumers
        .iter()
        .map(|&c| [position(t, effect(c, POST)), position(t, effect(c, REPORT))])
        .collect();
    let finished: Vec<usize> = consumers
        .iter()
        .map(|&c| {
            position(
                t,
                |x| matches!(*x, Trace::Finished { task, .. } if task == c),
            )
        })
        .collect();

    let max = |v: &[usize]| *v.iter().max().unwrap();
    let min = |v: &[usize]| *v.iter().min().unwrap();
    assert!(max(&pre_wait) < min(&suspended), "{t:#?}");
    // The claimant's driver takes the request after its run suspended.
    let claimant = consumers
        .iter()
        .position(|&c| c == report.requests[0].task)
        .unwrap();
    assert!(
        suspended[claimant] < requested && requested < min(&work),
        "{t:#?}"
    );
    assert!(max(&work) < collected, "{t:#?}");
    assert!(collected < produced && produced < published, "{t:#?}");
    // Each resumed consumer runs POST, then REPORT, then is finished; the
    // two resumed runs are independent of each other.
    for ([post, report_at], done) in resumed.iter().zip(&finished) {
        assert!(
            published < *post && post < report_at && report_at < done,
            "{t:#?}"
        );
    }
    let Trace::Published { key, woke, .. } = &t[published] else {
        unreachable!()
    };
    assert_eq!(*key, report.key);
    let mut woke = woke.clone();
    woke.sort();
    assert_eq!(woke, consumers, "one publication woke both consumers");

    // The same milestones in the executor record.
    let ev = &report.events;
    let waits = seqs(
        ev,
        |e| matches!(e.kind, EventKind::Released(OutcomeKind::Waiting(k)) if k == report.key),
    );
    let work_starts: Vec<usize> = report
        .work
        .iter()
        .map(|&w| one(ev, dispatched(w)).seq)
        .collect();
    let work_done: Vec<usize> = report
        .work
        .iter()
        .map(|&w| one(ev, is(w, EventKind::Released(OutcomeKind::Completed))).seq)
        .collect();
    let collected = one(ev, |e| matches!(e.kind, EventKind::Collected(_))).seq;
    let producer_done = one(
        ev,
        is(report.producer, EventKind::Released(OutcomeKind::Completed)),
    )
    .seq;
    let wakes = seqs(ev, |e| {
        matches!(
            e.kind,
            EventKind::Enqueued {
                reason: EnqueueReason::Woken,
                ..
            }
        )
    });
    assert_eq!(waits.len(), 2);
    assert_eq!(wakes.len(), 2);
    assert!(max(&waits) < min(&work_starts), "{ev:#?}");
    assert!(max(&work_done) < collected, "{ev:#?}");
    assert!(
        collected < producer_done && producer_done < min(&wakes),
        "{ev:#?}"
    );
    // Per consumer: woken, resumed, completed. (One consumer may complete
    // before the other is even enqueued.)
    for &c in &consumers {
        let woken = one(ev, |e| {
            e.task == Some(c)
                && matches!(
                    e.kind,
                    EventKind::Enqueued {
                        reason: EnqueueReason::Woken,
                        ..
                    }
                )
        })
        .seq;
        let resumed = one(ev, is(c, EventKind::Dispatched { execution: 2 })).seq;
        let done = one(ev, is(c, EventKind::Released(OutcomeKind::Completed))).seq;
        assert!(woken < resumed && resumed < done, "{ev:#?}");
    }
}
