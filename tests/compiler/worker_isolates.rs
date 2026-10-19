//! Deferred units analyzed in per-worker V8 isolates (docs/compiler-concurrency.md,
//! "As built (light-cljs-339)"): with several slots every unit but a
//! `:test` one is analyzed by a pool worker in its own isolate, which
//! installs the source macros, helpers and extensions it needs from the
//! shared catalog, and the build emits the program, warnings and errors of
//! a build with one slot.
use crate::macro_free_units::{Built, Sources, build, execute};

/// Build `entry` at one slot, then at two, four and eight, three rounds
/// each, and check each against the first; return the one-slot build.
fn same_at_one_two_four_and_eight(sources: &Sources, entry: &str, warning_mode: bool) -> Built {
    let serial = build(sources, entry, 1, warning_mode);
    for slots in [2, 4, 8] {
        for _ in 0..3 {
            let pooled = build(sources, entry, slots, warning_mode);
            assert_eq!(pooled.output, serial.output, "{slots} slots");
            assert_eq!(pooled.warnings, serial.warnings, "{slots} slots");
            assert_eq!(
                pooled.stats.macro_calls, serial.stats.macro_calls,
                "{slots} slots"
            );
            assert_eq!(
                pooled.stats.peak_macro_nodes, serial.stats.peak_macro_nodes,
                "{slots} slots"
            );
        }
    }
    serial
}

const MACROS: &str = r#"(ns app.m)
(defn helper [x] (list 'inc x))
(def table {:a 1 :b 2})
(defmacro twice [x] (helper (helper x)))
(defmacro looked-up [k] (get table k))
(defmacro named [] (let [s (gensym "v")] `(let [~s 1] (+ ~s 1))))
(defmacro thread [x & forms] `(-> ~x ~@forms))"#;

const PROTOCOLS: &str = r#"(ns app.p)
(defprotocol Shape (area [s]))
(defrecord Square [side] Shape (area [_] (* side side)))
(extend-type number Shape (area [n] n))"#;

#[test]
fn units_expanding_source_and_core_macros_emit_the_one_slot_program() {
    let sources = Sources::new();
    sources
        .write("app/m.cljc", MACROS)
        .write("app/p.cljs", PROTOCOLS)
        .write(
            "app/a.cljs",
            r#"(ns app.a (:require [app.m :refer [twice looked-up named thread]] [app.p :as p]))
(def x (twice 1))
(def y (looked-up :b))
(defn f [n] (when (pos? n) (-> n inc (* 2))))
(def g (named))
(def h (p/area (p/->Square 3)))
(def i (p/area 5))
(defn k [] (vec (for [j (range 3)] (twice j))))
(defn l [n] (thread n inc (* 3)))
(defmulti kind :kind)
(defmethod kind :a [_] "a")
(defmethod kind :default [_] "other")
(def m (kind {:kind :a}))"#,
        )
        .write(
            "app/main.cljs",
            r#"(ns app.main (:require [app.a :as a] [app.m :refer [twice]]))
(def z (twice a/x))
(js/console.log a/x a/y (a/f 2) a/g a/h a/i (pr-str (a/k)) (a/l 1) a/m z (a/kind {}))"#,
        );
    let serial = same_at_one_two_four_and_eight(&sources, "app.main", false);
    assert_eq!(
        execute(&serial.output.unwrap()),
        "3 2 6 2 9 5 [2 3 4] 6 a 5 other\n"
    );
    // The workers took the units, in isolates of their own, and initialized
    // the helper roots the macros call there.
    let units = build(&sources, "app.main", 4, false).stats.deferred_units;
    if cfg!(feature = "v8-macros") {
        assert!(units.pooled > 10, "{units:?}");
        assert!((1..=3).contains(&units.worker_isolates), "{units:?}");
        assert!(units.worker_initializations > 0, "{units:?}");
    } else {
        assert_eq!(units.pooled, 0, "{units:?}");
    }
}

#[test]
fn warnings_and_errors_of_units_on_workers_are_the_one_slot_ones() {
    let sources = Sources::new();
    sources.write("app/m.cljc", MACROS).write(
        "app/w.cljs",
        r#"(ns app.w (:require [app.m :refer [twice]]))
(defn uses [] (twice missing-one))
(defn other [] (+ 1 missing-two))
(def ok (twice 1))
(js/console.log ok)"#,
    );
    let serial = same_at_one_two_four_and_eight(&sources, "app.w", true);
    assert!(
        serial.warnings.iter().any(|w| w.1.contains("missing-one")),
        "{:?}",
        serial.warnings
    );
    // A macro that throws in a unit fails the build with the unit's error.
    sources.write(
        "app/e.cljs",
        r#"(ns app.e (:require [app.m :refer [twice]]))
(defmacro boom [] (throw (js/Error. "boom in a unit")))
(def fine (twice 1))
(def bad (boom))
(def after (twice 2))"#,
    );
    let serial = same_at_one_two_four_and_eight(&sources, "app.e", false);
    let error = serial.output.unwrap_err();
    assert!(error.contains("boom in a unit"), "{error}");
}

#[test]
fn a_helper_demanded_from_a_worker_initializes_in_its_isolate() {
    // `macros/table` is a root `dep`'s units read through `looked-up`, and
    // `app.main`'s top-level expansion reads it in the main isolate: each
    // isolate initializes it, and the analysis is shared.
    let sources = Sources::new();
    sources.write("app/m.cljc", MACROS).write(
        "app/main.cljs",
        r#"(ns app.main (:require [app.m :refer [looked-up twice]]))
(def a (looked-up :a))
(def b (twice (looked-up :b)))
(js/console.log a b (looked-up :a))"#,
    );
    let serial = same_at_one_two_four_and_eight(&sources, "app.main", false);
    assert_eq!(execute(&serial.output.unwrap()), "1 4 1\n");
}

#[test]
fn a_helper_with_test_metadata_demanded_from_a_worker_is_analyzed_there() {
    // `x`'s unit expands `m`, which calls `f`: a worker demands `f`'s unit,
    // whose `:test` unit it analyzes through its own catalog view.
    let sources = Sources::new();
    sources.write(
        "app/t.cljs",
        r#"(ns app.t)
(declare f)
(defmacro m [] (f))
(def x (m))
(defn f {:test (fn [] true)} [] 42)
(js/console.log x)"#,
    );
    let serial = same_at_one_two_four_and_eight(&sources, "app.t", false);
    assert_eq!(execute(&serial.output.unwrap()), "42\n");
}

#[test]
fn a_worker_isolate_that_runs_out_of_time_fails_its_unit_and_the_build_ends() {
    // `a`'s unit spins in its worker's isolate until the macro deadline:
    // the engine fails for good and is retired, the coordinator waiting
    // for `a` gets its failure, and the build fails at every slot count.
    let sources = Sources::new();
    sources.write("app/m.cljc", MACROS).write(
        "app/s.cljs",
        r#"(ns app.s (:require [app.m :refer [twice]]))
(defmacro spin [] (loop [n 0] (recur (inc n))))
(def before (twice 1))
(def a (spin))
(def b (twice 2))
(def c (twice 3))"#,
    );
    for slots in [1, 2, 4] {
        let mut host = light_cljs::MacroHost::new();
        host.evaluator_options.timeout = std::time::Duration::from_millis(300);
        let built = crate::macro_free_units::build_with(&sources, "app.s", slots, false, host);
        let error = built.output.unwrap_err();
        // Wall-clock failures are scheduling-sensitive ("Semantics and
        // determinism", Limits): the unit and the kind of failure are the
        // same, the message may be its expansion's or its charge's.
        assert!(
            error.contains("app/s.cljs:4:8") && error.contains("deadline exceeded"),
            "{slots} slots: {error}"
        );
    }
}

#[test]
fn a_persistent_worker_sees_each_units_own_macro_inventory() {
    // `seen` reads the macro inventory in `&env`: `before`'s snapshot has
    // no `later`, `after`'s has it, whichever worker analyzes each.
    let sources = Sources::new();
    sources.write(
        "app/main.cljs",
        r#"(ns app.main)
(defmacro seen [] (contains? (:light-cljs/macros &env) 'app.main/later))
(def before (seen))
(defmacro later [] 1)
(def after (seen))
(def again (seen))
(js/console.log before after again)"#,
    );
    let serial = same_at_one_two_four_and_eight(&sources, "app.main", false);
    assert_eq!(execute(&serial.output.unwrap()), "false true true\n");
}
