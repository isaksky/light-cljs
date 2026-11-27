//! Conformance of the bundled `cljs.test` with upstream semantics: Clojure's
//! own `clojure.test` and fixture tests adapted to JavaScript, the report
//! events' shapes and locations, test composition and `test-ns-hook`,
//! `are` templates, map and function fixtures with synchronous and
//! asynchronous tests (including rejected awaits and `done` called twice),
//! the persistent cache of test namespaces, and `--optimize advanced`
//! removing tests nothing runs. Every program runs in Node at `none` and
//! `advanced` and, where it needs no timers, in the embedded V8 of `--eval`.
use crate::temp_dir::TempDir;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn light_cljs(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// `text` with the source root `root` written as `SRC`.
fn relative(text: String, root: &Path) -> String {
    let canonical = fs::canonicalize(root).unwrap();
    text.replace(canonical.to_str().unwrap(), "SRC")
        .replace(root.to_str().unwrap(), "SRC")
}

/// The JavaScript of an uncached build of `entry` at `level`.
fn compile(dir: &Path, root: &Path, entry: &str, level: &str) -> String {
    stdout(light_cljs(
        dir,
        &[
            "--compile",
            "--entry",
            entry,
            "--source-root",
            root.to_str().unwrap(),
            "--no-cache",
            "--optimize",
            level,
        ],
    ))
}

/// What `entry` prints in Node when built at `none` and at `advanced`,
/// asserted equal, and, when `embedded`, what requiring the namespace
/// `entry_ns` (which loads `entry`'s namespaces without enabling console
/// printing) prints in `--eval`.
fn run_everywhere(dir: &Path, root: &Path, entry: &str, embedded: Option<&str>) -> String {
    let none = relative(
        crate::canonical_json::node_stdout(&compile(dir, root, entry, "none")),
        root,
    );
    let advanced = relative(
        crate::canonical_json::node_stdout(&compile(dir, root, entry, "advanced")),
        root,
    );
    assert_eq!(none, advanced, "none and advanced differ");
    if let Some(entry_ns) = embedded {
        let evaluated = relative(
            stdout(light_cljs(
                dir,
                &[
                    "--eval",
                    "--source-root",
                    root.to_str().unwrap(),
                    "--text",
                    &format!("(require '{entry_ns})"),
                ],
            )),
            root,
        );
        assert_eq!(format!("{none}nil\n"), evaluated, "--eval differs");
    }
    none
}

/// Clojure's `test_clojure/test.clj`, adapted: JavaScript classes and
/// errors replace the JVM's, and the unbound-symbol tests upstream leaves
/// commented out are left out. Its `test-ns-hook` rebinds `report` to turn
/// each event whose type matches its message into a pass.
const UPSTREAM_ASSERTIONS: &str = r#";; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

;;; Adapted for light-cljs from Clojure's test_clojure/test.clj, by Stuart Sierra.

(ns conf.assertions-test
  (:require [clojure.test :refer [deftest is are report test-all-vars]]))

(deftest can-test-symbol
  (let [x true]
    (is x "Should pass"))
  (let [x false]
    (is x "Should fail")))

(deftest can-test-boolean
  (is true "Should pass")
  (is false "Should fail"))

(deftest can-test-nil
  (is nil "Should fail"))

(deftest can-test-=
  (is (= 2 (+ 1 1)) "Should pass")
  (is (= 3 (+ 2 2)) "Should fail"))

(deftest can-test-instance
  (is (instance? js/Date (js/Date.)) "Should pass")
  (is (instance? js/RegExp (+ 1 1)) "Should fail"))

(deftest can-test-thrown
  (is (thrown? js/RangeError (.toFixed 1 200)) "Should pass")
  ;; No exception is thrown:
  (is (thrown? js/Error (+ 1 1)) "Should fail")
  ;; Wrong class of exception is thrown:
  (is (thrown? js/RangeError (throw (js/TypeError.))) "Should error"))

(deftest can-test-thrown-with-msg
  (is (thrown-with-msg? js/RangeError #"digits" (.toFixed 1 200)) "Should pass")
  ;; Wrong message string:
  (is (thrown-with-msg? js/RangeError #"Something else" (.toFixed 1 200)) "Should fail")
  ;; No exception is thrown:
  (is (thrown? js/Error (+ 1 1)) "Should fail")
  ;; Wrong class of exception is thrown:
  (is (thrown-with-msg? js/TypeError #"digits" (.toFixed 1 200)) "Should error"))

(deftest can-catch-unexpected-exceptions
  (is (= 1 (throw (js/Error.))) "Should error"))

(deftest can-test-method-call
  (is (.startsWith "abc" "a") "Should pass")
  (is (.startsWith "abc" "d") "Should fail"))

(deftest can-test-anonymous-fn
  (is (#(.startsWith % "a") "abc") "Should pass")
  (is (#(.startsWith % "d") "abc") "Should fail"))

(deftest can-test-regexps
  (is (re-matches #"^ab.*$" "abbabba") "Should pass")
  (is (re-matches #"^cd.*$" "abbabba") "Should fail")
  (is (re-find #"ab" "abbabba") "Should pass")
  (is (re-find #"cd" "abbabba") "Should fail"))

(deftest ^{:has-meta true} can-add-metadata-to-tests
  (is (:has-meta (meta #'can-add-metadata-to-tests)) "Should pass"))

(declare ^:dynamic original-report)

(defn custom-report [data]
  (let [event (:type data)
        msg (:message data)
        expected (:expected data)
        actual (:actual data)
        passed (cond
                 (= event :fail) (= msg "Should fail")
                 (= event :pass) (= msg "Should pass")
                 (= event :error) (= msg "Should error")
                 :else true)]
    (if passed
      (original-report {:type :pass, :message msg,
                        :expected expected, :actual actual})
      (original-report {:type :fail, :message (str msg " but got " event)
                        :expected expected, :actual actual}))))

(defn test-ns-hook []
  (binding [original-report report
            report custom-report]
    (test-all-vars 'conf.assertions-test)))

(deftest clj-1588-symbols-in-are-isolated-from-test-clauses
  (binding [report original-report]
    (are [x y] (= x y)
      ((fn [x] (inc x)) 1) 2)))
"#;

#[test]
fn upstream_assertion_tests_pass_under_their_rebinding_test_ns_hook() {
    let dir = TempDir::new("cljs-test-conformance-assertions");
    let root = dir.join("src");
    write(&root, "conf/assertions_test.cljs", UPSTREAM_ASSERTIONS);
    write(
        &root,
        "conf/assertions_run.cljs",
        "(ns conf.assertions-run (:require [cljs.test :as t] [conf.assertions-test]))
(defmethod t/report [::t/default :end-run-tests] [m] (prn :summary (dissoc m :type)))
(t/run-tests 'conf.assertions-test)",
    );
    write(
        &root,
        "conf/assertions_main.cljs",
        "(ns conf.assertions-main (:require [conf.print] [conf.assertions-run]))",
    );
    write(
        &root,
        "conf/print.cljs",
        "(ns conf.print) (enable-console-print!)",
    );
    // Every one of the 50 assertions is the type its message names, and
    // the `are` clause runs with the original reporter.
    assert_eq!(
        run_everywhere(
            dir.path(),
            &root,
            "conf.assertions-main",
            Some("conf.assertions-run")
        ),
        "\nTesting conf.assertions-test\n\nRan 12 tests containing 51 assertions.\n\
         0 failures, 0 errors.\n:summary {:test 12, :pass 51, :fail 0, :error 0}\n"
    );
}

/// A reporter that records each assertion's event as `[type expected
/// actual message contexts vars-and-location]` and prints them when the run
/// ends, and namespaces of assertions on predicates, macros, special and
/// interop forms, exceptions, contexts, `are` templates and composed tests.
fn shapes_program(root: &Path) {
    write(
        root,
        "conf/collect.cljs",
        r#"(ns conf.collect (:require [cljs.test :as t]))

(def events (atom []))

(defn- shown [x]
  (cond
    (instance? js/Error x) [(.-name x) (.-message x)]
    (fn? x) :fn
    :else x))

(defn- record! [m]
  (swap! events conj [(:type m) (:expected m) (shown (:actual m))
                      (:message m) (t/testing-contexts-str) (t/testing-vars-str m)]))

(defmethod t/report [::t/collect :pass] [m] (t/inc-report-counter! :pass) (record! m))
(defmethod t/report [::t/collect :fail] [m] (t/inc-report-counter! :fail) (record! m))
(defmethod t/report [::t/collect :error] [m] (t/inc-report-counter! :error) (record! m))
(defmethod t/report [::t/collect :begin-test-ns] [m] (prn :ns (:ns m)))
(defmethod t/report [::t/collect :end-run-tests] [m]
  (doseq [e @events] (prn e))
  (reset! events [])
  (prn :summary (dissoc m :type)))
"#,
    );
    write(
        root,
        "conf/shapes_test.cljs",
        r#"(ns conf.shapes-test
  (:require [cljs.test :as t :refer [deftest is are testing]]))

(defmacro unless [test then] `(if ~test nil ~then))

(deftest predicates
  (is (= 1 1))
  (is (= 1 2) "unequal")
  (is (contains? #{:a} :b)))

(deftest non-predicates
  (is (and true false))
  (is (if true nil 1))
  (is (unless false :then))
  (is (.startsWith "abc" "d"))
  (is (let [x 2] (odd? x)))
  (is :truthy))

(deftest instances
  (is (instance? js/Date (js/Date.)))
  (is (instance? js/Date 1)))

(deftest exceptions
  (is (thrown? js/TypeError (throw (js/TypeError. "kind"))))
  (is (thrown? js/TypeError :no-throw))
  (is (thrown-with-msg? js/Error #"^b" (throw (js/Error. "boom"))))
  (is (thrown-with-msg? js/Error #"^x" (throw (js/Error. "boom"))) "message")
  (is (thrown? js/TypeError (throw (js/RangeError. "other"))))
  (is (= 1 (throw (js/Error. "unexpected")))))

(deftest contexts
  (testing "outer"
    (testing "inner"
      (is true))
    (is true))
  (try
    (testing "thrown"
      (testing "through"
        (throw (js/Error. "out"))))
    (catch :default _))
  (is (empty? (:testing-contexts (t/get-current-env))) "popped"))

(deftest values
  (let [caught (is (thrown? js/Error (throw (js/Error. "returned"))))]
    (is (= "returned" (.-message caught))))
  (is (= 3 (is (+ 1 2))))
  (is (false? (is false))))

(deftest templates
  (are [] true)
  (are [x] (pos? x) 1 -1)
  (are [x y] (= x y) ((fn [x] (inc x)) 1) 2))
"#,
    );
    write(
        root,
        "conf/compose_test.cljs",
        r#"(ns conf.compose-test
  (:require [cljs.test :refer [deftest is]]))

(deftest leaf-a (is (= 1 1)))
(deftest leaf-b (is (= 1 2)))
(deftest both (leaf-a) (leaf-b))
(deftest skipped (is false))
(defn test-ns-hook [] (both))
"#,
    );
    write(
        root,
        "conf/shapes_run.cljs",
        r#"(ns conf.shapes-run
  (:require [cljs.test :as t] [conf.collect] [conf.shapes-test] [conf.compose-test :as c]))
(t/run-tests (t/empty-env ::t/collect) 'conf.shapes-test)
(t/run-all-tests #"conf\.comp.*" (t/empty-env ::t/collect))
(c/leaf-b)
(prn :direct (:report-counters (t/get-current-env)))
"#,
    );
    write(
        root,
        "conf/print.cljs",
        "(ns conf.print) (enable-console-print!)",
    );
    write(
        root,
        "conf/shapes_main.cljs",
        "(ns conf.shapes-main (:require [conf.print] [conf.shapes-run]))",
    );
}

/// Predicates report their evaluated arguments, other forms their value;
/// `instance?` reports the object's type, `thrown?` the exception, and an
/// exception other than the one expected is an error. Each assertion has
/// the source location of its `is` (an `are` the location of the `are`),
/// contexts are popped when an exception leaves them, `is` returns its
/// value, a composed test reports both test names, and a test called
/// directly reports to the default reporter.
const SHAPES_OUTPUT: &str = r#":ns conf.shapes-test
[:pass (= 1 1) (= 1 1) nil "" "(predicates) (SRC/conf/shapes_test.cljs:7:3)"]
[:fail (= 1 2) (not (= 1 2)) "unequal" "" "(predicates) (SRC/conf/shapes_test.cljs:8:3)"]
[:fail (contains? #{:a} :b) (not (contains? #{:a} :b)) nil "" "(predicates) (SRC/conf/shapes_test.cljs:9:3)"]
[:fail (and true false) false nil "" "(non-predicates) (SRC/conf/shapes_test.cljs:12:3)"]
[:fail (if true nil 1) nil nil "" "(non-predicates) (SRC/conf/shapes_test.cljs:13:3)"]
[:pass (unless false :then) :then nil "" "(non-predicates) (SRC/conf/shapes_test.cljs:14:3)"]
[:fail (.startsWith "abc" "d") false nil "" "(non-predicates) (SRC/conf/shapes_test.cljs:15:3)"]
[:fail (let [x 2] (odd? x)) false nil "" "(non-predicates) (SRC/conf/shapes_test.cljs:16:3)"]
[:pass :truthy :truthy nil "" "(non-predicates) (SRC/conf/shapes_test.cljs:17:3)"]
[:pass (instance? js/Date (js/Date.)) :fn nil "" "(instances) (SRC/conf/shapes_test.cljs:20:3)"]
[:fail (instance? js/Date 1) :fn nil "" "(instances) (SRC/conf/shapes_test.cljs:21:3)"]
[:pass (thrown? js/TypeError (throw (js/TypeError. "kind"))) ["TypeError" "kind"] nil "" "(exceptions) (SRC/conf/shapes_test.cljs:24:3)"]
[:fail (thrown? js/TypeError :no-throw) nil nil "" "(exceptions) (SRC/conf/shapes_test.cljs:25:3)"]
[:pass (thrown-with-msg? js/Error #"^b" (throw (js/Error. "boom"))) ["Error" "boom"] nil "" "(exceptions) (SRC/conf/shapes_test.cljs:26:3)"]
[:fail (thrown-with-msg? js/Error #"^x" (throw (js/Error. "boom"))) ["Error" "boom"] "message" "" "(exceptions) (SRC/conf/shapes_test.cljs:27:3)"]
[:error (thrown? js/TypeError (throw (js/RangeError. "other"))) ["RangeError" "other"] nil "" "(exceptions) (SRC/conf/shapes_test.cljs:28:3)"]
[:error (= 1 (throw (js/Error. "unexpected"))) ["Error" "unexpected"] nil "" "(exceptions) (SRC/conf/shapes_test.cljs:29:3)"]
[:pass true true nil "outer inner" "(contexts) (SRC/conf/shapes_test.cljs:34:7)"]
[:pass true true nil "outer" "(contexts) (SRC/conf/shapes_test.cljs:35:5)"]
[:pass (empty? (:testing-contexts (t/get-current-env))) (empty? ()) "popped" "" "(contexts) (SRC/conf/shapes_test.cljs:41:3)"]
[:pass (thrown? js/Error (throw (js/Error. "returned"))) ["Error" "returned"] nil "" "(values) (SRC/conf/shapes_test.cljs:44:16)"]
[:pass (= "returned" (.-message caught)) (= "returned" "returned") nil "" "(values) (SRC/conf/shapes_test.cljs:45:5)"]
[:pass (+ 1 2) (+ 1 2) nil "" "(values) (SRC/conf/shapes_test.cljs:46:12)"]
[:pass (= 3 (is (+ 1 2))) (= 3 3) nil "" "(values) (SRC/conf/shapes_test.cljs:46:3)"]
[:fail false false nil "" "(values) (SRC/conf/shapes_test.cljs:47:15)"]
[:pass (false? (is false)) (false? false) nil "" "(values) (SRC/conf/shapes_test.cljs:47:3)"]
[:pass (pos? 1) (pos? 1) nil "" "(templates) (SRC/conf/shapes_test.cljs:51:3)"]
[:fail (pos? -1) (not (pos? -1)) nil "" "(templates) (SRC/conf/shapes_test.cljs:51:3)"]
[:pass (= ((fn [x] (inc x)) 1) 2) (= 2 2) nil "" "(templates) (SRC/conf/shapes_test.cljs:52:3)"]
:summary {:test 7, :pass 16, :fail 11, :error 2}
:ns conf.compose-test
[:pass (= 1 1) (= 1 1) nil "" "(both leaf-a) (SRC/conf/compose_test.cljs:4:17)"]
[:fail (= 1 2) (not (= 1 2)) nil "" "(both leaf-b) (SRC/conf/compose_test.cljs:5:17)"]
:summary {:test 3, :pass 1, :fail 1, :error 0}

FAIL in (leaf-b) (SRC/conf/compose_test.cljs:5:17)
expected: (= 1 2)
  actual: (not (= 1 2))
:direct {:test 1, :pass 0, :fail 1, :error 0}
"#;

#[test]
fn assertion_events_have_upstream_shapes_locations_and_contexts() {
    let dir = TempDir::new("cljs-test-conformance-shapes");
    let root = dir.join("src");
    shapes_program(&root);
    assert_eq!(
        run_everywhere(
            dir.path(),
            &root,
            "conf.shapes-main",
            Some("conf.shapes-run")
        ),
        SHAPES_OUTPUT
    );
}

/// Clojure's `test_clojure/test_fixtures.clj`, adapted only in expecting
/// the `:end-test-vars` event the bundled (and upstream ClojureScript)
/// `test-vars` reports.
const UPSTREAM_FIXTURES: &str = r#";; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

;;; Adapted for light-cljs from Clojure's test_clojure/test_fixtures.clj, by
;;; Stuart Sierra.

(ns conf.fixtures-test
  (:require [clojure.test :refer [deftest is use-fixtures test-vars report]]))

(declare ^:dynamic *a* ^:dynamic *b* ^:dynamic *c* ^:dynamic *d*)

(def ^:dynamic *n* 0)

(defn fixture-a [f]
  (binding [*a* 3] (f)))

(defn fixture-b [f]
  (binding [*b* 5] (f)))

(defn fixture-c [f]
  (binding [*c* 7] (f)))

(defn fixture-d [f]
  (binding [*d* 11] (f)))

(defn inc-n-fixture [f]
  (binding [*n* (inc *n*)] (f)))

(def side-effects (atom 0))
(defn side-effecting-fixture [f]
  (swap! side-effects inc)
  (f))

(use-fixtures :once fixture-a fixture-b)

(use-fixtures :each fixture-c fixture-d inc-n-fixture side-effecting-fixture)
(use-fixtures :each fixture-c fixture-d inc-n-fixture side-effecting-fixture)

(deftest can-use-once-fixtures
  (is (= 3 *a*))
  (is (= 5 *b*)))

(deftest can-use-each-fixtures
  (is (= 7 *c*))
  (is (= 11 *d*)))

(deftest use-fixtures-replaces
  (is (= *n* 1)))

(deftest can-run-a-single-test-with-fixtures
  (let [side-effects-so-far @side-effects
        reported (atom [])]
    (binding [report (fn [m] (swap! reported conj (:type m)))]
      (test-vars [#'can-use-each-fixtures]))
    (is (= [:begin-test-var :pass :pass :end-test-var :end-test-vars] @reported))
    (is (= (inc side-effects-so-far) @side-effects))))

(defn should-not-trigger-fixtures [])

(deftest a-var-lacking-test-meta-should-not-trigger-fixtures
  (let [side-effects-so-far @side-effects]
    (test-vars [#'should-not-trigger-fixtures])
    (is (= side-effects-so-far @side-effects))))
"#;

/// Asynchronous tests under map fixtures: awaited and rejected promises in
/// `^:async` tests, `done` called twice, and an `async` block whose body
/// awaits a rejection before it calls `done`.
const ASYNC_TESTS: &str = r#"(ns conf.async-test
  (:require [cljs.test :refer [deftest is async use-fixtures]]))

(def log (atom []))
(defn- note! [x] (swap! log conj x))

(use-fixtures :once {:before #(note! :before-once) :after #(note! :after-once)})
(use-fixtures :each {:before #(note! :before) :after #(note! :after)})

(deftest continued
  (async done
    (.then (js/Promise.resolve 1) (fn [v] (note! :continued) (is (= 1 v)) (done)))))

(deftest ^:async resolved
  (is (= 2 (await (js/Promise.resolve 2))))
  (note! :resolved))

(deftest ^:async rejected
  (note! :rejected)
  (await (js/Promise.reject (js/Error. "rejected")))
  (note! :unreached))

(deftest ^:async rejected-in-is
  (is (= 1 (await (js/Promise.reject (js/Error. "in is"))))))

(deftest twice
  (async done
    (note! :twice)
    (done)
    (done)))

(deftest awaited-rejection
  (async done
    (note! :awaiting)
    (await (js/Promise.reject (js/Error. "awaited")))
    (done)))

(deftest after-all (note! :after-all) (is true))
"#;

const FIXTURES_OUTPUT: &str = "
Testing conf.fixtures-test

Testing conf.async-test

ERROR in (rejected) (SRC/conf/async_test.cljs:18:1)
Uncaught exception, not in assertion.
expected: nil
  actual: #object[Error Error: rejected]

ERROR in (rejected-in-is) (SRC/conf/async_test.cljs:24:3)
expected: (= 1 (await (js/Promise.reject (js/Error. \"in is\"))))
  actual: #object[Error Error: in is]
WARNING: Async test called done more than one time.

ERROR in (awaited-rejection) (SRC/conf/async_test.cljs:33:3)
Uncaught exception, not in assertion.
expected: nil
  actual: #object[Error Error: awaited]

Ran 13 tests containing 14 assertions.
0 failures, 3 errors.
:log [:before-once :before :continued :after :before :resolved :after :before :rejected :after \
:before :after :before :twice :after :before :awaiting :after :before :after-all :after :after-once]
:summary {:test 13, :pass 11, :fail 0, :error 3}
";

#[test]
fn upstream_fixture_tests_and_async_tests_finish_in_order() {
    let dir = TempDir::new("cljs-test-conformance-fixtures");
    let root = dir.join("src");
    write(&root, "conf/fixtures_test.cljs", UPSTREAM_FIXTURES);
    write(&root, "conf/async_test.cljs", ASYNC_TESTS);
    write(
        &root,
        "conf/fixtures_run.cljs",
        "(ns conf.fixtures-run (:require [cljs.test :as t] [conf.fixtures-test] [conf.async-test]))
(defmethod t/report [::t/default :end-run-tests] [m]
  (prn :log @conf.async-test/log)
  (prn :summary (dissoc m :type)))
(t/run-tests 'conf.fixtures-test 'conf.async-test)",
    );
    write(
        &root,
        "conf/print.cljs",
        "(ns conf.print) (enable-console-print!)",
    );
    write(
        &root,
        "conf/fixtures_main.cljs",
        "(ns conf.fixtures-main (:require [conf.print] [conf.fixtures-run]))",
    );
    assert_eq!(
        run_everywhere(
            dir.path(),
            &root,
            "conf.fixtures-main",
            Some("conf.fixtures-run")
        ),
        FIXTURES_OUTPUT
    );
}

/// Node's timers: `done` called from a `js/setTimeout` callback, an
/// awaited timer in an `^:async` test, and a later test that waits for both.
#[test]
fn async_tests_continue_from_timers_in_node() {
    let dir = TempDir::new("cljs-test-conformance-timers");
    let root = dir.join("src");
    write(
        &root,
        "conf/timer_test.cljs",
        r#"(ns conf.timer-test
  (:require [cljs.test :as t :refer [deftest is async]]))

(def log (atom []))
(defn later [ms v] (js/Promise. (fn [resolve] (js/setTimeout #(resolve v) ms))))

(deftest timeout
  (async done
    (js/setTimeout (fn [] (swap! log conj :timeout) (is (= 1 1)) (done)) 30)))

(deftest ^:async awaited
  (is (= 2 (await (later 10 2))))
  (swap! log conj :awaited))

(deftest last-test (swap! log conj :last))

(defmethod t/report [::t/default :end-run-tests] [m]
  (prn @log (dissoc m :type)))

(enable-console-print!)
(t/run-tests)
"#,
    );
    let expected = "\nTesting conf.timer-test\n\nRan 3 tests containing 2 assertions.\n\
                    0 failures, 0 errors.\n[:timeout :awaited :last] {:test 3, :pass 2, :fail 0, :error 0}\n";
    for level in ["none", "advanced"] {
        let js = compile(dir.path(), &root, "conf.timer-test", level);
        assert_eq!(crate::canonical_json::node_stdout(&js), expected, "{level}");
    }
}

#[test]
fn are_rejects_values_that_do_not_fill_its_argument_vector() {
    let dir = TempDir::new("cljs-test-conformance-are");
    for form in [
        "(are [x] (pos? x))",
        "(are [] true 1)",
        "(are [x y] (= x y) 1 2 3)",
    ] {
        write(
            &dir.join("src"),
            "app/main.cljs",
            &format!("(ns app.main (:require [cljs.test :refer [are]])) {form}"),
        );
        let output = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                "src",
                "--no-cache",
            ],
        );
        assert!(!output.status.success(), "{form}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("The number of args doesn't match are's argv."),
            "{form}: {error}"
        );
    }
}

/// The count `--stats` printed for `key`, 0 when it printed none.
fn stats_value(stderr: &str, key: &str) -> usize {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key} ")))
        .map_or(0, |value| value.parse().unwrap())
}

/// A persistent cache reinstalls unchanged test namespaces, compiles again
/// only a test namespace that changed, and reruns the `run-all-tests`
/// expansion when the build's namespaces change: a removed test namespace
/// is no longer run.
#[test]
fn cached_builds_reuse_test_namespaces_and_follow_the_namespace_inventory() {
    let dir = TempDir::new("cljs-test-conformance-cache");
    let root = dir.join("src");
    let test_ns = |name: &str, body: &str| {
        format!(
            "(ns app.{name}-test (:require [cljs.test :refer [deftest is]])) (deftest {name} {body})"
        )
    };
    let suite = |names: &[&str]| {
        let requires: String = names.iter().map(|n| format!(" [app.{n}-test]")).collect();
        format!("(ns app.suite (:require{requires}))")
    };
    write(&root, "app/a_test.cljs", &test_ns("a", "(is (= 1 1))"));
    write(&root, "app/b_test.cljs", &test_ns("b", "(is (= 2 2))"));
    write(&root, "app/c_test.cljs", &test_ns("c", "(is (= 3 3))"));
    write(&root, "app/suite.cljs", &suite(&["a", "b", "c"]));
    write(
        &root,
        "app/runner.cljs",
        r#"(ns app.runner (:require [cljs.test :as t] [app.suite]))
(defmethod t/report [::t/default :begin-test-ns] [m] (prn :ns (:ns m)))
(defmethod t/report [::t/default :summary] [m] (prn (dissoc m :type)))
(t/run-all-tests #"app\..*-test")
"#,
    );
    write(
        &root,
        "app/print.cljs",
        "(ns app.print) (enable-console-print!)",
    );
    write(
        &root,
        "app/main.cljs",
        "(ns app.main (:require [app.print] [app.runner]))",
    );
    let cache = dir.join("cache");
    let build = || {
        let output = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root.to_str().unwrap(),
                "--cache-dir",
                cache.to_str().unwrap(),
                "--stats",
            ],
        );
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stats = [
            "cache.install.namespaces",
            "cache.miss.source",
            "cache.miss.dependencies",
            "cache.miss.view",
            "cache.bypass.non-cacheable-input",
        ]
        .map(|key| stats_value(&stderr, key));
        (
            relative(crate::canonical_json::node_stdout(&stdout(output)), &root),
            stats,
        )
    };
    let all =
        ":ns app.a-test\n:ns app.b-test\n:ns app.c-test\n{:test 3, :pass 3, :fail 0, :error 0}\n";
    let (text, [installed, ..]) = build();
    assert_eq!((text.as_str(), installed), (all, 0));
    // A no-op rebuild installs every namespace from the cache but the
    // runner, whose `run-all-tests` lists the build's namespaces, and its
    // consumer.
    let (text, [total, source, dependencies, view, bypassed]) = build();
    assert_eq!(
        (text.as_str(), source, dependencies, view, bypassed),
        (all, 0, 0, 0, 2)
    );
    // Editing one test namespace compiles it again, and the namespace that
    // requires it, and installs every other one.
    write(&root, "app/b_test.cljs", &test_ns("b", "(is (= 2 3))"));
    assert_eq!(
        build(),
        (
            ":ns app.a-test\n:ns app.b-test\n\nFAIL in (b) (SRC/app/b_test.cljs:1:71)\n\
             expected: (= 2 3)\n  actual: (not (= 2 3))\n:ns app.c-test\n\
             {:test 3, :pass 2, :fail 1, :error 0}\n"
                .into(),
            [total - 2, 1, 1, 0, 2]
        )
    );
    // Removing a test namespace from the build reruns the expansion of
    // `run-all-tests`, which no longer names it; the suite that required
    // it and the test namespace loaded after it, whose view of the earlier
    // namespaces changed, compile again.
    write(&root, "app/suite.cljs", &suite(&["a", "c"]));
    assert_eq!(
        build(),
        (
            ":ns app.a-test\n:ns app.c-test\n{:test 2, :pass 2, :fail 0, :error 0}\n".into(),
            [total - 3, 1, 0, 1, 2]
        )
    );
}

/// At `advanced`, a `deftest` nothing reads or runs is removed with its
/// body, while the tests a program runs keep their behaviour.
#[test]
fn advanced_removes_tests_nothing_runs() {
    let dir = TempDir::new("cljs-test-conformance-dce");
    let root = dir.join("src");
    write(
        &root,
        "app/lib.cljs",
        r#"(ns app.lib (:require [cljs.test :refer [deftest is]]))
(defn used [x] (inc x))
(deftest unused-test (is (= "UNUSED_TEST_BODY" (str (used 1)))))
(deftest used-test (is (= 2 (used 1)) "USED_TEST_BODY"))
"#,
    );
    write(
        &root,
        "app/plain.cljs",
        "(ns app.plain (:require [app.lib :as lib])) (js/console.log (lib/used 41))",
    );
    write(
        &root,
        "app/one.cljs",
        "(ns app.one (:require [cljs.test :as t] [app.lib :as lib]))
(enable-console-print!)
(defmethod t/report [::t/default :end-test-vars] [m]
  (prn (:report-counters (t/get-current-env))))
(t/test-vars [#'lib/used-test])",
    );
    write(
        &root,
        "app/all.cljs",
        "(ns app.all (:require [cljs.test :as t] [app.lib]))
(enable-console-print!)
(defmethod t/report [::t/default :summary] [m] (prn (dissoc m :type)))
(t/run-tests 'app.lib)",
    );
    let plain = compile(dir.path(), &root, "app.plain", "advanced");
    assert_eq!(crate::canonical_json::node_stdout(&plain), "42\n");
    assert!(!plain.contains("TEST_BODY"));
    let one = compile(dir.path(), &root, "app.one", "advanced");
    assert_eq!(
        crate::canonical_json::node_stdout(&one),
        "{:test 1, :pass 1, :fail 0, :error 0}\n"
    );
    assert!(!one.contains("UNUSED_TEST_BODY") && one.contains("USED_TEST_BODY"));
    // `run-tests` reads every test of the namespace.
    let all = compile(dir.path(), &root, "app.all", "advanced");
    assert!(all.contains("UNUSED_TEST_BODY") && all.contains("USED_TEST_BODY"));
    assert!(
        crate::canonical_json::node_stdout(&all)
            .ends_with("{:test 2, :pass 1, :fail 1, :error 0}\n")
    );
}

/// A definition with `:test` metadata that nothing reads is removed at
/// `advanced` with its test function, as `deftest`s are.
#[test]
fn advanced_removes_unread_definitions_with_test_metadata() {
    let dir = TempDir::new("cljs-test-conformance-test-metadata");
    let root = dir.join("src");
    write(
        &root,
        "app/main.cljs",
        r#"(ns app.main)
(defn used [x] (inc x))
(def ^{:test (fn [] (js/console.log "UNREAD_TEST"))} unread (fn [] (used 2)))
(def ^{:test (fn [] (js/console.log "READ_TEST"))} read (fn [] (used 3)))
(js/console.log (used 41) (read) (some? (:test (meta #'read))))
"#,
    );
    let js = compile(dir.path(), &root, "app.main", "advanced");
    assert_eq!(crate::canonical_json::node_stdout(&js), "42 4 true\n");
    assert!(
        !js.contains("UNREAD_TEST") && js.contains("READ_TEST"),
        "{js}"
    );
}
