//! Public prim-seq fixed arities execute in both isolated compiler phases.
use light_cljs::{
    analyze_with_macros,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE},
    parse, MacroHost,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn both(host: &mut MacroHost, source: &str, expected: &str) {
    let program = analyze_with_macros(source, &parse(source).unwrap(), host)
        .unwrap_or_else(|error| panic!("{source}: {error}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .print(evaluator.evaluate(&program).unwrap())
            .unwrap(),
        expected
    );
    let mut child = Command::new("node")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        child.stdin.take().unwrap(),
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&program)
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
}

#[test]
fn public_prim_seq_runs_full_literal_contract_in_applications_and_macros() {
    let contract = include_str!("fixtures/compiler-prim-seq.cljs");
    let mut host = MacroHost::new();
    both(
        &mut host,
        &format!("{contract} (prim-seq-contract)"),
        "true",
    );
    both(
        &mut host,
        &format!("{contract} (defmacro witness [] (prim-seq-contract)) (witness)"),
        "true",
    );
    both(
        &mut MacroHost::new(),
        &format!("{contract} (defmacro fresh [] (prim-seq-contract)) (fresh)"),
        "true",
    );
}

#[test]
fn prim_seq_preserves_original_probe_and_separate_phase_roots() {
    let mut host = MacroHost::new();
    let literal = r#"[(vec (prim-seq #js [4 5])) (vec (prim-seq #js [4 5] 1)) (prim-seq #js []) (prim-seq #js [4 5] 2) (vec (prim-seq "ab")) (vec (prim-seq "ab" 1))]"#;
    both(&mut host, literal, r#"[[4 5] [5] nil nil ["a" "b"] ["b"]]"#);
    both(
        &mut host,
        &format!("(defmacro prim-probe [] (list (quote quote) {literal})) (prim-probe)"),
        r#"[[4 5] [5] nil nil ["a" "b"] ["b"]]"#,
    );
    both(&mut host, r#"(def events (atom []))
      (defn probe [] (vec (prim-seq (do (swap! events conj :input) #js [4 5])
                                  (do (swap! events conj :start) 1))))
      (defmacro phase [] (list 'quote [(probe) @events]))
      [(phase) @events (probe) @events (phase) @events]"#,
      "[[[5] [:input :start]] [] [5] [:input :start] [[5] [:input :start :input :start]] [:input :start]]");
}

#[test]
fn prim_seq_arity_and_result_domain_failures_are_located_and_recover() {
    let mut host = MacroHost::new();
    for source in [
        "\n(prim-seq)",
        "\n(cljs.core/prim-seq #js [] 0 1)",
        "(defmacro bad [] (prim-seq))\n(bad)",
        "(defmacro bad [] (apply prim-seq []))\n(bad)",
    ] {
        let error = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap_err();
        assert!(
            error.to_string().to_lowercase().contains("invalid arity"),
            "{error}"
        );
        assert!(
            error.span.start < error.span.end && error.span.end <= source.len(),
            "{error}"
        );
        both(
            &mut host,
            "(defmacro recovered [] (first (prim-seq #js [42]))) (recovered)",
            "42",
        );
    }
    both(
        &mut host,
        r#"(let [f prim-seq] [(try (f) (catch :default e (.-message e)))
        (try (apply f [#js [] 0 1]) (catch :default e (.-message e)))])"#,
        r#"["Invalid arity: 0" "Invalid arity: 3"]"#,
    );
    for body in [
        "prim-seq",
        "(first (prim-seq #js [(js-obj)]))",
        "(prim-seq #js [(js-obj)])",
    ] {
        let source = format!("(defmacro unsupported [] {body})\n(unsupported)");
        let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host).unwrap_err();
        assert!(
            error.to_string().contains("Unsupported macro result"),
            "{error}"
        );
        assert!(error.span.start >= source.find('\n').unwrap(), "{error}");
        both(
            &mut host,
            "(defmacro recovered [] (first (prim-seq #js [42]))) (recovered)",
            "42",
        );
    }
}
