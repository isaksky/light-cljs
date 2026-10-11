//! Complete pinned promised library bodies in canonical applications and macros.
use light_cljs::MacroHost;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::process::Command;

fn both(source: &str, expected: &str) {
    let root = std::env::temp_dir().join(format!("light-library-bodies-{}", std::process::id()));
    std::fs::create_dir_all(root.join("librarybody")).unwrap();
    let namespace = if source.contains("clojure.set/") {
        "clojure.set"
    } else if source.contains("clojure.string/") {
        "clojure.string"
    } else {
        "clojure.walk"
    };
    std::fs::write(
        root.join("librarybody/probe.cljs"),
        format!("(ns librarybody.probe (:require [{namespace}])) {source}"),
    )
    .unwrap();
    let javascript = light_cljs::compile_namespace_with_macros(
        "librarybody.probe",
        std::slice::from_ref(&root),
        &mut MacroHost::new(),
    )
    .unwrap_or_else(|e| panic!("{source}: {e}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .print(evaluator.evaluate_js(&javascript).unwrap())
            .unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let output = Command::new("node")
        .args(["-e", &format!("console.log({javascript});")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn complete_promised_library_bodies_execute_in_both_phases() {
    for row in include_str!("fixtures/compiler-library-bodies.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        eprintln!("complete library body: {}", fields[0]);
        both(fields[1], fields[2]);
        both(
            &format!("(defmacro upstream [] {}) (upstream)", fields[1]),
            fields[2],
        );
    }
}
