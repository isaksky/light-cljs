//! Public positional construction in both canonical execution phases.
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
fn core_factories_execute_in_applications_and_macros() {
    for row in concat!(
        include_str!("../fixtures/compiler-core-factories.tsv"),
        include_str!("../fixtures/compiler-core-factory-bodies.tsv")
    )
    .lines()
    .filter(|r| !r.starts_with('#') && !r.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        eprintln!("{}", fields[0]);
        both(fields[1], fields[2]);
        both(
            &format!("(defmacro witness [] {}) (witness)", fields[1]),
            fields[2],
        );
    }
}

#[test]
fn factory_phase_isolation_diagnostics_and_bounded_repetition() {
    both(
        r#"(def counter (atom 0))
      (defn witness [] (let [n (swap! counter inc) e (->Eduction (map inc) [n]) u (apply ->UUID ["ABC" n])]
        [(vec e) (str u) (hash u)]))
      (defmacro phase [] (witness))
      [(phase) @counter (witness) @counter (phase) @counter]"#,
        "[[[2] \"ABC\" 1] 0 [[2] \"ABC\" 1] 1 [[3] \"ABC\" 2] 1]",
    );
    both(
        r#"(loop [n 1000 total 0] (if (zero? n) total
      (let [x (->IndexedSeq #js [1 2 3] 1 nil) r (apply ->IntegerRange [nil 0 3 1 3 nil])]
        (recur (dec n) (+ total (reduce + x) (reduce + r))))))"#,
        "8000",
    );
    let mut host = MacroHost::new();
    for expression in [
        "->UUID",
        "->Range",
        "->MultiFn",
        "(->SeqIter INIT (list 1))",
        "(->RangedIterator 0 0 #js [1] [1] 0 1)",
    ] {
        let source = format!("(defmacro bad [] {expression}) (bad)");
        let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Unsupported macro result") && error.contains("bad"),
            "{error}"
        );
        let source = "(defmacro good [] (str (->UUID \"ABC\" nil))) (good)";
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            evaluator
                .print(evaluator.evaluate(&program).unwrap())
                .unwrap(),
            "\"ABC\""
        );
    }
}
