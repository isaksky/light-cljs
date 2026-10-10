//! Deferred runtime values checked independently of the shared result encoder.
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

#[test]
fn deferred_effects_and_metadata_pending_states_have_direct_host_expectations() {
    assert_eq!(
        node(
            "(let [n (atom 0) x (lazy-seq (swap! n inc) [7]) y (with-meta x {:a 1})] (array (realized? x) (realized? y) @n (first x) (realized? x) (realized? y) (first y) @n (identical? x y) (= x y)))"
        ),
        "[false,false,0,7,true,false,7,1,false,true]\n"
    );
    assert_eq!(
        node(
            "(let [n (atom 0) d (delay (if (< (swap! n inc) 2) (throw 9) 7))] (array (try @d (catch :default x x)) (realized? d) @d @d @n (realized? d)))"
        ),
        "[9,false,7,7,2,true]\n"
    );
    assert_eq!(
        node(
            "(let [n (atom 0) f (fn f [i] (lazy-seq (swap! n inc) (cons i (f (inc i))))) xs (f 0)] (array (reduce (fn [a x] (if (= x 2) (reduced (+ a x)) (+ a x))) 0 xs) @n))"
        ),
        "[3,3]\n"
    );
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
fn namespace_aliases_native_macro_output_and_captured_bindings_are_deferred() {
    let root = std::env::temp_dir().join(format!("light-cljs-deferred-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("macros.clj"), "(ns macros) (defmacro later [x] (list 'cljs.core/delay x)) (defmacro lazy-later [x] (list 'cljs.core/lazy-seq x))").unwrap();
    fs::write(root.join("main.cljs"), "(ns main (:require [cljs.core :as c]) (:require-macros [macros :as m])) (def ^:dynamic *x* 1) (let [v 7 d (m/later (+ v *x*)) xs (m/lazy-later [v]) ys (c/lazy-seq [9]) pending c/realized?] (js/console.log (pending d) (binding [*x* 2] (c/force d)) (first xs) (first ys)))").unwrap();
    let javascript = compile_namespace("main", std::slice::from_ref(&root)).unwrap();
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
    fs::remove_dir_all(root).unwrap();
}
