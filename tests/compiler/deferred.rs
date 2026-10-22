//! Deferred runtime values checked independently of the shared result encoder.
use crate::temp_dir::TempDir;
use light_cljs::{compile, compile_namespace};
use std::{fs, process::Command};

fn node(source: &str) -> String {
    let javascript = compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    let output = Command::new("node")
        .args([
            "--expose-gc",
            "--max-old-space-size=96",
            "-e",
            &format!("console.log(JSON.stringify({javascript}));"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Deferred values with effects, and the JSON each prints; also built by
/// tests/compiler/schedule_stress.rs.
pub(crate) const DEFERRED_EFFECTS: &[(&str, &str)] = &[
    (
        "(let [n (atom 0) x (lazy-seq (swap! n inc) [7]) y (with-meta x {:a 1})] (array (realized? x) (realized? y) @n (first x) (realized? x) (realized? y) (first y) @n (identical? x y) (= x y)))",
        "[false,false,0,7,true,false,7,1,false,true]",
    ),
    (
        "(let [n (atom 0) d (delay (if (< (swap! n inc) 2) (throw 9) 7))] (array (try @d (catch :default x x)) (realized? d) @d @d @n (realized? d)))",
        "[9,false,7,7,2,true]",
    ),
    (
        "(let [n (atom 0) f (fn f [i] (lazy-seq (swap! n inc) (cons i (f (inc i))))) xs (f 0)] (array (reduce (fn [a x] (if (= x 2) (reduced (+ a x)) (+ a x))) 0 xs) @n))",
        "[3,3]",
    ),
];

/// Macros that defer their argument, and a namespace using them through
/// aliases; it prints `false 9 7 9`.
pub(crate) const DEFERRED_NAMESPACES: &[(&str, &str)] = &[
    (
        "macros.cljs",
        "(ns macros) (defmacro later [x] (list 'cljs.core/delay x)) (defmacro lazy-later [x] (list 'cljs.core/lazy-seq x))",
    ),
    (
        "main.cljs",
        "(ns main (:require [cljs.core :as c] [macros :as m])) (def ^:dynamic *x* 1) (let [v 7 d (m/later (+ v *x*)) xs (m/lazy-later [v]) ys (c/lazy-seq [9]) pending c/realized?] (js/console.log (pending d) (binding [*x* 2] (c/force d)) (first xs) (first ys)))",
    ),
];

#[test]
fn deferred_effects_and_metadata_pending_states_have_direct_host_expectations() {
    for (source, expected) in DEFERRED_EFFECTS {
        assert_eq!(node(source), format!("{expected}\n"));
    }
}

#[test]
fn long_nested_wrappers_and_abandoned_failures_survive_normal_gc() {
    assert_eq!(
        node(
            "(let [xs (loop [n 100000 xs (list 8)] (if (zero? n) xs (recur (dec n) (lazy-seq xs))))] (array (first xs) (count xs)))"
        ),
        "[8,1]\n"
    );
    assert_eq!(
        node(
            "(let [saved (delay #js [42])] (dotimes [i 5000] (let [capture (js/Array. 1000) d (delay (throw capture)) xs (lazy-seq (throw capture))] (try @d (catch :default _ nil)) (try (first xs) (catch :default _ nil)) (when (= 0 (mod i 500)) (js/gc)))) (js/gc) (array (aget @saved 0) (identical? @saved @saved)))"
        ),
        "[42,true]\n"
    );
}

#[test]
fn namespace_aliases_compiled_macro_output_and_captured_bindings_are_deferred() {
    let root = TempDir::new("cljs-deferred");
    for (path, source) in DEFERRED_NAMESPACES {
        fs::write(root.join(path), source).unwrap();
    }
    let javascript = compile_namespace("main", std::slice::from_ref(root.path())).unwrap();
    crate::cache_contract::check_mixed(root.path(), "main");
    let output = Command::new("node")
        .args(["-e", &javascript])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"false 9 7 9\n");
}

/// A macro that compiles a unit (`twice`) before it reads code in the
/// namespace that calls it: the engine's reader contexts are registered once
/// and then only as they change, also inside the unit's callback, and the
/// read still sees each namespace's aliases, refers and definitions so far
/// (light-cljs-423).
#[test]
fn macro_reads_reader_contexts_registered_as_they_change() {
    let root = TempDir::new("cljs-reader-registration");
    fs::write(
        root.join("a.cljs"),
        "(ns a) (defn twice [x] (* 2 x)) (defmacro resolved [text] (twice 1) (list 'quote (second (read-string (str \"`\" text)))))",
    )
    .unwrap();
    fs::write(
        root.join("b.cljs"),
        "(ns b (:require [a :as al] [clojure.string :refer [join]])) (def local 1) (js/console.log (pr-str [(al/resolved \"local\") (al/resolved \"al/twice\") (al/resolved \"join\")])) (defmacro mine [] 1) (def later (mine)) (js/console.log (pr-str [(al/resolved \"later\") (al/resolved \"mine\") later]))",
    )
    .unwrap();
    let javascript = compile_namespace("b", std::slice::from_ref(root.path())).unwrap();
    let output = Command::new("node")
        .args(["-e", &javascript])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "[b/local a/twice clojure.string/join]\n[b/later b/mine 1]\n"
    );
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn deferred_constructors() {
        run(&[
            Case::observed_during_expansion(
                "lazy-constructors",
                include_str!("../fixtures/phase-oracles/deferred/lazy-constructors.cljs"),
                r#"{"id":"lazy-constructors","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed_during_expansion(
                "deferred-constructor-false",
                include_str!("../fixtures/phase-oracles/deferred/deferred-constructor-false.cljs"),
                r#"{"id":"deferred-constructor-false","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
        ]);
    }
}
