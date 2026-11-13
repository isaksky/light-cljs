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
        "[new SharedArrayBuffer(20 * 1024 * 1024), new Array(5 * 1024 * 1024).fill(1)]",
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
    ] {
        let evaluator = session();
        assert_eq!(
            number(
                &evaluator,
                &format!(
                    "(() => {{ globalThis.keep = [new Array({elements}).fill(1), {buffer}]; return 777; }})()",
                    elements = 12 * 1024 * 1024 / SLOT_BYTES
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
        globalThis.keep = [new SharedArrayBuffer(20 * 1024 * 1024), new Array(5 * 1024 * 1024).fill(1)];
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
            "globalThis.keep = [new SharedArrayBuffer(20 * 1024 * 1024), new Array(5 * 1024 * 1024).fill(1)]; while (true) {}".into()
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
    // About 20 MiB of heap on every engine: a forced full collection of a
    // heap past V8's limit would abort the process.
    let elements = 20 * 1024 * 1024 / SLOT_BYTES;
    assert_eq!(
        number(
            &evaluator,
            &format!(
                "(() => {{ globalThis.discard = new Array({elements}).fill(1); return 1; }})()"
            )
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
fn backing_store_budgets_cover_construction_and_aggregate_storage() {
    let mut failures = Vec::new();
    for (name, expression) in [
        ("fixed ArrayBuffer", "new ArrayBuffer(33 * 1024 * 1024)"),
        (
            "fixed SharedArrayBuffer",
            "new SharedArrayBuffer(33 * 1024 * 1024)",
        ),
        (
            "aggregate fixed buffers",
            "Array.from({length: 8}, () => new ArrayBuffer(5 * 1024 * 1024))",
        ),
        (
            "aggregate mixed buffers",
            "[new ArrayBuffer(12 * 1024 * 1024), new SharedArrayBuffer(12 * 1024 * 1024), new ArrayBuffer(12 * 1024 * 1024)]",
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
        "new SharedArrayBuffer(33 * 1024 * 1024)",
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
        || "try { new SharedArrayBuffer(33 * 1024 * 1024); } catch (_) {} while (true) {}".into(),
    );
    let Err(error) = result else {
        panic!("startup must fail")
    };
    assert!(error.0.contains("memory limit exhausted"), "{error}");
}

/// Stock V8 maps these outside the `ArrayBuffer::Allocator` the budget
/// charges, so the macro realm leaves them out (`RESTRICTED_BUFFERS`).
#[test]
fn storage_outside_the_allocator_is_unavailable_and_catchable() {
    let evaluator = session();
    assert_eq!(
        number(
            &evaluator,
            r#"(() => {
        let caught = 0;
        for (const make of [
            () => new ArrayBuffer(8, {maxByteLength: 16}),
            () => new SharedArrayBuffer(8, {maxByteLength: 16}),
            () => new (class extends ArrayBuffer {})(8, {maxByteLength: 16}),
            () => Reflect.construct(ArrayBuffer, [8, {maxByteLength: 16}]),
        ]) { try { make(); } catch (e) { if (e instanceof TypeError) ++caught; } }
        return typeof WebAssembly === "undefined" && !("WebAssembly" in globalThis) ? caught : -1;
    })()"#
        ),
        4.0
    );
    assert_eq!(number(&evaluator, "42"), 42.0);
}

#[test]
fn fixed_buffers_keep_their_native_semantics() {
    let evaluator = session();
    assert_eq!(
        number(
            &evaluator,
            r#"(() => {
        const checks = [];
        const b = new ArrayBuffer(8, {});
        checks.push(b.byteLength === 8 && !b.resizable && b.maxByteLength === 8);
        checks.push(new ArrayBuffer(4, {maxByteLength: undefined}).byteLength === 4);
        checks.push(b instanceof ArrayBuffer && b.constructor === ArrayBuffer);
        checks.push(Object.getPrototypeOf(b) === ArrayBuffer.prototype);
        checks.push(new Uint8Array(4).buffer instanceof ArrayBuffer);
        checks.push(ArrayBuffer.isView(new DataView(b)) && ArrayBuffer.name === "ArrayBuffer");
        checks.push(b.slice(2).byteLength === 6 && b.slice(2) instanceof ArrayBuffer);
        class Sub extends ArrayBuffer {}
        const s = new Sub(3);
        checks.push(s instanceof Sub && s instanceof ArrayBuffer && s.byteLength === 3);
        const shared = new SharedArrayBuffer(4);
        checks.push(shared instanceof SharedArrayBuffer && shared.constructor === SharedArrayBuffer && !shared.growable);
        let threw = false;
        try { ArrayBuffer(1); } catch (e) { threw = e instanceof TypeError; }
        checks.push(threw);
        const t = new ArrayBuffer(4).transfer(8);
        checks.push(t.byteLength === 8 && !t.resizable);
        return checks.every(Boolean) ? 1 : checks.indexOf(false) + 100;
    })()"#
        ),
        1.0
    );
}

#[test]
fn unreachable_backing_stores_are_reclaimed_during_allocation_retry() {
    for source in [
        "(() => { for (let i=0; i<40; ++i) { const b = new ArrayBuffer(4*1024*1024); new Uint8Array(b)[0] = i; } return 1; })()",
        "(() => { for (let i=0; i<40; ++i) { const b = new SharedArrayBuffer(4*1024*1024); new Uint8Array(b)[0] = i; } return 1; })()",
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
        ("new SharedArrayBuffer(1048576)", true),
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
