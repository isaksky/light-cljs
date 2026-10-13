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

const COLLECTION_SOURCE: &str = "[{:a 1} #{2 3} (apply (fn [a b] (+ a b)) [4 5])]";

#[test]
fn compiler_units_execute_with_identical_expected_results_in_quickjs_and_node() {
    let cases = [
        ("(+ 20 22)", "42"),
        (COLLECTION_SOURCE, "[{:a 1} #{3 2} 9]"),
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
fn complete_collection_fixture_preserves_typed_values_and_printing_in_both_engines() {
    // Both pinned references establish the complete value and exact set order.
    let p = program(COLLECTION_SOURCE);
    let observer = include_str!("support/evaluator_collection_observation.js");
    let expected = include_str!("fixtures/evaluator-collection-observation.json").trim();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator.evaluate(&p).unwrap();
    let observe = evaluator
        .evaluate_js(&format!(
            "value => ({observer})(rt.inspect, rt.core.prStr, value)"
        ))
        .unwrap();
    let observation = evaluator.call(observe, &[value]).unwrap();
    assert_eq!(evaluator.string(observation).unwrap(), expected, "QuickJS");
    let script = format!(
        "const rt = {}; console.log(({})(rt.inspect, rt.core.prStr, ({})(rt)));",
        light_cljs::evaluator::RUNTIME_SOURCE,
        observer,
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
        "Node"
    );
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

#[test]
fn wide_radix_literals_round_once_in_applications_and_macro_arguments() {
    // Exact decimal values independently converted to f64. These nondecimal
    // spellings exceed u128 and expose per-digit Number accumulation errors.
    let cases = [
        (
            "2r1100010101010010000101100110010011110011101000111100010101110001011101000111011000011000100110010111010110100011101011011011001100100101010010101001010010010011",
            "1126501793780955840178000892951443180509553398931",
        ),
        (
            "0171163550502456051221373553237567242501240341420455477",
            "1384156059891590194823490597700216239803630508863",
        ),
        (
            "0x2911675d60067d39d169a3f8cad5e297e6b1e665",
            "234456738611503117298165641691112104301020964453",
        ),
        (
            "36r5eh0k9j7aajt0etk7t6w4mlhhztx1e0",
            "264016499809241824936904025929579949326130642504",
        ),
    ];
    for (literal, decimal) in cases {
        for sign in ["", "-"] {
            let source = format!("{sign}{literal}");
            let expected: f64 = format!("{sign}{decimal}").parse().unwrap();
            let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
            assert_eq!(
                e.number(e.evaluate(&program(&source)).unwrap())
                    .unwrap()
                    .to_bits(),
                expected.to_bits(),
                "application: {source}"
            );
            let mut host = MacroHost::new();
            let macro_source = format!("(defmacro literal [x] x) (literal {source})");
            let p = light_cljs::analyze_with_macros(
                &macro_source,
                &parse(&macro_source).unwrap(),
                &mut host,
            )
            .unwrap();
            assert_eq!(
                e.number(e.evaluate(&p).unwrap()).unwrap().to_bits(),
                expected.to_bits(),
                "macro: {source}"
            );
            let script = format!(
                "console.log(Object.is({}, ({})( {} )));",
                expected,
                emit_js_unit(&p),
                light_cljs::evaluator::RUNTIME_SOURCE
            );
            let out = Command::new("node").args(["-e", &script]).output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                String::from_utf8(out.stdout).unwrap().trim(),
                "true",
                "Node: {source}"
            );
        }
    }
}

#[test]
fn quoted_javascript_macro_literals_execute_in_fresh_quickjs_and_node_heaps() {
    let cases = [
        ("(defmacro m [] '#js []) (alength (m))", "0"),
        // Keep the original failure path through the generated let binding.
        ("(defmacro m [] '(let [out #js []] out)) (alength (m))", "0"),
        (
            r#"(defmacro m [] '(let [out #js [#js {:x #js [42]}]] out))
            (aget (.-x (aget (m) 0)) 0)"#,
            "42",
        ),
        (
            r#"(defmacro m [] '#js {:x #js [42]}) (aget (.-x (m)) 0)"#,
            "42",
        ),
        (
            r#"(defmacro m [] `[ #js [#js {:x #js [42]}] #js {:y 7}])
            (let [out (m)] [(aget (.-x (aget (nth out 0) 0)) 0) (.-y (nth out 1))])"#,
            "[42 7]",
        ),
        (
            r#"(defmacro m [] '(quote ^:kept [#js [unresolved] #js {:x (unresolved)}]))
            (let [out (m)] [(:kept (meta out)) (name (aget (nth out 0) 0))
                            (name (first (.-x (nth out 1))))])"#,
            "[true \"unresolved\" \"unresolved\"]",
        ),
    ];
    let mut host = MacroHost::new();
    for _ in 0..2 {
        for (source, expected) in cases {
            let p = light_cljs::analyze_with_macros(source, &parse(source).unwrap(), &mut host)
                .unwrap();
            let evaluator = host.new_evaluator().unwrap();
            assert_eq!(
                evaluator.print(evaluator.evaluate(&p).unwrap()).unwrap(),
                expected,
                "QuickJS: {source}"
            );
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
}
