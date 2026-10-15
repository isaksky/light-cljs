//! `^:async` functions and `await` (light-cljs-261), as upstream has them
//! since CLJS-3470 (research/clojurescript/changes.md, pinned a5e94c98):
//! `^:async` on `fn`, a `fn`'s self-name, `defn`, `defn-` and `letfn`
//! functions makes an `async function`, and `cljs.core/await` is legal only
//! in its body. A function the emitter wraps around expressions that await
//! inside it (`let`, `loop`, `try`, `case`, `do`, `letfn` and the others in
//! value position) is then an awaited `async` one, as upstream's
//! `iife-open`, whose value is boxed so that it never adopts a Promise.
//!
//! The suite runs upstream's `src/test/cljs/cljs/async_await_test.cljs` in
//! Node at every level, as one script and as one ES module. The embedded
//! QuickJS runs no promise jobs, so programs here run in Node only.
use super::optimization::execute;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry_with_provider;
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput};
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, analyze, parse, prepare_application,
};
use std::process::Command;

const UPSTREAM: &str =
    include_str!("../../research/clojurescript/src/test/cljs/cljs/async_await_test.cljs");

/// `clojure.test` is not provided (README, "Known boundaries"): this one
/// keeps upstream's `deftest` and `is` spellings, registering each test as a
/// function, `^:async` when its name is, and recording each assertion.
const CLOJURE_TEST: &str = r#"(ns clojure.test)
(def tests (atom []))
(def results (atom []))
(defn record! [form value]
  (swap! results conj [form (boolean value)])
  value)
(defmacro is [form]
  `(record! '~form ~form))
(defmacro deftest [name & body]
  `(do (defn ~name [] ~@body)
       (swap! tests conj ['~name ~name])))
"#;

/// Upstream's src/test/cljs/cljs/macro_test/macros.clj as a `.cljs` source
/// (light-cljs never loads `.clj` macro companions). The syntax quote of its
/// `await!` resolves `await` to `cljs.core/await`, where upstream's resolves
/// it to `clojure.core/await`; the test spells the latter itself too.
const MACROS: &str = include_str!(
    "../fixtures/compiler-residual-source-context/macro-source/cljs/macro_test/macros.cljs"
);

/// The Closure Library is not provided: `goog.object/equals` compares the
/// keys and values of two objects, as Closure's does.
const GOOG_OBJECT: &str = r#"(ns goog.object)
(defn equals [a b]
  (and (= (vec (js-keys a)) (vec (js-keys b)))
       (every? #(= (aget a %) (aget b %)) (js-keys a))))
"#;

/// Replaces upstream's last test, which drives `cljs.test`'s own report and
/// `run-block` machinery: the failing async test's rejection is caught where
/// it is awaited instead. Then every registered test runs in order, each
/// awaited, and the program prints what the assertions gave.
const TAIL: &str = r#"(deftest ^:async async-test-error-reporting-test
  (let [reports (try
                  (vreset! throw-in-async-error-test true)
                  (try
                    (await (simulated-async-error-test))
                    []
                    (catch :default e [{:actual e}]))
                  (finally
                    (vreset! throw-in-async-error-test false)))]
    (is (= 1 (count reports)))
    (let [{:keys [actual]} (first reports)]
      (is (= "simulated" (ex-message actual))))))

(defn ^:async run-tests []
  (loop [tests @t/tests]
    (when (seq tests)
      (await ((second (first tests))))
      (recur (rest tests))))
  (pr-str {:tests (count @t/tests)
           :pass (count (filter second @t/results))
           :fail (vec (remove second @t/results))}))

(.then (run-tests) (fn [summary] (js/console.log summary)))
"#;

/// Upstream's test namespace with these adaptations, each of which must
/// still apply to the pinned file:
/// - its `:require-macros` becomes an ordinary `:require` (light-cljs
///   retires legacy macro imports);
/// - the `cljs.test` reporting test and its `defmethod` give way to [`TAIL`].
fn adapted() -> String {
    let mut source = UPSTREAM.to_owned();
    for (from, to) in [(
        "            [goog.object :as gobj])\n  (:require-macros [cljs.macro-test.macros :refer [await!] :as macros]))",
        "            [goog.object :as gobj]\n            [cljs.macro-test.macros :refer [await!] :as macros]))",
    )] {
        assert!(source.contains(from), "upstream no longer has {from:?}");
        source = source.replace(from, to);
    }
    let tail = source
        .find("(def async-error-reports")
        .expect("upstream's reporting test");
    source.truncate(tail);
    source.push_str(TAIL);
    source
}

/// The program of entry namespace `entry` from in-memory `files`.
fn namespace_program(entry: &str, files: &[(&str, &str)]) -> light_cljs::Program {
    let provider = |request: &LoadRequest| -> Result<Option<SourceInput>, LoadError> {
        let relative = String::from_utf16_lossy(request.namespace.name())
            .replace('.', "/")
            .replace('-', "_");
        Ok(files
            .iter()
            .find(|(path, _)| {
                path.strip_suffix(".cljs")
                    .or_else(|| path.strip_suffix(".cljc"))
                    .is_some_and(|stem| stem == relative)
            })
            .map(|(path, source)| SourceInput::new(*path, *source)))
    };
    analyze_namespace_entry_with_provider(
        entry,
        &provider,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

/// Run the ES module `module` in Node and return what it prints.
fn execute_module(module: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--input-type=module", "--eval", module])
        .output()
        .expect("async tests require Node");
    assert!(
        output.status.success(),
        "{}\n{module}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// `program` prints `expected` at every level, as a script and as a module.
fn every_level(program: &light_cljs::Program, expected: &str) {
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(program, ApplicationOptions::new(level));
        assert_eq!(execute(&prepared.emit_js()), expected, "script at {level}");
        assert_eq!(
            execute_module(&prepared.emit_esm()),
            expected,
            "module at {level}"
        );
    }
}

/// The program of one source, analyzed alone.
fn source_program(source: &str) -> light_cljs::Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

/// Upstream's async/await tests, all 20 with their 41 assertions: `^:async`
/// `defn`s and `fn`s of one or several arities, fixed or variadic, called
/// directly and through `apply`; `await` in `let` initializers, nested `let`,
/// `letfn`, `loop`, `case`, `try`/`catch`/`finally`, `throw`, `do`, `if`
/// tests, constructor arguments, `#js` and collection literals and
/// destructuring defaults; and `await` through a `:refer`/`:rename`, a
/// `cljs.core` alias, `cljs.core/` and `clojure.core/` spellings and a macro.
#[test]
fn upstream_async_await_tests_pass_at_every_level() {
    let test = adapted();
    let program = namespace_program(
        "cljs.async-await-test",
        &[
            ("cljs/async_await_test.cljs", &test),
            ("clojure/test.cljc", CLOJURE_TEST),
            ("cljs/macro_test/macros.cljs", MACROS),
            ("goog/object.cljs", GOOG_OBJECT),
        ],
    );
    every_level(&program, "{:tests 20, :pass 41, :fail []}\n");
}

/// A host class's static method keeps its class as `this` (light-cljs-263),
/// as upstream resolves `Promise/resolve` to `js/Promise.resolve`: through
/// `:refer-global`, its `:rename`, the dotted `Promise.resolve` spelling and
/// a built-in class such as `String`. Reading the method first, as in
/// `((.-resolve Promise) 0)`, still calls it without a receiver, as
/// upstream's `.call(null, 0)` does.
#[test]
fn refer_global_static_methods_keep_their_class_as_this() {
    let source = r#"(ns probe (:refer-global :only [Promise Date] :rename {Date D}))
(set! (.-whoami js/String) (fn [] (this-as cls cls)))
(set! (.-whoami js/Date) (fn [] (this-as cls cls)))
(def explicit
  (try ((.-resolve Promise) 0) :receiver
       (catch :default e (if (instance? js/TypeError e) :no-receiver e))))
(.then (js/Promise.all #js [(Promise/resolve 0) (Promise.resolve 1)])
       (fn [values]
         (js/console.log
          (pr-str [(instance? Promise (Promise/resolve 0)) (vec values)
                   (identical? js/String (String/whoami))
                   (identical? js/Date (D/whoami)) explicit]))))
"#;
    let program = namespace_program("probe", &[("probe.cljs", source)]);
    every_level(&program, "[true [0 1] true true :no-receiver]\n");
}

/// `await` in the body of anything but an `^:async` function is upstream's
/// failed assertion, located at the `await`: at the top level (also under
/// `--eval`, where upstream has no top-level `await` either), in a plain
/// `fn`, and in a function nested in an `^:async` one, including the
/// functions `#()`, `delay`, `lazy-seq`, `for` and type methods make.
#[test]
fn await_outside_an_async_function_is_a_located_analysis_error() {
    for source in [
        "(await (js/Promise.resolve 1))",
        "(defn f [] (await 1))",
        "(fn [] (let [x (await 1)] x))",
        "(defn ^:async f []\n  (fn [] (await 1)))",
        "(defn ^:async f [] #(await %))",
        "(defn ^:async f [] (delay (await 1)))",
        "(defn ^:async f [] (lazy-seq (await 1)))",
        "(defn ^:async f [] (for [x [1]] (await x)))",
        "(defn ^:async f [] (reify Object (toString [_] (await 1))))",
        "(defn ^:async f [] (letfn [(g [] (await 1))] (g)))",
        "(defn ^:async f [] (fn ^:async g [] (fn [] (await 1))))",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "macro expansion: Assert failed: await can only be used in async contexts\n(:async &env)",
            "{source}"
        );
        let at = source.find("await").unwrap();
        assert!(
            error.span.start < at && at < error.span.end,
            "{source}: {:?}",
            error.span
        );
    }
    // A namespace's error names its line and column.
    let error = analyze_namespace_entry_with_provider(
        "bad.main",
        &|_: &LoadRequest| -> Result<Option<SourceInput>, LoadError> {
            Ok(Some(SourceInput::new(
                "bad/main.cljs",
                "(ns bad.main)\n(defn f []\n  (await 1))",
            )))
        },
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error
            .message
            .contains("await can only be used in async contexts")
    );
    assert_eq!(error.line_column, Some((3, 3)), "{error}");
    // A local or a definition named await is an ordinary call.
    let program = source_program(
        "(defn ^:async f [] (let [await inc] (await 1)))
         (.then (f) (fn [v] (js/console.log v)))",
    );
    every_level(&program, "2\n");
}

/// `^:async` on a `fn` symbol, a `fn`'s self-name, `defn-` and `letfn`; an
/// `^:async` function nested in a plain one and the reverse; and an `await`
/// inside `assert`, `this-as`, `binding`, `with-out-str` and a `def`, whose
/// emitted functions in value position are awaited.
#[test]
fn async_functions_and_their_wrappers_at_every_level() {
    let program = source_program(
        r#"(def ^:dynamic *d* 0)
        (defn- ^:async private-add [a b] (+ (await a) b))
        (defn plain [] (^:async fn [x] (inc (await x))))
        (defn ^:async outer [x] (let [g (fn [y] (* 2 y))] (g (await x))))
        (defn ^:async wrappers [p]
          [(assert (await p))
           (this-as t (some? (await p)))
           (binding [*d* (await p)] *d*)
           (with-out-str (print (await p)))
           (do (def defined (await p)) defined)
           (await ((fn ^:async self [n] (if (pos? n) (await (self (dec n))) (await p))) 3))
           (letfn [(^:async g [] (await p))] (await (g)))])
        (.then (js/Promise.all
                 #js [(private-add (js/Promise.resolve 1) 2)
                      ((plain) (js/Promise.resolve 4))
                      (outer (js/Promise.resolve 5))
                      (wrappers (js/Promise.resolve 7))])
               (fn [values] (js/console.log (pr-str (vec values)))))"#,
    );
    every_level(&program, "[3 5 10 [nil true 7 \"7\" 7 7 7]]\n");
    // A macro reads `helper`, so the macro heap compiles it too, where a
    // nested definition's initializer is a thunk of its own. A macro's
    // `&env` has `:async` in an `^:async` function's body only.
    let program = namespace_program(
        "ct.main",
        &[
            (
                "ct/m.cljc",
                "(ns ct.m)
                 (defn ^:async helper [p]
                   (def captured (await p))
                   (defonce kept (await p))
                   [captured kept])
                 (defmacro twice [x] (when (fn? helper) `(* 2 ~x)))
                 (defmacro in-async? [] (boolean (:async &env)))
                 (def counter (atom 0))
                 (defonce once 99)
                 (defn ^:async again [p]
                   (defonce once (do (swap! counter inc) (await p)))
                   once)
                 (defmacro counted [] (again (js/Promise.resolve 1)) @counter)",
            ),
            (
                "ct/main.cljs",
                "(ns ct.main (:require [ct.m :as m]))
                 (defn ^:async in-async [] [(m/in-async?) ((fn [] (m/in-async?)))])
                 (.then (m/helper (js/Promise.resolve (m/twice 3)))
                        (fn [v]
                          (.then (in-async)
                                 (fn [a]
                                   (.then (m/again (js/Promise.resolve 1))
                                          (fn [o]
                                            (js/console.log
                                             (pr-str [v a (m/in-async?) (m/counted) o @m/counter]))))))))",
            ),
        ],
    );
    // `counted` ran `again` while expanding: its `defonce` kept 99 there
    // without evaluating the initializer, as it does at run time.
    every_level(&program, "[[6 6] [true false] false 0 99 0]\n");
}

/// A function wrapped around expressions that await is awaited, but its
/// value is the one the expressions produce, even a Promise; a wrapper
/// around expressions that do not await stays a plain function. Values are
/// thus the same at every level, where `Advanced` folds a `let` away or
/// writes a `do` as a comma expression. A multi-arity `^:async` function's
/// dispatcher is async too, as upstream's, so an arity error rejects.
#[test]
fn wrappers_keep_values_and_dispatchers_reject() {
    let program = source_program(
        r#"(defn ^:async wrapped [p]
          [(instance? js/Promise (let [] p))
           (identical? p (do p))
           (identical? p (let [x (await p)] p))
           (identical? p (do (await p) p))
           (identical? p (try (await p) p (finally nil)))
           (identical? p (let [] (await p) p))
           (= 1 (let [x (await p)] x))])
        (defn ^:async multi ([] 0) ([x] x))
        (def rejected (js/Reflect.apply multi nil #js [1 2]))
        (.then (js/Promise.all
                #js [(wrapped (js/Promise.resolve 1))
                     (multi 7)
                     (.catch rejected (fn [e] (ex-message e)))])
               (fn [values]
                 (js/console.log (pr-str (conj (vec values) (instance? js/Promise rejected))))))"#,
    );
    every_level(
        &program,
        "[[true true true true true true true] 7 \"Invalid arity: 2\" true]\n",
    );
}

/// A call of an `^:async` function returns a Promise even in an unused
/// definition's initializer: preparation does not take its body's value for
/// the call's and remove the definition as inert, so the initializer runs
/// and `count` of the Promises throws at every level.
#[test]
fn async_calls_are_not_inert_initializers() {
    let program = source_program(
        "(def unused (mapv count (mapv (^:async fn [x] x) [[1]])))
         (js/console.log \"unreached\")",
    );
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!(
                "try {{ {js}; }} catch (e) {{ console.log(e.message.split(\":\")[0]); }}"
            )),
            "count not supported on this type\n",
            "at {level}"
        );
    }
}

/// A call of an `^:async` function is a Promise at every level: `Advanced`
/// neither inlines it nor infers its body's type for the call, and never
/// moves an argument that awaits past the reads of the body it is inlined
/// into, whose globals other code may change while the `await` waits.
#[test]
fn advanced_passes_keep_awaits_and_promises() {
    let program = source_program(
        r#"(defn ^:async one [] 1)
        (defn ^:async pair [] [1 2])
        (def g 1)
        (defn add-g [x] (+ g x))
        (defn ^:async later [p] (add-g (await p)))
        (def p (js/Promise. (fn [resolve]
                              (js/setTimeout (fn [] (set! g 10) (resolve 5)) 0))))
        (.then (later p)
               (fn [v]
                 (.then (one)
                        (fn [o]
                          (js/console.log
                           (pr-str [(instance? js/Promise (one)) (number? (one)) o v
                                    (try (nth (pair) 0) (catch :default e (ex-message e)))]))))))"#,
    );
    every_level(
        &program,
        "[true false 1 15 \"nth not supported on this type\"]\n",
    );
}

/// A devirtualized protocol call in an `^:async` function holds its
/// operands in locals of the function (light-cljs-262), so calls that wait
/// on their arguments at the same time each keep their own, also around an
/// awaited wrapper and when the receiver itself awaits.
#[test]
fn devirtualized_calls_keep_their_operands_across_awaits() {
    let program = source_program(
        r#"(defprotocol P (m [x a b]))
        (deftype T [v] P (m [_ a b] [v a b]))
        (defn later [ms v] (js/Promise. (fn [resolve] (js/setTimeout #(resolve v) ms))))
        (defn ^:async f [v p q]
          (m (T. v) (let [x (await p)] (inc x)) (m (T. (await q)) 0 (await q))))
        (defn ^:async g [p] (m (T. (await p)) (await p) 1))
        (.then (js/Promise.all #js [(f 1 (later 20 2) (later 0 3)) (f 4 (later 0 5) (later 20 6))
                                    (g (later 10 7))])
               (fn [values] (js/console.log (pr-str (vec values)))))"#,
    );
    every_level(&program, "[[1 3 [3 0 3]] [4 6 [6 0 6]] [7 7 1]]\n");
    let js = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .emit_js();
    assert!(!js.contains("=>r0"), "{js}");
}
