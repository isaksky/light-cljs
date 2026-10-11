//! Canonical source expansion data and execution stay consistent in both phases.
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
fn quoted_when_body_shape_and_effects_execute_in_both_phases() {
    both(
        "(defmacro observed-shape [] (list 'quote [(count (macroexpand-1 '(when true 1))) (pr-str (macroexpand-1 '(when true 1))) (count (macroexpand '(when true 1))) (pr-str (macroexpand '(when true 1))) (count (macroexpand-1 '(when-not false 2))) (pr-str (macroexpand-1 '(when-not false 2))) (count (macroexpand '(when-not false 2))) (pr-str (macroexpand '(when-not false 2)))])) (observed-shape)",
        "[3 \"(if true (do 1))\" 3 \"(if true (do 1))\" 4 \"(if false nil (do 2))\" 4 \"(if false nil (do 2))\"]",
    );
    for line in include_str!("fixtures/compiler-quoted-when.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        let source = fields[1];
        both(source, "true");
        // A demanded macro computes the complete contract once. Its mutable
        // root is separate from the identically named application root.
        both(
            &format!(
                "(def phase-state (atom 0)) (defmacro observed-when [] (swap! phase-state inc) (list 'quote [(do {source}) @phase-state])) [(observed-when) @phase-state]"
            ),
            "[[true 1] 0]",
        );
    }
}

#[test]
fn when_condition_and_body_keep_original_diagnostic_spans() {
    for source in [
        "; λ\n(when missing 1)",
        "; λ\n(when true 1 missing)",
        "; λ\n(when-not false missing 1)",
    ] {
        let mut host = MacroHost::new();
        let error = light_cljs::compile_with_macros(source, &mut host).unwrap_err();
        assert!(
            error.to_string().contains("unresolved symbol: missing"),
            "{error}"
        );
        let light_cljs::CompileError::Analysis(error) = error else {
            panic!()
        };
        assert_eq!(&source[error.span], "missing");
    }
}
