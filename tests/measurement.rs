//! The measurement harness's workloads and read-only stats (pellet
//! clj-quickjs-15, `docs/design.md` §15), asserted at small sizes.
//!
//! These tests never assert times or allocator byte counts (benchmarks are
//! not pass/fail thresholds): they check that every workload of
//! `examples/bench/workloads.rs` runs and produces its Rust-computed
//! results, and the deterministic structural facts the memory rows report:
//! consumers share one input graph and one executable (each parked consumer
//! adds exactly its own task-owned heap objects, never a copy of the
//! input), and [`Task::stats`] / [`Runtime::heap_usage`] describe what they
//! claim. The JavaScript is synthetic (not light-cljs output).

#[path = "../examples/bench/workloads.rs"]
mod workloads;

use clj_quickjs::frontend::compile_function;
use clj_quickjs::{Completion, ObjectCounts, RunOutcome, Runtime, TaskStatus};

use workloads::{ConsumerKind, Fixtures, Probe};

fn no_probe() -> Probe {
    Probe::default()
}

/// The facts both memory tables rest on, for one consumer fixture: each
/// parked consumer adds exactly `own` (its own task-owned heap objects),
/// the input is never copied, every consumer returns the input's entry
/// itself, and dropping the tasks leaves only the rooted input.
fn assert_shared_input(kind: ConsumerKind, own: impl Fn(usize) -> ObjectCounts) {
    let width = 50;
    let mut parked_objects = Vec::new();
    for consumers in [1, 8, 64] {
        let m = workloads::memory(kind, consumers, width, &no_probe);
        assert_eq!(m.kind, kind);
        assert_eq!(m.input_objects, workloads::input_objects(width));
        assert_eq!(m.waiting, consumers, "every consumer parked");
        assert_eq!(m.objects_parked, m.input_objects + consumers);
        let input = ObjectCounts {
            vector: 1 + 2 * width,
            map: width,
            ..ObjectCounts::default()
        };
        let own = own(consumers);
        assert_eq!(own.total(), consumers);
        assert_eq!(
            m.usage_parked.objects,
            ObjectCounts {
                builder: own.builder,
                function: own.function,
                ..input
            }
        );
        assert_eq!(m.identical, consumers);
        assert_eq!(m.objects_after_drop, m.input_objects);
        parked_objects.push(m.objects_parked - m.input_objects);
        let t = m.task_parked;
        assert!(t.pending_need);
        match kind {
            ConsumerKind::Frame => {
                assert_eq!((t.frames, t.native_frames), (1, 0));
                assert_eq!(m.usage_parked.known_shareable, m.input_objects);
            }
            ConsumerKind::NativeFold => {
                // Suspended inside the reduce's callback.
                assert_eq!((t.frames, t.native_frames), (2, 1));
            }
        }
    }
    assert_eq!(parked_objects, [1, 8, 64]);
}

#[test]
fn consumers_share_the_input_graph_and_code() {
    assert_shared_input(ConsumerKind::Frame, |n| ObjectCounts {
        builder: n,
        ..ObjectCounts::default()
    });
}

#[test]
fn consumers_suspended_in_a_native_fold_share_the_input_and_resume() {
    // Each keeps one function object (its fold callback) besides its native
    // continuation, which lives in its task-owned VM state.
    assert_shared_input(ConsumerKind::NativeFold, |n| ObjectCounts {
        function: n,
        ..ObjectCounts::default()
    });
}

#[test]
fn task_stats_are_unavailable_while_a_run_holds_the_task() {
    let fx = Fixtures::compile();
    let rt = Runtime::new();
    let task = rt
        .spawn(&fx.identity, &[&workloads::num(&rt, 5.0)])
        .unwrap();
    let before = task.stats().expect("unstarted task has VM state");
    let guard = task.try_begin_run().unwrap();
    assert_eq!(task.stats(), None, "the run holds the task body");
    drop(guard);
    assert_eq!(
        task.stats(),
        Some(before),
        "an unused guard changes nothing"
    );
    assert_eq!(workloads::number(&workloads::finish(&task)), 5.0);
    assert_eq!(task.stats(), None);
}

#[test]
fn every_timing_workload_runs_and_checks_its_results() {
    let fx = Fixtures::compile();
    let rt = Runtime::new();
    let s = workloads::startup(&rt, &fx, 10);
    assert_eq!(s.tasks, 10);
    let r = workloads::ready_reads(&rt, &fx, 1, 100);
    assert_eq!(r.reads, 100);
    workloads::rust_ready_reads(&rt, &fx, 2, 100);
    let p = workloads::park_cycles(&rt, &fx, 10, 20);
    assert_eq!(p.cycles, 20);
    let p = workloads::persistent(&rt, &fx, 8, 50, 20);
    assert_eq!((p.size, p.ops, p.updates), (8, 50, 20));
    assert!(p.update_reclaimed >= p.update_objects);
    let [conj, assoc, _] = workloads::update_bytes(&fx, 50, &no_probe);
    assert_eq!((conj.bytes, assoc.bytes), (0, 0), "no probe, no counts");
}

#[test]
fn work_runs_while_consumers_wait_and_they_resume_after_publication() {
    let w = workloads::work_while_waiting(2, 8, 4, 50).expect("bounded workload");
    assert_eq!(w.still_waiting, 8, "no parked consumer ran during the work");
}

#[test]
fn task_stats_describe_saved_frames_natives_and_pending_needs() {
    let rt = Runtime::new();
    let nested = compile_function(
        "// Synthetic fixture, not light-cljs output.\n\
         function outer(d) {\n\
           function inner(x) { return __rt.need(x, 1) + 1; }\n\
           return inner(d);\n\
         }",
    )
    .unwrap();
    let task = rt.spawn(&nested, &[&workloads::num(&rt, 3.0)]).unwrap();
    let fresh = task.stats().expect("unstarted task has VM state");
    assert_eq!((fresh.frames, fresh.native_frames), (1, 0));
    assert!(!fresh.pending_need);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let parked = task.stats().expect("parked task has VM state");
    assert_eq!(parked.frames, 2, "suspended inside inner");
    assert!(parked.pending_need);
    assert!(parked.shallow_bytes > fresh.shallow_bytes);
    let request = task
        .take_producer_requests()
        .pop()
        .expect("the need claimed");
    let one = workloads::num(&rt, 1.0);
    let woke = rt
        .complete(request.key(), request.producer(), Completion::Ready(&one))
        .unwrap();
    assert_eq!(woke.task_ids(), [task.id()]);
    match task.run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(workloads::number(&v), 2.0),
        other => panic!("{other:?}"),
    }
    assert_eq!(task.status(), TaskStatus::Finished);
    assert_eq!(task.stats(), None, "a finished task keeps no VM state");

    let fold = compile_function(
        "// Synthetic fixture, not light-cljs output.\n\
         function fold(v) {\n\
           return __rt.reduce(function (acc, x) { return acc + __rt.need(x, 1); }, 0, v);\n\
         }",
    )
    .unwrap();
    let v = rt.scope(|s| {
        let n = |x: f64| s.primitive(clj_quickjs::Primitive::Number(x));
        s.persist(s.new_vector(&[n(20.0), n(21.0)]).unwrap())
    });
    let task = rt.spawn(&fold, &[&v]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let parked = task.stats().unwrap();
    assert_eq!(parked.native_frames, 1, "the reduce is in progress");
    assert_eq!(parked.frames, 2, "suspended in the callback");
    assert!(parked.pending_need);
}

#[test]
fn heap_usage_matches_heap_stats_and_counts_kinds() {
    let rt = Runtime::new();
    let form = workloads::build_input(&rt, 10);
    let usage = rt.heap_usage();
    assert_eq!(usage.objects.total(), rt.heap_stats().objects);
    assert_eq!(usage.objects.vector, 21);
    assert_eq!(usage.objects.map, 10);
    assert_eq!(usage.slots, 31);
    assert_eq!(usage.free_slots, 0);
    assert!(usage.shallow_bytes() >= usage.object_bytes);
    drop(form);
    rt.collect().unwrap();
    let swept = rt.heap_usage();
    assert_eq!(swept.objects.total(), 0);
    assert_eq!((swept.slots, swept.free_slots), (31, 31));
}
