//! Complete syntax semantics in applications and actual MacroHost bodies.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::io::Write;
use std::process::{Command, Stdio};

const FIXTURE: &str = include_str!("../fixtures/compiler-syntax-context.tsv");
fn unhex(value: &str) -> String {
    assert_eq!(value.len() % 2, 0);
    String::from_utf8(
        value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|part| u8::from_str_radix(std::str::from_utf8(part).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn node(javascript: &str) -> String {
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
        .write_all(javascript.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "Unexpected syntax observation stderr"
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn row(identity: &str) -> Vec<&'static str> {
    let rows: Vec<_> = FIXTURE
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
        .map(|r| r.split('\t').collect::<Vec<_>>())
        .collect();
    assert_eq!(rows.len(), 8);
    let selected: Vec<_> = rows.into_iter().filter(|r| r[0] == identity).collect();
    assert_eq!(selected.len(), 1);
    let result = selected.into_iter().next().unwrap();
    assert_eq!(result.len(), 5);
    result
}
fn literal(identity: &str) {
    let fields = row(identity);
    let source = unhex(fields[2]);
    let expected = unhex(fields[3]);
    for macro_phase in [false, true] {
        // Serialize observed list data inside MacroHost before it becomes
        // expansion data; an unquoted returned list would be executable source.
        let observed_source = if macro_phase {
            format!("(defmacro syntax-gate [] (pr-str {source})) (syntax-gate)")
        } else {
            format!("(pr-str {source})")
        };
        let parsed = parse(&observed_source).unwrap();
        let program =
            analyze_with_macros(&observed_source, &parsed, &mut MacroHost::new()).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let actual = evaluator
            .string(evaluator.evaluate(&program).unwrap())
            .unwrap();
        println!("syntax-observation\t{identity}\tmacro={macro_phase}\tQuickJS\t{actual}");
        assert_eq!(actual, expected, "{identity} macro={macro_phase} QuickJS");
        let script = format!(
            "const rt = {RUNTIME_SOURCE}; console.log(({})(rt));",
            emit_js_unit(&program)
        );
        let actual = node(&script);
        println!("syntax-observation\t{identity}\tmacro={macro_phase}\tNode\t{actual}");
        assert_eq!(actual, expected, "{identity} macro={macro_phase} Node");
    }
}
#[test]
fn syntax_control_rest_and_recursion_execute_in_both_phases() {
    literal("control-rest-recursion");
}
#[test]
fn syntax_exception_interop_and_template_execute_in_both_phases() {
    literal("exception-interop-template");
}
#[test]
fn syntax_quote_and_case_star_execute_in_both_phases() {
    literal("quote-case-star");
}
#[test]
fn syntax_definition_and_var_state_execute_in_both_phases() {
    literal("definition-var-state");
}
#[test]
fn syntax_raw_type_constructor_executes_in_both_phases() {
    literal("raw-type-constructor");
}
#[test]
fn syntax_raw_record_constructor_executes_in_both_phases() {
    literal("raw-record-constructor");
}
#[test]
fn syntax_namespace_context_executes_with_actual_macro_environment() {
    let fields = row("namespace-header-context");
    let original = unhex(fields[2]);
    let root = std::env::temp_dir().join(format!("light-syntax-context-{}", std::process::id()));
    std::fs::create_dir_all(root.join("syntax")).unwrap();
    for extension in ["cljs", "clj"] {
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "tests/fixtures/compiler-syntax-context/local/syntax/observer.{extension}"
            )),
            root.join(format!("syntax/observer.{extension}")),
        )
        .unwrap();
    }
    for macro_phase in [false, true] {
        let source = if macro_phase {
            let (prefix, remainder) = original.split_once("(defn run [observe] ").unwrap();
            let body = remainder.strip_suffix(")\n(pr-str (run nil))\n").unwrap();
            format!("{prefix}(defmacro syntax-gate [] {body})\n(pr-str (syntax-gate))\n")
        } else {
            original.clone()
        };
        std::fs::write(root.join("syntax/header.cljs"), source).unwrap();
        let javascript = light_cljs::compile_namespace_with_macros(
            "syntax.header",
            std::slice::from_ref(&root),
            &mut MacroHost::new(),
        )
        .unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let actual = evaluator
            .string(evaluator.evaluate_js(&javascript).unwrap())
            .unwrap();
        println!(
            "syntax-observation\tnamespace-header-context\tmacro={macro_phase}\tQuickJS\t{actual}"
        );
        assert_eq!(actual, "true");
        let actual = node(&format!("console.log({javascript});"));
        println!(
            "syntax-observation\tnamespace-header-context\tmacro={macro_phase}\tNode\t{actual}"
        );
        assert_eq!(actual, "true");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn syntax_raw_nsstar_alias_executes_in_fresh_and_reused_macro_hosts() {
    let original = unhex(row("namespace-nsstar-alias")[2]);
    assert_eq!(
        original,
        include_str!("../fixtures/compiler-nsstar/syntax/nsstar.cljs")
    );
    let root = std::env::temp_dir().join(format!("light-nsstar-context-{}", std::process::id()));
    std::fs::create_dir_all(root.join("syntax")).unwrap();
    let mut reused = MacroHost::new();
    for _ in 0..2 {
        for host in [&mut reused, &mut MacroHost::new()] {
            for macro_phase in [false, true] {
                let source = if macro_phase {
                    let (prefix, remainder) = original.split_once("(defn run [observe] ").unwrap();
                    let body = remainder.strip_suffix(")\n(pr-str (run nil))\n").unwrap();
                    format!("{prefix}(defmacro syntax-gate [] {body})\n(pr-str (syntax-gate))\n")
                } else {
                    original.clone()
                };
                std::fs::write(root.join("syntax/nsstar.cljs"), source).unwrap();
                let javascript = light_cljs::compile_namespace_with_macros(
                    "syntax.nsstar",
                    std::slice::from_ref(&root),
                    host,
                )
                .unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&javascript).unwrap())
                    .unwrap();
                assert_eq!(actual, "true", "ns* macro={macro_phase} QuickJS");
                assert_eq!(
                    node(&format!("console.log({javascript});")),
                    "true",
                    "ns* macro={macro_phase} Node"
                );
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn raw_nsstar_validation_keeps_source_ownership_and_recovers() {
    let root = std::env::temp_dir().join(format!("light-nsstar-validation-{}", std::process::id()));
    std::fs::create_dir_all(root.join("syntax")).unwrap();
    let path = root.join("syntax/nsstar.cljs");
    let mut host = MacroHost::new();
    for (body, expected) in [
        (
            "(ns* (:require [clojure.string :as text]))",
            "must be quoted",
        ),
        ("(ns* (:require 'missing.library))", "not found"),
        ("(ns* (:require '[clojure.string :as 1]))", ":as alias"),
        (
            "(ns* (:require '[clojure.string :refer [absent]]))",
            "unresolved",
        ),
        (
            "(ns* (:require '[clojure.string :as text :as other]))",
            "duplicate require option",
        ),
        (
            "(ns* (:require '[clojure.string :rename {join combine}]))",
            "not referred",
        ),
        (
            "(ns* (:require '[clojure.string :as text])) (ns* (:require '[clojure.set :as text]))",
            "conflicting namespace alias",
        ),
        (
            "(fn [] (ns* (:require '[clojure.string :as text])))",
            "top-level",
        ),
        ("(ns* syntax.other)", "literal namespace directive"),
        (
            "(ns* (:require 'clojure.string) (:require 'clojure.set))",
            "one namespace directive",
        ),
        ("(ns* (:require-macros 'syntax.jvm))", "retired"),
        (
            "(ns* (:require '[clojure.string :as-alias text])) (text/join [] )",
            "unresolved",
        ),
    ] {
        let source = format!("(ns syntax.nsstar)\n{body}");
        std::fs::write(&path, &source).unwrap();
        let error = light_cljs::compile_namespace_with_macros(
            "syntax.nsstar",
            std::slice::from_ref(&root),
            &mut host,
        )
        .unwrap_err();
        assert!(error.message.contains(expected), "{body}: {error}");
        assert_eq!(
            error.path.as_ref(),
            Some(&std::fs::canonicalize(&path).unwrap())
        );
        // Unresolved unused refers retain the existing namespace-header diagnostic.
        let expected_line = if body.contains(":refer [absent]") {
            1
        } else {
            2
        };
        assert_eq!(error.line_column.unwrap().0, expected_line, "{error}");
        std::fs::write(
            &path,
            include_str!("../fixtures/compiler-nsstar/syntax/nsstar.cljs"),
        )
        .unwrap();
        let javascript = light_cljs::compile_namespace_with_macros(
            "syntax.nsstar",
            std::slice::from_ref(&root),
            &mut host,
        )
        .unwrap();
        assert_eq!(node(&format!("console.log({javascript});")), "true");
    }
    // The raw form adds refers without changing the declared source owner.
    std::fs::write(&path, "(ns syntax.nsstar)\n(do (ns* (:require (quote [clojure.string :refer [join] :rename {join combine}]))))\n(def value (combine \"-\" [1 2]))\n(pr-str (= syntax.nsstar/value \"1-2\"))").unwrap();
    let javascript = light_cljs::compile_namespace_with_macros(
        "syntax.nsstar",
        std::slice::from_ref(&root),
        &mut host,
    )
    .unwrap();
    assert_eq!(node(&format!("console.log({javascript});")), "true");
    // Reused hosts cannot carry a previous source's alias into a new compile.
    std::fs::write(&path, "(ns syntax.nsstar)\n(text/upper-case \"a\")").unwrap();
    let error = light_cljs::compile_namespace_with_macros(
        "syntax.nsstar",
        std::slice::from_ref(&root),
        &mut host,
    )
    .unwrap_err();
    assert!(error.message.contains("unresolved"), "{error}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn raw_nsstar_imports_keep_ordinary_cljs_macro_and_definition_ownership() {
    let root = std::env::temp_dir().join(format!("light-nsstar-ownership-{}", std::process::id()));
    std::fs::create_dir_all(root.join("syntax")).unwrap();
    let path = root.join("syntax/nsstar.cljs");
    std::fs::write(
        root.join("syntax/library.clj"),
        "(ns syntax.library) (defmacro snapshot [] false)",
    )
    .unwrap();
    let mut reused = MacroHost::new();
    for value in [7, 9] {
        std::fs::write(root.join("syntax/library.cljs"), format!("(ns syntax.library) (def value {value}) (defmacro snapshot [] [value (str (get-in &env [:ns :name]))])")).unwrap();
        std::fs::write(&path, format!("(ns syntax.nsstar)\n(ns* (:require '[syntax.library :as l :refer [snapshot]]))\n(def owned l/value)\n(pr-str (= [{value} [{value} \"syntax.nsstar\"] [{value} \"syntax.nsstar\"]] [syntax.nsstar/owned (snapshot) (l/snapshot)]))")).unwrap();
        for host in [&mut reused, &mut MacroHost::new()] {
            let javascript = light_cljs::compile_namespace_with_macros(
                "syntax.nsstar",
                std::slice::from_ref(&root),
                host,
            )
            .unwrap();
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            assert_eq!(
                evaluator
                    .string(evaluator.evaluate_js(&javascript).unwrap())
                    .unwrap(),
                "true"
            );
            assert_eq!(node(&format!("console.log({javascript});")), "true");
        }
    }
    std::fs::write(
        &path,
        "(ns syntax.nsstar)\n(ns* (:require '[syntax.library :as l]))\nl/snapshot",
    )
    .unwrap();
    let error = light_cljs::compile_namespace_with_macros(
        "syntax.nsstar",
        std::slice::from_ref(&root),
        &mut reused,
    )
    .unwrap_err();
    assert!(error.message.contains("macro"), "{error}");
    assert_eq!(
        error.path.as_ref(),
        Some(&std::fs::canonicalize(&path).unwrap())
    );
    assert_eq!(error.line_column.unwrap().0, 3);
    std::fs::remove_dir_all(root).unwrap();
}
