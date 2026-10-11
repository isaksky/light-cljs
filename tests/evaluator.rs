use light_cljs::{
    MacroHost, analyze,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions},
    parse,
};
use std::{
    process::Command,
    time::{Duration, Instant},
};

fn program(source: &str) -> light_cljs::Program {
    analyze(source, &parse(source).unwrap()).unwrap()
}

#[test]
fn compiler_units_execute_with_identical_expected_results_in_quickjs_and_node() {
    let cases = [
        ("(+ 20 22)", "42"),
        (
            "[{:a 1} #{2 3} (apply (fn [a b] (+ a b)) [4 5])]",
            "[{:a 1} #{2 3} 9]",
        ),
        ("(let [a (atom 1)] (swap! a inc) @a)", "2"),
        (
            "(try (throw (js/Error. \"boom\")) (catch :default e (.-message e)))",
            "\"boom\"",
        ),
        ("(vec (take 4 (map inc (range))))", "[1 2 3 4]"),
        (
            "(defprotocol P (f [this])) (deftype T [x] P (f [_] x)) (f (T. 42))",
            "42",
        ),
        ("(defrecord R [x]) (:x (->R 42))", "42"),
        (
            "[(js/Math.sqrt 81) (.toUpperCase \"hi\") (.test (js/RegExp. \"a+\") \"aaa\") (aget (js/JSON.parse \"{\\\"a\\\":7}\") \"a\") (alength (js/Object.keys #js {:a 1}))]",
            "[9 \"HI\" true 7 1]",
        ),
        ("(if false (js/process.exit 1) 42)", "42"),
    ];
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    for (source, expected) in cases {
        let p = program(source);
        let id = evaluator.evaluate(&p).unwrap();
        assert_eq!(evaluator.print(id).unwrap(), expected, "QuickJS: {source}");
        let script = format!(
            "const rt = {}; console.log(rt.core.prStr(({})(rt)));",
            light_cljs::evaluator::RUNTIME_SOURCE,
            emit_js_unit(&p)
        );
        let out = Command::new("node").args(["-e", &script]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8(out.stdout).unwrap().trim(),
            expected,
            "Node: {source}"
        );
    }
}
#[test]
fn units_share_brands_and_functions_but_sessions_reject_foreign_ids() {
    let a = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let b = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let vector = a.evaluate(&program("[40 2]")).unwrap();
    let sum = a.evaluate(&program("(fn [v] (apply + v))")).unwrap();
    let result = a.call(sum, &[vector]).unwrap();
    assert_eq!(a.number(result).unwrap(), 42.0);
    assert!(b.number(result).unwrap_err().0.contains("another"));
    assert!(b.call(sum, &[]).unwrap_err().0.contains("another"));
    assert_eq!(b.number(b.evaluate_js("42").unwrap()).unwrap(), 42.0);
    a.release(vector).unwrap();
    assert!(a.print(vector).unwrap_err().0.contains("Released"));
}
#[test]
fn javascript_capabilities_are_explicit_and_demanded() {
    for name in ["document", "process", "fetch", "require"] {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert!(e.evaluate_js(name).unwrap_err().0.contains(name));
    }
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    e.install_callback("hostEcho", |control, text| {
        control.poll()?;
        Ok(format!("hello {text}"))
    })
    .unwrap();
    assert_eq!(
        e.string(e.evaluate_js("hostEcho('world')").unwrap())
            .unwrap(),
        "hello world"
    );
}
#[test]
fn terminal_controls_survive_catch_and_fresh_host_sessions_recover() {
    let mut host = MacroHost::new();
    host.evaluator_options.poll_budget = 1_000;
    let e = host.new_evaluator().unwrap();
    assert!(
        e.evaluate_js("(()=>{try { while(true) {} } catch(e) {} return 42;})()")
            .unwrap_err()
            .0
            .contains("budget")
    );
    assert!(e.evaluate_js("42").is_err());
    let fresh = host.new_evaluator().unwrap();
    assert_eq!(
        fresh.number(fresh.evaluate_js("42").unwrap()).unwrap(),
        42.0
    );
    let e = host.new_evaluator().unwrap();
    e.install_callback("cancelBridge", |control, _| {
        control.cancel();
        control.poll()?;
        Ok("ignored".into())
    })
    .unwrap();
    assert!(
        e.evaluate_js("(()=>{try {cancelBridge('')} catch(e) {} return 42})()")
            .unwrap_err()
            .0
            .contains("cancelled")
    );
    let e = host.new_evaluator().unwrap();
    e.control().cancel();
    assert!(
        e.evaluate(&program("42"))
            .unwrap_err()
            .0
            .contains("cancelled")
    );
}
#[test]
fn deadline_allocation_and_panics_produce_owned_failures() {
    let e = Evaluator::new(EvaluatorOptions {
        timeout: Duration::from_millis(100),
        ..Default::default()
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(110));
    assert!(e.evaluate_js("42").unwrap_err().0.contains("deadline"));
    let e = Evaluator::new(EvaluatorOptions {
        memory_limit: 4 * 1024 * 1024,
        ..Default::default()
    })
    .unwrap();
    assert!(e.evaluate_js("(()=>{try { Array.from({length: 1000000}, (_, i) => ({i})); } catch(e) {} return 42;})()").unwrap_err().0.contains("memory limit"));
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    e.install_callback("panicBridge", |_, _| panic!("test panic containment"))
        .unwrap();
    assert!(
        e.evaluate_js("(()=>{try{panicBridge('')}catch(e){} return 1})()")
            .is_err()
    );
}
#[test]
fn clean_sessions_drop_cycles_callbacks_and_mutable_roots() {
    let host = MacroHost::new();
    let mut baseline = None;
    for _ in 0..30 {
        let e = host.new_evaluator().unwrap();
        let bytes = e.memory_usage_bytes();
        if let Some(baseline) = baseline {
            assert_eq!(bytes, baseline);
        } else {
            baseline = Some(bytes);
        }
        e.install_callback("echo", |_, text| Ok(text)).unwrap();
        assert_eq!(
            e.number(
                e.evaluate_js("globalThis.counter = (globalThis.counter || 0) + 1")
                    .unwrap()
            )
            .unwrap(),
            1.0
        );
        e.evaluate_js("(()=>{const o={callback:echo};o.self=o;return o})()")
            .unwrap();
    }
}
#[test]
#[ignore = "focused startup/retention measurement; run explicitly in release mode"]
fn startup_measurement() {
    let start = Instant::now();
    let mut bytes = 0;
    for _ in 0..100 {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        bytes = e.memory_usage_bytes();
        assert_eq!(e.number(e.evaluate_js("42").unwrap()).unwrap(), 42.0);
    }
    println!(
        "100 fresh sessions: {:?}; canonical heap bytes: {bytes}; runtime source bytes: {}",
        start.elapsed(),
        light_cljs::evaluator::RUNTIME_SOURCE.len()
    );
}

#[test]
fn teardown_does_not_invoke_mutated_collection_methods() {
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    e.evaluate_js("Map.prototype.clear = () => { while (true) {} }")
        .unwrap();
    drop(e);
}

#[test]
fn unit_runtime_parameter_preserves_raw_javascript_hygiene() {
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let p = program("[(js* \"typeof __lightSessionRuntime0\") 42]");
    assert_eq!(
        e.print(e.evaluate(&p).unwrap()).unwrap(),
        "[\"undefined\" 42]"
    );
}
