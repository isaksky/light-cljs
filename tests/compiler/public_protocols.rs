//! Public protocol identities and dispatch in both canonical execution phases.
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
fn public_protocol_api_arities_and_macro_phase_execute() {
    for row in include_str!("../fixtures/compiler-public-protocols.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        both(fields[1], fields[2]);
        // Macro values are persistent booleans; intermediate types, markers and
        // saved protocol methods execute in the canonical macro runtime.
        both(
            &format!("(defmacro witness [] {}) (witness)", fields[1]),
            fields[2],
        );
    }
    for row in include_str!("../fixtures/compiler-public-protocol-bodies.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        both(fields[1], fields[2]);
        both(
            &format!("(defmacro upstream [] {}) (upstream)", fields[1]),
            fields[2],
        );
    }
    both(
        r#"(let [vs [#'Inst #'IUUID #'ASeq #'IDerefWithTimeout #'inst-ms* #'-deref-with-timeout]]
      (assert (= '[Inst IUUID ASeq IDerefWithTimeout inst-ms* -deref-with-timeout] (mapv #(-> % meta :name) vs)))
      (assert (every? #(= 'cljs.core (-> % meta :ns)) vs))
      (assert (every? #(number? (-> % meta :line)) vs))
      (assert (= '([inst]) (:arglists (meta #'inst-ms*))))
      (assert (= '([o msec timeout-val]) (:arglists (meta #'-deref-with-timeout))))
      (assert (identical? @#'Inst Inst)) true)"#,
        "true",
    );
    both(
        r#"
      (def counter (atom 0))
      (defn witness []
        (let [x (reify Inst (inst-ms* [_] (swap! counter inc)))] (inst-ms x)))
      (defmacro phase [] (witness))
      [(phase) @counter (witness) @counter (phase) @counter]
    "#,
        "[1 0 1 1 2 1]",
    );
}

#[test]
fn unsupported_protocol_values_keep_expansion_diagnostics_and_recover() {
    let mut host = MacroHost::new();
    for expression in [
        "(reify IUUID)",
        "(reify Inst (inst-ms* [_] 42))",
        "Inst",
        "IDerefWithTimeout",
    ] {
        let source = format!("(defmacro bad [] {expression}) (bad)");
        let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Unsupported macro result") && error.contains("bad"),
            "{error}"
        );
        let source = "(defmacro good [] (inst-ms #inst \"1970-01-01T00:00:00.042Z\")) (good)";
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
fn public_protocol_invalid_implementations_are_located() {
    for source in [
        "(reify Inst (inst-ms* [_ n] n))",
        "(reify IDerefWithTimeout (-deref-with-timeout [_ t] t))",
        "(reify ASeq (-first [_] 1))",
        "(reify IUUID (uuid? [_] true))",
    ] {
        let error = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
            .unwrap_err();
        assert!(error.span.end > error.span.start);
        assert!(error.to_string().contains("protocol"), "{error}");
    }
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn public_protocol_bodies() {
        run(&[
            Case::expression(
                "test-preds",
                include_str!("../fixtures/phase-oracles/public-protocols/test-preds.cljs"),
            ),
            Case::expression(
                "protocol-public",
                include_str!("../fixtures/phase-oracles/public-protocols/protocol-public.cljs"),
            ),
            Case::expression(
                "protocol-custom-state",
                include_str!(
                    "../fixtures/phase-oracles/public-protocols/protocol-custom-state.cljs"
                ),
            ),
            Case::expression(
                "clone-find-protocol-workflow",
                include_str!(
                    "../fixtures/phase-oracles/public-protocols/clone-find-protocol-workflow.cljs"
                ),
            ),
        ]);
    }
}
