//! Synthetic JavaScript fixtures, not light-cljs output.
use big_boy_pants_js::exec::{ExecutorConfig, Placement, WorkerId};
use big_boy_pants_js::{
    Budget, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView, frontend::compile_script,
};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn timer_callbacks_use_ordinary_call_dispatch() {
    let mut outcomes = Vec::new();
    for callback in [
        "new Proxy(() => __rt.effect(1, 42), { apply() {} })",
        "class C { constructor() { __rt.effect(1, 42); } }",
    ] {
        for timer in [false, true] {
            let call = if timer {
                "os.setTimeout(callback, 0);"
            } else {
                "callback();"
            };
            let source = format!("let callback = {callback}; {call} 7;");
            let rt = Runtime::new();
            let code = compile_script("timer-call-dispatch.js", &source).unwrap();
            let task = rt.spawn(&code, &[]).unwrap();
            task.install_sim_host_bindings().unwrap();
            let mut effects = Vec::new();
            struct Effects<'a>(&'a mut Vec<Primitive>);
            impl Host for Effects<'_> {
                fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
                    self.0.push(call.arg);
                    Ok(Primitive::Undefined)
                }
            }
            let outcome = task
                .run_with(Budget::UNLIMITED, &mut Effects(&mut effects))
                .unwrap();
            let kind = match outcome {
                RunOutcome::Failed(error) => Some(error.kind()),
                RunOutcome::Complete(_) => None,
                other => panic!("{source}: unexpected {other:?}"),
            };
            outcomes.push((kind, effects));
        }
    }
    assert_eq!(
        outcomes,
        vec![(Some(big_boy_pants_js::VmErrorKind::TypeError), vec![]); 4],
        "ordinary calls and timer callbacks must reject before running the target"
    );
}

/// Bounded rendezvous before collection, matching the executor tests' leaf
/// synchronization. It never waits for a collection while holding access.
#[derive(Default)]
struct Meet {
    arrived: Mutex<usize>,
    changed: Condvar,
}
struct Meeting(Arc<Meet>);
impl Host for Meeting {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        let mut arrived = self.0.arrived.lock().unwrap();
        *arrived += 1;
        self.0.changed.notify_all();
        let (arrived, _) = self
            .0
            .changed
            .wait_timeout_while(arrived, TIMEOUT, |n| *n < 2)
            .unwrap();
        if *arrived < 2 {
            return Err("second worker did not arrive".into());
        }
        Ok(Primitive::Undefined)
    }
}

#[test]
fn gc_point_in_two_tasks() {
    let rt = Runtime::new();
    let code = compile_script("two-gc-points.js", "let kept = { n: 42 }; __rt.effect(0, 0); std.gc(); let i = 0; while(i < 20) { std.gc(); i++; } kept.n;").unwrap();
    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let task = rt.spawn(&code, &[]).unwrap();
            task.install_sim_host_bindings().unwrap();
            task
        })
        .collect();
    let meet = Arc::new(Meet::default());
    let exec = ExecutorConfig::new()
        .workers(2)
        .budget(Budget::instructions(10000))
        .host(move |_| Box::new(Meeting(meet.clone())))
        .start(&rt)
        .unwrap();
    let ids: Vec<_> = tasks.iter().map(|task| task.id()).collect();
    for (index, task) in tasks.into_iter().enumerate() {
        exec.submit_with(task, Placement::Worker(WorkerId::new(index)))
            .unwrap();
    }
    for id in ids {
        let Some(RunOutcome::Complete(value)) = exec.take_outcome(id, TIMEOUT) else {
            panic!("both collection-point tasks must finish before the deadline");
        };
        assert_eq!(
            rt.scope(|s| s.view(s.local(&value).unwrap())),
            ValueView::Number(42.0)
        );
    }
    assert!(rt.heap_stats().collections > 0);
    exec.shutdown(TIMEOUT).unwrap();
}
