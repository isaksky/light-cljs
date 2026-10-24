//! Adapted QuickJS cases; synthetic fixtures, not light-cljs output.
//! Each fixture carries its source ranges, adaptations and license notice.
use clj_quickjs::frontend::compile_script;
use clj_quickjs::host::sim::SimHost;
use clj_quickjs::{Budget, RunOutcome, Runtime, Task, ValueView};

const GENERATOR: &str = include_str!("fixtures/js/quickjs_test_generator.js");
const ASYNC_GC: &str = include_str!("fixtures/js/quickjs_test_async_gc.js");
const ASYNC_REJECTION: &str = include_str!("fixtures/js/quickjs_test_async_promise_rejection.js");

fn fixture(rt: &Runtime, source: &str) -> Task {
    let code = compile_script("quickjs-fixture.js", source).expect("fixture compiles");
    let task = rt.spawn(&code, &[]).unwrap();
    task.install_sim_host_bindings().unwrap();
    task
}

fn finish(rt: &Runtime, task: &Task) -> RunOutcome {
    let mut host = SimHost::new(rt);
    // Completion includes draining Promise jobs and virtual timers. Small
    // budgets exercise resumable callbacks without an unbounded test loop.
    for _ in 0..10_000 {
        let outcome = task.run_with(Budget::instructions(7), &mut host).unwrap();
        match outcome {
            RunOutcome::Yielded => {}
            other => return other,
        }
    }
    panic!("fixture did not finish within its instruction bound");
}

fn assert_complete(rt: &Runtime, task: &Task) {
    let outcome = finish(rt, task);
    let RunOutcome::Complete(value) = outcome else {
        panic!("fixture failed: {outcome:?}");
    };
    assert_eq!(value.view(), ValueView::Undefined);
    let report = task.unhandled_rejections();
    assert!(report.entries.is_empty(), "{report:?}");
    assert_eq!(report.dropped, 0);
}

#[test]
fn quickjs_test_generator() {
    let rt = Runtime::new();
    assert_complete(&rt, &fixture(&rt, GENERATOR));
}

#[test]
fn quickjs_test_async_gc() {
    let rt = Runtime::new();
    // Initialize the realm and opt-in bindings before measuring live objects.
    let warm = fixture(&rt, "Promise.resolve();");
    assert!(matches!(finish(&rt, &warm), RunOutcome::Complete(_)));
    drop(warm);
    rt.collect().unwrap();
    let baseline = rt.heap_stats().objects;
    let collections = rt.heap_stats().collections;

    let task = fixture(&rt, ASYNC_GC);
    assert_complete(&rt, &task);
    assert!(rt.heap_stats().collections > collections, "std.gc job ran");
    // Keep the completed task alive: its diagnostic activation index must
    // not retain the unreachable suspended activation or its captures.
    rt.collect().unwrap();
    assert_eq!(rt.heap_usage().objects.async_state, 0);
    assert_eq!(rt.heap_stats().objects, baseline);
}

#[test]
fn quickjs_test_async_promise_rejection() {
    let rt = Runtime::new();
    assert_complete(&rt, &fixture(&rt, ASYNC_REJECTION));
}

#[test]
fn quickjs_async_assert_failure_fails_run() {
    let rt = Runtime::new();
    assert_eq!(ASYNC_REJECTION.matches("assert(counter, 3)").count(), 1);
    let broken = ASYNC_REJECTION.replace("assert(counter, 3)", "assert(counter, 4)");
    let task = fixture(&rt, &broken);
    let outcome = finish(&rt, &task);
    assert!(matches!(outcome, RunOutcome::Failed(_)), "{outcome:?}");

    // A Promise job throws into its derived Promise rather than failing the
    // task directly. The runner's no-unhandled-rejection check must catch it.
    let broken_job = broken.replace(
        "os.setTimeout(() => { assert(counter, 4) }, 10)",
        "Promise.resolve().then(() => { assert(counter, 4) })",
    );
    assert_ne!(broken_job, broken);
    let task = fixture(&rt, &broken_job);
    assert!(matches!(finish(&rt, &task), RunOutcome::Complete(_)));
    let report = task.unhandled_rejections();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.dropped, 0);
}
