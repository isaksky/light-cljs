//! Unchanged complete arity oracles in actual applications and macro computations.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
const ROWS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "cljs-extra-core-test-cljs-2944",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-extra-core-test-cljs-2944.cljs"),
    ),
    (
        "cljs-deftype-core",
        "cljs.core-test",
        "(run nil)",
        "true",
        include_str!("../fixtures/arity-after188/cljs-deftype-core.cljs"),
    ),
    (
        "local-api-source-sci-lazy-probe-doall-nil",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/local-api-source-sci-lazy-probe-doall-nil.cljs"),
    ),
    (
        "cljs-extension-test-cljs-2812",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-extension-test-cljs-2812.cljs"),
    ),
    (
        "cljs-final-core-uri-predicate",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-final-core-uri-predicate.cljs"),
    ),
    (
        "cljs-extra-seqs-test-iteration-opts",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-extra-seqs-test-iteration-opts.cljs"),
    ),
    (
        "cljs-final-seqs-test-iteration",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-final-seqs-test-iteration.cljs"),
    ),
    (
        "cljs-public-gate-core-factory-multifn",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-public-gate-core-factory-multifn.cljs"),
    ),
    (
        "public-string-buffer-writer",
        "user",
        "",
        "[\"a\" nil \"a\"]",
        include_str!("../fixtures/arity-after188/public-string-buffer-writer.cljs"),
    ),
    (
        "cljs-extra-seqs-test-rearrange-sequential",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-extra-seqs-test-rearrange-sequential.cljs"),
    ),
    (
        "cljs-sequence-completion-core-test-test-transducers",
        "user",
        "",
        "true",
        include_str!(
            "../fixtures/arity-after188/cljs-sequence-completion-core-test-test-transducers.cljs"
        ),
    ),
    (
        "public-print-writer-factories",
        "user",
        "",
        "[true true [[\"a\" true]] [[\"a\" true] [\"b\" true]] [[\"a\" true] [\"b\" true] [\"c\" true]] nil [[\"a\" true] [\"b\" true] [\"c\" true]]]",
        include_str!("../fixtures/arity-after188/public-print-writer-factories.cljs"),
    ),
    (
        "cljs-control-const-emission",
        "user",
        "",
        "true",
        include_str!("../fixtures/arity-after188/cljs-control-const-emission.cljs"),
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
fn new_arity_oracles_execute_in_actual_phases_and_lifecycles() {
    let artifacts = std::env::var_os("LIGHT_ARITY_AFTER188_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-arity-after188-{}", std::process::id()))
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
                    "arity-after188-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{actual}"
                );
                let packet = node(&format!(
                    "const source = {js:?}; const program = {program:?}; const sha = x => require('node:crypto').createHash('sha256').update(x).digest('hex'); console.log(sha(source) + '\\t' + sha(program)); console.log(String(eval(source)));"
                ));
                let (hashes, actual) = packet.split_once('\n').expect("Node execution/hash packet");
                println!("arity-after188-artifacts\t{id}\t{lifecycle}\t{phase}\t{hashes}");
                assert_eq!(actual, expected, "{id} {lifecycle} {phase} Node");
                println!("arity-after188-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}
