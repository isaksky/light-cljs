use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::host::{TimeZone, sim::SimTimeHost};
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
    // Every byte below a successful probe's peak fails, including the last
    // charges (the global object's own `globalThis` property).
    let peak = byte_limited(usize::MAX / 2)
        .unwrap()
        .heap_stats()
        .peak_managed_bytes;
    for limit in peak - 1024..=peak {
        match byte_limited(limit) {
            Err(error) => assert!(limit < peak && error == RuntimeStartError::HeapLimit),
            Ok(_) => assert_eq!(limit, peak, "started below its peak"),
        }
    }
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
    run_sliced_on(rt, source, &mut Effectless)
}

/// [`run_sliced`] with `host` answering leaf effects (a clock, a zone).
fn run_sliced_on(rt: &Runtime, source: &str, host: &mut dyn Host) -> RunOutcome {
    let code = compile_function(source).unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    loop {
        match task.run_with(Budget::instructions(97), host).unwrap() {
            RunOutcome::Yielded => {}
            outcome => return outcome,
        }
    }
}

fn caught(rt: &Runtime, body: &str) {
    caught_on(rt, body, &mut Effectless)
}

fn caught_on(rt: &Runtime, body: &str, host: &mut dyn Host) {
    let source = format!(
        "function f() {{ let kept = []; try {{ {body} }} catch (e) {{ kept = null; return e === 'heap byte limit exceeded' || String(e); }} return 'not denied'; }}"
    );
    let outcome = run_sliced_on(rt, &source, host);
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

/// The collection a denial requests runs at the handler's first
/// instruction, before it drops anything; a host scope opened between runs
/// sees the released data only after a later collection, which
/// `byte_starved` tells the embedder to request.
#[test]
fn byte_starved_tells_a_host_scope_to_collect_what_a_handler_released() {
    let (rt, limit) = with_headroom(2 * 1024 * 1024);
    assert!(!rt.byte_starved());
    let code = compile_function(
        "function f() { let kept = []; try { for (;;) kept.push({a: 1, b: 2}); } catch (e) { kept = null; return e === 'heap byte limit exceeded'; } }",
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(result) = task.run_with(Budget::UNLIMITED, &mut Effectless).unwrap()
    else {
        panic!("the denial is caught");
    };
    assert_eq!(result.view(), ValueView::Bool(true));
    drop((result, task));
    assert!(rt.byte_starved());
    // A quarter of the limit: two bytes per code unit.
    let large = || Primitive::string(&"x".repeat(limit / 8));
    assert!(rt.try_root(large()).is_none(), "{:?}", rt.heap_stats());
    rt.request_collection().unwrap();
    assert!(!rt.byte_starved(), "{:?}", rt.heap_stats());
    assert!(rt.try_root(large()).is_some(), "{:?}", rt.heap_stats());
    assert!(rt.heap_stats().peak_managed_bytes <= limit);
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
fn list_format_large_first_item_precharges_flattening() {
    // The 3 MB item fits, but the formatted output's flattened copy (read by
    // charCodeAt) would need another 3 MB: the format itself is denied.
    let (rt, limit) = with_headroom(5 * 1024 * 1024);
    caught(
        &rt,
        "const s = 'x'.repeat(1500000); kept.push(s); \
         const t = new Intl.ListFormat('en').format([s, 'y']); kept.push(t); \
         kept.push(t.charCodeAt(0));",
    );
    assert!(rt.heap_stats().peak_managed_bytes <= limit);
    // With room for both, the output owns its copy's charge before it is read.
    let (rt, _) = with_headroom(16 * 1024 * 1024);
    let before = rt.heap_stats().managed_bytes;
    let outcome = run_sliced(
        &rt,
        "function f() { const t = new Intl.ListFormat('en').format(['x'.repeat(1500000), 'y']); \
         return t.charCodeAt(1500005) === 121 ? t : 'wrong'; }",
    );
    let RunOutcome::Complete(result) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(result.string().unwrap().len(), 1_500_006);
    let held = rt.heap_stats().managed_bytes - before;
    assert!(
        held >= 2 * 2 * 1_500_000,
        "item and flat copy: {held} bytes"
    );
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

/// An `await` whose saved activation is denied must not leave a reaction
/// behind: the handler continues, awaits another promise, and only that
/// promise's settlement resumes it.
#[test]
fn await_snapshot_denial_does_not_leave_a_reaction() {
    for (kind, header) in [
        ("async function", "async function g()"),
        ("async generator", "async function* g()"),
    ] {
        let (rt, limit) = with_headroom(2 * 1024 * 1024);
        // The activation holds 20,000 locals (about 320 KiB); after the fill
        // and one release at most about 128 KiB are free, enough for the
        // reaction but not for the saved activation.
        let locals = (0..20_000)
            .map(|i| format!("a{i} = 0"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "function f() {{
               let res1, res2;
               const p1 = new Promise(r => res1 = r), p2 = new Promise(r => res2 = r);
               let kept = [];
               {header} {{
                 let r, v, {locals};
                 try {{ for (;;) kept.push(new ArrayBuffer(65536)); }} catch (e) {{}}
                 kept.pop();
                 for (let i = 0; i < 500; i++) {{}}
                 try {{ r = await p1; }} catch (e) {{ r = e === 'heap byte limit exceeded' ? 'denied' : 'other ' + e; }}
                 kept = null;
                 for (let i = 0; i < 500; i++) {{}}
                 try {{ v = await p2; }} catch (e) {{ v = 'second ' + e; }}
                 globalThis.out = r + ' ' + v;
               }}
               const it = g();
               (it.next ? it.next() : it).catch(e => globalThis.out = 'rejected ' + e);
               Promise.resolve().then(() => res1('one')).then(() => res2('two')).then(() => 0).then(() => 0).then(() => __rt.need(311, 1));
               return 0;
             }}"
        );
        let code = compile_function(&source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let outcome = loop {
            match task
                .run_with(Budget::instructions(97), &mut Effectless)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                outcome => break outcome,
            }
        };
        assert!(
            matches!(outcome, RunOutcome::Waiting(_)),
            "{kind}: {outcome:?}"
        );
        let out = task.global_value("out").unwrap().expect("g finished");
        assert_eq!(
            out.string().map(|s| s.to_string_lossy()).as_deref(),
            Some("denied two"),
            "{kind}"
        );
        assert!(rt.heap_stats().peak_managed_bytes <= limit, "{kind}");
    }
}

/// Settling a promise at the byte limit delivers every registered reaction:
/// each prepaid its job and queue slot when it registered, so the queue's
/// growth cannot be denied after the promise is already settled.
#[test]
fn promise_settlement_preserves_reactions_at_the_byte_limit() {
    let (rt, limit) = with_headroom(2 * 1024 * 1024);
    let code = compile_function(
        "function f() {
           let res;
           const p = new Promise(r => res = r);
           const out = [];
           for (let i = 0; i < 9; i++) p.then(v => { out.push(v + i); });
           let kept = [], settled;
           try { for (;;) kept.push({a: 1}); } catch (e) {}
           try { res(1); settled = 'settled'; } catch (e) { settled = 'denied ' + e; }
           kept = null;
           for (let i = 0; i < 500; i++) {}
           p.then(() => { globalThis.out = settled + ' ' + out.join(','); __rt.need(312, 1); });
           return 0;
         }",
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let outcome = loop {
        match task
            .run_with(Budget::instructions(97), &mut Effectless)
            .unwrap()
        {
            RunOutcome::Yielded => {}
            outcome => break outcome,
        }
    };
    assert!(matches!(outcome, RunOutcome::Waiting(_)), "{outcome:?}");
    let out = task.global_value("out").unwrap().expect("reactions ran");
    assert_eq!(
        out.string().map(|s| s.to_string_lossy()).as_deref(),
        Some("settled 1,2,3,4,5,6,7,8,9")
    );
    assert!(rt.heap_stats().peak_managed_bytes <= limit);
}

/// `allocation_threshold: None` disables automatic collection, including
/// the run-entry collections that follow a byte-limit denial; the embedder
/// still sees the starvation and can collect explicitly.
#[test]
fn byte_denial_respects_disabled_automatic_collection() {
    let limit = realm_bytes() + 1024 * 1024;
    let rt = Runtime::try_with_heap_policy(
        Default::default(),
        HeapPolicy {
            allocation_threshold: None,
            hard_limit_bytes: Some(limit),
            ..Default::default()
        },
    )
    .unwrap();
    let before = rt.heap_stats().collections;
    caught(&rt, "for (;;) kept.push({a: 1, b: 2});");
    assert!(rt.byte_starved());
    let _ = run_sliced(
        &rt,
        "function f() { let n = 0; for (let i = 0; i < 1000; i++) n += i; return n; }",
    );
    assert_eq!(
        rt.heap_stats().collections,
        before,
        "no automatic collection"
    );
    rt.request_collection().unwrap();
    assert!(!rt.byte_starved());
}

/// Join and String.raw retain one charged part per element; once they hand
/// the parts to the string builder, the charge must stay while the builder
/// copies them in slices across yields.
#[test]
fn join_and_raw_keep_part_charges_across_string_build_yields() {
    for source in [
        "function f() { return new Array(20000).fill('x').join(''); }",
        "function f() { return String.raw({raw: new Array(20000).fill('x')}); }",
    ] {
        let (rt, _) = with_headroom(64 * 1024 * 1024);
        let before = rt.heap_stats().managed_bytes;
        let code = compile_function(source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let mut samples = vec![];
        loop {
            match task
                .run_with(Budget::instructions(1), &mut Effectless)
                .unwrap()
            {
                RunOutcome::Yielded => samples.push(rt.heap_stats().managed_bytes),
                RunOutcome::Complete(result) => {
                    assert_eq!(result.string().unwrap().len(), 20000, "{source}");
                    break;
                }
                outcome => panic!("{source}: {outcome:?}"),
            }
        }
        // The last yields are inside the string build, after every part.
        assert!(samples.len() > 20, "{source}: {} yields", samples.len());
        let late = samples[samples.len() - 5];
        assert!(
            late - before >= 20000 * 64,
            "{source}: parts charge released while building: {} bytes above start",
            late - before
        );
    }
}

/// Leaf reads and formatters return fresh string bodies that a caller can
/// keep. Writing them over a preallocated array's existing slots grows no
/// other charged storage, so only the outputs' own charges can reach the
/// limit (light-cljs-492).
/// Bindings the retained-output loops below read from.
const SETUP: &str = "const s = 'xyz'; const w = new String('xyz'); const d = new Date(0); \
                     const invalid = new Date(NaN); const f = new Intl.DateTimeFormat('en'); \
                     const numberLike = {valueOf() { return 0; }}; \
                     const styled = new Intl.DateTimeFormat('en', {dateStyle: 'full', timeStyle: 'full'}); \
                     const money = new Intl.NumberFormat('en', {style: 'currency', currency: 'EUR'}); \
                     const seg = new Intl.Segmenter('en'); const t = true; \
                     const ta = new Uint8Array(1); const pr = new Intl.PluralRules('en'); \
                     const coll = new Intl.Collator('en'); const nf = new Intl.NumberFormat('en'); \
                     const lf = new Intl.ListFormat('en'); \
                     const dn = new Intl.DisplayNames('en', {type: 'region'}); \
                     const rtf = new Intl.RelativeTimeFormat('en'); \
                     const meters = new Intl.NumberFormat('en', {style: 'unit', unit: 'meter'}); \
                     const du = new Intl.DurationFormat('en');";

#[test]
fn retained_leaf_string_outputs_obey_byte_limit() {
    const SLOTS: usize = 40_000;
    let outputs = [
        "s[1]",
        "w[1]",
        "d.toISOString()",
        "d.toString()",
        "d.toUTCString()",
        "d.toDateString()",
        "invalid.toString()",
        "f.format(0)",
        // A number-like argument completes ToNumber in a later native step.
        "f.format(numberLike)",
        // Clock reads.
        "f.format()",
        "Date()",
        "styled.format(0)",
        // Data-dependent resolvedOptions() fields: a field read out of the
        // options object outlives the object's charge.
        "f.resolvedOptions().locale",
        "f.resolvedOptions().timeZone",
        "money.resolvedOptions().currency",
        "seg.resolvedOptions().locale",
        "meters.resolvedOptions().unit",
        "coll.resolvedOptions().locale",
        "nf.resolvedOptions().locale",
        "pr.resolvedOptions().locale",
        "lf.resolvedOptions().locale",
        "dn.resolvedOptions().locale",
        "rtf.resolvedOptions().locale",
        "du.resolvedOptions().locale",
    ];
    let setup = SETUP;
    let host = || {
        SimTimeHost::new(
            1506098258091.0,
            TimeZone::fixed_offset_seconds(3600).unwrap(),
        )
    };
    let (rt, limit) = with_headroom(4 * 1024 * 1024);
    for output in outputs {
        caught_on(
            &rt,
            &format!(
                "{setup} const a = new Array({SLOTS}).fill(0); kept.push(a); \
                 for (let i = 0; i < a.length; i++) a[i] = {output};"
            ),
            &mut host(),
        );
        let stats = rt.heap_stats();
        assert!(stats.peak_managed_bytes <= limit, "{output}: {stats:?}");
    }
    // With room for all of them, each kept output holds at least its body.
    for output in outputs {
        let (rt, _) = with_headroom(64 * 1024 * 1024);
        let before = rt.heap_stats().managed_bytes;
        let outcome = run_sliced_on(
            &rt,
            &format!(
                "function f() {{ {setup} const a = new Array({SLOTS}).fill(0); \
                 for (let i = 0; i < a.length; i++) a[i] = {output}; \
                 return a[{SLOTS} - 1] === a[0] && typeof a[0] === 'string' ? a : 'wrong'; }}"
            ),
            &mut host(),
        );
        let RunOutcome::Complete(result) = outcome else {
            panic!("{output}: {outcome:?}");
        };
        assert!(result.string().is_none(), "{output}: {:?}", result.view());
        let held = rt.heap_stats().managed_bytes - before;
        assert!(held >= SLOTS * 256, "{output}: {held} bytes held");
    }
}

/// Live bytes the system allocator holds for the current thread (allocations
/// minus frees made on it). Each test runs its engine tasks on its own
/// thread, so concurrent tests do not disturb one another's count.
mod live {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static LIVE: Cell<i64> = const { Cell::new(0) };
    }

    fn count(delta: i64) {
        // A const-initialized Cell has no destructor, so this never fails,
        // even while the thread exits.
        let _ = LIVE.try_with(|live| live.set(live.get() + delta));
    }

    pub fn bytes() -> i64 {
        LIVE.with(Cell::get)
    }

    struct Counting;

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    // SAFETY: every method forwards its arguments unchanged to `System`,
    // which implements `GlobalAlloc` correctly; the counting touches only a
    // const thread-local Cell and never allocates, so it cannot recurse.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: the caller upholds `alloc`'s contract for `layout`.
            let p = unsafe { System.alloc(layout) };
            if !p.is_null() {
                count(layout.size() as i64);
            }
            p
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            // SAFETY: as for `alloc`.
            let p = unsafe { System.alloc_zeroed(layout) };
            if !p.is_null() {
                count(layout.size() as i64);
            }
            p
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: `ptr` came from this allocator (hence `System`) with
            // `layout`.
            unsafe { System.dealloc(ptr, layout) };
            count(-(layout.size() as i64));
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            // SAFETY: `ptr`/`layout` came from this allocator (hence
            // `System`) and `new_size` is valid for `layout`'s alignment.
            let p = unsafe { System.realloc(ptr, layout, new_size) };
            if !p.is_null() {
                count(new_size as i64 - layout.size() as i64);
            }
            p
        }
    }
}

/// Fixed-text results share one body per spelling (as an executable's
/// constants do), so retaining one in every slot of an array allocates
/// nothing the byte budget would have to count. Each output is measured
/// against a loop doing the same work but storing a literal instead, after
/// a collection: the difference is what the retained results themselves
/// hold, well under one string body per slot.
#[test]
fn retained_fixed_text_outputs_share_one_body() {
    const SLOTS: usize = 40_000;
    let outputs = [
        "typeof i",
        "typeof a",
        "true.toString()",
        "t.toString()",
        "ta[Symbol.toStringTag]",
        "pr.select(1)",
        "pr.select(i)",
        "pr.resolvedOptions().type",
        "f.resolvedOptions().calendar",
        "f.resolvedOptions().numberingSystem",
        "coll.resolvedOptions().usage",
        "nf.resolvedOptions().style",
        "nf.resolvedOptions().notation",
        "lf.resolvedOptions().type",
        "dn.resolvedOptions().fallback",
        "rtf.resolvedOptions().numeric",
        "seg.resolvedOptions().granularity",
        "meters.resolvedOptions().unitDisplay",
        "du.resolvedOptions().style",
        "du.resolvedOptions().hours",
        "du.resolvedOptions().hoursDisplay",
        "du.resolvedOptions().numberingSystem",
    ];
    let host = || {
        SimTimeHost::new(
            1506098258091.0,
            TimeZone::fixed_offset_seconds(3600).unwrap(),
        )
    };
    let retained = |output: &str| {
        let (rt, _) = with_headroom(64 * 1024 * 1024);
        let start = live::bytes();
        let outcome = run_sliced_on(
            &rt,
            &format!(
                "function f() {{ {SETUP} const a = new Array({SLOTS}).fill(0); \
                 for (let i = 0; i < a.length; i++) a[i] = {output}; \
                 return typeof a[0] === 'string' ? a : 'wrong'; }}"
            ),
            &mut host(),
        );
        let RunOutcome::Complete(result) = &outcome else {
            panic!("{output}: {outcome:?}");
        };
        assert!(result.string().is_none(), "{output}: {:?}", result.view());
        rt.collect().unwrap();
        let held = live::bytes() - start;
        drop(outcome);
        held
    };
    for output in outputs {
        let same_work = retained(&format!("({output}, 'k')"));
        let held = retained(output);
        assert!(
            held - same_work < (SLOTS * 16) as i64,
            "{output}: {held} bytes live against {same_work} storing a literal"
        );
    }
}
