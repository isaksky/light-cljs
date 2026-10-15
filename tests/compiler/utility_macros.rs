//! The user-facing core macros `time`, `simple-benchmark`, `locking`,
//! `js-debugger`, `js-comment`, `unchecked-max` and `unchecked-min`
//! (light-cljs-260), which `src/compiler_core_macros.cljs` expands as
//! `cljs/core.cljc` of the pinned checkout and of the 1.12.134 release do.
//!
//! Every program runs at every optimization level in Node and in QuickJS and
//! must give what ClojureScript 1.12.134 gave for the same source, compiled by
//! `cljs.main -t node` with `:static-fns false` and run on Node 24 (October
//! 14, 2026). `time` reads `system-time`, which both compilers take from
//! `performance.now` when it exists, so the programs install a clock that
//! advances 2.25 ms per read; `simple-benchmark` reads `Date`, so the programs
//! replace its millisecond count with `N`.
use super::optimization::execute;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{ApplicationOptions, OptimizationLevel, analyze, parse, prepare_application};
use std::time::Duration;

/// A `performance.now` that reads 12.25, 14.5, ... (2.25 ms per read).
const CLOCK: &str = r#"(set! (.-performance js/globalThis)
  (js-obj "now" (let [t (atom 10)] (fn [] (swap! t + 2.25)))))"#;

/// The value of the JavaScript string expression `expression` in a fresh
/// QuickJS evaluator.
fn quickjs(expression: &str) -> String {
    let evaluator = Evaluator::new(EvaluatorOptions {
        memory_limit: 1 << 30,
        timeout: Duration::from_secs(60),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    let value = evaluator
        .evaluate_js(expression)
        .unwrap_or_else(|error| panic!("{error:?}"));
    evaluator.string(value).unwrap()
}

/// The program, whose value is a string, gives `expected` at every level in
/// Node and in QuickJS; returns the output of each level.
fn every_level(source: &str, expected: &str) -> Vec<String> {
    let program = analyze(source, &parse(source).unwrap())
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    OptimizationLevel::ALL
        .into_iter()
        .map(|level| {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            assert_eq!(
                execute(&format!("console.log({js});")),
                format!("{expected}\n"),
                "Node at {level}: {source}"
            );
            assert_eq!(quickjs(&js), expected, "QuickJS at {level}: {source}");
            js
        })
        .collect()
}

fn rejected(source: &str, message: &str) {
    let error = analyze(source, &parse(source).unwrap())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(message),
        "{source}\nexpected {message:?} in {error}"
    );
}

/// `time` evaluates its expression once, prints `"Elapsed time: <ms> msecs"`
/// with `prn` (quoted, six decimals, through `*print-fn*`, then a newline
/// when `*print-newline*` is set) and returns the value, at the top level, in
/// a function body and nested.
#[test]
fn time_prints_the_elapsed_milliseconds_and_returns_the_value() {
    let source = format!(
        r#"{CLOCK}
        (defn doubled [x] (time (* x 2)))
        (pr-str
         [(let [v (atom nil) n (atom 0)
                s (with-out-str (reset! v (time (do (swap! n inc) (+ 1 2)))))]
            [@v @n s])
          (let [out (atom []) print (fn [s] (swap! out conj s))]
            [(binding [*print-fn* print *print-newline* false] (time :k))
             (binding [*print-fn* print *print-newline* true] (time nil))
             @out])
          [(with-out-str (time (time 1))) (with-out-str (doubled 4))]])"#
    );
    every_level(
        &source,
        r#"[[3 1 "\"Elapsed time: 2.250000 msecs\"\n"] [:k nil ["\"Elapsed time: 2.250000 msecs\"" "\"Elapsed time: 2.250000 msecs\"" "\n"]] ["\"Elapsed time: 2.250000 msecs\"\n\"Elapsed time: 6.750000 msecs\"\n" "\"Elapsed time: 2.250000 msecs\"\n"]]"#,
    );
    // Compiled output printing to the console, as upstream's `-t node` does.
    let source = format!("(enable-console-print!) {CLOCK} (time (+ 1 2))");
    let program = analyze(&source, &parse(&source).unwrap()).unwrap();
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            "\"Elapsed time: 2.250000 msecs\"\n3\n",
            "at {level}"
        );
    }
    // The host's own clock: `performance.now` in Node, `Date.now` where the
    // engine has no `performance`.
    every_level(
        r#"(let [v (atom nil) s (with-out-str (reset! v (time (+ 1 2))))]
             (pr-str [@v (boolean (re-matches #"\"Elapsed time: \d+\.\d{6} msecs\"\n" s))]))"#,
        "[3 true]",
    );
}

/// `simple-benchmark` binds its bindings, runs its expression the given
/// number of times and calls `:print-fn` (default `println`) with the printed
/// bindings and expression, the count and the elapsed milliseconds,
/// returning what that call returns. The iteration expression is evaluated
/// twice, as upstream splices it twice.
#[test]
fn simple_benchmark_runs_its_expression_and_prints_one_line() {
    every_level(
        r#"(def norm (fn [s] (.replace s (js/RegExp. "\\d+ msecs" "g") "N msecs")))
        (pr-str
         [(let [out (atom []) n (atom 0)
                r (simple-benchmark [x 2 s "a" k :kw] (swap! n + x) 3
                    :print-fn (fn [line] (swap! out conj (norm line)) :printed))]
            [r @n @out])
          (let [r (atom :unset) s (with-out-str (reset! r (simple-benchmark [x 1] (+ x 1) (+ 1 1))))]
            [@r (norm s)])
          (let [out (atom []) its (atom 0)]
            [(simple-benchmark [] nil (do (swap! its inc) 2)
               :print-fn (fn [line] (swap! out conj (norm line))))
             @its @out])])"#,
        r#"[[:printed 6 ["[x 2 s \"a\" k :kw], (swap! n + x), 3 runs, N msecs"]] [nil "[x 1], (+ x 1), 2 runs, N msecs\n"] [["[], nil, 2 runs, N msecs"] 2 ["[], nil, 2 runs, N msecs"]]]"#,
    );
    // The bindings and expression print as light-cljs macro data, a
    // documented difference (docs/compiler.md, "Core macros, destructuring
    // and assertions"): upstream's JVM printer gives
    // `[x 2.0 c \a], (str c x 1.0), 1 runs, N msecs`.
    every_level(
        r#"(pr-str
         (let [out (atom [])]
           (simple-benchmark [x 2.0 c \a] (str c x 1.0) 1
             :print-fn (fn [line] (swap! out conj (.replace line (js/RegExp. "\\d+ msecs") "N msecs"))))
           @out))"#,
        r#"["[x 2 c \"a\"], (str c x 1), 1 runs, N msecs"]"#,
    );
    // Options are keyword arguments: upstream's destructuring rejects a
    // trailing map.
    rejected(
        "(simple-benchmark [] nil 1 {:print-fn prn})",
        "No value supplied for key: {:print-fn prn}",
    );
}

/// `locking` is its body: upstream discards the lock expression without
/// evaluating it.
#[test]
fn locking_evaluates_only_its_body() {
    every_level(
        r#"(pr-str
         (let [log (atom [])]
           [(locking (swap! log conj :lock) (swap! log conj :a) (swap! log conj :b) 3)
            @log (locking :x)]))"#,
        "[3 [:a :b] nil]",
    );
}

/// `js-debugger` is a `debugger;` statement and `js-comment` a `/** */` block
/// whose later lines lose three leading spaces and whose trailing empty lines
/// are dropped; both are nil and stay in the output at every level, as raw
/// `js*` text does.
#[test]
fn js_debugger_and_js_comment_emit_raw_statements_whose_value_is_nil() {
    let outputs = every_level(
        r#"(defn f-comment [] (js-comment "c"))
        (js-comment "a\n   b\n    c\n\n")
        (pr-str [(js-debugger) (let [x (js-debugger)] x) ((fn [] (js-debugger) 4)) (f-comment)])"#,
        "[nil nil 4 nil]",
    );
    for (level, js) in OptimizationLevel::ALL.into_iter().zip(outputs) {
        assert_eq!(js.matches("debugger;").count(), 3, "at {level}");
        assert!(js.contains("\n/**\n * a\n * b\n *  c\n */\n"), "at {level}");
        assert!(js.contains("\n/**\n * c\n */\n"), "at {level}");
    }
    // A comment is nil in a value position too, where upstream's bare
    // template emits `var x = /** ... */;`, which is not JavaScript.
    every_level(
        r#"(pr-str [(let [x (js-comment "v")] x) (js-comment "")])"#,
        "[nil nil]",
    );
    rejected("(js-comment :x)", "js-comment requires a string");
}

/// `unchecked-max` and `unchecked-min` evaluate each operand once, left to
/// right; two operands compare with `>` or `<`, so a NaN operand wins only
/// in second place, and more operands nest `max` or `min`.
#[test]
fn unchecked_max_and_min_compare_their_operands_once() {
    every_level(
        r#"(pr-str
         (let [log (atom []) f (fn [x] (swap! log conj x) x)]
           [(unchecked-max 1) (unchecked-max (f 1) (f 3)) (unchecked-min (f 4) (f 2))
            (unchecked-max 1 5 3) (unchecked-min 4 2 9 1)
            (unchecked-max 1 ##NaN) (unchecked-max ##NaN 1) (unchecked-min 1 ##NaN)
            (unchecked-min ##NaN 1) (unchecked-max -0 0) @log]))"#,
        "[1 3 2 5 1 ##NaN 1 ##NaN 1 0 [1 3 4 2]]",
    );
}
