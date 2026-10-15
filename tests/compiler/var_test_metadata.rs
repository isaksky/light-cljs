//! Executable definition metadata survives ordinary and demanded macro initialization.
use crate::temp_dir::TempDir;
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

fn both_namespace(namespace: &str, source: &str, host: &mut MacroHost, expected: &str) {
    let root = TempDir::new(&format!("metadata-reflection-{namespace}"));
    let path = root.join(format!(
        "{}.cljs",
        namespace.replace('.', "/").replace('-', "_")
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
    let js = compile_namespace_with_macros(namespace, &[root.to_path_buf()], host).unwrap();
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
fn direct_var_test_reflection_matches_exact_reference_source_in_both_phases() {
    let source = "(ns metadata.projection)\n(defn add {:test #(assert (= (add 2 3) 5))} [x y] (+ x y))\n(defn run [observe] (test add) (fn? (:test (meta #'add))))\nrun\n";
    let mut reused = proof_host();
    for _ in 0..2 {
        for phase in ["application", "macro"] {
            let suffix = if phase == "application" {
                "(pr-str (run nil))"
            } else {
                "(defmacro witness [] (run nil)) (pr-str (witness))"
            };
            both_namespace(
                "metadata.projection",
                &format!("{source}{suffix}"),
                &mut proof_host(),
                "true",
            );
            both_namespace(
                "metadata.projection",
                &format!("{source}{suffix}"),
                &mut reused,
                "true",
            );
        }
    }
}

#[test]
fn reflected_callbacks_are_real_stable_and_do_not_force_pending_roots() {
    let source = r#"
(ns metadata.pending)
(def roots (atom 0))
(def attached (atom 0))
(def called (atom 0))
(def ^{:doc "kept" :test (do (swap! attached inc) (fn [] (swap! called inc) :callback))}
  pending (do (swap! roots inc) (fn [] :ready)))
(def unrelated (do (swap! roots + 100) :unrelated))
(swap! roots + 1000)
(defn inspect-before []
  (let [v #'pending
        untouched (test (with-meta v {:test (fn [] :override)}))
        identity-count @attached
        m (meta v)
        callback (:test m)
        before @roots
        result (callback)
        override (with-meta v {:test (fn [] :override)})]
    [identity-count before @attached @called result
     (fn? callback) (identical? callback (:test (meta v)))
     (= 'pending (:name m)) (= 'metadata.pending (:ns m)) (= "kept" (:doc m))
     (string? (:file m)) (pos? (:line m)) (pos? (:column m))
     ((:test (meta override)))
     (pending) @roots (identical? callback (:test (meta #'pending)))
     (identical? callback (.-cljs$lang$test pending))
     (= (dissoc m :test) (dissoc (meta #'pending) :test))
     (test pending) (test v) @called @attached]))
(defmacro witness [] (list 'quote (inspect-before)))
(pr-str [(witness) @roots @called @attached])
"#;
    let expected = "[[0 0 1 1 :callback true true true true true true true true :override :ready 1 true true true :ok :ok 3 1] 1101 0 1]";
    let mut host = proof_host();
    for _ in 0..2 {
        both_namespace("metadata.pending", source, &mut host, expected);
    }
}

#[test]
fn reflected_metadata_failure_is_sticky_and_clean_hosts_recover() {
    let mut host = proof_host();
    let failure = r#"
(def attempts (atom 0))
(def ^{:test (do (swap! attempts inc) (throw (ex-info "metadata failed" {})))}
  broken (fn [] :never))
(defmacro witness []
  (let [a (try (meta #'broken) :bad (catch :default _ :failed))
        b (try (meta #'broken) :bad (catch :default _ :failed))]
    (if (= [:failed :failed 1] [a b @attempts])
      (throw (ex-info "verified sticky metadata failure" {}))
      (throw (ex-info "wrong metadata failure state" {})))))
(witness)
"#;
    for _ in 0..2 {
        let error = light_cljs::compile_with_macros(failure, &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("verified sticky metadata failure"),
            "{error}"
        );
        both(
            "(defn repaired {:test (fn [] :repaired)} [] 42) (defmacro witness [] (and (= :repaired ((:test (meta #'repaired)))) (= :ok (test repaired)))) (pr-str (witness))",
            &mut host,
            "true",
        );
    }
}

#[test]
fn reflected_callback_generations_keep_saved_values_and_current_root_reads() {
    let source = r#"
(defn target {:test (fn [] [:old (target)])} [] 1)
(def saved (:test (meta #'target)))
(defmacro first-witness [] (assert (= [:old 1] (saved))) nil)
(first-witness)
(defn target {:test (fn [] [:new (target)])} [] 2)
(defmacro second-witness []
  (list 'quote [(saved) ((:test (meta #'target)))
               (identical? saved (:test (meta #'target))) (test #'target)]))
(pr-str [(second-witness) (saved) ((:test (meta #'target)))])
"#;
    let mut host = proof_host();
    for _ in 0..2 {
        both(
            source,
            &mut host,
            "[[[:old 2] [:new 2] false :ok] [:old 2] [:new 2]]",
        );
    }
}

#[test]
fn executed_nested_definitions_expose_callback_metadata_and_overlays() {
    both(
        r#"
(declare nested)
(defn install []
  (defn nested {:test (fn [] :nested)} [] 42)
  [((:test (meta #'nested))) (test #'nested)])
(defmacro witness []
  (list 'quote [(install)
               (with-redefs [nested (fn [] :overlay)]
                 [(nil? (:test (meta #'nested))) (nested)])
               ((:test (meta #'nested)))]))
(pr-str [(witness) (install)])
"#,
        &mut proof_host(),
        "[[[:nested :ok] [true :overlay] :nested] [:nested :ok]]",
    );
}

#[test]
fn var_metadata_snapshots_ready_callback_properties() {
    let definitions = r#"
(defn target {:test (fn [] :old)} [] 42)
(defn run []
  (let [root target v #'target]
    (set! (.-cljs$lang$test target) (fn [] :new))
    [((:test (meta v))) ((:test (meta #'target))) (test v) (test target)]))
"#;
    both(
        &format!("{definitions} (pr-str (run))"),
        &mut proof_host(),
        "[:old :new :ok :ok]",
    );
    both(
        &format!("{definitions} (defmacro witness [] (list 'quote (run))) (pr-str (witness))"),
        &mut proof_host(),
        "[:old :new :ok :ok]",
    );
}

#[test]
fn declarations_without_initializers_do_not_attach_executable_tests() {
    let definition =
        "(def ^{:test (fn [] :unattached)} absent) (def nil-root nil) (def false-root false)";
    both(
        &format!(
            "{definition} (pr-str (and (nil? (:test (meta #'absent))) (not (undefined? (:test (meta #'nil-root)))) (not (undefined? (:test (meta #'false-root))))))"
        ),
        &mut proof_host(),
        "true",
    );
    both(
        &format!(
            "{definition} (defmacro witness [] (let [n nil-root f false-root] (and (nil? (:test (meta #'absent))) (not (undefined? (:test (meta #'nil-root)))) (not (undefined? (:test (meta #'false-root))))))) (pr-str (witness))"
        ),
        &mut proof_host(),
        "true",
    );
}
