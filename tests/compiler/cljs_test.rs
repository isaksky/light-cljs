//! The bundled `cljs.test`, required as `cljs.test` and as `clojure.test`:
//! assertions, contexts, `are`, map and function fixtures, `async` and
//! `^:async` tests, `test-ns-hook` and `run-all-tests`, in `--compile`
//! builds at every optimization level (uncached and cached), `--eval` and
//! `--repl`, with custom `assert-expr` methods and the macros' errors.
use crate::temp_dir::TempDir;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn light_cljs(dir: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn stderr(output: Output) -> String {
    assert!(!output.status.success(), "unexpected success");
    String::from_utf8(output.stderr).unwrap()
}

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// What a compiled program prints under Node, with the source root `root`
/// written as `SRC`.
fn run_node(js: &str, root: &Path) -> String {
    relative(crate::canonical_json::node_stdout(js), root)
}

/// `text` with the source root `root` written as `SRC`.
fn relative(text: String, root: &Path) -> String {
    let canonical = fs::canonicalize(root).unwrap();
    text.replace(canonical.to_str().unwrap(), "SRC")
        .replace(root.to_str().unwrap(), "SRC")
}

/// Four test namespaces, two requiring `cljs.test` and two `clojure.test`,
/// loaded by `app.suite`, and `app.runner`, which runs every namespace whose
/// name ends in `-test` and prints the fixtures' logs and the summary when
/// the run ends. `app.main` enables printing first, for Node.
fn suite_program(root: &Path) {
    write(
        root,
        "app/sum_test.cljs",
        r#"(ns app.sum-test
  (:require [cljs.test :as t :refer [deftest is testing are]]))

(deftest sums
  (testing "addition"
    (testing "of two"
      (is (= 4 (+ 2 2)))
      (is (= 5 (+ 2 2)) "bad math")))
  (are [x y] (= x y) 2 (+ 1 1) 4 (* 2 2)))

(deftest throws
  (is (thrown? js/Error (throw (js/Error. "boom"))))
  (is (thrown-with-msg? js/Error #"bo+m" (throw (js/Error. "boom"))))
  (is (thrown? js/Error (+ 1 2)))
  (is (instance? js/Date (js/Date.)))
  (is (some #{2} [1 2 3]))
  (is nil "never")
  (is (throw (js/Error. "unexpected"))))
"#,
    );
    write(
        root,
        "app/fixture_test.cljs",
        r#"(ns app.fixture-test
  (:require [clojure.test :refer [deftest is use-fixtures async]]))

(def log (atom []))

(use-fixtures :once {:before #(swap! log conj :once) :after #(swap! log conj :end-once)})
(use-fixtures :each {:before #(swap! log conj :each) :after #(swap! log conj :end-each)})

(deftest plain (swap! log conj :plain) (is true))

(deftest continued
  (async done
    (.then (js/Promise.resolve 2)
           (fn [v] (swap! log conj [:continued v]) (is (= 2 v)) (done)))))

(deftest ^:async awaited
  (let [v (await (js/Promise.resolve 3))]
    (swap! log conj [:awaited v])
    (is (= 3 v))))
"#,
    );
    write(
        root,
        "app/wrap_test.cljs",
        r#"(ns app.wrap-test
  (:require [clojure.test :as ct]))

(def log (atom []))

(defn once [f] (swap! log conj :once) (f) (swap! log conj :end-once))
(defn each [f] (swap! log conj :each) (f) (swap! log conj :end-each))
(ct/use-fixtures :once once)
(ct/use-fixtures :each each (ct/compose-fixtures each each))

(ct/deftest first-test (swap! log conj :first) (ct/is (= 2 (inc 1))))
(ct/deftest second-test (swap! log conj :second) (ct/is (pos? 1)))
"#,
    );
    write(
        root,
        "app/hook_test.cljs",
        r#"(ns app.hook-test
  (:require [cljs.test :refer [deftest is]]))

(deftest skipped (is false))
(deftest hooked (is (= :hook :hook)))
(defn test-ns-hook [] (hooked))
"#,
    );
    write(
        root,
        "app/suite.cljs",
        "(ns app.suite (:require [app.sum-test] [app.fixture-test] [app.wrap-test] [app.hook-test]))",
    );
    write(
        root,
        "app/runner.cljs",
        r#"(ns app.runner
  (:require [cljs.test :as t]
            [app.suite]))

(defmethod t/report [::t/default :end-run-tests] [m]
  (prn :logs @app.fixture-test/log @app.wrap-test/log)
  (prn :summary (dissoc m :type) (t/successful? m)))

(t/run-all-tests #"app\..*-test")
"#,
    );
    write(
        root,
        "app/print.cljs",
        "(ns app.print) (enable-console-print!)",
    );
    write(
        root,
        "app/main.cljs",
        "(ns app.main (:require [app.print] [app.runner]))",
    );
}

const SUITE_OUTPUT: &str = r#"
Testing app.sum-test

FAIL in (sums) (SRC/app/sum_test.cljs:8:7)
addition of two
bad math
expected: (= 5 (+ 2 2))
  actual: (not (= 5 4))

FAIL in (throws) (SRC/app/sum_test.cljs:14:3)
expected: (thrown? js/Error (+ 1 2))
  actual: nil

FAIL in (throws) (SRC/app/sum_test.cljs:17:3)
never
expected: nil
  actual: nil

ERROR in (throws) (SRC/app/sum_test.cljs:18:3)
expected: (throw (js/Error. "unexpected"))
  actual: #object[Error Error: unexpected]

Testing app.fixture-test

Testing app.wrap-test

Testing app.hook-test

Ran 8 tests containing 17 assertions.
3 failures, 1 errors.
:logs [:once :each :plain :end-each :each [:continued 2] :end-each :each [:awaited 3] :end-each :end-once] [:once :each :each :each :first :end-each :end-each :end-each :each :each :each :second :end-each :end-each :end-each :end-once]
:summary {:test 8, :pass 13, :fail 3, :error 1} false
"#;

fn stats_value(stderr: &str, key: &str) -> String {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key} ")))
        .unwrap_or_else(|| panic!("no {key} in {stderr}"))
        .to_owned()
}

#[test]
fn compile_runs_suites_at_every_level_uncached_and_cached() {
    let dir = TempDir::new("cljs-test-compile");
    let root = dir.join("src");
    suite_program(&root);
    let root_text = root.to_str().unwrap();
    for level in ["none", "prepare", "advanced"] {
        let js = stdout(light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root_text,
                "--no-cache",
                "--optimize",
                level,
            ],
            "",
        ));
        assert_eq!(run_node(&js, &root), SUITE_OUTPUT, "{level}");
    }
    let cache = dir.join("cache");
    let cached = || {
        let output = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root_text,
                "--cache-dir",
                cache.to_str().unwrap(),
                "--stats",
            ],
            "",
        );
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        (
            run_node(&stdout(output), &root),
            stats_value(&stderr, "cache.install.namespaces"),
            stats_value(&stderr, "cache.bypass.non-cacheable-input"),
        )
    };
    // Every namespace but the runner, whose `run-all-tests` lists the
    // build's namespaces, and its consumer is installed from the cache.
    assert_eq!(cached(), (SUITE_OUTPUT.into(), "0".into(), "2".into()));
    assert_eq!(cached(), (SUITE_OUTPUT.into(), "10".into(), "2".into()));
    // A test namespace added to the build, which the runner does not name,
    // is run by the cached build's `run-all-tests`.
    write(
        &root,
        "app/extra_test.cljs",
        "(ns app.extra-test (:require [clojure.test :refer [deftest is]])) (deftest extra (is (= 1 2)))",
    );
    write(
        &root,
        "app/suite.cljs",
        "(ns app.suite (:require [app.sum-test] [app.fixture-test] [app.wrap-test] [app.hook-test] [app.extra-test]))",
    );
    let (text, installed, _) = cached();
    assert_eq!(installed, "9");
    assert!(
        text.ends_with(
            "\nTesting app.extra-test\n\nFAIL in (extra) (SRC/app/extra_test.cljs:1:82)\n\
             expected: (= 1 2)\n  actual: (not (= 1 2))\n\n\
             Ran 9 tests containing 18 assertions.\n4 failures, 1 errors.\n\
             :logs [:once :each :plain :end-each :each [:continued 2] :end-each :each [:awaited 3] :end-each :end-once] \
             [:once :each :each :each :first :end-each :end-each :end-each :each :each :each :second :end-each :end-each :end-each :end-once]\n\
             :summary {:test 9, :pass 13, :fail 4, :error 1} false\n"
        ),
        "{text}"
    );
}

#[test]
fn eval_and_repl_run_suites_and_assertions() {
    let dir = TempDir::new("cljs-test-eval");
    let root = dir.join("src");
    suite_program(&root);
    let root_text = root.to_str().unwrap();
    assert_eq!(
        relative(
            stdout(light_cljs(
                dir.path(),
                &[
                    "--eval",
                    "--source-root",
                    root_text,
                    "--text",
                    "(require 'app.runner)",
                ],
                "",
            )),
            &root
        ),
        format!("{SUITE_OUTPUT}nil\n")
    );
    assert_eq!(
        relative(
            stdout(light_cljs(
                dir.path(),
                &["--repl", "--source-root", root_text],
                "(require 'app.runner)\n",
            )),
            &root
        ),
        format!("{SUITE_OUTPUT}nil\n")
    );
    // Tests defined and run in the session's namespace, through both
    // names of the library, with the value of each form; `run-all-tests`
    // without a pattern also runs the bundled namespaces loaded, which
    // define no tests, and an exception outside an assertion reports the
    // test's definition.
    let session = "(require '[clojure.test :as t :refer [deftest is are]] '[cljs.test :as ct])
(deftest a (is (= 1 1)) (is (= 1 (inc 1))))
(deftest b (are [x] (pos? x) 1 2))
(t/run-tests)
(ct/run-test a)
[(is (= 3 3)) (ct/is (thrown? :default (throw :x))) (t/successful? {:fail 0 :error 0})]
(t/test-var #'b)
(t/run-test missing)
(deftest loose (throw (js/Error. \"loose\")))
(t/run-all-tests)
";
    let output = light_cljs(dir.path(), &["--repl"], session);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "Unable to resolve missing to a test function.\n"
    );
    assert_eq!(
        stdout(output),
        "nil\n#object[Function]\n#object[Function]\n\
         \nTesting cljs.user\n\
         \nFAIL in (a) (<stdin>.cljc:1:25)\nexpected: (= 1 (inc 1))\n  actual: (not (= 1 2))\n\
         \nRan 2 tests containing 4 assertions.\n1 failures, 0 errors.\nnil\n\
         \nFAIL in (a) (<stdin>.cljc:1:25)\nexpected: (= 1 (inc 1))\n  actual: (not (= 1 2))\nnil\n\
         [true :x true]\nnil\nnil\n#object[Function]\n\
         \nTesting cljs.user\n\
         \nFAIL in (a) (<stdin>.cljc:1:25)\nexpected: (= 1 (inc 1))\n  actual: (not (= 1 2))\n\
         \nERROR in (loose) (<stdin>.cljc:9:1)\nUncaught exception, not in assertion.\nexpected: nil\n  actual: #object[Error Error: loose]\n\
         \nTesting clojure.walk\n\nTesting clojure.template\n\nTesting light.build\n\nTesting cljs.test\n\
         \nRan 3 tests containing 5 assertions.\n1 failures, 1 errors.\nnil\n"
    );
}

#[test]
fn assert_expr_methods_extend_is_from_namespaces_loaded_before_it_expands() {
    let dir = TempDir::new("cljs-test-assert-expr");
    let root = dir.join("src");
    write(
        &root,
        "app/assertions.cljs",
        "(ns app.assertions (:require [cljs.test :as t]))
(defmethod t/assert-expr 'roughly? [menv msg form]
  `(let [d# (js/Math.abs (- ~(nth form 1) ~(nth form 2)))]
     (t/do-report {:type (if (< d# 0.01) :pass :fail) :message ~msg :expected '~form :actual d#})))",
    );
    write(
        &root,
        "app/main.cljs",
        "(ns app.main (:require [cljs.test :as t :refer [deftest is]] [app.assertions]))
(enable-console-print!)
(defmethod t/assert-expr 'same? [menv msg form]
  `(t/do-report {:type (if (= ~@(rest form)) :pass :fail) :message ~msg :expected '~form :actual :same}))
(deftest custom (is (roughly? 1 1.001)) (is (roughly? 1 2) \"far\") (is (same? 1 1)))
(defmethod t/report [::t/default :end-test-vars] [m]
  (prn (:report-counters (t/get-current-env))))
(t/test-vars [#'custom])",
    );
    let js = stdout(light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root.to_str().unwrap(),
            "--no-cache",
        ],
        "",
    ));
    assert_eq!(
        run_node(&js, &root),
        "\nFAIL in (custom) (:)\nfar\nexpected: (roughly? 1 2)\n  actual: 1\n\
         {:test 1, :pass 2, :fail 1, :error 0}\n"
    );
}

#[test]
fn macros_reject_malformed_calls() {
    let dir = TempDir::new("cljs-test-errors");
    for (form, message) in [
        (
            "(are [x y] (= x y) 1)",
            "The number of args doesn't match are's argv.",
        ),
        (
            "(use-fixtures :always (fn [f] (f)))",
            "First argument to cljs.test/use-fixtures must be :once or :each",
        ),
        (
            "(run-tests 'cljs.user app.core)",
            "Argument to run-tests must be a quoted symbol, got app.core",
        ),
        (
            "(run-all-tests \"x\")",
            "run-all-tests takes a regular expression literal, got \"x\"",
        ),
    ] {
        write(
            &dir.join("src"),
            "app/main.cljs",
            &format!(
                "(ns app.main (:require [cljs.test :refer [are use-fixtures run-tests run-all-tests]])) {form}"
            ),
        );
        let error = stderr(light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                "src",
                "--no-cache",
            ],
            "",
        ));
        assert!(error.contains(message), "{form}: {error}");
    }
    // Fixtures of mixed types, and an async test under function fixtures,
    // stop the run.
    let runs = "(require '[cljs.test :as t :refer [deftest async]])
(def mixed [(fn [f] (f)) {:before identity}])
(deftest one)
(deftest later (async done (done)))
(defn attempt [once each vars]
  (try
    (t/set-env! (-> (t/empty-env) (assoc-in [:once-fixtures 'cljs.user] once)
                    (assoc-in [:each-fixtures 'cljs.user] each)))
    (t/test-vars vars)
    :ran
    (catch :default e (or (ex-message e) e))))
[(attempt mixed nil [#'one])
 (attempt [(fn [f] (f))] [{:before identity}] [#'one])
 (attempt [(fn [f] (f))] nil [#'later])
 (attempt [{:before identity}] nil [#'later])]";
    assert_eq!(
        stdout(light_cljs(dir.path(), &["--eval", "--text", runs], "")),
        "[\"Assert failed: Fixtures may not be of mixed types\\n(not-any? nil? types)\" \
         \"Assert failed: fixtures specified in :once and :each must be of the same type\\n(> 2 (count types))\" \
         \"Async tests require fixtures to be specified as maps.  Testing aborted.\" :ran]\n"
    );
}
