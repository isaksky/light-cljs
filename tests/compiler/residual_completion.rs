//! Complete remaining assertion-free upstream bodies; preserve actual oracles.
use light_cljs::MacroHost;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::process::Command;

fn both(source: &str, expected: &str, macro_phase: bool) {
    let root =
        std::env::temp_dir().join(format!("light-residual-completion-{}", std::process::id()));
    std::fs::create_dir_all(root.join("residualcomplete")).unwrap();
    let observed = if macro_phase {
        format!("(defmacro upstream [] {source}) (pr-str (upstream))")
    } else {
        format!("(pr-str {source})")
    };
    std::fs::write(
        root.join("residualcomplete/probe.cljs"),
        format!("(ns residualcomplete.probe (:require [cljs.array-access-test :refer [suppress-errs]] [clojure.edn] [cljs.reader])) {observed}"),
    )
    .unwrap();
    let javascript = light_cljs::compile_namespace_with_macros(
        "residualcomplete.probe",
        &[
            root.clone(),
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/compiler-residual-completion"),
        ],
        &mut MacroHost::new(),
    )
    .unwrap_or_else(|error| panic!("macro_phase={macro_phase}: {source}: {error}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .string(evaluator.evaluate_js(&javascript).unwrap())
            .unwrap(),
        expected,
        "QuickJS macro_phase={macro_phase}: {source}"
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
        "Node macro_phase={macro_phase}: {source}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn remaining_complete_residual_bodies_execute_in_both_phases() {
    let rows: Vec<_> = include_str!("../fixtures/compiler-residual-completion.tsv")
        .lines()
        .filter(|row| !row.starts_with('#') && !row.is_empty())
        .collect();
    assert_eq!(rows.len(), 9, "empty or partial upstream selection");
    for row in rows {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 4);
        eprintln!("remaining complete residual body: {}", fields[0]);
        both(fields[1], fields[2], false);
        both(fields[1], fields[2], true);
    }
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn cljs_2866_test() {
        run(&[Case::expression(
            "cljs-2866-test",
            include_str!("../fixtures/phase-oracles/residual-completion/cljs-2866-test.cljs"),
        )]);
    }
}
