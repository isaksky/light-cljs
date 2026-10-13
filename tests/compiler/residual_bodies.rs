//! Complete pinned residual language bodies in canonical applications and macros.
use light_cljs::MacroHost;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::process::Command;

fn both(id: &str, source: &str, expected: &str, macro_phase: bool) {
    // Rows run as parallel tests (threads of one process under `cargo test`), so
    // each row and phase compiles in its own directory.
    let root = std::env::temp_dir().join(format!(
        "light-residual-bodies-{}-{id}-{}",
        std::process::id(),
        if macro_phase { "macro" } else { "application" }
    ));
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

fn residual_body_executes_in_both_phases(index: usize) {
    let rows: Vec<_> = include_str!("../fixtures/compiler-residual-bodies.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
        .collect();
    assert_eq!(rows.len(), 21, "empty or partial upstream selection");
    let fields: Vec<_> = rows[index].split('\t').collect();
    assert_eq!(fields.len(), 4);
    eprintln!("complete residual body: {}", fields[0]);
    both(fields[0], fields[1], fields[2], false);
    both(fields[0], fields[1], fields[2], true);
}

/// One test per fixture row; together they cover all 21 rows.
mod complete_residual_bodies_execute_in_both_phases {
    use super::residual_body_executes_in_both_phases as run;
    row_tests!(run;
        row_00(0), row_01(1), row_02(2), row_03(3), row_04(4), row_05(5), row_06(6),
        row_07(7), row_08(8), row_09(9), row_10(10), row_11(11), row_12(12), row_13(13),
        row_14(14), row_15(15), row_16(16), row_17(17), row_18(18), row_19(19), row_20(20),
    );
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    // Each program of these bodies takes seconds to compile, so each body has a test
    // of its own.
    #[test]
    fn residual_body_0() {
        run(&[Case::namespace(
            "residual-body-0",
            "residualbody.case0",
            "(run)",
            include_str!("../fixtures/compiler-residual-bodies/residualbody/case0.cljs"),
        )]);
    }

    #[test]
    fn residual_body_2() {
        run(&[Case::namespace(
            "residual-body-2",
            "residualbody.case2",
            "(run)",
            include_str!("../fixtures/compiler-residual-bodies/residualbody/case2.cljs"),
        )]);
    }

    #[test]
    fn residual_body_10() {
        run(&[Case::namespace(
            "residual-body-10",
            "residualbody.case10",
            "(run)",
            include_str!("../fixtures/compiler-residual-bodies/residualbody/case10.cljs"),
        )]);
    }
}
