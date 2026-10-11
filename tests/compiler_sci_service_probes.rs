//! Distinguish hosted compiler services from standalone emitted-JS evidence.
//! Original SCI source operations have matching observations on both pins.
//! Normal tests assert standalone and macro execution; the optional audit only
//! records hosted diagnostics and supplies no coverage by itself.
use light_cljs::{EvalOptions, EvalSession};

#[test]
#[ignore = "explicit SCI service/phase diagnostic; records macro failures, no gate coverage"]
fn audit_original_sci_macroexpand_phase_services() {
    let mut observations = String::new();
    for (id, source, expected) in [
        (
            "sci-binding-macroexpand-let",
            "(first (macroexpand-1 '(let [[x] [1]] x)))",
            "let*",
        ),
        (
            "sci-binding-macroexpand-fn",
            "(first (macroexpand-1 '(fn [[x]] x)))",
            "fn*",
        ),
        (
            "sci-binding-macroexpand-defn",
            "(first (macroexpand-1 '(defn f [x] x)))",
            "def",
        ),
        (
            "sci-binding-macroexpand-letfn",
            "(seq? (macroexpand-1 '(letfn [(f [x] x)] (f 1))))",
            "true",
        ),
    ] {
        let original = include_str!("fixtures/sci-outcomes.tsv")
            .lines()
            .find(|line| line.split('\t').next() == Some(id))
            .unwrap();
        assert_eq!(original.split('\t').nth(2), Some(source));
        let mut session = EvalSession::new(EvalOptions {
            namespace: "user".into(),
            ..Default::default()
        })
        .unwrap();
        let result = session.eval_string(source).unwrap();
        let application = session.print(result.value).unwrap();
        session.release(result.value).unwrap();
        assert_eq!(application, expected, "hosted application: {id}");
        let macro_source =
            format!("(defmacro service-probe [] (list 'quote (do {source}))) (service-probe)");
        let macro_result = match session.eval_string(&macro_source) {
            Ok(result) => {
                let printed = session.print(result.value).unwrap();
                session.release(result.value).unwrap();
                format!("macro-returned={printed}")
            }
            Err(error) => format!("macro-error={}", error.message.replace(['\n', '\t'], " ")),
        };
        observations.push_str(&format!(
            "{id}\tapplication={application}\t{macro_result}\n"
        ));
    }
    print!("{observations}");
    if let Some(path) = std::env::var_os("LIGHT_CLJS_SCI_PHASE_PROBE_OUTPUT") {
        std::fs::write(path, observations).unwrap();
    }
}

use light_cljs::{
    analyze_with_macros,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions},
    parse, MacroHost,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn both(source: &str, expected: &str) {
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator
        .evaluate(&program)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    assert_eq!(evaluator.print(value).unwrap(), expected);
    let script = format!(
        "const rt = {}; console.log(rt.core.prStr(({})(rt)));",
        light_cljs::evaluator::RUNTIME_SOURCE,
        emit_js_unit(&program)
    );
    // ns-publics may request metadata for every mapped core Var. Feed the
    // resulting program over stdin instead of relying on the OS argv limit.
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
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), expected);
}

#[test]
fn quoted_expansion_runs_without_services_in_application_and_macro_phases() {
    for (source, expected) in [
        ("(first (macroexpand-1 '(let [[x] [1]] x)))", "let*"),
        ("(first (macroexpand-1 '(fn [[x]] x)))", "fn*"),
        ("(first (macroexpand-1 '(defn f [x] x)))", "def"),
        ("(seq? (macroexpand-1 '(letfn [(f [x] x)] (f 1))))", "true"),
    ] {
        both(source, expected);
        both(&format!("(def state (atom 0)) (defmacro service-probe [] (swap! state inc) (list 'quote (do {source}))) [(service-probe) @state]"), &format!("[{expected} 0]"));
    }
}
#[test]
fn quoted_expansion_contract_and_dynamic_host_service_are_independent() {
    both("[(macroexpand-1 '(unknown x)) (macroexpand '(+ 1 2)) (macroexpand-1 '()) (macroexpand '42) (macroexpand-1 '[1 2]) (macroexpand '(when true 7))]", "[(unknown x) (+ 1 2) () 42 [1 2] (if true (do 7) nil)]");
    both("(defmacro one [] '(two)) (defmacro two [] 42) [(macroexpand-1 '(one)) (macroexpand '(one))]", "[(two) 42]");
    both(
        "(let [macroexpand-1 (fn [x] :local)] (macroexpand-1 '(let [] 1)))",
        ":local",
    );
    both(
        "(defmacro macroexpand-1 [x] :custom) (macroexpand-1 '(let [] 1))",
        ":custom",
    );
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    let result = session
        .eval_string("(let [form (list 'let '[x 1] 'x)] (first (macroexpand-1 form)))")
        .unwrap();
    assert_eq!(session.print(result.value).unwrap(), "let*");
    session.release(result.value).unwrap();
    for source in [
        "\n(macroexpand-1 '(let [x] x))",
        "\n(macroexpand-1 (quote))",
        "\n(macroexpand-1 '(+ 1 2) 3)",
        "\n(defmacro broken [] (macroexpand-1 '(let [x] x))) (broken)",
    ] {
        let error = session.eval_string(source).unwrap_err();
        assert!(
            error.message.contains("expand") || error.message.contains("binding"),
            "{error:?}"
        );
        assert!(
            error.location.as_ref().is_some_and(|span| span
                .source()
                .line_column(span.bytes().start)
                .unwrap()
                .0
                == 2),
            "{error:?}"
        );
        let result = session.eval_string("(+ 20 22)").unwrap();
        assert_eq!(session.print(result.value).unwrap(), "42");
        session.release(result.value).unwrap();
    }
}

#[test]
fn expansion_limits_and_finite_macro_returns_recover() {
    let mut host = MacroHost::new();
    host.limits.expansion_depth = 4;
    for source in [
        "(defmacro forever [] '(forever)) (macroexpand '(forever))",
        "(defmacro bad [] (js* \"1n\")) (macroexpand-1 '(bad))",
    ] {
        assert!(light_cljs::compile_with_macros(source, &mut host).is_err());
        let source = "(macroexpand-1 '(+ 20 22))";
        assert!(light_cljs::compile_with_macros(source, &mut host).is_ok());
    }
}
