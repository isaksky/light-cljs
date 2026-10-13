//! Canonical declaration values, with complete typed packets in both engines/phases.
use light_cljs::{
    MacroHost,
    evaluator::{Evaluator, EvaluatorOptions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn node(script: &str) -> String {
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("console.log({script});").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}
fn host() -> MacroHost {
    let mut host = MacroHost::new();
    host.evaluator_options.timeout = std::time::Duration::from_secs(120);
    host
}

#[test]
fn complete_declaration_sources_execute_in_fresh_and_reused_hosts_and_both_phases() {
    let mut reused = host();
    let evaluator = Evaluator::new(EvaluatorOptions {
        timeout: std::time::Duration::from_secs(120),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    let adapter = include_str!("../support/sci_execution_observation.js");
    let mut node_batch = String::new();
    let mut expected_batch = Vec::new();
    for cycle in 0..2 {
        for row in include_str!("../fixtures/declare-values.tsv").lines() {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), 3);
            let (id, source, canonical_expected) = (fields[0], fields[1], fields[2]);
            for phase in ["application", "macro"] {
                // Reading an uninitialized macro root is a deliberate demand error.
                // Keep this phase outcome distinct from the canonical application.
                let expected = if phase == "macro" && id == "declare191-forward" {
                    r#"{"id":"declare191-forward","outcome":{"status":"threw","error":{"category":"error"}},"trace":[],"stdout":""}"#
                } else {
                    canonical_expected
                };
                for session in ["fresh", "reused"] {
                    let mut fresh = host();
                    let host = if session == "fresh" {
                        &mut fresh
                    } else {
                        &mut reused
                    };
                    let script = if phase == "application" {
                        let wrapped = format!("(fn* [observe] {source})");
                        let tree = light_cljs::parse(&wrapped).unwrap();
                        let program =
                            light_cljs::analyze_with_macros(&wrapped, &tree, host).unwrap();
                        let unit = light_cljs::emitter::emit_js_unit(&program);
                        format!(
                            "(() => {{ const rt = globalThis.declareProofRuntime ||= {}; const run = ({unit})(rt); return ({adapter})(rt)({id:?}, run); }})()",
                            light_cljs::evaluator::RUNTIME_SOURCE
                        )
                    } else {
                        // The macro really demands run, including every original source byte.
                        // Transport the typed packet as a finite string so an undefined
                        // callback result cannot be normalized by expansion serialization.
                        let js = format!("(({adapter})(rt))({id:?}, ~{{}})");
                        let computation = format!(
                            "(defn run [observe] {source}) (defmacro witness [] (js* {js:?} run)) (witness)"
                        );
                        let emitted = light_cljs::compile_with_macros(&computation, host).unwrap();
                        format!("eval({emitted:?})")
                    };
                    let actual = evaluator
                        .string(evaluator.evaluate_js(&script).unwrap())
                        .unwrap();
                    assert_eq!(actual, expected, "{id} {phase} {session} {cycle}");
                    let fresh_engine = Evaluator::new(EvaluatorOptions {
                        timeout: std::time::Duration::from_secs(120),
                        ..EvaluatorOptions::default()
                    })
                    .unwrap();
                    assert_eq!(
                        fresh_engine
                            .string(fresh_engine.evaluate_js(&script).unwrap())
                            .unwrap(),
                        expected
                    );
                    assert_eq!(node(&script), expected, "Node {id} {phase} {session}");
                    node_batch.push_str(&format!("console.log({script});\n"));
                    expected_batch.push(expected.to_owned());
                    println!("declare-value-packet\t{cycle}\t{phase}\t{session}\t{id}\t{actual}");
                }
            }
        }
    }
    // One Node process also executes all application generations and macro results.
    assert_eq!(
        node(&format!("(() => {{ {node_batch} return 'done'; }})()")),
        format!("{}\ndone", expected_batch.join("\n"))
    );
}

#[test]
fn declaration_demand_failures_ownership_and_generations_recover() {
    let mut host = host();
    for _ in 0..2 {
        for (source, diagnostic) in [
            (
                "(defn run [] (let [d (declare missing other)] missing)) (defmacro m [] (run)) (m)",
                "unavailable definition: cljs.user/missing",
            ),
            (
                "(let [d (declare foreign/x)] d)",
                "binding must be an unqualified symbol",
            ),
            (
                "(let [d (declare 1)] d)",
                "binding must be an unqualified symbol",
            ),
            (
                "(declare missing) (defmacro m [] missing) (m)",
                "unavailable definition: cljs.user/missing",
            ),
            (
                "(defn run [observe] (declare x y) (defn f [] x) (def x 42) [(var? (declare x)) (f) (nil? y)]) (defmacro m [] (run identity)) (m)",
                "unavailable definition: cljs.user/y",
            ),
        ] {
            let error = light_cljs::compile_with_macros(source, &mut host)
                .unwrap_err()
                .to_string();
            assert!(error.contains(diagnostic), "{error}");
        }
        let source = r#"
(def effects (atom []))
(def pending (do (swap! effects conj :pending) 42))
(def unrelated (do (swap! effects conj :unrelated) 99))
(defn install [] (let [d (declare pending spare)] [(undefined? d) @effects]))
(defmacro first-witness [] (list 'quote [(install) @effects pending @effects]))
(first-witness)
(def pending 43)
(defmacro next-witness [] (list 'quote [(install) pending @effects]))
(pr-str [(next-witness) pending @effects])
"#;
        let js = light_cljs::compile_with_macros(source, &mut host).unwrap();
        assert_eq!(
            node(&format!("eval({js:?})")),
            "[[[true [:pending]] 43 [:pending]] 43 [:pending :unrelated]]"
        );
        let evaluator = Evaluator::new(EvaluatorOptions {
            timeout: std::time::Duration::from_secs(120),
            ..EvaluatorOptions::default()
        })
        .unwrap();
        assert_eq!(
            evaluator
                .string(evaluator.evaluate_js(&js).unwrap())
                .unwrap(),
            "[[[true [:pending]] 43 [:pending]] 43 [:pending :unrelated]]"
        );
    }
    // Unlike the pins' malformed single-binding JavaScript, the production
    // definition expression supplies the same undefined value in a direct let.
    let direct =
        light_cljs::compile("(pr-str (let [d (declare single)] [(undefined? d) (nil? d)]))")
            .unwrap();
    assert_eq!(node(&format!("eval({direct:?})")), "[true true]");
    let direct_engine = Evaluator::new(EvaluatorOptions {
        timeout: std::time::Duration::from_secs(120),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    assert_eq!(
        direct_engine
            .string(direct_engine.evaluate_js(&direct).unwrap())
            .unwrap(),
        "[true true]"
    );

    // Current-namespace qualified declarations retain the established local ownership contract.
    let js = light_cljs::compile(
        "(let [d (declare cljs.user/x cljs.user/y)] (def x 7) (pr-str [(undefined? d) x]))",
    )
    .unwrap();
    assert_eq!(node(&format!("eval({js:?})")), "[true 7]");
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn namespace_declarations() {
        run(&[
            Case::observed_during_expansion(
                "declare-original",
                include_str!("../fixtures/phase-oracles/declare-values/declare-original.cljs"),
                r#"{"id":"declare-original","outcome":{"status":"returned","value":{"$vector":[false,null,null]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-single",
                include_str!("../fixtures/phase-oracles/declare-values/declare-single.cljs"),
                r#"{"id":"declare-single","outcome":{"status":"returned","value":{"$vector":[true,true,false,false]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-multiple",
                include_str!("../fixtures/phase-oracles/declare-values/declare-multiple.cljs"),
                r#"{"id":"declare-multiple","outcome":{"status":"returned","value":{"$vector":[true,true,false,false]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-zero",
                include_str!("../fixtures/phase-oracles/declare-values/declare-zero.cljs"),
                r#"{"id":"declare-zero","outcome":{"status":"returned","value":{"$vector":[false,true,false,true]}},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-nested",
                include_str!("../fixtures/phase-oracles/declare-values/declare-nested.cljs"),
                r#"{"id":"declare-nested","outcome":{"status":"returned","value":{"$vector":[true,true,true,null]}},"trace":[{"$keyword":{"namespace":null,"name":"before","spelling":"before"}}],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-callback",
                include_str!("../fixtures/phase-oracles/declare-values/declare-callback.cljs"),
                r#"{"id":"declare-callback","outcome":{"status":"returned","value":{"$vector":[{"$vector":[{"$value":"undefined"},{"$value":"undefined"}]},{"$vector":[null,null]}]}},"trace":[1,2,3,4],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "declare-redefinition",
                include_str!("../fixtures/phase-oracles/declare-values/declare-redefinition.cljs"),
                r#"{"id":"declare-redefinition","outcome":{"status":"returned","value":{"$vector":[true,3,2,null]}},"trace":[],"stdout":""}"#,
            ),
        ]);
    }
}
