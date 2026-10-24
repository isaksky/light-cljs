//! Cross-feature cases: features that are tested alone elsewhere, combined
//! under the executor with waits, migration and collection.
//!
//! * A resumable native fold whose JavaScript callback suspends while a
//!   `finally` block is pending (and, separately, a fold running inside a
//!   `finally` block with a pending `return`), collected and migrated at
//!   every suspension.
//! * An atom replaced (and collected) while a parked task holds an earlier
//!   read of it.
//! * Recursion (deep frames waiting at the bottom; mutually recursive
//!   published callables reaching each other through `__rt.need`) kept
//!   apart from a genuine active wait cycle, using the host-side
//!   `DependencyGraph` over the executor's real waits.
//! * A producer failure delivered to many waiters guarded by catch and
//!   finally while consumer cancellations race it (clj-quickjs-27's
//!   catchable dependency failures and cancellation cleanup, under the
//!   executor).

use std::collections::HashSet;
use std::sync::{Barrier, mpsc};
use std::thread;

use clj_quickjs::exec::{EventKind, OutcomeKind, Placement};
use clj_quickjs::{
    Assembler, CasOutcome, Completion, DemandSite, DependencyGraph, DependencyId, DependencyKey,
    Executable, Opcode, Primitive, ResultKind, Rooted, RunOutcome, Runtime, TaskId, ValueView,
    VmErrorKind, WeakHandle,
};

use crate::blocker_exe;
use crate::harness::{
    Config, Harness, NOTE, POST, PRE, compile, dispatches, releases, scenario, w,
};

const CALLABLE: u32 = 2;

fn key(h: &Harness, def: u64) -> DependencyKey {
    DependencyKey::new(
        h.rt().world(),
        DependencyId::new(def),
        ResultKind::new(CALLABLE),
    )
}

/// The items of persistent vector `v`, checked against `expected` by
/// identity.
fn assert_items_identical(h: &Harness, v: &Rooted, expected: &[Rooted]) {
    h.rt().scope(|s| {
        let items = s.items(s.local(v).unwrap()).unwrap();
        assert_eq!(items.len(), expected.len());
        for (item, want) in items.into_iter().zip(expected) {
            assert!(s.same_object(item, s.local(want).unwrap()), "identity kept");
        }
    });
}

fn worker_sequence(h: &Harness, task: TaskId) -> Vec<usize> {
    dispatches(&h.events(), task)
        .into_iter()
        .map(|(w, _)| w.index())
        .collect()
}

/// Drive a fold task that needs definitions `defs` one after another:
/// at each suspension, check that cleanup has not run, collect, check that
/// the input and every earlier result survived, then publish the next
/// result. Returns the published values.
fn feed_fold(h: &Harness, id: TaskId, defs: &[u64], input: &WeakHandle) -> Vec<Rooted> {
    let mut values: Vec<Rooted> = Vec::new();
    for (i, &def) in defs.iter().enumerate() {
        let key = key(h, def);
        assert_eq!(h.wait_release(id, i + 1), OutcomeKind::Waiting(key));
        assert!(
            h.effects(id, POST).is_empty(),
            "a wait is not an exception: no cleanup has run"
        );
        let c = h.collect();
        assert!(
            input.is_live(),
            "the suspended native frame holds its input"
        );
        h.note(format!("suspension {i}: collection #{} done", c.number));
        let request = h.take_request(key);
        // Only the cell holds this value once the host drops its handle;
        // the accumulator holds the earlier ones.
        let value = h.vector(&[def as f64, i as f64]);
        h.publish(&request, Completion::Ready(&value));
        values.push(value);
    }
    values
}

/// The task suspends inside the fold's callback, inside a `try` whose
/// `finally` is pending, three times; each time a worker collects and the
/// resumed run migrates. The fold never repeats an element, the cleanup
/// runs exactly once after the last result arrived, and the result holds
/// the published objects themselves.
#[test]
fn a_fold_callback_suspended_under_finally_survives_collection_and_migration() {
    scenario("fold under finally", Config::default(), |h| {
        let exe = compile(include_str!("../fixtures/js/concurrency_fold_finally.js"));
        let defs = [61, 62, 63];
        let input = h.vector(&defs.map(|d| d as f64));
        let input_weak = input.downgrade();
        let task = h.spawn_rooted(h.rt().world(), &exe, &[&input, &h.number(8.0)]);
        drop(input);
        let id = h.submit(&task, Placement::Worker(w(0)));
        let values = feed_fold(h, id, &defs, &input_weak);
        let result = h.complete_value(id);
        assert_items_identical(h, &result, &values);

        let cleanup = h.effects(id, POST);
        assert_eq!(cleanup.len(), 1, "finally ran once: {cleanup:?}");
        assert_eq!(cleanup[0].1, 3.0, "each element folded exactly once");
        assert_eq!(
            h.effects(id, PRE).len(),
            1,
            "nothing before the fold replays"
        );
        let notes = h.notes();
        let last_publish = notes
            .iter()
            .rposition(|n| matches!(n, crate::harness::Note::Test(s) if s.starts_with("publish")))
            .unwrap();
        let cleanup_at = notes
            .iter()
            .position(
                |n| matches!(n, crate::harness::Note::Effect { task, op: POST, .. } if *task == id),
            )
            .unwrap();
        assert!(
            cleanup_at > last_publish,
            "cleanup only after the last result"
        );
        assert_eq!(
            worker_sequence(h, id),
            [0, 1, 0, 1],
            "every resumption migrates"
        );
        drop((result, task, values));
        h.collect();
        assert!(!input_weak.is_live(), "reclaimable once the task finished");
    });
}

/// The same fold inside a `finally` block whose `try` returned: the
/// pending `return` completion waits on the handler stack through every
/// suspension, collection and migration, and is delivered after the fold.
#[test]
fn a_fold_inside_finally_keeps_the_pending_return_across_suspensions() {
    scenario("fold inside finally", Config::default(), |h| {
        let exe = compile(include_str!(
            "../fixtures/js/concurrency_fold_in_finally.js"
        ));
        let defs = [64, 65];
        let input = h.vector(&defs.map(|d| d as f64));
        let input_weak = input.downgrade();
        let task = h.spawn_rooted(h.rt().world(), &exe, &[&input, &h.number(17.0)]);
        drop(input);
        let id = h.submit(&task, Placement::Worker(w(1)));
        feed_fold(h, id, &defs, &input_weak);
        assert_eq!(h.complete_value(id).view(), ValueView::Number(17.0));
        assert_eq!(h.effects(id, PRE).len(), 1);
        assert_eq!(
            h.effects(id, POST).iter().map(|e| e.1).collect::<Vec<_>>(),
            [2.0]
        );
        assert_eq!(worker_sequence(h, id), [1, 0, 1]);
    });
}

/// Early VM test (hand-assembled bytecode: the source subset has no atom
/// intrinsics): `held = atom_load(a); dep = need(51, 2);
/// return [held, atom_load(a), dep]`.
fn atom_reader() -> Executable {
    let mut asm = Assembler::new();
    asm.get_local(0).op(Opcode::AtomLoad).set_local(1);
    asm.constant(Primitive::Number(51.0))
        .constant(Primitive::Number(f64::from(CALLABLE)))
        .op(Opcode::Need)
        .set_local(2);
    asm.get_local(1)
        .get_local(0)
        .op(Opcode::AtomLoad)
        .get_local(2)
        .new_vector(3)
        .op(Opcode::Return);
    asm.finish("atom_reader", 1, 3).unwrap()
}

/// A task reads an atom, then parks on a dependency. While it is parked a
/// native job on the other worker replaces the atom's value three times and
/// a collection runs: the value only the parked task's read holds survives,
/// the intermediate values nobody holds are reclaimed, and the resumed run
/// (migrated) sees its old read unchanged and the newest value on a fresh
/// load. Once the task is gone, the old value is reclaimed too.
#[test]
fn an_atom_replaced_while_a_parked_task_holds_an_earlier_read() {
    scenario(
        "atom replacement during a retained read",
        Config::default(),
        |h| {
            let initial = h.vector(&[1.0, 2.0, 3.0]);
            let initial_weak = initial.downgrade();
            let atom = h.rt().new_atom(&initial).unwrap();
            drop(initial);
            let task = h.spawn_rooted(h.rt().world(), &atom_reader(), &[&atom]);
            let id = h.submit(&task, Placement::Worker(w(0)));
            let key = key(h, 51);
            assert_eq!(h.wait_release(id, 1), OutcomeKind::Waiting(key));

            let (tx, rx) = mpsc::channel::<(Rooted, Vec<WeakHandle>)>();
            let rt: Runtime = h.rt().clone();
            let writer_atom = atom.clone_root(h);
            let job = h.exec().submit_native(Placement::Worker(w(1)), move |_| {
                let mut replaced = Vec::new();
                let mut last = None;
                for i in 0..3 {
                    let current = rt.atom_load(&writer_atom).unwrap();
                    let next = rt.scope(|s| {
                        let n = s.primitive(Primitive::Number(10.0 + f64::from(i)));
                        let v = s.new_vector(&[n]).unwrap();
                        s.persist(v)
                    });
                    match rt
                        .atom_compare_and_set(&writer_atom, &current, &next)
                        .unwrap()
                    {
                        CasOutcome::Swapped => {}
                        CasOutcome::Conflict { .. } => panic!("the only writer conflicted"),
                    }
                    if i > 0 {
                        replaced.push(current.downgrade());
                    }
                    last = Some(next);
                }
                let _ = tx.send((last.unwrap(), replaced));
            });
            h.wait_events("the writer job finished", |events| {
                events.iter().any(|e| {
                    e.kind
                        == EventKind::NativeFinished {
                            job,
                            panicked: false,
                        }
                })
            });
            let (newest, replaced) = rx.recv().unwrap();
            h.collect();
            assert!(initial_weak.is_live(), "the parked task's read keeps it");
            assert!(
                replaced.iter().all(|r| !r.is_live()),
                "replaced values nobody holds are reclaimed"
            );
            assert!(h.rt().atom_load(&atom).unwrap().same_object(&newest));

            let dep = h.vector(&[51.0]);
            let request = h.take_request(key);
            h.publish(&request, Completion::Ready(&dep));
            let result = h.complete_value(id);
            let expected = [initial_weak.upgrade().unwrap(), newest, dep];
            assert_items_identical(h, &result, &expected);
            assert_eq!(worker_sequence(h, id), [0, 1]);
            drop((result, expected, task));
            h.collect();
            assert!(
                !initial_weak.is_live(),
                "the old read is released with the task"
            );
        },
    );
}

/// Helper so a `Rooted` can be moved into a native job while the test
/// keeps its own handle.
trait CloneRoot {
    fn clone_root(&self, h: &Harness) -> Rooted;
}

impl CloneRoot for Rooted {
    fn clone_root(&self, h: &Harness) -> Rooted {
        h.rt().scope(|s| s.persist(s.local(self).unwrap()))
    }
}

/// Deep recursion and mutually recursive callables are not a wait cycle.
/// A consumer recurses 24 frames deep and needs `is_even` there; it parks,
/// a collection runs, the producer task publishes `is_even`, and the
/// consumer resumes on the other worker, calls it, and parks again deeper
/// on `is_odd`, which `is_even` needs when called. After `is_odd` is
/// published the two callables recurse through ready reads to the answer.
/// The host's graph sees two plain waits and no cycle; the consumer's log
/// records both keys as uses.
#[test]
fn recursion_through_waits_is_not_a_wait_cycle() {
    scenario("recursion is not a wait cycle", Config::default(), |h| {
        let world = h.rt().world();
        let (even_key, odd_key) = (key(h, 71), key(h, 72));
        let descend = compile(include_str!("../fixtures/js/concurrency_descend.js"));
        let parity = compile(include_str!("../fixtures/js/concurrency_parity.js"));
        let mut graph = DependencyGraph::new();

        let consumer = h.spawn(world, &descend, &[24.0, 10.0]);
        let c = h.submit(&consumer, Placement::Worker(w(0)));
        for (n, (key, me)) in [(even_key, 0.0), (odd_key, 1.0)].into_iter().enumerate() {
            assert_eq!(h.wait_release(c, n + 1), OutcomeKind::Waiting(key));
            let site = DemandSite::new(format!("descend: need {key} (suspension {n})"));
            graph
                .wait(c, key, site)
                .unwrap_or_else(|cycle| h.fail(format!("recursion reported as {cycle}")));
            let request = h.take_request(key);
            let producer = h.spawn(world, &parity, &[me]);
            graph.bind_producer(key, request.producer(), producer.id());
            h.collect();
            let p = h.submit(&producer, Placement::Any);
            let callable = h.complete_value(p);
            graph.resolve(key);
            h.publish(&request, Completion::Ready(&callable));
        }
        assert_eq!(h.complete_value(c).view(), ValueView::Number(25.0));
        assert_eq!(graph.active_waits(), 0);
        let log = consumer.dependency_log();
        assert_eq!(
            log.edges(),
            [even_key, odd_key],
            "recursive uses, recorded once"
        );
        assert_eq!(log.waiting(), None);
        assert_eq!(h.effects(c, PRE).len(), 1);
        assert_eq!(worker_sequence(h, c), [0, 1, 0]);
    });
}

/// Two producers that each need the other's result form a genuine active
/// wait cycle on two workers. The graph reports it at the closing wait,
/// naming both tasks and keys; the host fails the newest demand's own
/// result, which fails the other producer, whose failure settles the first:
/// every task and cell on the cycle settles and nothing stays parked.
#[test]
fn a_genuine_wait_cycle_is_reported_and_settled() {
    scenario("genuine wait cycle", Config::default(), |h| {
        let world = h.rt().world();
        let (a, b) = (key(h, 81), key(h, 82));
        let exe = compile(include_str!("../fixtures/js/concurrency_cycle.js"));
        let (ra, rb) = (h.rt().claim(a).unwrap(), h.rt().claim(b).unwrap());
        let pa = h.spawn(world, &exe, &[82.0]);
        let pb = h.spawn(world, &exe, &[81.0]);
        let mut graph = DependencyGraph::new();
        graph.bind_producer(a, ra.producer(), pa.id());
        graph.bind_producer(b, rb.producer(), pb.id());

        let pa_id = h.submit(&pa, Placement::Worker(w(0)));
        assert_eq!(h.wait_release(pa_id, 1), OutcomeKind::Waiting(b));
        graph
            .wait(pa_id, b, DemandSite::new("produce_after(82)"))
            .expect("one wait is not a cycle");
        let pb_id = h.submit(&pb, Placement::Worker(w(1)));
        assert_eq!(h.wait_release(pb_id, 1), OutcomeKind::Waiting(a));
        let cycle = graph
            .wait(pb_id, a, DemandSite::new("produce_after(81)"))
            .expect_err("the closing wait is a cycle");
        h.note(format!("{cycle}"));
        let tasks: HashSet<TaskId> = cycle.tasks().into_iter().collect();
        assert_eq!(tasks, HashSet::from([pa_id, pb_id]));
        let keys: HashSet<DependencyKey> = cycle.keys().into_iter().collect();
        assert_eq!(keys, HashSet::from([a, b]));

        let error = h.number(-26.0);
        graph.resolve(b);
        assert_eq!(h.publish(&rb, Completion::Failed(&error)).runnable, [pa_id]);
        assert_eq!(h.failure(pa_id), VmErrorKind::DependencyFailed(b));
        graph.resolve(a);
        assert_eq!(h.publish(&ra, Completion::Failed(&error)).runnable, [pb_id]);
        assert_eq!(h.failure(pb_id), VmErrorKind::DependencyFailed(a));
        assert_eq!(graph.active_waits(), 0);
        assert_eq!(h.claimed(), 0, "the host's claims were the only producers");
        for id in [pa_id, pb_id] {
            assert_eq!(h.effects(id, PRE).len(), 1);
            assert_eq!(
                releases(&h.events(), id),
                [
                    OutcomeKind::Waiting(if id == pa_id { b } else { a }),
                    OutcomeKind::Failed
                ]
            );
        }
    });
}

/// Eight consumers park on one key inside `try`/`catch`/`finally` on two
/// workers; a collection runs. With both workers held, the producer
/// publishes a failure while, on other threads released by one barrier,
/// half the consumers are cancelled. Whichever side wins each race, a
/// cancelled consumer runs its finally block once, never its catch block
/// or the code after the try, and ends `Cancelled`; every other consumer is
/// woken once, migrates, catches the failure at its need, runs finally
/// once and completes. Nothing before the need repeats.
#[test]
fn a_producer_failure_races_cancellation_of_guarded_waiters() {
    scenario("failure vs cancellation, guarded", Config::default(), |h| {
        let exe = compile(include_str!("../fixtures/js/concurrency_guarded.js"));
        let key = key(h, 33);
        let tasks: Vec<_> = (0..8)
            .map(|i| h.spawn(h.rt().world(), &exe, &[i as f64, 33.0, f64::from(CALLABLE)]))
            .collect();
        let ids: Vec<TaskId> = tasks
            .iter()
            .enumerate()
            .map(|(i, t)| h.submit(t, Placement::Worker(w(i % 2))))
            .collect();
        for &id in &ids {
            assert_eq!(h.wait_release(id, 1), OutcomeKind::Waiting(key));
        }
        let request = h.take_request(key);
        h.collect();
        let busy = h.occupy(&blocker_exe(), 100);
        let error = h.number(-33.0);
        let barrier = Barrier::new(5);
        let woken = thread::scope(|s| {
            for task in &tasks[..4] {
                let barrier = &barrier;
                s.spawn(move || {
                    barrier.wait();
                    h.cancel(task);
                });
            }
            barrier.wait();
            h.publish(&request, Completion::Failed(&error))
        });
        assert!(
            ids[4..].iter().all(|id| woken.runnable.contains(id)),
            "every uncancelled waiter is woken: {woken}"
        );
        busy.release(h);
        for (i, &id) in ids.iter().enumerate() {
            // The terminal outcome first: effects are only final after it.
            if i < 4 {
                assert!(matches!(h.outcome(id), RunOutcome::Cancelled));
                assert!(
                    h.effects(id, NOTE).is_empty(),
                    "{id}: no catch, nothing after"
                );
            } else {
                assert_eq!(h.complete_value(id).view(), ValueView::Number(2.0));
                let notes: Vec<f64> = h.effects(id, NOTE).into_iter().map(|e| e.1).collect();
                assert_eq!(notes, [20.0, 2.0], "{id}: caught at the need");
                let runs = dispatches(&h.events(), id);
                assert_eq!(runs.len(), 2);
                assert_ne!(runs[0].0, runs[1].0, "the woken run migrates");
            }
            assert_eq!(
                h.effects(id, PRE).len(),
                1,
                "{id}: nothing before the need repeats"
            );
            assert_eq!(
                h.effects(id, POST).len(),
                1,
                "{id}: finally runs exactly once"
            );
        }
    });
}
