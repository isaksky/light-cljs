//! Atom Object equiv methods in both engines and phases.
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
fn atom_equiv_contracts_execute_in_both_phases() {
    for line in include_str!("fixtures/compiler_atom_object_equiv.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        eprintln!("Atom equiv contract: {}", fields[0]);
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
fn failed_atom_equiv_leaves_fresh_and_reused_hosts_usable() {
    const SETUP: &str = r#"(def v (atom 1))
      (extend-type Atom IEquiv
        (-equiv [this other]
          (if (identical? other 13)
            (throw (js/Error. "Atom equiv failure"))
            (identical? this other))))"#;
    const FAILURE: &str = "(.equiv v 13)";
    const RECOVERY: &str = "[(.equiv v v) (.equiv v (atom 1)) (= v v) (swap! v inc) @v]";
    for reused in [false, true] {
        let mut session = light_cljs::EvalSession::new(Default::default()).unwrap();
        if reused {
            session.eval_string(SETUP).unwrap();
        }
        let failing = if reused {
            FAILURE.to_string()
        } else {
            format!("{SETUP} {FAILURE}")
        };
        let error = session.eval_string(&failing).unwrap_err();
        assert!(error.message.contains("Atom equiv failure"), "{error}");
        let value = session.eval_string(RECOVERY).unwrap();
        assert_eq!(session.print(value.value).unwrap(), "[true false true 2 2]");

        let mut host = MacroHost::new();
        if reused {
            light_cljs::compile_with_macros(
                "(defmacro prior [] (.equiv (atom 0) (atom 0))) (prior)",
                &mut host,
            )
            .unwrap();
        }
        let failing_macro =
            format!("(defmacro broken-atom-equiv [] {SETUP} {FAILURE}) (broken-atom-equiv)");
        let error = light_cljs::compile_with_macros(&failing_macro, &mut host).unwrap_err();
        assert!(error.to_string().contains("Atom equiv failure"), "{error}");
        // A subsequent compilation on the same host gets fresh phase roots.
        let source = format!(
            "(defmacro restored-atom-equiv [] {SETUP} (pr-str {RECOVERY})) (restored-atom-equiv)"
        );
        let js = light_cljs::compile_with_macros(&source, &mut host).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let value = evaluator.evaluate_js(&js).unwrap();
        assert_eq!(evaluator.print(value).unwrap(), "\"[true false true 2 2]\"");
    }
    let recovered = format!("{SETUP} {RECOVERY}");
    both(&recovered, "[true false true 2 2]");
    both(
        &format!("(defmacro fresh-atom-equiv [] (pr-str (do {recovered}))) (fresh-atom-equiv)"),
        "\"[true false true 2 2]\"",
    );
}
