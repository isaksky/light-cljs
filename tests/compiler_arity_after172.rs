//! Unchanged complete arity oracles in actual applications and macro computations.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
const ROWS: &[(&str, &str, &str, &str)] = &[
    (
        "arity-resume-core-factory-multifn",
        "user",
        "",
        include_str!("fixtures/arity-after172/arity-resume-core-factory-multifn.cljs"),
    ),
    (
        "arity-resume-core-factory-body-core-test-test-transducers",
        "user",
        "",
        include_str!(
            "fixtures/arity-after172/arity-resume-core-factory-body-core-test-test-transducers.cljs"
        ),
    ),
    (
        "cljs-sequence-completion-src-test-cljs-cljs-interop-test-test-es6-interfaces",
        "user",
        "",
        include_str!(
            "fixtures/arity-after172/cljs-sequence-completion-src-test-cljs-cljs-interop-test-test-es6-interfaces.cljs"
        ),
    ),
    (
        "cljs-utilities-completion-src-test-cljs-cljs-parse-test-test-parse-uuid",
        "user",
        "",
        include_str!(
            "fixtures/arity-after172/cljs-utilities-completion-src-test-cljs-cljs-parse-test-test-parse-uuid.cljs"
        ),
    ),
    (
        "cljs-residual-body-0",
        "residualbody.case0",
        "(run)",
        include_str!("fixtures/arity-after172/cljs-residual-body-0.cljs"),
    ),
    (
        "cljs-residual-body-2",
        "residualbody.case2",
        "(run)",
        include_str!("fixtures/arity-after172/cljs-residual-body-2.cljs"),
    ),
    (
        "cljs-residual-body-10",
        "residualbody.case10",
        "(run)",
        include_str!("fixtures/arity-after172/cljs-residual-body-10.cljs"),
    ),
    (
        "cljs-multimethods-test-multimethods-2",
        "cljs.core-test",
        "(run nil)",
        include_str!("fixtures/arity-after172/cljs-multimethods-test-multimethods-2.cljs"),
    ),
    (
        "arity-resume-public-constructor-body-collections-test-test-vectors",
        "user",
        "",
        include_str!(
            "fixtures/arity-after172/arity-resume-public-constructor-body-collections-test-test-vectors.cljs"
        ),
    ),
    (
        "arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds",
        "user",
        "",
        include_str!(
            "fixtures/arity-after172/arity-resume-public-protocol-body-src-test-cljs-cljs-predicates-test-test-preds.cljs"
        ),
    ),
    (
        "cljs-sorted-seqs-test-test-empy-and-seq",
        "user",
        "",
        include_str!("fixtures/arity-after172/cljs-sorted-seqs-test-test-empy-and-seq.cljs"),
    ),
    (
        "arity-resume-core-factory-body-core-test-test-multimethods-1",
        "user",
        "",
        include_str!(
            "fixtures/arity-after172/arity-resume-core-factory-body-core-test-test-multimethods-1.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-replace",
        "librarybody.case0",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-replace.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-replace",
        "librarybody.case1",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-replace.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-cljs-walk-test-t-stringify-keys",
        "librarybody.case2",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-stringify-keys.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-order",
        "librarybody.case3",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-prewalk-order.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-order",
        "librarybody.case4",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-t-postwalk-order.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-cljs-walk-test-test-walk",
        "librarybody.case5",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-cljs-walk-test-test-walk.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-union",
        "librarybody.case8",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-union.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-intersection",
        "librarybody.case9",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-intersection.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-difference",
        "librarybody.case10",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-difference.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-select",
        "librarybody.case11",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-select.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-project",
        "librarybody.case12",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-project.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-rename-keys",
        "librarybody.case14",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-rename-keys.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-index",
        "librarybody.case15",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-index.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-join",
        "librarybody.case16",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-join.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-subset-p",
        "librarybody.case18",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-subset-p.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-set-test-test-superset-p",
        "librarybody.case19",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-set-test-test-superset-p.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-string-test-test-api",
        "librarybody.case20",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-string-test-test-api.cljs"
        ),
    ),
    (
        "cljs-library-body-src-test-cljs-clojure-walk-test-test-map-entry",
        "librarybody.case23",
        "(run)",
        include_str!(
            "fixtures/arity-after172/cljs-library-body-src-test-cljs-clojure-walk-test-test-map-entry.cljs"
        ),
    ),
];
fn node(source: &str) -> String {
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
        .write_all(source.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty(), "unexpected stderr");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
#[test]
fn complete_arity_oracles_execute_in_actual_phases_and_lifecycles() {
    let artifacts = std::env::var_os("LIGHT_ARITY_AFTER172_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-arity-after172-{}", std::process::id()))
        });
    let mut reused = MacroHost::new();
    let warm = artifacts.join("warmup");
    std::fs::create_dir_all(&warm).unwrap();
    std::fs::write(
        warm.join("warm.cljs"),
        "(ns warm) (defmacro initialized [] :warm) (initialized)",
    )
    .unwrap();
    let warmed = compile_namespace_with_macros("warm", &[warm], &mut reused).unwrap();
    std::fs::write(artifacts.join("warmup.js"), warmed).unwrap();
    for &(id, namespace, invoke, source) in ROWS {
        for lifecycle in ["fresh", "reused"] {
            for phase in ["application", "macro"] {
                // Namespace cases retain their entire setup and unchanged run function.
                // Expression cases execute their unchanged complete expression as the macro body.
                let program = if !invoke.is_empty() {
                    if phase == "macro" {
                        format!(
                            "{source}\n(defmacro arity-witness [] {invoke})\n(pr-str (arity-witness))\n"
                        )
                    } else {
                        format!("{source}\n(pr-str {invoke})\n")
                    }
                } else if phase == "macro" {
                    format!(
                        "(ns user)\n(defmacro arity-witness [] {source})\n(pr-str (arity-witness))\n"
                    )
                } else {
                    format!("(ns user)\n(pr-str {source})\n")
                };
                let root = artifacts.join(id).join(lifecycle).join(phase);
                let relative = namespace.replace('.', "/").replace('-', "_") + ".cljs";
                let path = root.join(relative);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, program).unwrap();
                let mut fresh = MacroHost::new();
                let host = if lifecycle == "fresh" {
                    &mut fresh
                } else {
                    &mut reused
                };
                let js = compile_namespace_with_macros(namespace, std::slice::from_ref(&root), host)
                    .unwrap_or_else(|e| panic!("{id} {lifecycle} {phase}: {e}"));
                std::fs::write(root.join("compiled.js"), &js).unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&js).unwrap())
                    .unwrap();
                assert_eq!(actual, "true", "{id} {lifecycle} {phase} QuickJS");
                println!(
                    "arity-after172-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{actual}"
                );
                let actual = node(&format!("console.log(String({js}));"));
                assert_eq!(actual, "true", "{id} {lifecycle} {phase} Node");
                println!("arity-after172-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}
