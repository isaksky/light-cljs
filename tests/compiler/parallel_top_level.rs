//! Stage C parallel top-level namespace analysis (`light-cljs-1090`):
//! verifies strict require-closure visibility, protocol-extension frontier
//! isolation across sibling namespaces, reflection freshness fallback,
//! deterministic error precedence, warning ordering, lazy namespace discovery,
//! and byte-identical JS output across `--jobs 1, 2, 4, 8`.

use crate::macro_free_units::{
    Sources, build_blocked, build_with, execute, isolate_per_slot, same_at_every_slot_count,
};

fn assert_identical_across_all_slots(
    sources: &Sources,
    entry: &str,
    warning_mode: bool,
) -> crate::macro_free_units::Built {
    let serial = build_with(sources, entry, 1, warning_mode, isolate_per_slot());
    for slots in [2, 3, 4, 8] {
        let pooled = build_with(sources, entry, slots, warning_mode, isolate_per_slot());
        assert_eq!(
            pooled.output, serial.output,
            "{slots} slots (warning_mode={warning_mode}) output diverged from 1 slot"
        );
        assert_eq!(
            pooled.warnings, serial.warnings,
            "{slots} slots (warning_mode={warning_mode}) warnings diverged from 1 slot"
        );
        assert_eq!(
            pooled.spans, serial.spans,
            "{slots} slots (warning_mode={warning_mode}) spans diverged from 1 slot"
        );
        assert_eq!(
            pooled.mapped, serial.mapped,
            "{slots} slots (warning_mode={warning_mode}) mapped output diverged from 1 slot"
        );
    }
    serial
}

#[test]
fn wide_dag_visibility_and_byte_identity_across_slots() {
    let sources = Sources::new();
    sources
        .write(
            "app/macros.cljc",
            r#"(ns app.macros)
(defn helper [x] (list '+ x 1))
(def table {:a 10 :b 20 :c 30})
(defmacro add-two [x] (helper (helper x)))
(defmacro from-table [k] (get table k 0))
(defmacro named-add [x] (let [s (gensym "tmp")] `(let [~s ~x] (+ ~s 7))))"#,
        )
        .write(
            "app/proto.cljs",
            r#"(ns app.proto)
(defprotocol Metric (score [x]))
(extend-type number Metric (score [n] (* n 2)))
(defmulti dispatch :kind)
(defmethod dispatch :default [_] 0)"#,
        );

    let mut main_requires = String::new();
    let mut main_exprs = String::new();

    // 12 independent sibling namespaces: mix of pure-Rust and top-level macro
    // expanders, records, protocol extensions, multimethods, defonce, and :test.
    for i in 0..12 {
        let ns_name = format!("app.s{i}");
        let path = format!("app/s{i}.cljs");
        let body = if i % 3 == 0 {
            format!(
                r#"(ns {ns_name} (:require [app.macros :refer [add-two from-table named-add]] [app.proto :as p]))
(defprotocol LocalP{i} (local-v{i} [x]))
(defrecord Rec{i} [v]
  p/Metric (score [_] (+ v {i}))
  LocalP{i} (local-v{i} [_] (* v 3)))
(extend-protocol p/Metric
  string (score [s] (+ (count s) {i})))
(defmethod p/dispatch :k{i} [m] (+ (:v m) {i}))
(defonce stable-{i} (add-two {i}))
(def top-{i} (named-add (from-table :b)))
(defn compute-{i} {{:test (fn [] true)}} [x]
  (+ (p/score (->Rec{i} x)) (local-v{i} (->Rec{i} x)) (p/dispatch {{:kind :k{i} :v x}}) stable-{i} top-{i}))"#,
            )
        } else if i % 3 == 1 {
            format!(
                r#"(ns {ns_name} (:require [app.proto :as p]))
(defn pure-{i} [x] (+ (* x 4) {i} (p/score x)))
(def val-{i} (pure-{i} {i}))"#,
            )
        } else {
            format!(
                r#"(ns {ns_name} (:require [app.macros :refer [add-two]]))
(defmacro wrap-{i} [x] `(+ (add-two ~x) {i}))
(def top-{i} (wrap-{i} {i}))
(defn compute-{i} [x] (+ (wrap-{i} x) top-{i}))"#,
            )
        };
        sources.write(&path, &body);
        main_requires.push_str(&format!("[{ns_name} :as s{i}] "));
        if i % 3 == 0 {
            main_exprs.push_str(&format!(" (s{i}/compute-{i} {i})"));
        } else if i % 3 == 1 {
            main_exprs.push_str(&format!(" s{i}/val-{i}"));
        } else {
            main_exprs.push_str(&format!(" (s{i}/compute-{i} {i})"));
        }
    }

    // Second wave depending on siblings that define macros (`app.s2`, `app.s5`, `app.s8`, `app.s11`).
    for (k, dep) in [2usize, 5, 8, 11].into_iter().enumerate() {
        let ns_name = format!("app.l2-{k}");
        sources.write(
            &format!("app/l2_{k}.cljs"),
            &format!(
                r#"(ns {ns_name} (:require [app.s{dep} :as dep] [app.s{pure} :as p]))
(def top-l2 (dep/wrap-{dep} {k}))
(defn run-l2 [x] (+ top-l2 (dep/wrap-{dep} x) (p/pure-{pure} x)))"#,
                pure = if dep >= 1 { dep - 1 } else { 1 },
            ),
        );
        main_requires.push_str(&format!("[{ns_name} :as l{k}] "));
        main_exprs.push_str(&format!(" (l{k}/run-l2 {k})"));
    }

    sources.write(
        "app/main.cljs",
        &format!(
            "(ns app.main (:require {main_requires}))\n(js/console.log (pr-str [{main_exprs}]))"
        ),
    );

    let serial_release = assert_identical_across_all_slots(&sources, "app.main", false);
    let serial_dev = assert_identical_across_all_slots(&sources, "app.main", true);
    let out_release = execute(serial_release.output.as_ref().unwrap());
    let out_dev = execute(serial_dev.output.as_ref().unwrap());
    assert_eq!(out_release, out_dev);
}

#[test]
fn sibling_protocol_extension_isolation_in_macro_expansion() {
    // `app.before`, `app.extender`, and `app.after` are siblings that only
    // require `app.p` and `app.m`. `app.extender` extends `app.p/Tag` on
    // `number` at compile time.
    // - `app.before` (canonically before `app.extender`) must observe the
    //   base extension (`+ 10`) even when workers reuse isolates across siblings.
    // - `app.after` (canonically after `app.extender`) must observe `app.extender`'s
    //   extension (`+ 1000`), matching `--jobs 1` even if `app.after` ran
    //   speculatively before `app.extender` published.
    let sources = Sources::new();
    sources
        .write(
            "app/p.cljs",
            r#"(ns app.p)
(defprotocol Tag (tag-of [x]))
(extend-type number Tag (tag-of [n] (+ n 10)))"#,
        )
        .write(
            "app/m.cljs",
            r#"(ns app.m (:require [app.p :as p]))
(defmacro eval-tag [x] (p/tag-of x))"#,
        )
        .write(
            "app/before1.cljs",
            r#"(ns app.before1 (:require [app.p] [app.m :refer [eval-tag]]))
(def v (eval-tag 5))"#,
        )
        .write(
            "app/before2.cljs",
            r#"(ns app.before2 (:require [app.p] [app.m :refer [eval-tag]]))
(def v (eval-tag 6))"#,
        )
        .write(
            "app/extender.cljs",
            r#"(ns app.extender (:require [app.p :as p] [app.m :refer [eval-tag]]))
(extend-type number p/Tag (tag-of [n] (+ n 1000)))
(def v (eval-tag 7))"#,
        )
        .write(
            "app/after1.cljs",
            r#"(ns app.after1 (:require [app.p] [app.m :refer [eval-tag]]))
(def v (eval-tag 8))"#,
        )
        .write(
            "app/after2.cljs",
            r#"(ns app.after2 (:require [app.p] [app.m :refer [eval-tag]]))
(def v (eval-tag 9))"#,
        )
        .write(
            "app/main.cljs",
            r#"(ns app.main (:require [app.before1 :as b1] [app.before2 :as b2] [app.extender :as e] [app.after1 :as a1] [app.after2 :as a2]))
(js/console.log b1/v b2/v e/v a1/v a2/v)"#,
        );

    let serial = assert_identical_across_all_slots(&sources, "app.main", false);
    assert_eq!(
        execute(serial.output.as_ref().unwrap()),
        "15 16 1007 1008 1009\n"
    );
}

#[test]
fn namespace_reflection_invalidates_stale_parallel_lookahead() {
    // `app.reflect` inspects `(find-ns 'app.s1)` at top level without requiring
    // `app.s1`. Because `app.s1` precedes `app.reflect` in canonical DFS order,
    // `is_fresh` must invalidate any speculative run of `app.reflect` whose
    // snapshot predated `app.s1`'s publication.
    let sources = Sources::new();
    sources
        .write(
            "app/rmac.cljs",
            r#"(ns app.rmac)
(defmacro saw-ns? [sym] (boolean (find-ns sym)))"#,
        )
        .write("app/s0.cljs", "(ns app.s0) (def a 1)")
        .write("app/s1.cljs", "(ns app.s1) (def b 2)")
        .write(
            "app/reflect.cljs",
            r#"(ns app.reflect (:require [app.rmac :refer [saw-ns?]]))
(def saw-s1 (saw-ns? app.s1))
(def saw-later (saw-ns? app.s2))"#,
        )
        .write("app/s2.cljs", "(ns app.s2) (def c 3)")
        .write(
            "app/main.cljs",
            r#"(ns app.main (:require [app.s0] [app.s1] [app.reflect :as r] [app.s2]))
(js/console.log r/saw-s1 r/saw-later)"#,
        );

    let serial = assert_identical_across_all_slots(&sources, "app.main", false);
    assert_eq!(execute(serial.output.as_ref().unwrap()), "true false\n");
}

#[test]
fn top_level_find_ns_and_all_ns_fall_back_to_canonical_registry() {
    // `app.reflect` expands `(def-saw-ns ...)` directly at top level, calling
    // `find-ns` and `all-ns` inside JS during a parallel lookahead pass.
    // The compile-time `findNs`/`allNs` hooks must trigger coordinator fallback
    // so the emitted program matches `--jobs 1` byte-for-byte.
    let sources = Sources::new();
    sources
        .write(
            "app/rmac.cljs",
            r#"(ns app.rmac)
(defmacro def-saw-ns [var-name sym]
  (let [found (boolean (find-ns sym))
        listed (boolean (some #(= (ns-name %) sym) (all-ns)))]
    `(def ~var-name [~found ~listed])))
(defmacro def-touched [sym v] `(def ~sym ~v))"#,
        )
        .write(
            "app/s0.cljs",
            "(ns app.s0 (:require [app.rmac :refer [def-touched]])) (def-touched a 1)",
        )
        .write(
            "app/s1.cljs",
            "(ns app.s1 (:require [app.rmac :refer [def-touched]])) (def-touched b 2)",
        )
        .write(
            "app/reflect.cljs",
            r#"(ns app.reflect (:require [app.rmac :refer [def-saw-ns]]))
(def-saw-ns saw-s1 app.s1)
(def-saw-ns saw-s2 app.s2)"#,
        )
        .write("app/s2.cljs", "(ns app.s2) (def c 3)")
        .write(
            "app/main.cljs",
            r#"(ns app.main (:require [app.rmac] [app.s0] [app.s1] [app.reflect :as r] [app.s2]))
(js/console.log (pr-str [r/saw-s1 r/saw-s2]))"#,
        );

    let serial = assert_identical_across_all_slots(&sources, "app.main", false);
    assert_eq!(
        execute(serial.output.as_ref().unwrap()),
        "[[true true] [false false]]\n"
    );
}

#[test]
fn earlier_namespace_top_level_error_wins_over_later_parallel_siblings() {
    let sources = Sources::new();
    sources
        .write(
            "app/failmac.cljs",
            r#"(ns app.failmac)
(defmacro slow-boom []
  (let [end (+ (js/Date.now) 150)]
    (loop [] (when (< (js/Date.now) end) (recur))))
  (throw (js/Error. "earlier top-level failure")))
(defmacro fast-boom [label]
  (throw (js/Error. (str "later failure: " label))))"#,
        )
        .write(
            "app/n0.cljs",
            "(ns app.n0 (:require [app.failmac :refer [slow-boom]])) (def x (slow-boom))",
        )
        .write(
            "app/n1.cljs",
            "(ns app.n1 (:require [app.failmac :refer [fast-boom]])) (def x (fast-boom \"n1\"))",
        )
        .write(
            "app/n2.cljs",
            "(ns app.n2 (:require [app.failmac :refer [fast-boom]])) (def x (fast-boom \"n2\"))",
        )
        .write(
            "app/n3.cljs",
            "(ns app.n3 (:require [app.failmac :refer [fast-boom]])) (def x (fast-boom \"n3\"))",
        )
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.n0] [app.n1] [app.n2] [app.n3])) (js/console.log 1)",
        );

    for warning_mode in [false, true] {
        let serial = assert_identical_across_all_slots(&sources, "app.main", warning_mode);
        let err = serial.output.unwrap_err();
        assert!(
            err.contains("earlier top-level failure") && err.contains("app/n0.cljs"),
            "warning_mode={warning_mode}: {err}"
        );
    }
}

#[test]
fn warnings_order_and_spans_match_one_slot_across_wide_dag() {
    let sources = Sources::new();
    sources.write(
        "app/wmac.cljs",
        "(ns app.wmac) (defmacro use-sym [s] `(+ 1 ~s))",
    );
    let mut requires = String::new();
    let mut exprs = String::new();
    for i in 0..10 {
        let ns_name = format!("app.w{i}");
        sources.write(
            &format!("app/w{i}.cljs"),
            &format!(
                r#"(ns {ns_name} (:require [app.wmac :refer [use-sym]]))
(defn f{i} [] (+ undeclared-top-{i} (use-sym undeclared-mac-{i})))
(def ok-{i} {i})"#,
            ),
        );
        requires.push_str(&format!("[{ns_name} :as w{i}] "));
        exprs.push_str(&format!(" w{i}/ok-{i}"));
    }
    sources.write(
        "app/main.cljs",
        &format!("(ns app.main (:require {requires})) (js/console.log {exprs})"),
    );

    let serial = assert_identical_across_all_slots(&sources, "app.main", true);
    assert!(serial.warnings.len() >= 20, "{:?}", serial.warnings);
    assert_eq!(
        execute(serial.output.as_ref().unwrap()),
        "0 1 2 3 4 5 6 7 8 9\n"
    );
}

#[test]
fn qualified_symbol_without_require_preserves_canonical_order() {
    let sources = Sources::new();
    sources
        .write("app/first.cljs", "(ns app.first) (def answer 42)")
        .write(
            "app/second.cljs",
            "(ns app.second) (defn read-answer [] (+ app.first/answer 1))",
        )
        .write(
            "app/third.cljs",
            "(ns app.third) (defn read-again [] (+ app.first/answer 2))",
        )
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.first] [app.second :as s] [app.third :as t])) (js/console.log (s/read-answer) (t/read-again))",
        );

    let serial = same_at_every_slot_count(&sources, "app.main", false);
    assert_eq!(execute(serial.output.as_ref().unwrap()), "43 44\n");
}

#[test]
fn warning_mode_coordinator_per_form_drains_skip_single_unit_pooling() {
    // A linear require chain forces every namespace to run on the coordinator
    // (no lookahead namespace is ever ready ahead of `current_index`). In
    // `warning_mode`, per-form drains produce 1-unit batches and must execute
    // inline without submitting single-unit round trips to `UnitPool`.
    let chain = Sources::new();
    for i in 0..4 {
        let req = if i == 0 {
            String::new()
        } else {
            format!(" (:require [app.c{} :as prev])", i - 1)
        };
        let call_prev = if i == 0 {
            format!("{i}")
        } else {
            "(prev/step 1)".to_owned()
        };
        chain.write(
            &format!("app/c{i}.cljs"),
            &format!(
                r#"(ns app.c{i}{req})
(defrecord Box{i} [a])
(when false (Box{i}. 1 2))
(defn warn-{i} [] missing-var-{i})
(defn f0 [x] (+ x {i}))
(defn f1 [x] (when (pos? x) (f0 x)))
(defn f2 [x] (let [y (f1 x)] (+ y {call_prev})))
(defn step [x] (+ (.-a (->Box{i} (f2 x))) {i}))"#
            ),
        );
    }
    chain.write(
        "app/main.cljs",
        "(ns app.main (:require [app.c3 :as c]))\n(js/console.log (c/step 2))",
    );

    let serial = build_with(&chain, "app.main", 1, true, isolate_per_slot());
    assert!(serial.output.is_ok(), "{:?}", serial.output);
    assert!(
        serial
            .warnings
            .iter()
            .any(|(kind, msg, _, _)| kind == "fn-arity" && msg.contains("passed to Box0")),
        "missing record constructor arity warning: {:?}",
        serial.warnings
    );
    for slots in [2, 4, 8] {
        let built = build_with(&chain, "app.main", slots, true, isolate_per_slot());
        assert_eq!(built.output, serial.output, "{slots} slots output");
        assert_eq!(built.warnings, serial.warnings, "{slots} slots warnings");
        assert_eq!(built.spans, serial.spans, "{slots} slots spans");
        assert_eq!(built.mapped, serial.mapped, "{slots} slots mapped");
        assert_eq!(
            built.stats.deferred_units.pooled, 0,
            "{slots} slots should not pool 1-unit per-form warning drains"
        );
        assert_eq!(
            built.stats.deferred_units.pool.started, 0,
            "{slots} slots linear chain should not ping-pong units on pool"
        );
    }
    assert_eq!(execute(serial.output.as_ref().unwrap()), "17\n");

    // A cache blocker disables `ParallelTopLevel` on a wide sibling DAG too:
    // per-form coordinator drains must still skip 1-unit pooling.
    let wide = Sources::new();
    wide.write("app/blocker.cljs", "(ns app.blocker) (def k 1)");
    let mut requires = String::from("[app.blocker]");
    for i in 0..4 {
        wide.write(
            &format!("app/b{i}.cljs"),
            &format!(
                "(ns app.b{i} (:require [app.blocker :as b]))\n\
                 (defn f0 [x] (+ x b/k {i}))\n\
                 (defn f1 [x] (when x (f0 x)))\n\
                 (defn w [] missing-b-{i})"
            ),
        );
        requires.push_str(&format!(" [app.b{i}]"));
    }
    wide.write(
        "app/main.cljs",
        &format!("(ns app.main (:require {requires}))\n(js/console.log (app.b3/f1 2))"),
    );
    let blockers = vec!["app.blocker".to_owned()];
    let serial_blocked = build_blocked(&wide, "app.main", 1, true, isolate_per_slot(), &blockers);
    for slots in [2, 4, 8] {
        let built = build_blocked(
            &wide,
            "app.main",
            slots,
            true,
            isolate_per_slot(),
            &blockers,
        );
        assert_eq!(built.output, serial_blocked.output, "blocked {slots} slots");
        assert_eq!(
            built.warnings, serial_blocked.warnings,
            "blocked {slots} slots warnings"
        );
        assert_eq!(
            built.stats.deferred_units.pooled, 0,
            "blocked {slots} slots"
        );
        assert_eq!(
            built.stats.deferred_units.pool.started, 0,
            "blocked {slots} slots"
        );
    }
    assert_eq!(execute(serial_blocked.output.as_ref().unwrap()), "6\n");
}

#[test]
fn warning_mode_uses_parallel_top_level_at_two_slots_without_unit_ping_pong() {
    let sources = Sources::new();
    sources.write(
        "app/macros.cljs",
        "(ns app.macros)\n(defmacro add-one [x] `(+ ~x 1))",
    );
    let mut requires = String::new();
    let mut calls = String::new();
    for i in 0..8 {
        let ns_name = format!("app.s{i}");
        sources.write(
            &format!("app/s{i}.cljs"),
            &format!(
                r#"(ns {ns_name} (:require [app.macros :refer [add-one]]))
(defrecord Item{i} [v])
(when false (Item{i}. 1 2))
(defn warned-{i} [] undeclared-s-{i})
(defn compute-{i} [x] (+ (.-v (->Item{i} (add-one x))) {i}))"#
            ),
        );
        requires.push_str(&format!("[{ns_name} :as s{i}] "));
        calls.push_str(&format!(" (s{i}/compute-{i} {i})"));
    }
    sources.write(
        "app/main.cljs",
        &format!("(ns app.main (:require {requires}))\n(js/console.log (pr-str [{calls}]))"),
    );

    let serial = assert_identical_across_all_slots(&sources, "app.main", true);
    assert_eq!(serial.warnings.len(), 16, "{:?}", serial.warnings);
    assert_eq!(
        execute(serial.output.as_ref().unwrap()),
        "[1 3 5 7 9 11 13 15]\n"
    );

    for slots in [2, 4, 8] {
        let built = build_with(&sources, "app.main", slots, true, isolate_per_slot());
        assert_eq!(built.output, serial.output, "{slots} slots");
        assert_eq!(built.warnings, serial.warnings, "{slots} slots");
        assert_eq!(
            built.stats.deferred_units.pooled, 0,
            "{slots} slots should not submit 1-unit warning drains"
        );
        assert!(
            built.stats.deferred_units.pool.started > 0,
            "{slots} slots should run lookahead namespaces via ParallelTopLevel in warning_mode"
        );
    }

    // Early failure cutoff in a sibling namespace matches `--jobs 1` across
    // slots 2, 4, 8: warnings from earlier forms/namespaces stay, later ones stop.
    sources.write(
        "app/s2.cljs",
        r#"(ns app.s2 (:require [app.macros :refer [add-one]]))
(defn before-fail [] undeclared-before-fail)
(quote)
(defn after-fail [] undeclared-after-fail)"#,
    );
    let failed = assert_identical_across_all_slots(&sources, "app.main", true);
    let err = failed.output.unwrap_err();
    assert!(err.contains("quote"), "{err}");
    assert!(
        failed
            .warnings
            .iter()
            .any(|(_, msg, _, _)| msg.contains("undeclared-before-fail")),
        "{:?}",
        failed.warnings
    );
    assert!(
        failed
            .warnings
            .iter()
            .all(|(_, msg, _, _)| !msg.contains("undeclared-after-fail")),
        "{:?}",
        failed.warnings
    );
}

#[test]
fn warning_mode_macro_defining_namespace_keeps_multi_unit_batch_pooling() {
    let sources = Sources::new();
    sources.write(
        "app/m.cljs",
        r#"(ns app.m)
(defmacro emit-inc [x] `(+ ~x 1))
(defn f0 [x] (when x (+ x 1)))
(defn f1 [x] (let [y (emit-inc x)] y))
(defn f2 [] undeclared-m-2)
(defn f3 [x] (+ (f0 x) (f1 x)))"#,
    );
    sources.write(
        "app/main.cljs",
        "(ns app.main (:require [app.m :as m]))\n(js/console.log (m/f3 10))",
    );

    let serial = assert_identical_across_all_slots(&sources, "app.main", true);
    assert_eq!(execute(serial.output.as_ref().unwrap()), "22\n");
    for slots in [2, 4, 8] {
        let built = build_with(&sources, "app.main", slots, true, isolate_per_slot());
        assert!(
            built.stats.deferred_units.pooled >= 4,
            "{slots} slots should still pool macro-defining namespace's deferred batch: {:?}",
            built.stats.deferred_units
        );
    }

    // Failing after multiple deferred units in a macro-defining namespace runs
    // `drained_before_failure` -> `drain_deferred_keeping(true)` with >1 units,
    // which still pools the multi-unit batch and keeps exact warning/error parity.
    sources.write(
        "app/m.cljs",
        r#"(ns app.m)
(defmacro emit-inc [x] `(+ ~x 1))
(defn f0 [] undeclared-m-0)
(defn f1 [] undeclared-m-1)
(defn f2 [] undeclared-m-2)
(defn f3 [] undeclared-m-3)
(quote)
(defn f4 [] undeclared-m-4)"#,
    );
    let failed = assert_identical_across_all_slots(&sources, "app.main", true);
    assert!(failed.output.as_ref().unwrap_err().contains("quote"));
    let msgs: Vec<&str> = failed
        .warnings
        .iter()
        .map(|(_, m, _, _)| m.as_str())
        .collect();
    assert_eq!(
        msgs,
        [
            "Use of undeclared Var app.m/undeclared-m-0",
            "Use of undeclared Var app.m/undeclared-m-1",
            "Use of undeclared Var app.m/undeclared-m-2",
            "Use of undeclared Var app.m/undeclared-m-3",
        ]
    );
    for slots in [2, 4, 8] {
        let built = build_with(&sources, "app.main", slots, true, isolate_per_slot());
        assert!(
            built.stats.deferred_units.pooled >= 4,
            "{slots} slots should still pool >1-unit batches in drain_deferred_keeping: {:?}",
            built.stats.deferred_units
        );
    }
}
