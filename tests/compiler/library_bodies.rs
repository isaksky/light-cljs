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
    for row in include_str!("../fixtures/compiler-library-bodies.tsv")
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

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn walk_test_bodies() {
        run(&[
            Case::namespace(
                "walk-t-prewalk-replace",
                "librarybody.case0",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case0.cljs"),
            ),
            Case::namespace(
                "walk-t-postwalk-replace",
                "librarybody.case1",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case1.cljs"),
            ),
            Case::namespace(
                "walk-t-stringify-keys",
                "librarybody.case2",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case2.cljs"),
            ),
            Case::namespace(
                "walk-t-prewalk-order",
                "librarybody.case3",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case3.cljs"),
            ),
            Case::namespace(
                "walk-t-postwalk-order",
                "librarybody.case4",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case4.cljs"),
            ),
            Case::namespace(
                "walk-test-walk",
                "librarybody.case5",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case5.cljs"),
            ),
            Case::namespace(
                "walk-test-map-entry",
                "librarybody.case23",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case23.cljs"),
            ),
        ]);
    }

    #[test]
    fn set_test_bodies() {
        run(&[
            Case::namespace(
                "set-test-union",
                "librarybody.case8",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case8.cljs"),
            ),
            Case::namespace(
                "set-test-intersection",
                "librarybody.case9",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case9.cljs"),
            ),
            Case::namespace(
                "set-test-difference",
                "librarybody.case10",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case10.cljs"),
            ),
            Case::namespace(
                "set-test-select",
                "librarybody.case11",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case11.cljs"),
            ),
            Case::namespace(
                "set-test-project",
                "librarybody.case12",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case12.cljs"),
            ),
            Case::namespace(
                "set-test-rename-keys",
                "librarybody.case14",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case14.cljs"),
            ),
            Case::namespace(
                "set-test-index",
                "librarybody.case15",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case15.cljs"),
            ),
            Case::namespace(
                "set-test-join",
                "librarybody.case16",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case16.cljs"),
            ),
            Case::namespace(
                "set-test-subset-p",
                "librarybody.case18",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case18.cljs"),
            ),
            Case::namespace(
                "set-test-superset-p",
                "librarybody.case19",
                "(run)",
                include_str!("../fixtures/compiler-library-bodies/librarybody/case19.cljs"),
            ),
        ]);
    }

    #[test]
    fn string_test_bodies() {
        run(&[Case::namespace(
            "string-test-api",
            "librarybody.case20",
            "(run)",
            include_str!("../fixtures/compiler-library-bodies/librarybody/case20.cljs"),
        )]);
    }
}
