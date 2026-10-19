use super::*;

#[test]
fn initial_heap_threshold_can_grow_for_live_data_within_the_session_budget() {
    let evaluator = V8Evaluator::new_with_initial_heap_limit(
        EvaluatorOptions {
            memory_limit: 128 * 1024 * 1024,
            timeout: Duration::from_secs(5),
            ..Default::default()
        },
        16 * 1024 * 1024,
        || include_str!("v8_services.js").replace("__LIGHT_RUNTIME_SOURCE__", RUNTIME_SOURCE),
        None,
    )
    .unwrap();
    assert_eq!(
        number(
            &evaluator,
            "(() => { globalThis.keep = []; for (let i = 0; i < 48; i++) keep.push(new Array(262144).fill(3)); return keep.length; })()"
        ),
        48.0
    );
    assert!(
        evaluator.memory.growths.get() > 0,
        "the test must exercise adaptive growth"
    );
    assert_eq!(number(&evaluator, "keep[0][0] + keep[47][262143]"), 6.0);
}

#[test]
fn adaptive_heap_growth_still_stops_at_the_session_budget() {
    let evaluator = V8Evaluator::new_with_initial_heap_limit(
        EvaluatorOptions {
            memory_limit: 64 * 1024 * 1024,
            timeout: Duration::from_secs(5),
            ..Default::default()
        },
        16 * 1024 * 1024,
        || include_str!("v8_services.js").replace("__LIGHT_RUNTIME_SOURCE__", RUNTIME_SOURCE),
        None,
    )
    .unwrap();
    let error = evaluator
        .evaluate_js(
            "(() => { const keep = []; while (true) keep.push(new Array(262144).fill(1)); })()",
        )
        .unwrap_err();
    assert!(error.0.contains("memory limit exhausted"), "{error}");
    assert!(evaluator.memory.growths.get() > 0);
}

#[test]
fn heap_and_backing_stores_share_one_session_budget() {
    let mut failures = Vec::new();
    for source in [
        "[new Array(5 * 1024 * 1024).fill(1), new ArrayBuffer(20 * 1024 * 1024)]",
        "[new SharedArrayBuffer(20 * 1024 * 1024, {maxByteLength: 24 * 1024 * 1024}), new Array(5 * 1024 * 1024).fill(1)]",
    ] {
        let evaluator = session();
        let result = evaluator.evaluate_js(source);
        let diagnostic = match result {
            Err(error) => error.0,
            Ok(value) => {
                let mut isolate = evaluator.isolate.borrow_mut();
                let entered = Entered::new(&mut isolate);
                let stats = entered.0.get_heap_statistics();
                let diagnostic = format!(
                    "budget bypass: heap={} backing={}",
                    stats.used_heap_size(),
                    evaluator.state.buffers.used()
                );
                drop(entered);
                drop(isolate);
                evaluator.release(value).unwrap();
                diagnostic
            }
        };
        if !diagnostic.contains("memory limit exhausted") {
            failures.push(format!("{source}: {diagnostic}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn combined_storage_below_the_limit_is_not_double_counted() {
    for buffer in [
        "new ArrayBuffer(12 * 1024 * 1024)",
        "new SharedArrayBuffer(12 * 1024 * 1024)",
        "new ArrayBuffer(12 * 1024 * 1024, {maxByteLength: 32 * 1024 * 1024})",
        "new SharedArrayBuffer(12 * 1024 * 1024, {maxByteLength: 32 * 1024 * 1024})",
        "new WebAssembly.Memory({initial: 192, maximum: 1024})",
        "new WebAssembly.Memory({initial: 192, maximum: 1024, shared: true})",
    ] {
        let evaluator = session();
        assert_eq!(
            number(
                &evaluator,
                &format!(
                    "(() => {{ globalThis.keep = [new Array(3 * 1024 * 1024).fill(1), {buffer}]; return 777; }})()"
                )
            ),
            777.0,
            "{buffer}"
        );
        assert_eq!(evaluator.state.buffers.used(), 12 * 1024 * 1024);
        assert_eq!(number(&evaluator, "42"), 42.0);
    }
}

#[test]
fn combined_budget_interrupts_a_loop_after_heap_growth() {
    let evaluator = session();
    let error = evaluator
        .evaluate_js(
            r#"(() => {
        globalThis.keep = [new SharedArrayBuffer(20 * 1024 * 1024,
            {maxByteLength: 24 * 1024 * 1024}), new Array(5 * 1024 * 1024).fill(1)];
        while (true) {}
    })()"#,
        )
        .unwrap_err();
    assert!(error.0.contains("memory limit exhausted"), "{error}");
    assert!(
        evaluator
            .evaluate_js("1")
            .unwrap_err()
            .0
            .contains("memory limit exhausted")
    );
}

#[test]
fn combined_budget_covers_runtime_initialization() {
    let result = V8Evaluator::new_with_startup(
        EvaluatorOptions {
            memory_limit: 32 * 1024 * 1024,
            timeout: std::time::Duration::from_secs(5),
            ..Default::default()
        },
        || {
            "globalThis.keep = [new SharedArrayBuffer(20 * 1024 * 1024, {maxByteLength: 24 * 1024 * 1024}), new Array(5 * 1024 * 1024).fill(1)]; while (true) {}".into()
        },
    );
    let Err(error) = result else {
        panic!("startup must fail")
    };
    assert!(error.0.contains("memory limit exhausted"), "{error}");
}

#[test]
fn garbage_collection_refreshes_the_heap_share_before_allocation_retry() {
    let evaluator = session();
    assert_eq!(
        number(
            &evaluator,
            "(() => { globalThis.discard = new Array(5 * 1024 * 1024).fill(1); return 1; })()"
        ),
        1.0
    );
    // Publish the size of the still-live heap. The next allocation must collect
    // the now-unreachable array and refresh that observation before retrying.
    collect(&evaluator);
    assert_eq!(
        number(
            &evaluator,
            "(() => { globalThis.discard = null; globalThis.keep = new ArrayBuffer(20 * 1024 * 1024); return keep.byteLength; })()"
        ),
        (20 * 1024 * 1024) as f64
    );
    assert_eq!(number(&evaluator, "42"), 42.0);
}

fn session() -> V8Evaluator {
    V8Evaluator::new(EvaluatorOptions {
        memory_limit: 32 * 1024 * 1024,
        timeout: std::time::Duration::from_secs(5),
        ..Default::default()
    })
    .unwrap()
}

fn number(evaluator: &V8Evaluator, source: &str) -> f64 {
    let value = evaluator.evaluate_js(source).unwrap();
    let result = evaluator.number(value).unwrap();
    evaluator.release(value).unwrap();
    result
}

fn collect(evaluator: &V8Evaluator) {
    let mut isolate = evaluator.isolate.borrow_mut();
    let entered = Entered::new(&mut isolate);
    entered.0.low_memory_notification();
}

#[test]
fn backing_store_budgets_cover_construction_growth_and_aggregate_storage() {
    let mut failures = Vec::new();
    for (name, expression) in [
        ("fixed ArrayBuffer", "new ArrayBuffer(33 * 1024 * 1024)"),
        (
            "fixed SharedArrayBuffer",
            "new SharedArrayBuffer(33 * 1024 * 1024)",
        ),
        (
            "resizable ArrayBuffer",
            "new ArrayBuffer(33 * 1024 * 1024, { maxByteLength: 34 * 1024 * 1024 })",
        ),
        (
            "growable SharedArrayBuffer",
            "new SharedArrayBuffer(33 * 1024 * 1024, { maxByteLength: 34 * 1024 * 1024 })",
        ),
        (
            "ArrayBuffer.resize",
            "(() => { const b = new ArrayBuffer(8, { maxByteLength: 34 * 1024 * 1024 }); b.resize(33 * 1024 * 1024); return b; })()",
        ),
        (
            "SharedArrayBuffer.grow",
            "(() => { const b = new SharedArrayBuffer(8, { maxByteLength: 34 * 1024 * 1024 }); b.grow(33 * 1024 * 1024); return b; })()",
        ),
        (
            "Wasm memory",
            "new WebAssembly.Memory({ initial: 528, maximum: 1024 })",
        ),
        (
            "shared Wasm memory",
            "new WebAssembly.Memory({ initial: 528, maximum: 1024, shared: true })",
        ),
        (
            "Wasm memory.grow",
            "(() => { const m = new WebAssembly.Memory({ initial: 1, maximum: 1024 }); m.grow(527); return m; })()",
        ),
        (
            "shared Wasm memory.grow",
            "(() => { const m = new WebAssembly.Memory({ initial: 1, maximum: 1024, shared: true }); m.grow(527); return m; })()",
        ),
        (
            "aggregate fixed buffers",
            "Array.from({length: 8}, () => new ArrayBuffer(5 * 1024 * 1024))",
        ),
        (
            "aggregate growable buffers",
            "Array.from({length: 8}, () => new SharedArrayBuffer(5 * 1024 * 1024, {maxByteLength: 6 * 1024 * 1024}))",
        ),
        (
            "aggregate mixed buffers",
            "[new ArrayBuffer(8 * 1024 * 1024), new SharedArrayBuffer(8 * 1024 * 1024), new ArrayBuffer(8 * 1024 * 1024, {maxByteLength: 9 * 1024 * 1024}), new SharedArrayBuffer(9 * 1024 * 1024, {maxByteLength: 10 * 1024 * 1024})]",
        ),
    ] {
        let evaluator = V8Evaluator::new(EvaluatorOptions {
            memory_limit: 32 * 1024 * 1024,
            ..Default::default()
        })
        .unwrap();
        match evaluator.evaluate_js(expression) {
            Err(error) if error.0.contains("memory limit exhausted") => {
                assert!(
                    evaluator
                        .evaluate_js("1")
                        .unwrap_err()
                        .0
                        .contains("memory limit exhausted"),
                    "{name}"
                );
            }
            Err(error) => failures.push(format!("{name}: wrong error {error}")),
            Ok(value) => {
                failures.push(format!("{name}: exceeded budget without error"));
                evaluator.release(value).unwrap();
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn backing_store_failure_cannot_be_hidden_by_javascript_catch() {
    for expression in [
        "new ArrayBuffer(33 * 1024 * 1024)",
        "new SharedArrayBuffer(33 * 1024 * 1024, {maxByteLength: 34 * 1024 * 1024})",
        "new ArrayBuffer(8, {maxByteLength: 34 * 1024 * 1024}).resize(33 * 1024 * 1024)",
        "new SharedArrayBuffer(8, {maxByteLength: 34 * 1024 * 1024}).grow(33 * 1024 * 1024)",
        "new WebAssembly.Memory({initial: 1, maximum: 1024}).grow(527)",
    ] {
        let evaluator = session();
        let error = evaluator
            .evaluate_js(&format!(
                "(() => {{ try {{ {expression}; }} catch (_) {{}} while (true) {{}} }})()"
            ))
            .unwrap_err();
        assert!(
            error.0.contains("memory limit exhausted"),
            "{expression}: {error}"
        );
    }
}

#[test]
fn backing_store_budget_applies_during_runtime_startup() {
    let result = V8Evaluator::new_with_startup(
        EvaluatorOptions {
            memory_limit: 32 * 1024 * 1024,
            timeout: std::time::Duration::from_secs(5),
            ..Default::default()
        },
        || {
            "try { new SharedArrayBuffer(33 * 1024 * 1024, {maxByteLength: 34 * 1024 * 1024}); } catch (_) {} while (true) {}".into()
        },
    );
    let Err(error) = result else {
        panic!("startup must fail")
    };
    assert!(error.0.contains("memory limit exhausted"), "{error}");
}

#[test]
fn wasm_memory_grow_instruction_cannot_silently_exceed_the_budget() {
    let evaluator = session();
    // (module (memory (export "m") 1 1024)
    //   (func (export "g") (param i32) (result i32)
    //     local.get 0 memory.grow))
    let error = evaluator
        .evaluate_js(
            r#"(() => {
        const module = new WebAssembly.Module(new Uint8Array([
            0,97,115,109,1,0,0,0,
            1,6,1,96,1,127,1,127,
            3,2,1,0,
            5,5,1,1,1,128,8,
            7,9,2,1,109,2,0,1,103,0,0,
            10,8,1,6,0,32,0,64,0,11
        ]));
        return new WebAssembly.Instance(module).exports.g(527);
    })()"#,
        )
        .unwrap_err();
    assert!(error.0.contains("memory limit exhausted"), "{error}");
}

#[test]
fn invalid_growth_is_a_catchable_language_error_without_budget_failure() {
    let evaluator = session();
    assert_eq!(
        number(
            &evaluator,
            r#"(() => {
        let caught = 0;
        for (const grow of [
            () => new ArrayBuffer(1, {maxByteLength: 65536}).resize(65537),
            () => new SharedArrayBuffer(1, {maxByteLength: 65536}).grow(65537),
            () => new WebAssembly.Memory({initial: 1, maximum: 1}).grow(1)
        ]) { try { grow(); } catch (e) { if (e instanceof RangeError) ++caught; } }
        return caught;
    })()"#
        ),
        3.0
    );
    assert_eq!(number(&evaluator, "42"), 42.0);
}

#[test]
fn reservations_charge_committed_pages_and_shrink_refunds_them() {
    let evaluator = session();
    let before = evaluator.state.buffers.used();
    assert_eq!(
        number(
            &evaluator,
            "(() => { globalThis.b = new ArrayBuffer(1, {maxByteLength: 1024 * 1024 * 1024}); return b.byteLength; })()"
        ),
        1.0
    );
    let one_page = evaluator.state.buffers.used() - before;
    assert!((4096..=65536).contains(&one_page), "{one_page}");
    assert_eq!(
        number(
            &evaluator,
            "(() => { b.resize(4 * 1024 * 1024); new Uint8Array(b).fill(123); b.resize(1); return b.byteLength; })()"
        ),
        1.0
    );
    assert_eq!(evaluator.state.buffers.used(), before + one_page);
    assert_eq!(
        number(
            &evaluator,
            "(() => { b.resize(4 * 1024 * 1024); const a = new Uint8Array(b); return a[0] === 123 && a.subarray(1).every(x => x === 0) ? 1 : 0; })()"
        ),
        1.0
    );
    assert_eq!(evaluator.state.buffers.used(), before + 4 * 1024 * 1024);
    assert_eq!(
        number(
            &evaluator,
            "(() => { b.resize(0); return b.byteLength; })()"
        ),
        0.0
    );
    assert_eq!(evaluator.state.buffers.used(), before);
}

#[test]
fn buffer_coercions_and_growth_preserve_native_semantics() {
    let evaluator = session();
    assert_eq!(
        number(
            &evaluator,
            r#"(() => {
        const calls = [];
        const size = { valueOf() { calls.push('size'); return 7; } };
        const options = { get maxByteLength() {
            calls.push('max'); return { valueOf() { calls.push('max-value'); return 65536; } };
        }};
        const b = new ArrayBuffer(size, options);
        const a = new Uint8Array(b);
        if (!a.every(x => x === 0)) return 0;
        a[0] = 19;
        b.resize({valueOf() { calls.push('resize'); return 32768; }});
        if (a.length !== 32768 || a[0] !== 19 || !a.subarray(1).every(x => x === 0)) return 0;
        const s = new SharedArrayBuffer(1, {maxByteLength: 65536});
        new Uint8Array(s)[0] = 23;
        s.grow({valueOf() { calls.push('grow'); return 32768; }});
        if (new Uint8Array(s)[0] !== 23 || !new Uint8Array(s).subarray(1).every(x => x === 0)) return 0;
        const m = new WebAssembly.Memory({initial: 1, maximum: 4, shared: true});
        new Uint8Array(m.buffer)[0] = 29;
        const old = m.grow(1);
        return old === 1 && m.buffer.byteLength === 131072 && new Uint8Array(m.buffer)[0] === 29
            && calls.join(',') === 'size,max,max-value,resize,grow' ? 1 : 0;
    })()"#
        ),
        1.0
    );
}

#[test]
fn unreachable_backing_stores_are_reclaimed_during_allocation_retry() {
    for source in [
        "(() => { for (let i=0; i<40; ++i) { const b = new ArrayBuffer(4*1024*1024); new Uint8Array(b)[0] = i; } return 1; })()",
        "(() => { for (let i=0; i<40; ++i) { const b = new SharedArrayBuffer(4*1024*1024, {maxByteLength: 8*1024*1024}); new Uint8Array(b)[0] = i; } return 1; })()",
        "(() => { for (let i=0; i<40; ++i) { const m = new WebAssembly.Memory({initial: 64, maximum: 128}); new Uint8Array(m.buffer)[0] = i; } return 1; })()",
    ] {
        let evaluator = session();
        let before = evaluator.state.buffers.used();
        assert_eq!(number(&evaluator, source), 1.0);
        collect(&evaluator);
        assert_eq!(evaluator.state.buffers.used(), before, "{source}");
        assert_eq!(number(&evaluator, "42"), 42.0);
    }
}

#[test]
fn backing_store_allocator_lives_until_last_native_owner_drops() {
    for (source, shared) in [
        ("new ArrayBuffer(1048576)", false),
        ("new ArrayBuffer(1048576, {maxByteLength: 2097152})", false),
        ("new SharedArrayBuffer(1048576)", true),
        (
            "new SharedArrayBuffer(1048576, {maxByteLength: 2097152})",
            true,
        ),
        (
            "new WebAssembly.Memory({initial:16, maximum:32}).buffer",
            false,
        ),
        (
            "new WebAssembly.Memory({initial:16, maximum:32, shared:true}).buffer",
            true,
        ),
    ] {
        let evaluator = session();
        let budget = evaluator.state.buffers.clone();
        let store = evaluator
            .with_active(|active| {
                active.enter(|scope| {
                    let value = script(scope, source)?;
                    Ok(if shared {
                        Local::<v8::SharedArrayBuffer>::try_from(value)
                            .unwrap()
                            .get_backing_store()
                    } else {
                        Local::<v8::ArrayBuffer>::try_from(value)
                            .unwrap()
                            .get_backing_store()
                    })
                })
            })
            .unwrap();
        assert_eq!(budget.used(), 1048576, "{source}");
        drop(evaluator);
        assert_eq!(budget.used(), 1048576, "native owner: {source}");
        drop(store);
        assert_eq!(budget.used(), 0, "last native owner: {source}");
        assert_eq!(Arc::strong_count(&budget), 1, "allocator owner: {source}");
    }
}
