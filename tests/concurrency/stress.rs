//! Many waiters, competing producer requests, world/result-kind isolation,
//! terminal stability and seeded repeated schedules, all through the VM
//! `__rt.need`, the two-worker executor and the collector.
//!
//! Seeded schedules are supplemental: a seed fixes each scenario's shape
//! (consumer count and roles, who claims the producer, when the result is
//! published, placements), and the threads still race. Every assertion is
//! one that must hold for every interleaving; the scenario name carries the
//! seed, so a failure is rerun with the same shape.

use std::collections::HashSet;
use std::thread;

use clj_quickjs::exec::{OutcomeKind, Placement};
use clj_quickjs::{
    CellStatus, ClaimError, CompleteError, Completion, DependencyId, DependencyKey,
    ProducerRequest, ResultKind, Rooted, RunOutcome, Task, TaskId, TaskStatus, Wake, WorldId,
};

use crate::consumer_exe;
use crate::harness::{
    Config, Harness, Note, POST, PRE, Rng, Route, collections, dispatches, releases, scenario, w,
};

fn spawn_consumer(h: &Harness, world: WorldId, tag: f64, def: u64, kind: u32) -> Task {
    h.spawn(world, &consumer_exe(), &[tag, def as f64, f64::from(kind)])
}

/// Wait until every task in `ids` has parked (its latest release is
/// `Waiting`).
fn wait_all_parked(h: &Harness, ids: &[TaskId]) {
    h.wait_events(&format!("all of {ids:?} parked"), |events| {
        ids.iter()
            .all(|&id| matches!(releases(events, id).last(), Some(OutcomeKind::Waiting(_))))
    });
}

/// A completed cell rejects every later settlement and keeps its value.
fn assert_settled(h: &Harness, key: DependencyKey, request: &ProducerRequest, value: &Rooted) {
    let other = h.vector(&[0.0]);
    assert_eq!(
        h.rt()
            .complete(key, request.producer(), Completion::Ready(&other))
            .err(),
        Some(CompleteError::AlreadyComplete { key })
    );
    assert_eq!(
        h.rt().cancel_cell(key).err(),
        Some(CompleteError::AlreadyComplete { key })
    );
    assert_eq!(
        h.rt().claim(key).err(),
        Some(ClaimError::AlreadyComplete { key })
    );
    match h.rt().cell_status(key) {
        CellStatus::Ready(v) => assert!(v.same_object(value), "the outcome never changes"),
        other => h.fail(format!("{key} should stay ready, is {other:?}")),
    }
}

/// Sixteen consumers on two workers need one key while the host races
/// their first needs with its own claim: exactly one producer exists. A
/// collection runs while all are parked; one publication wakes each
/// exactly once (migrating), each resumes after its need with the same
/// shared object and no repeated effect; a late consumer reads it ready.
#[test]
fn many_waiters_share_one_producer_and_one_publication() {
    for round in 0..4 {
        scenario(
            &format!("many waiters, round {round}"),
            Config::default(),
            move |h| {
                let world = h.rt().world();
                let key = DependencyKey::new(world, DependencyId::new(5), ResultKind::new(2));
                let tasks: Vec<Task> = (0..16)
                    .map(|i| spawn_consumer(h, world, i as f64, 5, 2))
                    .collect();
                let ids: Vec<TaskId> = tasks.iter().map(Task::id).collect();
                // Submit on the test thread while another thread claims.
                let host_claim = thread::scope(|s| {
                    let claimer = s.spawn(|| h.rt().claim(key));
                    for (i, task) in tasks.iter().enumerate() {
                        h.submit(task, Placement::Worker(w(i % 2)));
                    }
                    claimer.join().unwrap()
                });
                wait_all_parked(h, &ids);
                let mut requests: Vec<ProducerRequest> =
                    h.take_requests().into_iter().map(|(_, r)| r).collect();
                let host_won = host_claim.is_ok();
                requests.extend(host_claim.ok());
                assert_eq!(
                    requests.len(),
                    1,
                    "one producer per key (host won: {host_won})"
                );
                let request = requests.pop().unwrap();
                match h.rt().cell_status(key) {
                    CellStatus::Pending { producer, waiters } => {
                        assert_eq!(producer, Some(request.producer()));
                        let waiting: HashSet<TaskId> = waiters.into_iter().collect();
                        assert_eq!(waiting, ids.iter().copied().collect());
                    }
                    other => h.fail(format!("pending expected, got {other:?}")),
                }
                h.collect();
                assert!(
                    ids.iter()
                        .all(|&id| h.task(id).status() == TaskStatus::Waiting)
                );

                let value = h.vector(&[42.0, 43.0]);
                let delivered = h.publish(&request, Completion::Ready(&value));
                let woken: HashSet<TaskId> = delivered.runnable.iter().copied().collect();
                assert_eq!(woken.len(), 16, "each waiter woken once: {delivered}");
                assert_eq!(woken, ids.iter().copied().collect());
                for &id in &ids {
                    assert!(h.complete_value(id).same_object(&value));
                }
                // A late consumer reads the result ready in one run and
                // records the edge.
                let late = spawn_consumer(h, world, 99.0, 5, 2);
                let late_id = h.submit(&late, Placement::Any);
                assert!(h.complete_value(late_id).same_object(&value));

                let events = h.events();
                for &id in &ids {
                    assert_eq!(
                        releases(&events, id),
                        [OutcomeKind::Waiting(key), OutcomeKind::Completed]
                    );
                    let runs = dispatches(&events, id);
                    assert_ne!(runs[0].0, runs[1].0, "the woken run migrates");
                    assert_eq!(h.effects(id, PRE).len(), 1);
                    assert_eq!(h.effects(id, POST).len(), 1);
                    for handle in &delivered.handles {
                        assert!(matches!(h.task(id).wake(*handle), Wake::Stale));
                    }
                }
                assert_eq!(releases(&events, late_id), [OutcomeKind::Completed]);
                assert!(late.dependency_log().depends_on(key));
                assert_eq!(late.dependency_log().waiting(), None);
                assert_settled(h, key, &request, &value);
            },
        );
    }
}

/// Consumers of the same definition in two worlds and two result kinds
/// park on three distinct cells. A producer of one cell cannot complete
/// another, and each publication wakes exactly its own cell's waiters
/// while the others stay parked.
#[test]
fn worlds_and_result_kinds_are_isolated_under_the_executor() {
    scenario("worlds and kinds", Config::default(), |h| {
        let (w1, w2) = (h.rt().world(), h.rt().new_world());
        let def = 9;
        let cells = [(w1, 1u32), (w1, 2), (w2, 1)];
        let keys: Vec<DependencyKey> = cells
            .iter()
            .map(|&(world, kind)| {
                DependencyKey::new(world, DependencyId::new(def), ResultKind::new(kind))
            })
            .collect();
        let mut by_cell: Vec<Vec<TaskId>> = vec![Vec::new(); 3];
        for i in 0..9 {
            let (world, kind) = cells[i % 3];
            let task = spawn_consumer(h, world, i as f64, def, kind);
            by_cell[i % 3].push(h.submit(&task, Placement::Worker(w(i % 2))));
        }
        let all: Vec<TaskId> = by_cell.concat();
        wait_all_parked(h, &all);
        let requests: Vec<ProducerRequest> = keys.iter().map(|&k| h.take_request(k)).collect();
        assert!(h.take_requests().is_empty(), "one request per cell");

        // World 2's producer cannot complete world 1's cell of the same
        // definition and kind; the cell keeps its waiters.
        let stray = h.vector(&[2.0]);
        let refused = h
            .rt()
            .complete(keys[0], requests[2].producer(), Completion::Ready(&stray));
        assert!(
            matches!(refused, Err(CompleteError::NotProducer { .. })),
            "{refused:?}"
        );

        let values: Vec<Rooted> = (0..3).map(|i| h.vector(&[i as f64])).collect();
        for cell in [2, 1, 0] {
            let d = h.publish(&requests[cell], Completion::Ready(&values[cell]));
            let woken: HashSet<TaskId> = d.runnable.iter().copied().collect();
            assert_eq!(woken, by_cell[cell].iter().copied().collect());
            for &id in &by_cell[cell] {
                assert!(h.complete_value(id).same_object(&values[cell]));
                assert_eq!(h.task(id).dependency_log().edges(), [keys[cell]]);
            }
            for other in (0..3).filter(|&o| o < cell) {
                for &id in &by_cell[other] {
                    assert_eq!(
                        h.task(id).status(),
                        TaskStatus::Waiting,
                        "{id} stays parked"
                    );
                }
                assert!(matches!(
                    h.rt().cell_status(keys[other]),
                    CellStatus::Pending { .. }
                ));
            }
        }
        for (cell, key) in keys.iter().enumerate() {
            assert_settled(h, *key, &requests[cell], &values[cell]);
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// Needs and either reads the result ready or waits for it.
    Plain,
    /// Cancelled while gated before its need.
    CancelBeforeNeed,
    /// Cancelled while parked, before the publication.
    CancelParked,
}

/// Seeded repeated schedules: a seed picks the consumer count and roles,
/// whether the host or the first need claims the producer, how many plain
/// consumers have parked before the publication (the rest race it), and
/// every placement. Whatever the interleaving: plain consumers complete with
/// the one published object after at most one suspension, cancelled ones
/// end cancelled without their post-wait effect and are never woken, no
/// pre-wait effect repeats, exactly one producer exists, and late readers
/// read the result ready.
#[test]
fn seeded_schedules_keep_every_invariant() {
    for seed in 0..24u64 {
        let config = Config {
            route: Route::Seeded(seed),
            ..Config::default()
        };
        scenario(&format!("seeded schedule {seed}"), config, move |h| {
            seeded(h, seed)
        });
    }
}

fn seeded(h: &Harness, seed: u64) {
    let mut rng = Rng::new(seed ^ 0x5eed);
    let world = h.rt().world();
    let key = DependencyKey::new(world, DependencyId::new(3), ResultKind::new(4));
    let n = 4 + rng.below(9) as usize;
    let roles: Vec<Role> = (0..n)
        .map(|_| match rng.below(4) {
            0 | 1 => Role::Plain,
            2 => Role::CancelBeforeNeed,
            _ => Role::CancelParked,
        })
        .collect();
    let plain_count = roles.iter().filter(|&&r| r == Role::Plain).count();
    let host_claim = (rng.below(2) == 0).then(|| h.rt().claim(key).unwrap());
    let publish_after = rng.below(plain_count as u64 + 1) as usize;
    h.note(format!(
        "seed {seed}: roles {roles:?}, host claims {}, publish after {publish_after} parked",
        host_claim.is_some()
    ));

    let mut ids = Vec::new();
    for (i, role) in roles.iter().enumerate() {
        let tag = i as f64 + 1.0;
        let task = spawn_consumer(h, world, tag, 3, 4);
        let id = task.id();
        match role {
            Role::Plain => {
                h.submit(&task, Placement::Any);
            }
            Role::CancelBeforeNeed => {
                h.arm(tag as u32);
                h.submit(&task, Placement::Any);
                h.wait_gated(id, tag as u32);
                assert!(!h.cancel(&task));
                h.open(tag as u32);
            }
            Role::CancelParked => {
                h.submit(&task, Placement::Any);
                h.wait_parked(id);
                assert!(
                    h.cancel(&task),
                    "a parked task is enqueued by its canceller"
                );
            }
        }
        ids.push(id);
    }
    let plain: Vec<TaskId> = ids
        .iter()
        .zip(&roles)
        .filter(|(_, r)| **r == Role::Plain)
        .map(|(id, _)| *id)
        .collect();
    h.wait_notes(
        &format!("{publish_after} plain consumers parked"),
        |notes| {
            notes
                .iter()
                .filter(|n| {
                    matches!(n, Note::RunEnd { task, outcome: OutcomeKind::Waiting(_), .. }
                    if plain.contains(task))
                })
                .count()
                >= publish_after
        },
    );
    let (request, host_claimed) = match host_claim {
        Some(r) => (r, true),
        None if plain.is_empty() && !roles.contains(&Role::CancelParked) => {
            // Nobody reached a need: the host claims now.
            (h.rt().claim(key).unwrap(), true)
        }
        None => {
            h.wait_notes("a claimed producer request", |notes| {
                notes.iter().any(|n| matches!(n, Note::Requested { .. }))
            });
            (h.take_request(key), false)
        }
    };
    let value = h.vector(&[seed as f64]);
    let delivered = h.publish(&request, Completion::Ready(&value));
    let late: Vec<TaskId> = (0..1 + rng.below(2))
        .map(|i| {
            let task = spawn_consumer(h, world, 100.0 + i as f64, 3, 4);
            h.submit(&task, Placement::Any)
        })
        .collect();

    for (&id, role) in ids.iter().zip(&roles) {
        // Each arm takes the terminal outcome before reading effects.
        match role {
            Role::Plain => {
                assert!(h.complete_value(id).same_object(&value));
                assert_eq!(h.effects(id, POST).len(), 1);
                let rel = releases(&h.events(), id);
                assert!(
                    matches!(
                        rel.as_slice(),
                        [OutcomeKind::Completed]
                            | [OutcomeKind::Waiting(_), OutcomeKind::Completed]
                            | [OutcomeKind::Yielded, OutcomeKind::Completed]
                    ),
                    "{id}: at most one suspension, got {rel:?}"
                );
            }
            Role::CancelBeforeNeed | Role::CancelParked => {
                assert!(matches!(h.outcome(id), RunOutcome::Cancelled));
                assert!(h.effects(id, POST).is_empty());
                assert!(
                    !delivered.runnable.contains(&id),
                    "a cancelled waiter is withdrawn"
                );
                assert!(!delivered.remembered.contains(&id));
            }
        }
        assert_eq!(h.effects(id, PRE).len(), 1, "{id} ({role:?}): PRE once");
    }
    for id in late {
        assert!(h.complete_value(id).same_object(&value));
        assert_eq!(releases(&h.events(), id), [OutcomeKind::Completed]);
    }
    assert_eq!(
        h.claimed(),
        u64::from(!host_claimed),
        "one producer per key"
    );
    assert_settled(h, key, &request, &value);
    assert!(collections(&h.events()).is_empty());
}
