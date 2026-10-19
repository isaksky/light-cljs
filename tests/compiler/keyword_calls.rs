//! A keyword-literal call, `(:k m)` or `(:k m d)`, is emitted at every level
//! as the runtime's `get(m, :k)` or `get(m, :k, d)`, which is what the
//! keyword's `call` does after testing its brand and counting its arguments
//! (light-cljs-370). A keyword called through a local, `map`, `apply` or any
//! other higher-order use stays a call of the keyword's `IFn`. The keyword
//! calls the runtime's own `get`, so replacing `cljs.core/get` does not change
//! what a keyword-literal call returns.
//!
//! Every program runs at every level in Node and in QuickJS and must give
//! the same value.
use super::optimization::{execute, readable};
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{
    ApplicationOptions, OptimizationLevel, Program, analyze, parse, prepare_application,
};
use std::time::Duration;

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

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
/// Node and in QuickJS; returns the readable output of each level.
fn every_level(source: &str, expected: &str) -> Vec<String> {
    let program = program(source);
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
            prepare_application(&program, readable(level)).emit_js()
        })
        .collect()
}

/// Missing keys, the default argument (also `nil`), `nil` and other
/// receivers that are not maps (strings, vectors, host objects, a `deftype`
/// without `ILookup`), records, sorted maps, sets and transients answer as
/// the keyword's `call` answers; the receiver is read before the default.
#[test]
fn keyword_literal_calls_are_get_calls() {
    let source = r#"
      (defrecord R [a b])
      (deftype T [])
      (defn look [r] [(:a r) (:b r 5) (:a r nil) (:x/a r :d)])
      (def xs [{:a 1} {:b 2} nil "s" [1 2] (->R 3 4) (T.) (sorted-map :a 9) (js-obj "a" 7)
               #{:a} (transient {:a 2}) {:x/a 6}])
      (pr-str [(mapv look xs)
               (map :a [{:a 1} {:b 2}])
               (apply :a [{:a 1}])
               (apply :a [{} :d])
               (let [k :a] [(k {:a 3}) (k {} 4)])
               (let [log (atom [])]
                 [(:a (do (swap! log conj :m) {}) (do (swap! log conj :d) 2)) @log])])"#;
    let texts = every_level(
        source,
        "[[[1 5 1 :d] [nil 2 nil :d] [nil 5 nil :d] [nil 5 nil :d] [nil 5 nil :d] \
         [3 4 3 :d] [nil 5 nil :d] [9 5 9 :d] [nil 5 nil :d] [:a 5 :a :d] [2 5 2 :d] \
         [nil 5 nil 6]] (1 nil) 1 :d [3 4] [2 [:m :d]]]",
    );
    for (level, text) in OptimizationLevel::ALL.into_iter().zip(&texts) {
        let program = &text[text.rfind("cljs.user").unwrap_or(0)..];
        let get = if level.advanced() {
            "rt$get("
        } else {
            "rt.get("
        };
        assert!(program.contains(get), "{level}: {program}");
        assert!(
            !program.contains("\")).call(null") && !program.contains("k0).call(null"),
            "{level}: a keyword literal is still called: {program}"
        );
    }
}

/// With `cljs.core/get` replaced, `(get m k)` calls the replacement and a
/// keyword-literal call still looks the key up, as the keyword's `call` does.
#[test]
fn keyword_literal_calls_ignore_a_replaced_get() {
    let source = r#"
      (set! cljs.core/get (fn [& _] :replaced))
      (defn look [m] [(:a m) (:a m 2) (get m :a)])
      (pr-str (mapv look [{:a 1} {}]))"#;
    every_level(source, "[[1 1 :replaced] [nil 2 :replaced]]");
}
