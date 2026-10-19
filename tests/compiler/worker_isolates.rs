//! Deferred units analyzed in per-worker V8 isolates (docs/compiler-concurrency.md,
//! "As built (light-cljs-339)"): with several slots every unit but a
//! `:test` one is analyzed by a pool worker in its own isolate, which
//! installs the source macros, helpers and extensions it needs from the
//! shared catalog, and the build emits the program, warnings and errors of
//! a build with one slot.
use crate::macro_free_units::{Sources, build, execute, same_at_every_slot_count};

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
    let serial = same_at_every_slot_count(&sources, "app.main", false);
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
fn helper_built_macro_results_map_alike_in_every_isolate() {
    // `table` builds `{:k i}` at compile time. Its keyword is the engine's
    // interned one, which no input of either `rows` call holds: the result's
    // spans are each call's own, never those of an earlier form some
    // isolate imported `:k` from (light-cljs-379).
    let sources = Sources::new();
    sources
        .write(
            "app/m.cljs",
            "(ns app.m) (defn table [n] (vec (for [i (range n)] {:k i}))) (defmacro rows [n] (table n))",
        )
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.m :refer [rows]])) (def a (rows 3)) (def b (rows 2)) (js/console.log (count a) (count b))",
        );
    let serial = same_at_every_slot_count(&sources, "app.main", false);
    assert_eq!(execute(&serial.output.unwrap()), "3 2\n");
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
    let serial = same_at_every_slot_count(&sources, "app.w", true);
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
    let serial = same_at_every_slot_count(&sources, "app.e", false);
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
    let serial = same_at_every_slot_count(&sources, "app.main", false);
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
    let serial = same_at_every_slot_count(&sources, "app.t", false);
    assert_eq!(execute(&serial.output.unwrap()), "42\n");
}

#[test]
fn a_first_pooled_build_compiles_the_bootstrap_before_its_deadline_starts() {
    // As the first build of its test process, this one compiles the
    // bootstrap units, before its isolates share a deadline: in one of
    // them the compilation would spend it (a debug assertion in
    // `ExecutionControl::exempt`, light-cljs-378).
    let sources = Sources::new();
    sources.write("app/m.cljc", MACROS).write(
        "app/s.cljs",
        r#"(ns app.s (:require [app.m :refer [twice]]))
(def a (twice 1))
(def b (for [x [1 2]] (twice x)))
(js/console.log (pr-str [a (vec b)]))"#,
    );
    let built = crate::macro_free_units::build(&sources, "app.s", 4, false);
    assert_eq!(execute(&built.output.unwrap()), "[3 [3 4]]\n");
}

#[test]
fn a_worker_isolate_that_runs_out_of_time_fails_its_unit_and_the_build_ends() {
    // `a`'s unit spins in its worker's isolate until the macro deadline:
    // the engine fails for good and is retired, the coordinator waiting
    // for `a` gets its failure, and the build fails at every slot count.
    let program = |a: &str| {
        let sources = Sources::new();
        sources.write("app/m.cljc", MACROS).write(
            "app/s.cljs",
            &format!(
                r#"(ns app.s (:require [app.m :refer [twice]]))
(defmacro spin [] (loop [n 0] (recur (inc n))))
(def before (twice 1))
(def a {a})
(def b (twice 2))
(def c (twice 3))"#
            ),
        );
        sources
    };
    let (sources, without_spin) = (program("(spin)"), program("0"));
    for slots in [1, 2, 4] {
        let mut host = light_cljs::MacroHost::new();
        host.evaluator_options.timeout = crate::macro_free_units::deadline_after(|| {
            let built = crate::macro_free_units::build(&without_spin, "app.s", slots, false);
            built.output.unwrap();
        });
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
    let serial = same_at_every_slot_count(&sources, "app.main", false);
    assert_eq!(execute(&serial.output.unwrap()), "false true true\n");
}

/// What a build that records its dependency manifest and captures every
/// namespace's artifact gives: its output, manifest, artifacts and stats.
struct Recorded {
    output: Result<String, String>,
    manifest: light_cljs::dependency_manifest::DependencyManifest,
    artifacts: Vec<(
        String,
        Result<light_cljs::namespace_artifact::NamespaceArtifact, String>,
    )>,
    stats: light_cljs::CompileStats,
}

fn recorded(sources: &Sources, entry: &str, slots: usize) -> Recorded {
    let mut manifest = Default::default();
    let mut artifacts = Vec::new();
    let mut sink = |capture: light_cljs::namespace_artifact::NamespaceCapture| {
        artifacts.push((capture.namespace, capture.artifact));
    };
    let mut stats = light_cljs::CompileStats::new();
    // Room for eight isolates: the default limit gives two shares, one
    // worker's and the coordinator's.
    let mut host = light_cljs::MacroHost::new();
    host.evaluator_options.memory_limit = 256 << 20;
    let output = light_cljs::compiler::analyze_namespace_entry_recorded(
        entry,
        std::slice::from_ref(sources.root()),
        &mut host,
        Default::default(),
        &mut Vec::new(),
        light_cljs::compiler::EntryRecorders {
            stats: Some(&mut stats),
            manifest: Some(&mut manifest),
            capture: Some(&mut sink),
            slots: light_cljs::scheduler::Slots::new(slots).unwrap(),
            ..Default::default()
        },
    )
    .map(|(program, _)| light_cljs::emit_js(&program))
    .map_err(|error| error.to_string());
    Recorded {
        output,
        manifest,
        artifacts,
        stats,
    }
}

#[test]
fn units_on_workers_keep_the_one_slot_manifest_and_artifacts() {
    // Recording a manifest or capturing artifacts leaves macro-expanding
    // units on the workers (light-cljs-374): what their analyses observe
    // (a resource read, by a macro and by a helper's initializer each
    // isolate runs, a loaded namespace named without a require, a
    // reflected namespace, a dotted symbol's absent namespace, source macro
    // calls) and what capture records of them (expansions, generated
    // names, accounting) are handed to the coordinator, which takes them in
    // unit order, so both match a build with one slot.
    let sources = Sources::new();
    sources
        .write(
            "lib/helpers.cljs",
            r#"(ns lib.helpers (:require [light.build :as b]))
(def reads (atom 0))
(def table (when (exists? js/lightCljsBuild) (swap! reads inc) (b/resource "lib/table.txt")))"#,
        )
        .write(
            "lib/macros.cljs",
            r#"(ns lib.macros (:require [lib.helpers :as h] [light.build :as b]))
(defmacro table [] (str h/table " #" @h/reads))
(defmacro inline [path] (b/resource path))
(defmacro named [] (let [s (gensym "v")] `(let [~s 1] (+ ~s 1))))
(defmacro twice [x] `(let [v# ~x] (+ v# v#)))"#,
        )
        .write("lib/other.cljs", "(ns lib.other) (def value 7)")
        .write("lib/table.txt", "rows")
        .write("app/data.txt", "data")
        .write(
            "app/a.cljs",
            r#"(ns app.a (:require [lib.macros :refer [table inline named twice]]))
(def t (table))
(def i (inline "app/data.txt"))
(def p (count (ns-publics 'lib.other)))
(def l (twice lib.other/value))
(def g (named))
(defn f [] (some? Math.PI))
(defn u1 [] (twice (named)))
(defn u2 [] (table))
(defn u3 [] (twice (inline "app/data.txt")))
(defn u4 [] (named))
(defn u5 [] (twice lib.other/value))
(defn u6 [] (count (ns-publics 'lib.other)))
(defn u7 [] (twice (table)))
(defn u8 [] (named))"#,
        )
        .write(
            "app/b.cljs",
            r#"(ns app.b (:require [lib.macros :refer [table named]]))
(def t (table))
(def g (named))"#,
        )
        .write(
            "app/main.cljs",
            r#"(ns app.main (:require [lib.other] [app.a :as a] [app.b :as b]))
(js/console.log a/t a/i a/p a/l a/g (a/f) b/t b/g)"#,
        );
    let serial = recorded(&sources, "app.main", 1);
    let js = serial.output.as_ref().unwrap();
    assert_eq!(execute(js), "rows #1 data 1 14 2 true rows #1 2\n");
    assert!(serial.artifacts.iter().all(|(_, a)| a.is_ok()));
    for slots in [2, 4, 8] {
        let mut isolates = 0;
        for _ in 0..3 {
            let pooled = recorded(&sources, "app.main", slots);
            isolates = isolates.max(pooled.stats.deferred_units.worker_isolates);
            assert_eq!(pooled.output, serial.output, "{slots} slots");
            assert_eq!(pooled.manifest, serial.manifest, "{slots} slots");
            assert_eq!(pooled.artifacts, serial.artifacts, "{slots} slots");
            assert_eq!(
                pooled.stats.macro_calls, serial.stats.macro_calls,
                "{slots} slots"
            );
            // The units that expand macros were the pool's too.
            let units = &pooled.stats.deferred_units;
            if cfg!(feature = "v8-macros") {
                assert!(units.pooled > units.macro_free, "{units:?}");
                assert!(units.worker_isolates > 0, "{units:?}");
            } else {
                assert_eq!(units.pooled, 0, "{units:?}");
            }
        }
        // Several workers recorded at once.
        if cfg!(feature = "v8-macros") && slots > 2 {
            assert!(isolates > 1, "{slots} slots: {isolates} worker isolates");
        }
    }
}
