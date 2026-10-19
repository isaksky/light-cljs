//! A cache blocker's affinity to the main isolate with several slots
//! (docs/compiler-concurrency.md, "As built (light-cljs-340)"): a
//! `--cache-blocker` namespace and every namespace that requires or names one,
//! transitively, analyze their deferred units on the coordinator's isolate
//! at their drains, as one slot does, with the persistent cache off; every
//! other namespace's units still go to the pool. `^:light-cljs/no-cache` is
//! a cache policy only and gives no affinity. QuickJS-only builds are
//! serial, so every slot count behaves as one there.
use crate::macro_free_units::{Built, Sources, build_blocked, execute};
use light_cljs::MacroHost;

fn blocked(sources: &Sources, entry: &str, slots: usize, blockers: &[&str]) -> Built {
    let blockers: Vec<String> = blockers.iter().map(|b| (*b).to_owned()).collect();
    build_blocked(sources, entry, slots, false, MacroHost::new(), &blockers)
}

/// The output of `entry` with `blockers` at one slot, after checking that
/// two, four and eight slots, twice each, emit the same program, warnings
/// and macro counts.
fn same_at_every_slot_count(sources: &Sources, entry: &str, blockers: &[&str]) -> String {
    let serial = blocked(sources, entry, 1, blockers);
    for slots in [2, 4, 8] {
        for _ in 0..2 {
            let pooled = blocked(sources, entry, slots, blockers);
            assert_eq!(pooled.output, serial.output, "{slots} slots");
            assert_eq!(pooled.warnings, serial.warnings, "{slots} slots");
            assert_eq!(
                pooled.stats.macro_calls, serial.stats.macro_calls,
                "{slots} slots"
            );
        }
    }
    serial.output.unwrap()
}

fn counter<'a>(sources: &'a Sources, provider: &str) -> &'a Sources {
    sources
        .write(
            "macros.cljs",
            &format!(
                "(ns {provider}) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))"
            ),
        )
        .write(
            "dep.cljs",
            "(ns dep (:require [macros :refer [next-value]])) (def value (next-value))",
        )
        .write(
            "main.cljs",
            "(ns main (:require [dep :as d] [macros :refer [next-value]])) (js/console.log d/value (next-value))",
        )
}

/// `1 1` where several slots run `dep`'s unit in a worker's isolate: the
/// documented behavior of an undeclared provider.
fn undeclared() -> &'static str {
    if cfg!(feature = "v8-macros") {
        "1 1\n"
    } else {
        "1 2\n"
    }
}

#[test]
fn the_counter_fixture_prints_one_two_at_every_slot_count_with_its_provider_blocked() {
    let sources = Sources::new();
    counter(&sources, "macros");
    // `dep` and `main` require the blocker, so `dep`'s unit expands
    // `next-value` in the main isolate, where `main`'s top-level call then
    // sees the counter at 1.
    for slots in [1, 2, 8] {
        let built = blocked(&sources, "main", slots, &["macros"]);
        assert_eq!(execute(&built.output.unwrap()), "1 2\n", "{slots} slots");
        assert_eq!(built.stats.deferred_units.pooled, 0, "{slots} slots");
    }
    assert_eq!(
        execute(&same_at_every_slot_count(&sources, "main", &["macros"])),
        "1 2\n"
    );
    // Without a blocker the same fixture pins the documented `1 1`
    // ("Semantics and determinism"): nothing detects the effect.
    let serial = blocked(&sources, "main", 1, &[]);
    assert_eq!(execute(&serial.output.unwrap()), "1 2\n");
    for slots in [2, 4, 8] {
        let built = blocked(&sources, "main", slots, &[]);
        assert_eq!(
            execute(&built.output.unwrap()),
            undeclared(),
            "{slots} slots"
        );
    }
    // A blocker the build does not load blocks nothing.
    let built = blocked(&sources, "main", 4, &["missing"]);
    assert_eq!(execute(&built.output.unwrap()), undeclared());
}

#[test]
fn a_consumer_through_a_fully_qualified_macro_keeps_the_main_isolate() {
    // `dep` requires nothing and expands `macros/next-value`, loaded by
    // `main` before it, fully qualified: its source names the blocker, so
    // it gets affinity although no require reaches it.
    let sources = Sources::new();
    counter(&sources, "macros")
        .write("dep.cljs", "(ns dep) (def value (macros/next-value))")
        .write(
            "main.cljs",
            "(ns main (:require [macros] [dep :as d])) (js/console.log d/value (macros/next-value))",
        );
    for slots in [1, 2, 8] {
        let built = blocked(&sources, "main", slots, &["macros"]);
        assert_eq!(execute(&built.output.unwrap()), "1 2\n", "{slots} slots");
    }
    for slots in [2, 8] {
        let built = blocked(&sources, "main", slots, &[]);
        assert_eq!(
            execute(&built.output.unwrap()),
            undeclared(),
            "{slots} slots"
        );
    }
}

#[test]
fn a_sibling_of_a_blockers_consumer_still_runs_on_workers() {
    let sources = Sources::new();
    counter(&sources, "macros")
        .write(
            "dep.cljs",
            "(ns dep (:require [macros :refer [next-value]])) (def value (next-value)) \
             (defn f [n] (when (pos? n) (inc n)))",
        )
        .write(
            "plain.cljs",
            "(ns plain) (defn g [x] (let [y (* x 2)] (when (pos? y) y))) (def table {:a 1}) \
             (defn h [n] (cond (neg? n) 0 :else (g n)))",
        )
        .write(
            "main.cljs",
            "(ns main (:require [dep :as d] [plain :as p] [macros :refer [next-value]])) \
             (js/console.log d/value (next-value) (d/f 1) (p/h 2) (:a p/table))",
        );
    let output = same_at_every_slot_count(&sources, "main", &["macros"]);
    assert_eq!(execute(&output), "1 2 2 4 1\n");
    // `plain` requires no blocker: its three units go to the pool, and only
    // they; with `plain` blocked too, none does.
    let built = blocked(&sources, "main", 4, &["macros"]);
    let blocked_too = blocked(&sources, "main", 4, &["macros", "plain"]);
    assert_eq!(blocked_too.output, built.output);
    assert_eq!(blocked_too.stats.deferred_units.pooled, 0);
    if cfg!(feature = "v8-macros") {
        assert_eq!(built.stats.deferred_units.pooled, 3);
        assert!(built.stats.deferred_units.worker_isolates > 0);
    } else {
        assert_eq!(built.stats.deferred_units.pooled, 0);
    }
}

#[test]
fn no_cache_on_a_provider_gives_its_consumers_no_affinity() {
    // Intended: `^:light-cljs/no-cache` keeps a namespace out of the
    // persistent cache and says nothing about isolates
    // (docs/compiler-concurrency.md, "The two policies stay separate"), so
    // the undeclared counter still prints `1 1` with several slots. Only
    // `--cache-blocker` gives affinity, with or without the metadata.
    let sources = Sources::new();
    counter(&sources, "^:light-cljs/no-cache macros");
    let serial = blocked(&sources, "main", 1, &[]);
    assert_eq!(execute(&serial.output.unwrap()), "1 2\n");
    for slots in [2, 8] {
        let built = blocked(&sources, "main", slots, &[]);
        assert_eq!(
            execute(&built.output.unwrap()),
            undeclared(),
            "{slots} slots"
        );
        let built = blocked(&sources, "main", slots, &["macros"]);
        assert_eq!(execute(&built.output.unwrap()), "1 2\n", "{slots} slots");
    }
}
