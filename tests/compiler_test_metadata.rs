//! Executable definition metadata survives ordinary and demanded macro initialization.
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
fn metadata_test_original_source_passes_both_phases_and_recovers() {
    let artifacts = std::env::var_os("LIGHT_TEST_METADATA_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-test-metadata-{}", std::process::id()))
        });
    std::fs::create_dir_all(&artifacts).unwrap();
    let executable = std::env::current_exe().unwrap();
    let executable_snapshot = artifacts.join("acceptance-test-executable");
    std::fs::copy(&executable, &executable_snapshot).unwrap();
    let executable_sha = node(&format!(
        "const fs = require('node:fs'), sha = p => require('node:crypto').createHash('sha256').update(fs.readFileSync(p)).digest('hex'); const actual = sha({:?}), saved = sha({:?}); if (actual !== saved) throw new Error('executable snapshot drift'); console.log(actual);",
        executable, executable_snapshot
    ));
    println!(
        "test-metadata-executable\t{}\t{}\t{executable_sha}",
        executable.display(),
        executable_snapshot.display()
    );
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
            {
                let js = compiled.unwrap();
                std::fs::write(root.join("compiled.js"), &js).unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&js).unwrap())
                    .unwrap();
                assert_eq!(actual, "true");
                println!("test-metadata-observation\t{lifecycle}\t{phase}\tQuickJS\t{actual}");
                let packet = node(&format!(
                    "const source = {js:?}; const program = {program:?}; const sha = x => require('node:crypto').createHash('sha256').update(x).digest('hex'); console.log(sha(source) + '\\t' + sha(program)); console.log(String(eval(source)));"
                ));
                let (hashes, actual) = packet.split_once('\n').unwrap();
                assert_eq!(actual, "true");
                println!("test-metadata-artifacts\t{lifecycle}\t{phase}\t{hashes}");
                println!("test-metadata-observation\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
            // Recovery uses the identical MacroHost after the original computation.
            let recovery = root.join("recovery");
            std::fs::create_dir_all(&recovery).unwrap();
            let program =
                "(ns user) (defmacro recovered-metadata [] (+ 40 2)) (pr-str (recovered-metadata))";
            std::fs::write(recovery.join("user.cljs"), program).unwrap();
            let js = compile_namespace_with_macros("user", std::slice::from_ref(&recovery), host)
                .unwrap();
            std::fs::write(recovery.join("compiled.js"), &js).unwrap();
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let actual = evaluator
                .string(evaluator.evaluate_js(&js).unwrap())
                .unwrap();
            assert_eq!(actual, "42");
            println!("test-metadata-recovery\t{lifecycle}\t{phase}\tQuickJS\t{actual}");
            let packet = node(&format!(
                "const source = {js:?}; const program = {program:?}; const sha = x => require('node:crypto').createHash('sha256').update(x).digest('hex'); console.log(sha(source) + '\\t' + sha(program)); console.log(String(eval(source)));"
            ));
            let (hashes, actual) = packet.split_once('\n').unwrap();
            assert_eq!(actual, "42");
            println!("test-metadata-recovery-artifacts\t{lifecycle}\t{phase}\t{hashes}");
            println!("test-metadata-recovery\t{lifecycle}\t{phase}\tNode\t{actual}");
        }
    }
}

fn both(source: &str, host: &mut MacroHost, expected: &str) {
    let js = light_cljs::compile_with_macros(source, host).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .string(evaluator.evaluate_js(&js).unwrap())
            .unwrap(),
        expected
    );
    assert_eq!(
        node(&format!("console.log(String(eval({js:?})));")),
        expected
    );
}

#[test]
fn callbacks_var_overrides_missing_and_non_callable_metadata_are_phase_local() {
    let definitions = r#"
(def hits (atom 0))
(def attached (atom 0))
(defn add {:test (do (swap! attached inc)
                    (fn [] (swap! hits inc) (assert (= (add 2 3) 5))))}
  [x y] (+ x y))
(defn plain [x] x)
(defn false-test {:test false} [x] x)
(defn invalid-test {:test 7} [x] x)
(defn run []
  (let [before @hits
        function-result (test add)
        var-result (test #'add)
        overridden (with-meta #'add {:test #(swap! hits inc)})
        override-result (test overridden)]
    [before @hits @attached function-result var-result override-result
     (add 4 4) (#'add 2 3)
     (test plain) (test #'plain) (test false-test) (test #'false-test)
     (test nil)
     (try (test invalid-test) false (catch :default _ true))
     (try (test #'invalid-test) false (catch :default _ true))]))
"#;
    let packet = "[0 3 1 :ok :ok :ok 8 5 :no-test :no-test :no-test :no-test :no-test true true]";
    let mut host = proof_host();
    for _ in 0..2 {
        both(&format!("{definitions}\n(pr-str (run))"), &mut host, packet);
        both(
            &format!(
                "{definitions}\n(defmacro witness [] (list 'quote (run)))\n(pr-str [(witness) @hits (run)])"
            ),
            &mut host,
            &format!("[{packet} 0 {packet}]"),
        );
    }
}

#[test]
fn callback_failure_preserves_same_host_recovery_and_nested_definitions() {
    let mut host = proof_host();
    let failing = r#"
(defn broken {:test #(throw (ex-info "metadata callback failed" {:reason :test}))} [] 1)
(defmacro witness [] (test #'broken))
(witness)
"#;
    for _ in 0..2 {
        let error = light_cljs::compile_with_macros(failing, &mut host)
            .unwrap_err()
            .to_string();
        assert!(error.contains("metadata callback failed"), "{error}");
        both(
            "(defmacro recovered [] (+ 40 2)) (pr-str (recovered))",
            &mut host,
            "42",
        );
    }
    both(
        r#"
(defn install []
  (defn nested {:test #(assert (= (nested 2 3) 5))} [x y] (+ x y))
  [(test nested) (test #'nested) (nested 4 4)])
(defmacro witness [] (list 'quote (install)))
(pr-str [(witness) (install)])
"#,
        &mut host,
        "[[:ok :ok 8] [:ok :ok 8]]",
    );
}
