//! Unchanged complete arity oracles in actual applications and macro computations.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
type CompleteArityOracle<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a [(&'a str, &'a str)],
);
const ROWS: &[CompleteArityOracle<'_>] = &[
    (
        "cljs-definitions-var-test",
        "cljs.var-test",
        "(run nil)",
        "true",
        include_str!("fixtures/arity-after190/cljs-definitions-var-test.cljs"),
        &[],
    ),
    (
        "cljs-public-gate-core-factory-multifn",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-public-gate-core-factory-multifn.cljs"),
        &[],
    ),
    (
        "cljs-helpers-map-entry-test-map-entry-interface-tests",
        "user",
        "",
        "true",
        include_str!(
            "fixtures/arity-after190/cljs-helpers-map-entry-test-map-entry-interface-tests.cljs"
        ),
        &[],
    ),
    (
        "arity-receiver-transient",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/arity-receiver-transient.cljs"),
        &[],
    ),
    (
        "cljs-core-gate-literal-protocol-public",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-core-gate-literal-protocol-public.cljs"),
        &[],
    ),
    (
        "cljs-core-gate-literal-protocol-custom-state",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-core-gate-literal-protocol-custom-state.cljs"),
        &[],
    ),
    (
        "cljs-core-gate-literal-clone-find-protocol-workflow",
        "user",
        "",
        "true",
        include_str!(
            "fixtures/arity-after190/cljs-core-gate-literal-clone-find-protocol-workflow.cljs"
        ),
        &[],
    ),
    (
        "cljs-core-gate-literal-chunk-public-factories",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-core-gate-literal-chunk-public-factories.cljs"),
        &[],
    ),
    (
        "cljs-core-gate-literal-chunk-public-pipeline",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-core-gate-literal-chunk-public-pipeline.cljs"),
        &[],
    ),
    (
        "cljs-sequence-completion-src-test-cljs-cljs-iterator-test-coll-iter-seq-match",
        "user",
        "",
        "true",
        include_str!(
            "fixtures/arity-after190/cljs-sequence-completion-src-test-cljs-cljs-iterator-test-coll-iter-seq-match.cljs"
        ),
        &[],
    ),
    (
        "arity-after190-canonical-print-meta-unreadable",
        "user",
        "",
        "\"^{:x true} [x]\"",
        include_str!("fixtures/arity-after190/arity-after190-canonical-print-meta-unreadable.cljs"),
        &[],
    ),
    (
        "arity-receiver-sorted-full",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/arity-receiver-sorted-full.cljs"),
        &[],
    ),
    (
        "cljs-extension-test-cljs-2812",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-extension-test-cljs-2812.cljs"),
        &[],
    ),
    (
        "cljs-deftype-core",
        "cljs.core-test",
        "(run nil)",
        "true",
        include_str!("fixtures/arity-after190/cljs-deftype-core.cljs"),
        &[],
    ),
    (
        "cljs-sequences-collections-test-test-string",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-sequences-collections-test-test-string.cljs"),
        &[],
    ),
    (
        "arity-extended-language-helper-js-conversions",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/arity-extended-language-helper-js-conversions.cljs"),
        &[],
    ),
    (
        "cljs-extra-collections-test-phm-fixed-hash",
        "user",
        "",
        "true",
        include_str!("fixtures/arity-after190/cljs-extra-collections-test-phm-fixed-hash.cljs"),
        &[],
    ),
    (
        "cljs-namespace-completion-require-alias",
        "probe.require-alias",
        "(run nil)",
        "true",
        include_str!("fixtures/arity-after190/cljs-namespace-completion-require-alias.cljs"),
        &[(
            "clojure/set.cljs",
            include_str!(
                "fixtures/arity-after190/source-context/cljs-namespace-completion-require-alias/clojure/set.cljs"
            ),
        )],
    ),
    (
        "cljs-namespace-completion-use",
        "use.probe",
        "(run nil)",
        "true",
        include_str!("fixtures/arity-after190/cljs-namespace-completion-use.cljs"),
        &[(
            "clojure/set.cljs",
            include_str!(
                "fixtures/arity-after190/source-context/cljs-namespace-completion-use/clojure/set.cljs"
            ),
        )],
    ),
    (
        "cljs-namespace-completion-refer-core-exclude",
        "refer.exclude",
        "(run nil)",
        "true",
        include_str!("fixtures/arity-after190/cljs-namespace-completion-refer-core-exclude.cljs"),
        &[],
    ),
    (
        "cljs-namespace-completion-refer-global",
        "global.probe",
        "(run nil)",
        "true",
        include_str!("fixtures/arity-after190/cljs-namespace-completion-refer-global.cljs"),
        &[],
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
    let artifacts = std::env::var_os("LIGHT_ARITY_AFTER190_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-arity-after190-{}", std::process::id()))
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
    for &(id, namespace, invoke, expected, source, companions) in ROWS {
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
                for &(relative, companion) in companions {
                    let path = root.join(relative);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, companion).unwrap();
                }
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
                    "arity-after190-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{actual}"
                );
                let packet = node(&format!(
                    "const source = {js:?}; const program = {program:?}; const sha = x => require('node:crypto').createHash('sha256').update(x).digest('hex'); console.log(sha(source) + '\\t' + sha(program)); console.log(String(eval(source)));"
                ));
                let (hashes, actual) = packet.split_once('\n').expect("Node execution/hash packet");
                println!("arity-after190-artifacts\t{id}\t{lifecycle}\t{phase}\t{hashes}");
                for &(relative, companion) in companions {
                    let hash = node(&format!(
                        "console.log(require('node:crypto').createHash('sha256').update({companion:?}).digest('hex'));"
                    ));
                    println!(
                        "arity-after190-companion-artifacts\t{id}\t{lifecycle}\t{phase}\t{relative}\t{hash}"
                    );
                }
                assert_eq!(actual, expected, "{id} {lifecycle} {phase} Node");
                println!("arity-after190-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}
