//! Complete pinned residual language bodies in canonical applications and macros.
use light_cljs::MacroHost;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::process::Command;

fn both(source: &str, expected: &str, macro_phase: bool) {
    let root = std::env::temp_dir().join(format!("light-residual-bodies-{}", std::process::id()));
    std::fs::create_dir_all(root.join("residualbody")).unwrap();
    let observed_source = if macro_phase {
        // Macro declarations must remain at namespace top level. The full
        // original body executes during expansion before its result prints.
        format!("(defmacro upstream [] {source}) (pr-str (upstream))")
    } else {
        format!("(pr-str {source})")
    };
    std::fs::write(
        root.join("residualbody/probe.cljs"),
        // Print inside the namespace's runtime, as the equality harness does.
        // Its collection classes belong to that runtime, not the evaluator's
        // separate instance; exporting only the canonical string avoids a
        // foreign-object display without changing the complete body oracle.
        format!("(ns residualbody.probe (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object])) {observed_source}"),
    )
    .unwrap();
    // The retained reader body performs all 7,392 instant round trips. Its
    // bounded macro computation needs a visible test budget above the default
    // interactive ten seconds; no iteration or original assertion is removed.
    let mut host = MacroHost::new();
    host.evaluator_options.timeout = std::time::Duration::from_secs(120);
    let javascript = light_cljs::compile_namespace_with_macros(
        "residualbody.probe",
        std::slice::from_ref(&root),
        &mut host,
    )
    .unwrap_or_else(|e| panic!("{source}: {e}"));
    let evaluator = Evaluator::new(EvaluatorOptions {
        timeout: std::time::Duration::from_secs(120),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    assert_eq!(
        evaluator
            .string(evaluator.evaluate_js(&javascript).unwrap())
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
fn complete_residual_bodies_execute_in_both_phases() {
    let rows: Vec<_> = include_str!("fixtures/compiler-residual-bodies.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
        .collect();
    assert_eq!(rows.len(), 21, "empty or partial upstream selection");
    for row in rows {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 4);
        eprintln!("complete residual body: {}", fields[0]);
        both(fields[1], fields[2], false);
        both(fields[1], fields[2], true);
    }
}
