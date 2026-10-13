//! Public ns-name preserves the supplied Namespace slot and compiler metadata strings.
use light_cljs::{
    EvalOptions, EvalSession, MacroHost, analyze_with_macros,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE},
    parse,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn ns_name_result_types_execute_in_both_engines_and_macro_phase() {
    let contract = include_str!("../fixtures/compiler-ns-name.cljs");
    let source = format!(
        "{contract}
      (defmacro inspect-names []
        (assert (ns-name-contract))
        (create-ns 'macro.only)
        (assert (string? (ns-name (:ns (meta #'ns-name-contract))))) true)
      [(ns-name-contract) (inspect-names) (nil? (find-ns 'macro.only))]"
    );
    let mut host = MacroHost::new();
    for _ in 0..2 {
        let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        for _ in 0..2 {
            let value = evaluator.evaluate(&program).unwrap();
            assert_eq!(evaluator.print(value).unwrap(), "[true true true]");
            evaluator.release(value).unwrap();
        }
        let script = format!(
            "const rt = {RUNTIME_SOURCE}; const run = {}; for(let i=0;i<2;i++) console.log(rt.core.prStr(run(rt)));",
            emit_js_unit(&program)
        );
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
            .write_all(script.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "[true true true]\n[true true true]"
        );
    }
    light_cljs::compile_with_macros("(def unavailable (js/unavailable)) (defmacro inspect [] (let [n (:ns (meta #'unavailable))] (assert (symbol? n)) (assert (= \"cljs.user\" (ns-name n))) true)) (inspect)", &mut MacroHost::new()).unwrap();
}

#[test]
fn metadata_names_preserve_qualification_and_session_isolation() {
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    for source in [
        "(ns qualified.scope) (def value 41) (defmacro name-in-macro [] (ns-name (:ns (meta #'value)))) [(:name (meta #'value)) (ns-name (:ns (meta #'value))) (name-in-macro)]",
        "[(ns-name (:ns (meta #'value))) (name-in-macro) (identical? (create-ns 'qualified.scope) (find-ns 'qualified.scope)) (ns-name *ns*)]",
    ] {
        let value = session.eval_string(source).unwrap().value;
        let expected = if source.starts_with("(ns") {
            "[value \"qualified.scope\" \"qualified.scope\"]"
        } else {
            "[\"qualified.scope\" \"qualified.scope\" true qualified.scope]"
        };
        assert_eq!(session.print(value).unwrap(), expected);
        session.release(value).unwrap();
    }
    let mut fresh = session.fresh_session().unwrap();
    let value = fresh
        .eval_string("[(nil? (find-ns 'qualified.scope)) (ns-name *ns*)]")
        .unwrap()
        .value;
    assert_eq!(fresh.print(value).unwrap(), "[true cljs.user]");
}
