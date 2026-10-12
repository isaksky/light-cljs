//! Unchanged complete arity oracles in actual applications and macro computations.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
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
#[ignore = "historical pre-repair metadata test callback phase diagnostic; zero semantic credit"]
fn metadata_test_original_source_preserves_phase_failures_and_recovery() {
    let artifacts = std::env::var_os("LIGHT_TEST_METADATA_AFTER188_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "light-test-metadata-after188-{}",
                std::process::id()
            ))
        });
    let source = include_str!("fixtures/arity-after188/cljs-definitions-var-test.cljs");
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
    for lifecycle in ["fresh", "reused"] {
        for phase in ["application", "macro"] {
            let program = if phase == "macro" {
                format!(
                    "{source}\n(defmacro metadata-witness [] (run nil))\n(pr-str (metadata-witness))\n"
                )
            } else {
                format!("{source}\n(pr-str (run nil))\n")
            };
            let root = artifacts.join(lifecycle).join(phase);
            std::fs::create_dir_all(root.join("cljs")).unwrap();
            std::fs::write(root.join("cljs/var_test.cljs"), &program).unwrap();
            let mut fresh = proof_host();
            let host = if lifecycle == "fresh" {
                &mut fresh
            } else {
                &mut reused
            };
            let compiled =
                compile_namespace_with_macros("cljs.var-test", std::slice::from_ref(&root), host);
            if phase == "application" {
                let js = compiled.unwrap();
                std::fs::write(root.join("compiled.js"), &js).unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&js).unwrap())
                    .unwrap();
                assert_eq!(actual, "true");
                println!(
                    "test-metadata-after188-observation\t{lifecycle}\t{phase}\tQuickJS\t{actual}"
                );
                let packet = node(&format!(
                    "const source = {js:?}; const program = {program:?}; const sha = x => require('node:crypto').createHash('sha256').update(x).digest('hex'); console.log(sha(source) + '\\t' + sha(program)); console.log(String(eval(source)));"
                ));
                let (hashes, actual) = packet.split_once('\n').unwrap();
                assert_eq!(actual, "true");
                println!("test-metadata-after188-artifacts\t{lifecycle}\t{phase}\t{hashes}");
                println!(
                    "test-metadata-after188-observation\t{lifecycle}\t{phase}\tNode\t{actual}"
                );
            } else {
                let detail = compiled.unwrap_err().to_string();
                assert!(
                    detail.contains("Assert failed: (= :ok (test cljs-3411-function))"),
                    "{detail}"
                );
                std::fs::write(root.join("diagnostic.txt"), &detail).unwrap();
                println!(
                    "test-metadata-after188-diagnostic\t{lifecycle}\t{phase}\t{}",
                    root.join("diagnostic.txt").display()
                );
            }
            // Recovery uses the identical MacroHost after the original computation.
            let recovery = root.join("recovery");
            std::fs::create_dir_all(&recovery).unwrap();
            let program =
                "(ns user) (defmacro recovered-after188 [] (+ 40 2)) (pr-str (recovered-after188))";
            std::fs::write(recovery.join("user.cljs"), program).unwrap();
            let js = compile_namespace_with_macros("user", std::slice::from_ref(&recovery), host)
                .unwrap();
            std::fs::write(recovery.join("compiled.js"), &js).unwrap();
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let actual = evaluator
                .string(evaluator.evaluate_js(&js).unwrap())
                .unwrap();
            assert_eq!(actual, "42");
            println!("test-metadata-after188-recovery\t{lifecycle}\t{phase}\tQuickJS\t{actual}");
            let packet = node(&format!(
                "const source = {js:?}; const program = {program:?}; const sha = x => require('node:crypto').createHash('sha256').update(x).digest('hex'); console.log(sha(source) + '\\t' + sha(program)); console.log(String(eval(source)));"
            ));
            let (hashes, actual) = packet.split_once('\n').unwrap();
            assert_eq!(actual, "42");
            println!("test-metadata-after188-recovery-artifacts\t{lifecycle}\t{phase}\t{hashes}");
            println!("test-metadata-after188-recovery\t{lifecycle}\t{phase}\tNode\t{actual}");
        }
    }
}
