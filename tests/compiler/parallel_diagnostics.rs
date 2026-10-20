//! Diagnostics and limits with several unit slots (docs/compiler-concurrency.md,
//! "Semantics and determinism"): the reported error is the one with the
//! earliest canonical position, limits name the unit the one-slot build
//! names, warnings keep their order, and the macro memory limit is one budget
//! across the isolates.
use crate::macro_free_units::{Built, Sources, build_with};
use light_cljs::MacroHost;

/// A host with these expansion and node limits, and a memory limit that
/// gives four isolates.
fn limited(expansions: usize, generated_nodes: usize) -> MacroHost {
    let mut host = MacroHost::new();
    host.limits = light_cljs::MacroLimits {
        expansions,
        generated_nodes,
        ..Default::default()
    };
    host.evaluator_options.memory_limit = 256 << 20;
    host
}

/// The program or error of `entry` at one and at four slots, which must be
/// the same; the one-slot build.
fn same_at_one_and_four(sources: &Sources, entry: &str, host: impl Fn() -> MacroHost) -> Built {
    let serial = build_with(sources, entry, 1, false, host());
    for _ in 0..2 {
        let pooled = build_with(sources, entry, 4, false, host());
        assert_eq!(pooled.output, serial.output, "four slots");
        assert_eq!(pooled.warnings, serial.warnings, "four slots");
    }
    serial
}

/// Units whose bodies expand a source macro, in the workers' isolates.
fn script_units(sources: &Sources) {
    sources.write("app/m.cljs", "(ns app.m) (defmacro mac [n] `[~@(range n)])");
    let mut body = String::from("(ns app.l (:require [app.m :refer [mac]]))\n");
    for k in 0..16 {
        body.push_str(&format!(
            "(defn f{k} [] [(mac {}) (mac {})])\n",
            k % 5 + 1,
            k % 3 + 2
        ));
    }
    body.push_str("(js/console.log (count (f15)))");
    sources.write("app/l.cljs", &body);
}

#[test]
fn exact_boundary_expansion_and_node_limits_name_the_same_unit_at_one_and_four_slots() {
    let sources = Sources::new();
    script_units(&sources);
    let unlimited = build_with(&sources, "app.l", 1, false, limited(100_000, 1_000_000));
    assert!(unlimited.output.is_ok(), "{:?}", unlimited.output);
    let pooled = build_with(&sources, "app.l", 4, false, limited(100_000, 1_000_000));
    if cfg!(feature = "v8-macros") {
        assert_eq!(pooled.stats.deferred_units.pooled, 16);
        assert_eq!(pooled.stats.deferred_units.worker_isolates, 3);
    }
    let calls = unlimited.stats.macro_calls;
    let nodes = unlimited.stats.peak_macro_nodes;
    let mut failures = 0;
    for expansions in 1..=calls + 1 {
        let serial = same_at_one_and_four(&sources, "app.l", || limited(expansions, 1_000_000));
        failures += usize::from(serial.output.is_err());
    }
    assert_eq!(failures, calls - 1);
    for generated in (1..=nodes + 1).step_by((nodes / 40).max(1)) {
        same_at_one_and_four(&sources, "app.l", || limited(100_000, generated));
    }
}

#[test]
fn exact_boundary_depth_failures_name_the_same_unit_at_one_and_four_slots() {
    let sources = Sources::new();
    sources.write(
        "app/m.cljs",
        "(ns app.m) (defmacro deep [n] (if (pos? n) `(deep ~(dec n)) n))",
    );
    let mut body = String::from("(ns app.d (:require [app.m :refer [deep]]))\n");
    for k in 0..12 {
        body.push_str(&format!("(defn g{k} [] (deep {}))\n", (k * 7) % 12));
    }
    body.push_str("(js/console.log (g11))");
    sources.write("app/d.cljs", &body);
    let mut failures = 0;
    for depth in 1..=14 {
        let serial = same_at_one_and_four(&sources, "app.d", || {
            let mut host = limited(100_000, 1_000_000);
            host.limits.expansion_depth = depth;
            host
        });
        failures += usize::from(serial.output.is_err());
    }
    assert!(failures > 1 && failures < 14, "{failures}");
}

/// A macro that fails at once, and one that fails after spinning, so that
/// a later unit's worker fails first.
const FAILING: &str = "(ns app.m) (defmacro fast-fail [] (throw (js/Error. \"later failure\"))) \
    (defmacro slow-fail [] (let [end (+ (js/Date.now) 300)] (loop [] (when (< (js/Date.now) end) (recur)))) \
    (throw (js/Error. \"earlier failure\")))";

#[test]
fn an_earlier_units_error_wins_over_a_later_workers_that_failed_first() {
    let sources = Sources::new();
    sources.write("app/m.cljs", FAILING);
    sources.write(
        "app/e.cljs",
        "(ns app.e (:require [app.m :refer [slow-fail fast-fail]]))\n\
         (defn f0 [] 0)\n(defn f1 [] (slow-fail))\n(defn f2 [] 2)\n(defn f3 [] (fast-fail))\n\
         (defn f4 [] (fast-fail))\n(js/console.log (f0))",
    );
    let serial = same_at_one_and_four(&sources, "app.e", || limited(1024, 100_000));
    let error = serial.output.unwrap_err();
    assert!(
        error.contains("earlier failure") && error.contains("app/e.cljs:3:"),
        "{error}"
    );
}

#[test]
fn a_later_namespace_does_not_run_after_an_earlier_failure() {
    // `app.a` fails in a unit its workers analyze while later units of it
    // run; `app.b`, which follows it, expands nothing and warns nothing.
    let sources = Sources::new();
    sources.write("app/m.cljs", FAILING);
    let mut a = String::from("(ns app.a (:require [app.m :refer [slow-fail fast-fail]]))\n");
    a.push_str("(defn f0 [] (fast-fail))\n");
    for k in 1..8 {
        a.push_str(&format!("(defn f{k} [] (slow-fail))\n"));
    }
    sources.write("app/a.cljs", &a);
    sources.write(
        "app/b.cljs",
        "(ns app.b (:require [app.m :refer [fast-fail]])) (def x (fast-fail)) (defn y [] undeclared-in-b)",
    );
    sources.write(
        "app/main.cljs",
        "(ns app.main (:require [app.a] [app.b])) (js/console.log app.b/x)",
    );
    for slots in [1, 4] {
        let started = std::time::Instant::now();
        let built = build_with(&sources, "app.main", slots, true, limited(1024, 100_000));
        let error = built.output.unwrap_err();
        assert!(
            error.contains("later failure") && error.contains("app/a.cljs:2:"),
            "{error}"
        );
        assert!(
            built.stats.namespaces.iter().all(|n| n.name != "app.b"),
            "{slots} slots"
        );
        assert!(
            built
                .warnings
                .iter()
                .all(|w| !w.1.contains("undeclared-in-b")),
            "{slots} slots: {:?}",
            built.warnings
        );
        // The units after the failure that workers started are cancelled
        // rather than left to spin.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{slots} slots: {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn a_kept_units_failure_stops_the_build_before_the_next_top_level_pass() {
    // With one slot `app.a`'s unit is analyzed at its dispatch and fails:
    // the build stops there, so `app.b`'s top-level pass never runs, and
    // the error is the one every slot count reports at `app.a`'s drain
    // point (light-cljs-391). Warning mode is off, so units are not drained
    // form by form.
    let sources = Sources::new();
    sources.write("app/m.cljs", FAILING);
    sources.write(
        "app/a.cljs",
        "(ns app.a (:require [app.m :refer [fast-fail]]))\n(defn f0 [] 0)\n(defn f1 [] (fast-fail))",
    );
    sources.write(
        "app/b.cljs",
        "(ns app.b (:require [app.m :refer [fast-fail]])) (fast-fail)",
    );
    sources.write(
        "app/main.cljs",
        "(ns app.main (:require [app.a] [app.b])) (js/console.log (app.a/f0))",
    );
    let serial = build_with(&sources, "app.main", 1, false, limited(1024, 100_000));
    let error = serial.output.as_ref().unwrap_err();
    assert!(
        error.contains("later failure") && error.contains("app/a.cljs:3:"),
        "{error}"
    );
    assert!(serial.stats.namespaces.iter().all(|n| n.name != "app.b"));
    for slots in [2, 4, 8] {
        let pooled = build_with(&sources, "app.main", slots, false, limited(1024, 100_000));
        assert_eq!(pooled.output, serial.output, "{slots} slots");
        assert_eq!(pooled.warnings, serial.warnings, "{slots} slots");
    }
}

#[test]
fn a_failed_drain_point_reports_its_error_and_drains_no_later_namespace() {
    // Without warning mode `app.a`'s unit fails at its drain point, which
    // comes after `app.b`'s top-level pass (light-cljs-391). That failure is
    // the error: `app.b`'s units, which fail too and which the pool does
    // not start once an earlier unit failed, are not waited for.
    let sources = Sources::new();
    sources.write("app/m.cljs", FAILING);
    sources.write(
        "app/a.cljs",
        "(ns app.a (:require [app.m :refer [fast-fail]]))\n(defn f0 [] (fast-fail))",
    );
    sources.write(
        "app/b.cljs",
        "(ns app.b (:require [app.m :refer [slow-fail]]))\n(defn g0 [] (slow-fail))\n(defn g1 [] (slow-fail))",
    );
    sources.write(
        "app/main.cljs",
        "(ns app.main (:require [app.a] [app.b])) (js/console.log (app.a/f0))",
    );
    for slots in [1, 2, 4, 8] {
        let started = std::time::Instant::now();
        let built = build_with(&sources, "app.main", slots, false, limited(1024, 100_000));
        let error = built.output.unwrap_err();
        assert!(
            error.contains("later failure") && error.contains("app/a.cljs:2:"),
            "{slots} slots: {error}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{slots} slots: {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn warnings_are_in_the_same_order_at_every_slot_count() {
    let sources = Sources::new();
    sources.write(
        "app/m.cljs",
        "(ns app.m) (defmacro call-missing [n] (list (symbol (str \"missing-\" n)) n)) (defn two [a b] [a b])",
    );
    let mut requires = String::new();
    for n in 0..3 {
        let mut body = format!("(ns app.w{n} (:require [app.m :refer [call-missing two]]))\n");
        for k in 0..8 {
            body.push_str(&format!(
                "(defn f{k} [] [(call-missing {k}) (two 1) (+ 1 {k})])\n(def v{k} (when (f{k}) undeclared-{n}-{k}))\n"
            ));
        }
        sources.write(&format!("app/w{n}.cljs"), &body);
        requires.push_str(&format!(" [app.w{n}]"));
    }
    sources.write(
        "app/main.cljs",
        &format!("(ns app.main (:require{requires})) (js/console.log (app.w0/f0 1) app.w2/v1)"),
    );
    let serial = build_with(&sources, "app.main", 1, true, limited(1024, 100_000));
    assert!(serial.output.is_ok(), "{:?}", serial.output);
    assert_eq!(serial.warnings.len(), 3 * 8 * 2, "{:?}", serial.warnings);
    for slots in [2, 4, 8] {
        for _ in 0..2 {
            let pooled = build_with(&sources, "app.main", slots, true, limited(1024, 100_000));
            assert_eq!(pooled.output, serial.output, "{slots} slots");
            assert_eq!(pooled.warnings, serial.warnings, "{slots} slots");
        }
    }
}

/// A macro that holds `mb` MiB of JavaScript heap while it runs.
const HOG: &str = "(ns app.m) (defmacro hog [mb] (let [keep #js []] (dotimes [_ mb] (.push keep (.fill (js/Array. 131072) 1.5))) (.-length keep)))";

fn hogs(sources: &Sources, mb: usize) {
    sources.write("app/m.cljs", HOG);
    let mut body = String::from("(ns app.h (:require [app.m :refer [hog]]))\n");
    for k in 0..6 {
        let size = if k == 3 { mb } else { 1 };
        body.push_str(&format!("(defn h{k} [] (hog {size}))\n"));
    }
    body.push_str("(js/console.log (h3))");
    sources.write("app/h.cljs", &body);
}

#[test]
fn the_memory_limit_is_one_budget_across_the_isolates() {
    // With 256 MiB and four slots, four isolates share the limit: each is
    // allowed 32 MiB and takes more from the 128 MiB the others leave.
    // A unit that needs 96 MiB fits, as with one slot, which an even
    // split of 64 MiB each would not admit.
    let sources = Sources::new();
    hogs(&sources, 96);
    for slots in [1, 4] {
        let built = build_with(&sources, "app.h", slots, false, limited(1024, 100_000));
        assert!(built.output.is_ok(), "{slots} slots: {:?}", built.output);
        if slots == 4 && cfg!(feature = "v8-macros") {
            assert_eq!(built.stats.deferred_units.worker_isolates, 3);
        }
    }
    // One that needs more than the whole limit fails at every slot count,
    // in the same unit.
    hogs(&sources, 320);
    let failure = |slots| {
        let built = build_with(&sources, "app.h", slots, false, limited(1024, 100_000));
        built.output.unwrap_err()
    };
    let serial = failure(1);
    assert!(
        serial.contains("app/h.cljs:5:") && serial.contains("memory limit"),
        "{serial}"
    );
    for _ in 0..2 {
        let pooled = failure(4);
        assert!(
            pooled.contains("app/h.cljs:5:") && pooled.contains("memory limit"),
            "{pooled}"
        );
    }
}

#[test]
fn deep_recursion_in_a_macro_on_a_pool_thread_meets_the_engine_stack_limit() {
    // V8's stack limit is set from the thread an engine entry starts on:
    // a worker's 8 MiB pool thread stops runaway recursion as the main
    // thread does, with a catchable RangeError, and a macro that catches
    // it goes on.
    let sources = Sources::new();
    sources.write(
        "app/m.cljs",
        "(ns app.m) (defn descend [n] (descend (inc n))) \
         (defmacro caught [] (try (descend 0) (catch :default e (str (.-name e))))) \
         (defmacro runaway [] (descend 0))",
    );
    let mut body = String::from("(ns app.s (:require [app.m :refer [caught runaway]]))\n");
    for k in 0..4 {
        body.push_str(&format!("(defn s{k} [] (caught))\n"));
    }
    body.push_str("(js/console.log (s3))");
    sources.write("app/s.cljs", &body);
    let serial = same_at_one_and_four(&sources, "app.s", || limited(1024, 100_000));
    assert_eq!(
        crate::macro_free_units::execute(&serial.output.unwrap()),
        "RangeError\n"
    );
    body.push_str("\n(defn s9 [] (runaway))");
    sources.write("app/s.cljs", &body);
    let serial = same_at_one_and_four(&sources, "app.s", || limited(1024, 100_000));
    let error = serial.output.unwrap_err();
    assert!(error.contains("call stack"), "{error}");
}

#[test]
fn headroom_a_worker_took_returns_for_the_main_isolate() {
    // With 256 MiB and two slots, the main isolate and one worker are each
    // allowed 64 MiB and share 128 MiB of headroom. A worker's unit that
    // needs 150 MiB takes most of it; the worker is retired after that job,
    // so the headroom is back when the next namespace's top-level macro on
    // the main isolate needs as much, as with one slot: `DRAIN_LAG`
    // namespaces between them put that pass after `app.h`'s drain point,
    // which waits for the worker (light-cljs-391); within the lag, the two
    // would need the headroom at once. QuickJS-only builds have no pool,
    // and QuickJS's values are twice as large.
    if !cfg!(feature = "v8-macros") {
        return;
    }
    let sources = Sources::new();
    sources.write("app/m.cljs", HOG);
    sources.write(
        "app/h.cljs",
        "(ns app.h (:require [app.m :refer [hog]])) (defn h0 [] (hog 150)) (defn h1 [] 1)",
    );
    let mut fillers = String::new();
    for k in 0..light_cljs::analyzer::DRAIN_LAG {
        sources.write(
            &format!("app/f{k}.cljs"),
            &format!("(ns app.f{k}) (def v {k})"),
        );
        fillers.push_str(&format!(" [app.f{k}]"));
    }
    sources.write(
        "app/t.cljs",
        &format!(
            "(ns app.t (:require [app.h]{fillers} [app.m :refer [hog]])) (hog 150) (js/console.log (app.h/h0))"
        ),
    );
    for slots in [1, 2] {
        let built = build_with(&sources, "app.t", slots, false, limited(1024, 100_000));
        assert!(built.output.is_ok(), "{slots} slots: {:?}", built.output);
        if slots == 2 && cfg!(feature = "v8-macros") {
            assert!(built.stats.deferred_units.worker_isolates >= 1);
        }
    }
}
