use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    Budget, HeapPolicy, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView, VmErrorKind,
};

fn limited() -> Runtime {
    Runtime::with_heap_policy(
        Default::default(),
        HeapPolicy {
            allocation_threshold: Some(32),
            hard_limit_objects: Some(2_048),
        },
    )
}

#[test]
fn object_limit_is_catchable_without_allocating_an_error_object() {
    let rt = limited();
    let code = compile_function("function f() { let kept = []; try { for (;;) { kept.push({}); } } catch (e) { kept = null; return e === 'heap object limit exceeded'; } }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(result) = task.run().unwrap() else {
        panic!("expected caught limit");
    };
    assert_eq!(result.view(), ValueView::Bool(true));
    assert!(rt.heap_stats().objects <= 2_048);
    assert!(rt.heap_stats().collections > 0);
}

#[test]
fn native_collection_allocation_limit_is_also_catchable() {
    for allocation in ["__rt.vector(1)", "__rt.formVector(1)"] {
        let rt = limited();
        let source = format!(
            "function f() {{ let kept = []; try {{ for (;;) {{ kept.push({allocation}); }} }} catch (e) {{ return e === 'heap object limit exceeded'; }} }}"
        );
        let code = compile_function(&source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let outcome = task.run().unwrap();
        let RunOutcome::Complete(result) = outcome else {
            panic!("{allocation}: {outcome:?}");
        };
        assert_eq!(result.view(), ValueView::Bool(true), "{allocation}");
    }
}

#[test]
fn quota_denial_while_building_an_error_remains_catchable() {
    struct Snapshot<'a>(&'a Runtime, usize);
    impl Host for Snapshot<'_> {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            self.1 = self.0.heap_stats().objects;
            Ok(Primitive::Undefined)
        }
    }
    let code = compile_function("function f() { __rt.effect(1, 0); try { null.x; } catch (e) { return e === 'heap object limit exceeded'; } }").unwrap();
    let baseline = Runtime::new();
    let mut host = Snapshot(&baseline, 0);
    let task = baseline.spawn(&code, &[]).unwrap();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Complete(_)
    ));
    let rt = Runtime::with_heap_policy(
        Default::default(),
        HeapPolicy {
            allocation_threshold: None,
            hard_limit_objects: Some(host.1),
        },
    );
    let mut host = Snapshot(&rt, 0);
    let task = rt.spawn(&code, &[]).unwrap();
    let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
    let RunOutcome::Complete(result) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(result.view(), ValueView::Bool(true));
}

#[test]
fn uncaught_object_limit_reports_a_resource_error_without_aborting() {
    let rt = limited();
    let code =
        compile_function("function f() { let kept = []; for (;;) { kept.push({}); } }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Failed(error) = task.run().unwrap() else {
        panic!("expected limit error");
    };
    assert_eq!(error.kind(), VmErrorKind::HeapLimit);
    assert_eq!(error.message(), "heap object limit exceeded");
    assert!(rt.heap_stats().objects <= 2_048);
    drop(task);
    rt.collect().unwrap();
    let code = compile_function("function f() { return 42; }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(result) = task.run().unwrap() else {
        panic!("runtime remains usable");
    };
    assert_eq!(result.view(), ValueView::Number(42.0));
}

#[test]
fn object_admission_limit_is_one_atomic_budget_across_eight_threads() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let rt = Runtime::with_heap_policy(
        Default::default(),
        HeapPolicy {
            allocation_threshold: Some(32),
            hard_limit_objects: Some(512),
        },
    );
    let admitted = AtomicUsize::new(0);
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|threads| {
        for _ in 0..8 {
            threads.spawn(|| {
                rt.scope(|scope| {
                    barrier.wait();
                    while scope.try_new_object().is_some() {
                        admitted.fetch_add(1, Ordering::Relaxed);
                    }
                    barrier.wait();
                    assert_eq!(rt.heap_stats().objects, 512);
                })
            });
        }
    });
    assert_eq!(admitted.load(Ordering::Relaxed), 512);
    assert_eq!(rt.collect().unwrap().reclaimed, 512);
    assert!(rt.scope(|scope| scope.try_new_object().is_some()));
}

#[test]
fn automatic_collection_does_not_charge_instruction_slices() {
    struct NoEffects;
    impl Host for NoEffects {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            unreachable!()
        }
    }
    let code = compile_function(
        "function f() { let a; for (let i = 0; i < 100; i++) { a = [i]; } return a[0]; }",
    )
    .unwrap();
    let run = |threshold| {
        let rt = Runtime::with_heap_policy(
            Default::default(),
            HeapPolicy {
                allocation_threshold: threshold,
                ..Default::default()
            },
        );
        let task = rt.spawn(&code, &[]).unwrap();
        let mut yields = 0;
        loop {
            match task
                .run_with(Budget::instructions(7), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Yielded => yields += 1,
                RunOutcome::Complete(result) => {
                    assert_eq!(result.view(), ValueView::Number(99.0));
                    return (yields, rt.heap_stats().collections);
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    };
    let explicit = run(None);
    let automatic = run(Some(1));
    assert_eq!(explicit.0, automatic.0);
    assert_eq!(explicit.1, 0);
    assert!(automatic.1 > 0);
}
