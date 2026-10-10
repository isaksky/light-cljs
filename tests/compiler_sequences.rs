//! Compiled sequence checks with literal host results, independent of the shared encoder.
use light_cljs::{compile, compile_namespace};
use std::{fs, process::Command};

fn node(source: &str) -> String {
    let javascript = compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    let output = Command::new("node")
        .args([
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
fn chunk_boundaries_and_repeated_traversal_have_literal_effect_counts() {
    assert_eq!(
        node(
            "(let [a (atom 0) s (map #(do (swap! a inc) %) (range 100))] (array @a (first s) @a (nth s 31) @a (nth s 32) @a (count s) @a (count s) @a))"
        ),
        "[0,0,32,31,32,32,64,100,100,100,100]\n"
    );
    assert_eq!(
        node(
            "(let [a (atom []) s (map #(do (swap! a conj %) %) '(0 1 2 3))] (array (reduce (fn [r x] (if (= x 1) (reduced (+ r x)) (+ r x))) 0 s) (to-array @a)))"
        ),
        "[1,[0,1]]\n"
    );
    assert_eq!(
        node(
            "(let [a (atom []) fs (for [x (range 3) :let [y (* x 10)]] (fn [] y))] (doseq [f fs] (swap! a conj (f))) (to-array @a))"
        ),
        "[0,10,20]\n"
    );
}

#[test]
fn long_empty_prefixes_are_stack_safe_and_finite_demand_terminates() {
    assert_eq!(
        node(
            "(array (first (filter #(= % 100000) (range))) (first (keep #(when (= % 20000) %) (iterate inc 0))) (first (for [x (range) :when (= x 100000)] x)) (reduce + (take 1000 (mapcat #(list %) (range)))))"
        ),
        "[100000,20000,100000,499500]\n"
    );
}

#[test]
fn nested_modifiers_keep_per_iteration_captures_and_stop_at_each_level() {
    assert_eq!(
        node(
            "(to-array (map to-array (for [a '(1 2) :let [p (* a 10)] b '(3 4) :when (odd? b) c (range 3) :while (< c 2) :let [q (+ p b c)] d [q]] [a b c d])))"
        ),
        "[[1,3,0,13],[1,3,1,14],[2,3,0,23],[2,3,1,24]]\n"
    );
    assert_eq!(
        node(
            "(let [a (atom [])] (doseq [x '(0 1) :let [p (* x 10)] y (range 3) :when (odd? y) z '(4 5) :while (< z 5)] (swap! a conj (fn [] (+ p y z)))) (to-array (map #(%) @a)))"
        ),
        "[5,15]\n"
    );
}

#[test]
fn namespaces_aliases_and_native_macro_output_use_compiled_comprehensions() {
    let root = std::env::temp_dir().join(format!("light-cljs-sequences-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("macros.clj"),
        "(ns macros) (defmacro squares [xs] (list 'cljs.core/for ['x xs] (list '* 'x 'x)))",
    )
    .unwrap();
    fs::write(root.join("main.cljs"), "(ns main (:require [cljs.core :as c]) (:require-macros [macros :as m])) (let [s (m/squares (c/range 4)) n (atom 0) f c/map-indexed] (c/doseq [x s] (swap! n + x)) (js/console.log @n (c/pr-str (f vector s)) (c/pr-str (c/for [x [1 2]] (inc x)))))").unwrap();
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
    assert_eq!(output.stdout, b"14 ([0 0] [1 1] [2 4] [3 9]) (2 3)\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_comprehensions_and_transducer_constructors_are_not_silent_empty_sequences() {
    for source in [
        "(for (x [1]) x)",
        "(for [x] x)",
        "(for [x [1] :bogus true] x)",
        "(doseq [x [1] :bogus true] x)",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
    assert_eq!(
        node(
            "(to-array (map (fn [f] (try (f identity) false (catch js/Error e (boolean (re-find #\"transducer constructor\" (.-message e)))))) [map map-indexed mapcat filter remove keep keep-indexed take drop take-while drop-while partition-all partition-by interpose take-nth]))"
        ),
        "[true,true,true,true,true,true,true,true,true,true,true,true,true,true,true]\n"
    );
}
