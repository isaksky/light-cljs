//! Public iterator helpers retain canonical iterator state in both phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn both(source: &str, expected: &str) {
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
        .unwrap_or_else(|e| panic!("{source}: {e}"));
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
fn iterator_helpers_execute_in_applications_and_macros() {
    let contract = include_str!("fixtures/compiler-iterator-helpers.cljs");
    both(&format!("{contract} (iterator-helper-contract)"), "true");
    both(
        &format!("{contract} (defmacro witness [] (iterator-helper-contract)) (witness)"),
        "true",
    );
    // Retain the complete pinned helper/record/iterator-equivalence body.
    for row in include_str!("fixtures/compiler-core-factory-bodies.tsv")
        .lines()
        .filter(|r| {
            r.starts_with(
                "core-factory-body-src-test-cljs-cljs-iterator-test-coll-iter-seq-match\t",
            )
        })
    {
        let fields: Vec<_> = row.split('\t').collect();
        both(fields[1], fields[2]);
        both(
            &format!("(defmacro witness [] {}) (witness)", fields[1]),
            fields[2],
        );
    }
}

#[test]
fn iterator_helper_phase_isolation_diagnostics_and_repetition() {
    both(
        r#"(def counter (atom 0))
      (defn inspect [] (let [i (seq-iter (lazy-seq (list (swap! counter inc))))
                            r (apply ranged-iterator [[10 20] 1 2])]
        [(.next i) (.next r)]))
      (defmacro phase [] (inspect))
      [(phase) @counter (inspect) @counter (phase) @counter]"#,
        "[[1 20] 0 [1 20] 1 [2 20] 1]",
    );
    both(
        r#"(loop [n 1000 total 0] (if (zero? n) total
      (let [i (seq-iter [1 2]) r (ranged-iterator [3 4 5] 1 3)]
        (recur (dec n) (+ total (.next i) (.next i) (.next r) (.next r))))))"#,
        "12000",
    );
    let mut host = MacroHost::new();
    for expression in [
        "seq-iter",
        "ranged-iterator",
        "(seq-iter [1])",
        "(ranged-iterator [1] 0 1)",
    ] {
        let source = format!("(defmacro bad [] {expression}) (bad)");
        let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Unsupported macro result") && error.contains("bad"),
            "{error}"
        );
        let source = "(defmacro good [] (.next (seq-iter [42]))) (good)";
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            evaluator
                .print(evaluator.evaluate(&program).unwrap())
                .unwrap(),
            "42"
        );
    }
}

#[test]
fn iterator_helpers_preserve_session_owned_progress() {
    use light_cljs::{EvalOptions, EvalSession};
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    for (source, expected) in [
        (
            "(def s (seq-iter [1 2])) (def r (ranged-iterator [3 4 5] 1 3)) [(.next s) (.next r)]",
            "[1 4]",
        ),
        (
            "[(.hasNext s) (.hasNext r) (.next s) (.next r)]",
            "[true true 2 5]",
        ),
        ("[(.hasNext s) (.hasNext r)]", "[false false]"),
    ] {
        let value = session.eval_string(source).unwrap().value;
        assert_eq!(session.print(value).unwrap(), expected);
        session.release(value).unwrap();
    }
    let mut fresh = session.fresh_session().unwrap();
    let value = fresh
        .eval_string("[(.next (seq-iter [1 2])) (.next (apply ranged-iterator [[3 4 5] 1 3]))]")
        .unwrap()
        .value;
    assert_eq!(fresh.print(value).unwrap(), "[1 4]");
    fresh.release(value).unwrap();
}
