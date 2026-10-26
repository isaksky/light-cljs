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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

fn byte_limited(bytes: usize) -> Result<Runtime, big_boy_pants_js::RuntimeStartError> {
    Runtime::try_with_heap_policy(
        Default::default(),
        HeapPolicy {
            hard_limit_bytes: Some(bytes),
            ..Default::default()
        },
    )
}

/// The fixed accounted footprint of a started runtime.
fn realm_bytes() -> usize {
    let rt = byte_limited(usize::MAX / 2).unwrap();
    let stats = rt.heap_stats();
    assert_eq!(stats.byte_limit, Some(usize::MAX / 2));
    assert!(stats.managed_bytes > 0);
    assert!(stats.peak_managed_bytes >= stats.managed_bytes);
    stats.managed_bytes
}

#[test]
fn checked_startup_fails_without_panicking_at_every_tight_byte_limit() {
    use big_boy_pants_js::RuntimeStartError;
    let realm = realm_bytes();
    let mut failed = 0;
    for step in 0..320 {
        let limit = realm * step / 256;
        match byte_limited(limit) {
            Err(error) => {
                assert_eq!(error, RuntimeStartError::HeapLimit, "limit {limit}");
                assert!(limit < realm * 2, "a generous limit {limit} failed");
                failed += 1;
            }
            Ok(rt) => {
                assert!(limit >= realm, "started below the realm footprint: {limit}");
                assert!(rt.heap_stats().peak_managed_bytes <= limit);
            }
        }
    }
    assert!(failed >= 256, "every limit below the realm footprint fails");
    let rt = byte_limited(realm * 2).unwrap();
    let code = compile_function("function f() { return [1, 2, 3].join('-'); }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(result) = task.run().unwrap() else {
        panic!("a started runtime runs a task");
    };
    assert_eq!(result.view(), ValueView::String);
}

#[test]
fn unchecked_startup_under_a_tiny_byte_limit_fails_the_first_task_without_abort() {
    let rt = Runtime::with_heap_policy(
        Default::default(),
        HeapPolicy {
            hard_limit_bytes: Some(4096),
            ..Default::default()
        },
    );
    let code = compile_function("function f() { return {a: 1}; }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Failed(error) = task.run().unwrap() else {
        panic!("the realm cannot fit in 4 KiB");
    };
    assert_eq!(error.kind(), VmErrorKind::HeapLimit);
    assert!(rt.heap_stats().peak_managed_bytes <= 4096);
}

struct Effectless;
impl Host for Effectless {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        Err("no host effects".into())
    }
}

/// A runtime whose byte limit leaves `headroom` above the realm.
fn with_headroom(headroom: usize) -> (Runtime, usize) {
    let limit = realm_bytes() + headroom;
    (byte_limited(limit).unwrap(), limit)
}

/// Run `source` (a zero-argument function) to completion in a slice loop,
/// so yields between instructions are exercised, and return its result.
fn run_sliced(rt: &Runtime, source: &str) -> RunOutcome {
    let code = compile_function(source).unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    loop {
        match task
            .run_with(Budget::instructions(97), &mut Effectless)
            .unwrap()
        {
            RunOutcome::Yielded => {}
            outcome => return outcome,
        }
    }
}

fn caught(rt: &Runtime, body: &str) {
    let source = format!(
        "function f() {{ let kept = []; try {{ {body} }} catch (e) {{ kept = null; return e === 'heap byte limit exceeded' || String(e); }} return 'not denied'; }}"
    );
    let outcome = run_sliced(rt, &source);
    let RunOutcome::Complete(result) = outcome else {
        panic!("{body}: {outcome:?}");
    };
    assert_eq!(
        result.view(),
        ValueView::Bool(true),
        "{body}: {:?}",
        result.string().map(|s| s.to_string_lossy())
    );
}

#[test]
fn public_byte_limit_denial_is_catchable_and_the_runtime_recovers() {
    let (rt, limit) = with_headroom(2 * 1024 * 1024);
    for body in [
        "for (;;) kept.push({a: 1, b: 2});",
        "for (;;) kept.push('x'.repeat(1000) + kept.length);",
        "for (;;) kept.push(new ArrayBuffer(4096));",
        "for (;;) kept.push(__rt.vector(1, 2, 3));",
        "for (;;) kept.push([1, 2, 3, 4]);",
    ] {
        caught(&rt, body);
        let stats = rt.heap_stats();
        assert!(stats.peak_managed_bytes <= limit, "{body}: {stats:?}");
        // No explicit collection: the next task's allocation pressure
        // collects the previous task's released data.
        let outcome = run_sliced(
            &rt,
            "function f() { let a = []; for (let i = 0; i < 2000; i++) a.push({i}); return a.length; }",
        );
        let RunOutcome::Complete(result) = outcome else {
            panic!("{body}: runtime did not recover: {outcome:?}");
        };
        assert_eq!(result.view(), ValueView::Number(2000.0), "{body}");
    }
}

#[test]
fn retained_key_snapshots_and_intl_buffers_are_charged() {
    let (rt, limit) = with_headroom(4 * 1024 * 1024);
    for body in [
        // A string's index keys are many times larger than its code units.
        "const s = 'x'.repeat(200000); kept.push(s); kept.push(Object.keys(s));",
        "const s = 'x'.repeat(200000); kept.push(s); for (const k in s) { kept.push(k); }",
        "const t = new Uint8Array(500000); kept.push(t); kept.push(Object.getOwnPropertyNames(t));",
        "const a = []; a[300000] = 1; a.fill(0, 0); kept.push(a); kept.push(Reflect.ownKeys(a));",
        // A trap result of one repeated key: the duplicate check runs only
        // after every element was listed, each as a key record several
        // times the size of the array slot it came from.
        "const like = new Array(100000).fill('k'); kept.push(like); \
         Reflect.ownKeys(new Proxy({}, {ownKeys: () => like}));",
        "function* names() { for (;;) yield 'n'; } new Intl.ListFormat('en').format(names());",
    ] {
        caught(&rt, body);
        let stats = rt.heap_stats();
        assert!(stats.peak_managed_bytes <= limit, "{body}: {stats:?}");
    }
}

#[test]
fn eight_workers_share_one_byte_limit_and_all_recover() {
    use std::sync::Barrier;
    let (rt, limit) = with_headroom(4 * 1024 * 1024);
    let code = compile_function(
        "function f() { let kept = []; try { for (;;) kept.push({v: 'y'.repeat(64) + kept.length}); } \
         catch (e) { const n = kept.length; kept = null; return e === 'heap byte limit exceeded' ? n : -1; } }",
    )
    .unwrap();
    let after = compile_function("function f() { return [1, 2, 3].length; }").unwrap();
    let start = Barrier::new(8);
    let denied = Barrier::new(8);
    let retained = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|threads| {
        for _ in 0..8 {
            threads.spawn(|| {
                let _registered = rt.register_thread();
                let task = rt.spawn(&code, &[]).unwrap();
                start.wait();
                let RunOutcome::Complete(n) = task.run().unwrap() else {
                    panic!("each worker catches its own denial");
                };
                let ValueView::Number(n) = n.view() else {
                    panic!("count");
                };
                assert!(n >= 0.0, "denial was not the byte limit");
                retained.lock().unwrap().push(n);
                drop(task);
                // Every worker has been denied by the one shared limit, then
                // each runs again without an explicit collection.
                denied.wait();
                let task = rt.spawn(&after, &[]).unwrap();
                assert!(matches!(task.run().unwrap(), RunOutcome::Complete(_)));
            });
        }
    });
    let stats = rt.heap_stats();
    assert!(stats.peak_managed_bytes <= limit, "{stats:?}");
    assert!(stats.collections > 0, "{stats:?}");
    let total: f64 = retained.lock().unwrap().iter().sum();
    assert!(total > 0.0);
}

#[test]
fn eight_worker_garbage_churn_many_times_the_limit_completes_without_explicit_collection() {
    let (rt, limit) = with_headroom(2 * 1024 * 1024);
    let code = compile_function(
        "function f() { let last; for (let i = 0; i < 4000; i++) { last = {i, s: 'z'.repeat(100) + i, a: [i, i, i]}; } return last.i; }",
    )
    .unwrap();
    let start = std::sync::Barrier::new(8);
    std::thread::scope(|threads| {
        for _ in 0..8 {
            threads.spawn(|| {
                let _registered = rt.register_thread();
                let task = rt.spawn(&code, &[]).unwrap();
                start.wait();
                let outcome = task.run().unwrap();
                let RunOutcome::Complete(result) = outcome else {
                    panic!("{outcome:?}");
                };
                assert_eq!(result.view(), ValueView::Number(3999.0));
            });
        }
    });
    let stats = rt.heap_stats();
    assert!(stats.peak_managed_bytes <= limit, "{stats:?}");
    assert!(
        stats.admitted_bytes_total > 4 * limit,
        "the churn must exceed the limit several times: {stats:?}"
    );
    assert!(stats.collections > 1, "{stats:?}");
    assert!(stats.last_live_bytes > 0 && stats.last_live_bytes <= limit);
}
