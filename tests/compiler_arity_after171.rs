//! Unchanged complete arity oracles in actual applications and macro computations.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
const ROWS: &[(&str, &str, bool, &str)] = &[
    (
        "cljs-equality-hashing-test-test-962-empty-literal-hashes",
        "user",
        false,
        include_str!(
            "fixtures/arity-after171/cljs-equality-hashing-test-test-962-empty-literal-hashes.cljs"
        ),
    ),
    (
        "cljs-equality-hashing-test-test-nil-hashing-cljs-1649",
        "user",
        false,
        include_str!(
            "fixtures/arity-after171/cljs-equality-hashing-test-test-nil-hashing-cljs-1649.cljs"
        ),
    ),
    (
        "cljs-equality-hashing-test-test-mumur-support",
        "user",
        false,
        include_str!("fixtures/arity-after171/cljs-equality-hashing-test-test-mumur-support.cljs"),
    ),
    (
        "cljs-equality-hashing-test-test-cljs-3410",
        "user",
        false,
        include_str!("fixtures/arity-after171/cljs-equality-hashing-test-test-cljs-3410.cljs"),
    ),
    (
        "cljs-printing-completion-test-print-with-opts",
        "cljs.printing-test",
        true,
        include_str!("fixtures/arity-after171/cljs-printing-completion-test-print-with-opts.cljs"),
    ),
    (
        "cljs-public-gate-iterator-helper-contract",
        "user",
        false,
        include_str!("fixtures/arity-after171/cljs-public-gate-iterator-helper-contract.cljs"),
    ),
    (
        "cljs-multimethods-test-hierarchy",
        "cljs.core-test",
        true,
        include_str!("fixtures/arity-after171/cljs-multimethods-test-hierarchy.cljs"),
    ),
    (
        "arity-resume-core-factory-body-core-test-test-multimethods-1",
        "user",
        false,
        include_str!(
            "fixtures/arity-after171/arity-resume-core-factory-body-core-test-test-multimethods-1.cljs"
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
    let artifacts = std::env::var_os("LIGHT_ARITY_AFTER171_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-arity-after171-{}", std::process::id()))
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
                let program = if invoke {
                    if phase == "macro" {
                        format!(
                            "{source}\n(defmacro arity-witness [] (run nil))\n(pr-str (arity-witness))\n"
                        )
                    } else {
                        format!("{source}\n(pr-str (run nil))\n")
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
                    "arity-after171-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{actual}"
                );
                let actual = node(&format!("console.log(String({js}));"));
                assert_eq!(actual, "true", "{id} {lifecycle} {phase} Node");
                println!("arity-after171-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}
