//! Variadic callable types bind rest sequences in both engines and phases.
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
fn callable_rest_sequences_receivers_effects_and_lazy_values_match_references() {
    for line in include_str!("fixtures/compiler_ifn_rest.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        both(fields[1], fields[2]);
        // Macro computation is executed in QuickJS, with its own fresh roots.
        both(
            &format!(
                "(defmacro rest-contract [] (pr-str (do {}))) (rest-contract)",
                fields[1]
            ),
            &format!("{:?}", fields[2]),
        );
    }
}

#[test]
fn invalid_rest_forms_and_failed_invocations_leave_reused_sessions_usable() {
    let mut session = light_cljs::EvalSession::new(Default::default()).unwrap();
    for source in [
        "(deftype BadRest [] IFn (-invoke [_ &] 1))",
        "(reify IFn (-invoke [_ & x y] x))",
        "(reify IFn (-invoke [_ & args] (inc (recur args))))",
    ] {
        let error = session.eval_string(source).unwrap_err();
        assert!(
            error
                .location
                .as_ref()
                .is_some_and(|span| !span.bytes().is_empty()),
            "{error}"
        );
        let result = session.eval_string("(+ 1 2)").unwrap();
        assert_eq!(session.print(result.value).unwrap(), "3");
    }
    session.eval_string("(deftype RecoverRest [] IFn (-invoke [_] :fixed) (-invoke [_ x & xs] [x (vec xs)])) (def callable (RecoverRest.))").unwrap();
    let error = session.eval_string("((reify IFn (-invoke [_ & xs] (if (= 2 (count xs)) (throw (js/Error. \"rest failure\")) :bad))) 1 2)").unwrap_err();
    assert!(error.message.contains("rest failure"), "{error}");
    let result = session
        .eval_string("[(callable) (callable 1 2 3)]")
        .unwrap();
    assert_eq!(session.print(result.value).unwrap(), "[:fixed [1 [2 3]]]");
    let mut host = MacroHost::new();
    assert!(light_cljs::compile_with_macros("(defmacro broken-rest [] ((reify IFn (-invoke [_ & xs] (throw (js/Error. \"rest failure\")))) 1)) (broken-rest)", &mut host).is_err());
    both(
        "(defmacro restored-rest [] (list 'quote ((reify IFn (-invoke [_ & xs] (vec xs))) 1 2))) (restored-rest)",
        "[1 2]",
    );
    let source = "(defmacro restored-rest [] (list 'quote ((reify IFn (-invoke [_ & xs] (vec xs))) 1 2))) (restored-rest)";
    let js =
        light_cljs::compile_with_macros(&format!("{source} (pr-str (restored-rest))"), &mut host)
            .unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator.evaluate_js(&js).unwrap();
    assert_eq!(evaluator.string(value).unwrap(), "[1 2]");
}
