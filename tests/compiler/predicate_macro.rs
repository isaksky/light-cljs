//! Public JavaScript function predicate preserves macro and application phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn both(host: &mut MacroHost, source: &str, expected: &str) {
    let program = analyze_with_macros(source, &parse(source).unwrap(), host).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .print(evaluator.evaluate(&program).unwrap())
            .unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let output = Command::new("node")
        .args([
            "-e",
            &format!(
                "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
                emit_js_unit(&program)
            ),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
}

#[test]
fn function_predicate_preserves_single_evaluation_and_separate_phase_roots() {
    let mut host = MacroHost::new();
    both(
        &mut host,
        r#"
        (def hits (atom []))
        (defn counted [] (swap! hits conj :called) (fn [] nil))
        (defmacro phase [] (js-fn? (counted)))
        [(phase) @hits (js-fn? (counted)) @hits (phase) @hits]
    "#,
        "[true [] true [:called] true [:called]]",
    );
    both(
        &mut host,
        r#"
        (defn plain [x] x)
        (let [m (with-meta plain {:tag :wrapped})]
          [(cljs.core/js-fn? js/Math.max) (js-fn? @#'plain)
           (js-fn? #'plain) (js-fn? m) (fn? m) (ifn? m) (m 3)])
    "#,
        "[true true false false true true 3]",
    );
}

#[test]
fn function_predicate_rejects_wrong_macro_arities_and_recovers() {
    let mut host = MacroHost::new();
    for source in [
        "(js-fn?)",
        "(js-fn? nil nil)",
        "(defmacro bad [] (js-fn?)) (bad)",
    ] {
        let error = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap_err();
        assert!(
            error.to_string().contains("js-fn? requires one argument"),
            "{error}"
        );
        assert!(error.span.end > error.span.start, "{error}");
        both(
            &mut host,
            "(defmacro good [] (js-fn? (fn [] nil))) (good)",
            "true",
        );
    }
}
