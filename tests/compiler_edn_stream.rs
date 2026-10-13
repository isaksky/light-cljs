//! Strict EDN stream arities, EOF precedence, effects and phase isolation.
use light_cljs::{
    MacroHost,
    evaluator::{Evaluator, EvaluatorOptions},
};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn compile(
    host: &mut MacroHost,
    source: &str,
) -> Result<String, light_cljs::compiler::NamespaceCompileError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "light-edn-stream-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("edn_stream")).unwrap();
    fs::write(root.join("edn_stream/main.cljs"), source).unwrap();
    let result = light_cljs::compile_namespace_with_macros(
        "edn-stream.main",
        std::slice::from_ref(&root),
        host,
    );
    fs::remove_dir_all(root).unwrap();
    result
}

fn both(host: &mut MacroHost, source: &str) {
    let js = compile(host, source).unwrap_or_else(|e| panic!("{e}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .string(evaluator.evaluate_js(&js).unwrap())
            .unwrap(),
        "true"
    );
    let output = Command::new("node")
        .args(["-e", &format!("console.log({js});")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "true\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn edn_stream_contract_executes_in_fresh_and_reused_application_and_macro_phases() {
    let mut reused = MacroHost::new();
    for contract in [
        include_str!("fixtures/compiler-edn-stream.cljs"),
        include_str!("fixtures/compiler-edn-stream-parity.cljs"),
    ] {
        for reuse in [false, true] {
            for macros in [false, true] {
                let mut fresh = MacroHost::new();
                let source = format!(
                    "{contract}\n{}\n(pr-str (run))",
                    if macros {
                        "(defmacro witness [] (stream-contract)) (defn run [] (witness))"
                    } else {
                        "(defn run [] (stream-contract))"
                    }
                );
                both(if reuse { &mut reused } else { &mut fresh }, &source);
            }
        }
    }
}

#[test]
fn edn_macro_failures_retain_source_locations_and_separate_result_domain() {
    let prefix = "(ns edn-stream.main (:require [clojure.edn :as edn] [cljs.reader :as reader]))";
    let mut host = MacroHost::new();
    for (body, category) in [
        (
            "(edn/read (reader/string-push-back-reader \"\") true :done {})",
            "EOF while reading",
        ),
        (
            "(edn/read (reader/string-push-back-reader \"\\n]\" \"macro.edn\") false :done {})",
            "macro.edn:2:1",
        ),
        ("(apply edn/read [1 2 3])", "Invalid arity: 3"),
        ("edn/read", "Unsupported macro result"),
        (
            "(reader/string-push-back-reader \"1\")",
            "Unsupported macro result",
        ),
    ] {
        let source = format!("{prefix}\n(defmacro bad [] {body})\n(bad)");
        let error = compile(&mut host, &source).unwrap_err();
        assert!(error.message.contains(category), "{error}");
        assert_eq!(
            error.path.as_ref().unwrap().file_name().unwrap(),
            "main.cljs"
        );
        let span = error.span.unwrap();
        assert_eq!(&source[span], "(bad)");
        both(
            &mut host,
            &format!(
                "{prefix}\n(defmacro recovered [] (edn/read (reader/string-push-back-reader \"true\") true nil {{}}))\n(pr-str (recovered))"
            ),
        );
    }
}
