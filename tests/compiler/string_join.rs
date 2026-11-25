//! Canonical join conversion and complete SCI source in fresh/reused phases.
use crate::temp_dir::TempDir;
use light_cljs::MacroHost;
#[cfg(light_v8)]
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

fn compile(
    host: &mut MacroHost,
    source: &str,
) -> Result<String, light_cljs::compiler::NamespaceCompileError> {
    let root = TempDir::new("string-join");
    fs::create_dir_all(root.join("string_join")).unwrap();
    fs::write(root.join("string_join/main.cljs"), source).unwrap();
    light_cljs::compile_namespace_with_macros(
        "string-join.main",
        std::slice::from_ref(root.path()),
        host,
    )
}

fn both(host: &mut MacroHost, source: &str, expected: &str) {
    let js = compile(host, source).unwrap_or_else(|e| panic!("{e}"));
    #[cfg(light_v8)]
    {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            evaluator
                .string(evaluator.evaluate_js(&js).unwrap())
                .unwrap(),
            expected
        );
    }
    let script = format!("console.log({js});");
    let output = if script.len() > 131_072 {
        let mut child = Command::new("node")
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
        child.wait_with_output().unwrap()
    } else {
        Command::new("node").args(["-e", &script]).output().unwrap()
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{expected}\n")
    );
    eprintln!("Node/embedded engine literal: {expected}");
    assert!(output.stderr.is_empty());
}

#[test]
fn join_contract_executes_in_fresh_and_reused_application_and_macro_phases() {
    let contract = include_str!("../fixtures/compiler-string-join.cljs");
    let original = include_str!("../fixtures/sci-outcomes.tsv")
        .lines()
        .find(|line| line.starts_with("sci-string-join-values\t"))
        .unwrap()
        .split('\t')
        .nth(2)
        .unwrap();
    assert!(contract.contains(&format!("(defn original-five [] {original})")));
    let mut reused = MacroHost::new();
    for reuse in [false, true] {
        for macros in [false, true] {
            for (body, expected) in [
                ("(join-contract)", "true"),
                (
                    "(original-five)",
                    r#"["falsexa" ",1,:x" "1null2" "1-2" "1:x2"]"#,
                ),
            ] {
                let mut fresh = MacroHost::new();
                let expression = if macros {
                    format!("(defmacro witness [] (list 'quote {body})) (pr-str (witness))")
                } else {
                    format!("(pr-str {body})")
                };
                eprintln!("join phase: reuse={reuse}, macro={macros}, body={body}");
                both(
                    if reuse { &mut reused } else { &mut fresh },
                    &format!("{contract}\n{expression}"),
                    expected,
                );
            }
        }
    }
}

#[test]
fn join_macro_execution_errors_and_result_domain_are_distinct() {
    let prefix = "(ns string-join.main (:require [clojure.string :as s]))";
    let mut host = MacroHost::new();
    for (body, category) in [
        ("(apply s/join [])", "Invalid arity: 0"),
        ("(apply s/join [nil [] :extra])", "Invalid arity: 3"),
        ("s/join", "Unsupported macro result"),
    ] {
        let source = format!("{prefix}\n(defmacro bad [] {body})\n(bad)");
        let error = compile(&mut host, &source).unwrap_err();
        assert!(error.message.contains(category), "{error}");
        assert_eq!(
            error.path.as_ref().unwrap().file_name().unwrap(),
            "main.cljs"
        );
        assert_eq!(&source[error.span.unwrap()], "(bad)");
        both(
            &mut host,
            &format!("{prefix}\n(defmacro recovered [] (s/join nil [1 2]))\n(recovered)"),
            "1null2",
        );
    }
}
