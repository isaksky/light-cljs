//! Number semantics and phase isolation without external Closure initialization.
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
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
}

#[test]
fn predicates_keep_number_boundaries_once_only_effects_and_phase_isolation() {
    let mut host = MacroHost::new();
    for _ in 0..2 {
        both(
            &mut host,
            r#"
        (def hits (atom []))
        (defn counted [x] (swap! hits conj x) x)
        (defn row [x] [(integer? x) (int? x) (pos-int? x) (neg-int? x) (nat-int? x)])
        (defmacro phase [] (list 'quote [(row (counted -0.0)) @hits]))
        [(phase) @hits (row (counted -1)) @hits
         (mapv row [1.5 js/NaN js/Infinity (- js/Infinity) 1e21 nil {} (js* "({constructor:{name:'Long'},isNegative(){throw Error('spoof')},isZero(){throw Error('spoof')}})")])]
        "#,
            "[[[true true false false true] [-0.0]] [] [true true false true false] [-1] [[false false false false false] [false false false false false] [false false false false false] [false false false false false] [false false false false false] [false false false false false] [false false false false false] [false false false false false]]]",
        );
    }
}

#[test]
fn predicate_receiver_errors_leave_macro_host_reusable() {
    let mut host = MacroHost::new();
    for name in ["int?", "pos-int?", "neg-int?", "nat-int?"] {
        let source =
            format!("(defmacro bad [] ({name} (throw (js/Error. \"integer recovery\")))) (bad)");
        let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host).unwrap_err();
        assert!(error.to_string().contains("integer recovery"), "{error}");
        assert!(error.span.end > error.span.start);
        both(
            &mut host,
            "(defmacro recovered [] (nat-int? 0)) [(recovered) (int? 1)]",
            "[true true]",
        );
    }
}
