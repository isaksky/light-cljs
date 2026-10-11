//! Canonical ECMAScript regex execution and finite macro-form transfer.
use light_cljs::eval_session::{EvalOptions, EvalSession};
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace, compile_with_macros};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
const CONTRACT: &str = include_str!("fixtures/compiler-regex-reference/reference/main.cljs");
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sources(PathBuf);
impl Sources {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "light-regex-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("reference")).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(self.0.join(name), source).unwrap();
    }
    fn compile(&self) -> String {
        compile_namespace("main", std::slice::from_ref(&self.0)).unwrap()
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn node(script: &str) -> String {
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
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn both(javascript: &str, expected: &str) {
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let result = e.evaluate_js(javascript).unwrap();
    assert_eq!(e.print(result).unwrap(), expected, "QuickJS");
    // Namespace bundles return their final expression's native JS representation.
    assert_eq!(
        node(&format!("console.log(String({javascript}));")),
        expected,
        "Node"
    );
}
#[test]
fn regex_api_arities_and_complete_upstream_bodies_execute_in_both_phases() {
    let sources = Sources::new();
    sources.write("reference/main.cljs", CONTRACT);
    sources.write(
        "macros.cljs",
        "(ns macros (:require [reference.main :as r])) (defmacro verify [] (list 'quote (r/run)))",
    );
    sources.write("main.cljs", "(ns main (:require [reference.main :as r] [macros :refer [verify]])) (= [[23 true] true] (r/run) (verify))");
    both(&sources.compile(), "true");
}
#[test]
fn regex_macro_results_preserve_flags_tags_and_diagnose_host_values() {
    let mut host = MacroHost::new();
    let source = r#"(defmacro regex [] (js/RegExp. "a(.)" "imsu"))
      (defmacro lone [] (js/RegExp. "\uD800" "u"))
      (defmacro tags [x] (list 'quote x))
      (let [re (regex) tags (tags [#"(?i)a" #inst "2020-01-01" #uuid "00000000-0000-0000-0000-000000000001" #js [1 "\uD800"]])]
        (and (= "a(.)" (.-source re)) (= "imsu" (.-flags re))
             (= ["Ab" "b"] (re-find re "Ab"))
             (= "\uD800" (re-find (lone) "\uD800"))
             (= "i" (.-flags (nth tags 0)))
             (regexp? (nth tags 0)) (inst? (nth tags 1)) (uuid? (nth tags 2))
             (= "\uD800" (aget (nth tags 3) 1))))"#;
    both(&compile_with_macros(source, &mut host).unwrap(), "true");
    let error = compile_with_macros("(defmacro bad [] (js/Object.)) (bad)", &mut host)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Unsupported macro result"), "{error}");
    assert!(error.contains("bad") && error.contains("$"), "{error}");
    assert!(
        compile_with_macros(
            "(defmacro broken [] (re-pattern \"[\")) (broken)",
            &mut host
        )
        .is_err()
    );
    both(
        &compile_with_macros("(= (re-find #\"a\" \"a\") \"a\")", &mut host).unwrap(),
        "true",
    );
}
#[test]
fn regex_failures_reuse_sessions_and_reset_clears_state_and_bounds() {
    let mut session = EvalSession::new(EvalOptions {
        max_source_bytes: 1024,
        max_output_bytes: 64,
        ..Default::default()
    })
    .unwrap();
    let error = session
        .eval_string("(re-pattern \"[\")")
        .unwrap_err()
        .message;
    assert!(
        error.contains("unexpected end") && error.contains("RegExp"),
        "{error}"
    );
    let value = session
        .eval_string("(def re (js/RegExp. \"a\" \"g\")) (re-find re \"aba\")")
        .unwrap()
        .value;
    assert_eq!(session.print(value).unwrap(), "\"a\"");
    let value = session.eval_string("(.-lastIndex re)").unwrap().value;
    assert_eq!(session.print(value).unwrap(), "1");
    assert!(
        session
            .eval_string(&format!("\"{}\"", "a".repeat(1024)))
            .is_err()
    );
    assert!(
        session
            .eval_string("(println (vec (re-seq #\".\" (apply str (repeat 100 \"a\")))))")
            .is_err()
    );
    session.reset().unwrap();
    let value = session
        .eval_string("(re-matches #\"a+\" \"aaa\")")
        .unwrap()
        .value;
    assert_eq!(session.print(value).unwrap(), "\"aaa\"");
    assert!(session.eval_string("re").is_err());
}
#[test]
fn built_in_backtracking_observes_budget_deadline_and_cancellation() {
    // The bounded input causes pathological backtracking inside native RegExp,
    // not an infinite JS loop. QuickJS 0.14 polls its regexp VM itself.
    let work = "(()=>{try { /^(a+)+$/.test('a'.repeat(28)+'!'); } catch(e) {} return 42;})()";
    for options in [
        EvaluatorOptions {
            poll_budget: 100,
            ..Default::default()
        },
        EvaluatorOptions {
            timeout: Duration::from_millis(40),
            ..Default::default()
        },
    ] {
        let mut host = MacroHost::new();
        host.evaluator_options = options;
        let e = host.new_evaluator().unwrap();
        let error = e.evaluate_js(work).unwrap_err().0;
        assert!(
            error.contains("budget") || error.contains("deadline"),
            "{error}"
        );
        assert!(e.evaluate_js("42").is_err());
        let fresh = host.new_evaluator().unwrap();
        assert_eq!(
            fresh.number(fresh.evaluate_js("42").unwrap()).unwrap(),
            42.0
        );
    }
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let control = e.control();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        control.cancel();
    });
    assert!(e.evaluate_js(work).unwrap_err().0.contains("cancelled"));
    canceller.join().unwrap();
}
#[test]
#[ignore = "requires Java, Clojure CLI and both pinned CLJS references"]
fn regex_contract_passes_both_pinned_references() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let checkout = root.join("research/clojurescript");
    for (kind, identity, dependency, checkout_arg) in [
        (
            "release",
            "1.12.134",
            "{:mvn/version \"1.12.134\"}".to_owned(),
            "-".to_owned(),
        ),
        (
            "checkout",
            "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e",
            format!("{{:local/root {:?}}}", checkout),
            checkout.to_string_lossy().into_owned(),
        ),
    ] {
        let output_dir = Sources::new();
        let output = Command::new("clojure")
            .current_dir(&root)
            .args([
                "-J-Xmx1g",
                "-Srepro",
                "-Sdeps",
                &format!("{{:deps {{org.clojure/clojurescript {dependency}}}}}"),
                "-M",
                "tests/reference/execution.clj",
                kind,
                identity,
                &checkout_arg,
            ])
            .arg(&output_dir.0)
            .args([
                "program",
                "tests/fixtures/compiler-regex-reference",
                "reference.main",
                "run",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("REFERENCE IDENTITY {kind} {identity}"))
        );
        assert_eq!(
            node(&format!(
                "process.chdir({:?}); require({:?}); console.log(cljs.core.pr_str.call(null, globalThis.lightProgramRun()));",
                output_dir.0,
                output_dir.0.join("reference.js")
            )),
            "[[23 true] true]",
            "{kind}"
        );
    }
}
#[test]
fn upstream_regex_full_body_provenance_is_retained() {
    let bodies = include_str!("fixtures/compiler-upstream-regex-tests.cljs");
    for (source, marker, end) in [
        (
            include_str!("../research/clojurescript/src/test/cljs/cljs/primitives_test.cljs"),
            "(deftest test-regexps",
            "(deftest test-arrays",
        ),
        (
            include_str!("../research/clojurescript/src/test/cljs/cljs/core_test.cljs"),
            "(deftest test-cljs-2453",
            "(deftest test-cljs-2001",
        ),
    ] {
        let raw = source[source.find(marker).unwrap()..source.find(end).unwrap()].trim();
        let copied = &bodies[bodies.find(marker).unwrap()..];
        let copied = copied
            .split("(deftest test-cljs-2453")
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(copied)
            .trim();
        assert!(raw.starts_with(copied), "{marker}");
    }
}
