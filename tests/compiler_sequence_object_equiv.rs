//! Existing sequence and name Object equiv methods in both engines and phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn both(source: &str, expected: &str) {
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator.evaluate(&program).unwrap();
    assert_eq!(evaluator.print(value).unwrap(), expected);
    let script = format!(
        "const rt = {}; console.log(rt.core.prStr(({})(rt)));",
        light_cljs::evaluator::RUNTIME_SOURCE,
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
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{expected}\n")
    );
}

#[test]
fn sequence_and_name_equiv_contracts_execute_in_both_phases() {
    for line in include_str!("fixtures/compiler_sequence_object_equiv.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        eprintln!("sequence/name equiv contract: {}", fields[0]);
        both(fields[1], fields[2]);
        both(
            &format!(
                "(defmacro equiv-contract [] (pr-str (do {}))) (equiv-contract)",
                fields[1]
            ),
            &format!("{:?}", fields[2]),
        );
    }
}

#[test]
fn failed_sequence_equiv_leaves_reused_application_and_macro_hosts_usable() {
    let mut session = light_cljs::EvalSession::new(Default::default()).unwrap();
    session.eval_string("(def v (list 1 2))").unwrap();
    let error = session
        .eval_string(
            "(.equiv v (map (fn [_] (throw (js/Error. \"sequence equiv failure\"))) [1 2]))",
        )
        .unwrap_err();
    assert!(error.message.contains("sequence equiv failure"), "{error}");
    let value = session
        .eval_string("[(.equiv v [1 2]) (.equiv v [1 3]) (.equiv (with-meta v {:x 1}) v)]")
        .unwrap();
    assert_eq!(session.print(value.value).unwrap(), "[true false true]");
    let mut host = MacroHost::new();
    assert!(light_cljs::compile_with_macros("(defmacro broken-sequence-equiv [] (.equiv (list 1) (map (fn [_] (throw (js/Error. \"sequence equiv failure\"))) [1]))) (broken-sequence-equiv)", &mut host).is_err());
    let source =
        "(defmacro restored-sequence-equiv [] (.equiv (list 1 2) [1 2])) (restored-sequence-equiv)";
    let js = light_cljs::compile_with_macros(source, &mut host).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator.evaluate_js(&js).unwrap();
    assert_eq!(evaluator.print(value).unwrap(), "true");
    both(source, "true");
}
