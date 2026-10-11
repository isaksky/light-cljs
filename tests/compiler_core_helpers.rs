//! Core helpers checked with literal host outcomes and independent runtime tests.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, analyze_with_macros, compile, emit_js, parse};
use std::{path::PathBuf, process::Command};

#[test]
fn helper_arities_order_lazy_tails_and_collections_match_expected_values() {
    for line in include_str!("fixtures/compiler-core-helpers.tsv").lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let javascript =
            compile(fields[1]).unwrap_or_else(|error| panic!("{}: {error}", fields[0]));
        let output = Command::new("node")
            .args([
                "--max-old-space-size=96",
                "-e",
                &format!("console.log(JSON.stringify({javascript}));"),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            fields[0],
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            fields[2],
            "{}",
            fields[0]
        );
    }
}

#[test]
fn helper_callbacks_and_owned_runtime_values_follow_the_callable_abi() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/js_core_helpers.test.js");
    let output = Command::new("node").arg(path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn applied_partial_and_juxt_constructors_preserve_lazy_tails_in_both_phases() {
    let cases = [
        (
            r#"
            (def hits (atom []))
            (defn probe []
              (letfn [(ints [n] (lazy-seq (swap! hits conj n) (cons n (ints (inc n)))))
                      (functions [n] (lazy-seq (swap! hits conj (+ 100 n))
                                      (cons (fn [] n) (functions (inc n)))))]
                [(fn? (apply partial vector (ints 0)))
                 (fn? (apply juxt (functions 0))) @hits]))
            (defmacro snapshot [] (probe))
            [(snapshot) @hits (probe) @hits]
            "#,
            "[[true true [0 1 2 3 4 100 101 102 103 104]] [] [true true [0 1 2 3 4 100 101 102 103 104]] [0 1 2 3 4 100 101 102 103 104]]",
        ),
        (
            r#"
            (defn probe []
              (let [hits (atom [])]
                (letfn [(ints [n] (lazy-seq (when (< n 8)
                                   (swap! hits conj n) (cons n (ints (inc n))))))]
                  (let [f (apply partial vector (ints 0)) before @hits
                        a (f 9) b (apply f [10 11])]
                    [before a b @hits]))))
            (defmacro snapshot [] (probe))
            [(snapshot) (probe)]
            "#,
            "[[[0 1 2 3 4] [0 1 2 3 4 5 6 7 9] [0 1 2 3 4 5 6 7 10 11] [0 1 2 3 4 5 6 7]] [[0 1 2 3 4] [0 1 2 3 4 5 6 7 9] [0 1 2 3 4 5 6 7 10 11] [0 1 2 3 4 5 6 7]]]",
        ),
    ];
    let mut host = MacroHost::new();
    for _ in 0..2 {
        for (source, expected) in cases {
            let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let value = evaluator.evaluate(&program).unwrap();
            assert_eq!(evaluator.print(value).unwrap(), expected, "QuickJS");
            let output = Command::new("node")
                .args([
                    "-e",
                    &format!("console.log(String({}));", emit_js(&program)),
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
                "Node"
            );
        }
    }
}
