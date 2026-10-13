//! Unchanged complete arity oracles in actual applications and macro computations.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
const ROWS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "cljs-transients-collections-test-test-transient-edge-case-2",
        "user",
        "",
        "true",
        include_str!(
            "../fixtures/arity-after185/cljs-transients-collections-test-test-transient-edge-case-2.cljs"
        ),
    ),
    (
        "cljs-residual-completion-cljs-2866-test",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-residual-completion-cljs-2866-test.cljs"),
    ),
    (
        "cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test",
        "user",
        "",
        "true",
        include_str!(
            "../fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aget-test.cljs"
        ),
    ),
    (
        "cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aset-test",
        "user",
        "",
        "true",
        include_str!(
            "../fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-checked-aset-test.cljs"
        ),
    ),
    (
        "cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-aget-test",
        "user",
        "",
        "true",
        include_str!(
            "../fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-aget-test.cljs"
        ),
    ),
    (
        "cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-set-test",
        "user",
        "",
        "true",
        include_str!(
            "../fixtures/arity-after185/cljs-utilities-completion-src-test-cljs-cljs-array-access-test-unchecked-set-test.cljs"
        ),
    ),
    (
        "cljs-printing-completion-test-print-knobs",
        "cljs.printing-test",
        "(run nil)",
        "true",
        include_str!("../fixtures/arity-after185/cljs-printing-completion-test-print-knobs.cljs"),
    ),
    (
        "arity-extended-language-helper-printing",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/arity-extended-language-helper-printing.cljs"),
    ),
    (
        "cljs-residual-source-context-apply",
        "cljs.apply-test",
        "(run)",
        "true",
        include_str!("../fixtures/arity-after185/cljs-residual-source-context-apply.cljs"),
    ),
    (
        "cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests",
        "user",
        "",
        "[true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true true]",
        include_str!(
            "../fixtures/arity-after185/cljs-residual-body-cljs-equality-map-entry-test-all-map-entry-tests.cljs"
        ),
    ),
    (
        "cljs-core-gate-literal-print-callback-failure",
        "user",
        "",
        "true",
        include_str!(
            "../fixtures/arity-after185/cljs-core-gate-literal-print-callback-failure.cljs"
        ),
    ),
    (
        "cljs-destructure-trailing-map-destructuring",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-destructure-trailing-map-destructuring.cljs"),
    ),
    (
        "cljs-exceptions-throwable-map",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-exceptions-throwable-map.cljs"),
    ),
    (
        "cljs-state-test-atoms-and-volatile",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-state-test-atoms-and-volatile.cljs"),
    ),
    (
        "cljs-definitions-test-var-predicate",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-definitions-test-var-predicate.cljs"),
    ),
    (
        "cljs-definitions-test-resolve",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-definitions-test-resolve.cljs"),
    ),
    (
        "cljs-definitions-test-cljs-3333",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-definitions-test-cljs-3333.cljs"),
    ),
    (
        "cljs-interop-core-test-memfn-test",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-interop-core-test-memfn-test.cljs"),
    ),
    (
        "cljs-interop-primitives-test-test-arrays",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-interop-primitives-test-test-arrays.cljs"),
    ),
    (
        "cljs-extension-test-cljs-2974",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extension-test-cljs-2974.cljs"),
    ),
    (
        "cljs-extension-test-414",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extension-test-414.cljs"),
    ),
    (
        "cljs-extra-seqs-test-distinct",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-seqs-test-distinct.cljs"),
    ),
    (
        "cljs-extra-primitives-test-misc",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-primitives-test-misc.cljs"),
    ),
    (
        "cljs-extra-seqs-test-partitionv-all",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-seqs-test-partitionv-all.cljs"),
    ),
    (
        "cljs-extra-seqs-test-partitionv",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-seqs-test-partitionv.cljs"),
    ),
    (
        "cljs-extra-primitives-test-496",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-primitives-test-496.cljs"),
    ),
    (
        "cljs-integration-test-run-bang",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-integration-test-run-bang.cljs"),
    ),
    (
        "cljs-extra-primitives-test-arithmetic",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-primitives-test-arithmetic.cljs"),
    ),
    (
        "cljs-extra-core-js-invoke-test",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-core-js-invoke-test.cljs"),
    ),
    (
        "cljs-extra-core-test-cljs-3400",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-extra-core-test-cljs-3400.cljs"),
    ),
    (
        "cljs-final-seqs-iteration-seq-equals-reduce",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-final-seqs-iteration-seq-equals-reduce.cljs"),
    ),
    (
        "cljs-protocol-test-extend-to-object",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after185/cljs-protocol-test-extend-to-object.cljs"),
    ),
];
fn proof_host() -> MacroHost {
    let mut host = MacroHost::new();
    host.evaluator_options.timeout = std::time::Duration::from_secs(120);
    host
}

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
    let artifacts = std::env::var_os("LIGHT_ARITY_AFTER185_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-arity-after185-{}", std::process::id()))
        });
    let mut reused = proof_host();
    let warm = artifacts.join("warmup");
    std::fs::create_dir_all(&warm).unwrap();
    std::fs::write(
        warm.join("warm.cljs"),
        "(ns warm) (defmacro initialized [] :warm) (initialized)",
    )
    .unwrap();
    let warmed = compile_namespace_with_macros("warm", &[warm], &mut reused).unwrap();
    std::fs::write(artifacts.join("warmup.js"), warmed).unwrap();
    for &(id, namespace, invoke, expected, source) in ROWS {
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
                        "(ns user)\n(defmacro arity-witness [] (do {source}))\n(pr-str (arity-witness))\n"
                    )
                } else {
                    format!("(ns user)\n(pr-str (do {source}))\n")
                };
                let root = artifacts.join(id).join(lifecycle).join(phase);
                let relative = namespace.replace('.', "/").replace('-', "_") + ".cljs";
                let path = root.join(relative);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &program).unwrap();
                let mut fresh = proof_host();
                let host = if lifecycle == "fresh" {
                    &mut fresh
                } else {
                    &mut reused
                };
                let js =
                    compile_namespace_with_macros(namespace, std::slice::from_ref(&root), host)
                        .unwrap_or_else(|e| panic!("{id} {lifecycle} {phase}: {e}"));
                std::fs::write(root.join("compiled.js"), &js).unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions {
                    timeout: std::time::Duration::from_secs(120),
                    ..EvaluatorOptions::default()
                })
                .unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&js).unwrap())
                    .unwrap();
                assert_eq!(actual, expected, "{id} {lifecycle} {phase} QuickJS");
                println!(
                    "arity-after185-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{actual}"
                );
                let packet = node(&format!(
                    "const source = {js:?}; const program = {program:?}; const sha = x => require('node:crypto').createHash('sha256').update(x).digest('hex'); console.log(sha(source) + '\\t' + sha(program)); console.log(String(eval(source)));"
                ));
                let (hashes, actual) = packet.split_once('\n').expect("Node execution/hash packet");
                println!("arity-after185-artifacts\t{id}\t{lifecycle}\t{phase}\t{hashes}");
                assert_eq!(actual, expected, "{id} {lifecycle} {phase} Node");
                println!("arity-after185-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}
