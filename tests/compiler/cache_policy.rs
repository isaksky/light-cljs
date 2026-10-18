//! Per-namespace persistent-cache eligibility decided from the current
//! settings, `ns` metadata and dependency manifest of a real compilation
//! (docs/compiler-cache.md, "Opt-outs and their propagation").
use crate::temp_dir::TempDir;
use light_cljs::cache_policy::{
    Bypass, CachePolicy, CacheSettings, NamespaceMetadata, evaluate, source_metadata,
};
use light_cljs::compiler::analyze_namespace_entry_with_dependencies;
use light_cljs::dependency_manifest::{DependencyManifest, EdgeKinds};
use light_cljs::{MacroHost, analyzer::AnalysisOptions};
use std::fs;

struct Sources(TempDir);

impl Sources {
    fn new(files: &[(&str, &str)]) -> Self {
        let sources = Self(TempDir::new("cljs-cache-policy"));
        for (path, source) in files {
            sources.write(path, source);
        }
        sources
    }
    fn write(&self, path: &str, source: &str) {
        fs::write(self.0.join(path), source).unwrap();
    }
    /// The manifest of `entry` and the metadata of its `ns` forms.
    fn compile(&self, entry: &str) -> (DependencyManifest, Vec<NamespaceMetadata>) {
        let mut host = MacroHost::new();
        let (_, _, manifest) = analyze_namespace_entry_with_dependencies(
            entry,
            std::slice::from_ref(self.0.path()),
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let metadata = source_metadata(&manifest, &host);
        (manifest, metadata)
    }
}

fn on(blockers: &[&str]) -> CacheSettings {
    CacheSettings {
        enabled: true,
        blockers: blockers.iter().map(|name| (*name).to_owned()).collect(),
    }
}

/// The namespaces `policy` bypasses, in load order, with the description.
fn bypassed(manifest: &DependencyManifest, policy: &CachePolicy) -> Vec<(String, String)> {
    manifest
        .namespaces
        .iter()
        .filter_map(|record| {
            let id = manifest.id(&record.name).unwrap();
            policy
                .describe(manifest, id)
                .map(|reason| (record.name.clone(), reason))
        })
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(name, reason)| ((*name).to_owned(), (*reason).to_owned()))
        .collect()
}

/// The counter example of "Cache contract".
fn counter() -> Sources {
    Sources::new(&[
        (
            "macros.cljs",
            "(ns macros) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))",
        ),
        (
            "dep.cljs",
            "(ns dep (:require [macros :refer [next-value]])) (def value (next-value))",
        ),
        (
            "main.cljs",
            "(ns main (:require [dep :as d] [macros :refer [next-value]])) \
             (js/console.log d/value (next-value))",
        ),
    ])
}

#[test]
fn the_counter_example_with_macros_as_a_blocker_bypasses_dep_and_main() {
    let (manifest, metadata) = counter().compile("main");
    let policy = evaluate(&manifest, &on(&[]), &metadata, &[]);
    assert!(bypassed(&manifest, &policy).is_empty());

    let policy = evaluate(&manifest, &on(&["macros"]), &metadata, &[]);
    assert_eq!(
        bypassed(&manifest, &policy),
        pairs(&[
            ("macros", "cache blocker through macros"),
            ("dep", "cache blocker through macros -> dep"),
            ("main", "cache blocker through macros -> main"),
        ])
    );
    // A blocker is a disk bypass: watch has nothing to recompile for it.
    assert!(policy.always_compile().is_empty());

    // Blocking `dep` leaves its provider alone.
    let policy = evaluate(&manifest, &on(&["dep"]), &metadata, &[]);
    assert_eq!(
        bypassed(&manifest, &policy),
        pairs(&[
            ("dep", "cache blocker through dep"),
            ("main", "cache blocker through dep -> main"),
        ])
    );
}

#[test]
fn ns_metadata_marks_no_cache_and_always_from_the_current_sources() {
    let sources = Sources::new(&[
        (
            "h.cljs",
            "(ns ^:light-cljs/no-cache h) (defn double [x] (* 2 x))",
        ),
        (
            "m.cljs",
            "(ns m {:dev/always true} (:require [h])) (defmacro twice [x] (h/double x))",
        ),
        ("r.cljs", "(ns ^{:dev/always false} r) (def base 1)"),
        (
            "app.cljs",
            "(ns app (:require [m :refer [twice]] [r])) (def value (+ r/base (twice 20)))",
        ),
    ]);
    let (manifest, metadata) = sources.compile("app");
    let flags = |name| metadata[manifest.id(name).unwrap().0 as usize];
    assert_eq!(
        flags("h"),
        NamespaceMetadata {
            no_cache: true,
            always: false
        }
    );
    assert_eq!(
        flags("m"),
        NamespaceMetadata {
            no_cache: false,
            always: true
        }
    );
    assert_eq!(flags("r"), NamespaceMetadata::default());
    assert_eq!(flags("app"), NamespaceMetadata::default());

    let policy = evaluate(&manifest, &on(&[]), &metadata, &[]);
    assert_eq!(
        bypassed(&manifest, &policy),
        pairs(&[
            ("h", "^:light-cljs/no-cache"),
            ("m", "^:dev/always through m"),
            ("app", "^:dev/always through m -> app"),
        ])
    );
    let always: Vec<_> = policy
        .always_compile()
        .iter()
        .map(|&id| manifest.record(id).name.as_str())
        .collect();
    assert_eq!(always, ["m", "app"]);

    // Editing the annotations away is reflected by the next build's policy
    // with no artifact consulted.
    sources.write("h.cljs", "(ns h) (defn double [x] (* 2 x))");
    sources.write(
        "m.cljs",
        "(ns m (:require [h])) (defmacro twice [x] (h/double x))",
    );
    let (manifest, metadata) = sources.compile("app");
    let policy = evaluate(&manifest, &on(&[]), &metadata, &[]);
    assert!(bypassed(&manifest, &policy).is_empty());
    assert!(policy.always_compile().is_empty());
    // A blocker on the helper reaches `app` through its helper edge.
    let policy = evaluate(&manifest, &on(&["h"]), &metadata, &[]);
    let app = manifest.id("app").unwrap();
    assert_eq!(policy.reason(app), Some(Bypass::Blocked));
    assert!(policy.may_use_cache(manifest.id("r").unwrap()));
}

#[test]
fn blockers_and_always_reach_consumers_through_observed_and_coarse_edges_only() {
    // `c` names `x/value` without requiring `x`: an observed edge only.
    let sources = Sources::new(&[
        ("x.cljs", "(ns x) (def value 7)"),
        ("c.cljs", "(ns c) (def doubled (* 2 x/value))"),
        (
            "main.cljs",
            "(ns main (:require [x] [c])) (js/console.log c/doubled)",
        ),
    ]);
    let (manifest, metadata) = sources.compile("main");
    let (c, x) = (manifest.id("c").unwrap(), manifest.id("x").unwrap());
    assert_eq!(manifest.edge(c, x), Some(EdgeKinds::OBSERVED));
    let policy = evaluate(&manifest, &on(&["x"]), &metadata, &[]);
    assert_eq!(policy.reason(c), Some(Bypass::Blocked));
    assert_eq!(
        policy.describe(&manifest, c).unwrap(),
        "cache blocker through x -> c"
    );

    // `c` ran a source macro, so it depends on `x`, which it neither
    // requires nor names, through its coarse edge alone.
    let sources = Sources::new(&[
        ("x.cljs", "(ns ^:dev/always x) (def plain 1)"),
        ("m.cljs", "(ns m) (defmacro one [] 1)"),
        ("c.cljs", "(ns c (:require [m :refer [one]])) (def v (one))"),
        ("d.cljs", "(ns d) (def v 2)"),
        (
            "main.cljs",
            "(ns main (:require [x] [c] [d])) (js/console.log c/v d/v)",
        ),
    ]);
    let (manifest, metadata) = sources.compile("main");
    let (c, x) = (manifest.id("c").unwrap(), manifest.id("x").unwrap());
    assert_eq!(manifest.edge(c, x), Some(EdgeKinds::COARSE));
    let policy = evaluate(&manifest, &on(&[]), &metadata, &[]);
    assert_eq!(
        bypassed(&manifest, &policy),
        pairs(&[
            ("x", "^:dev/always through x"),
            ("c", "^:dev/always through x -> c"),
            ("main", "^:dev/always through x -> main"),
        ])
    );
    let blocked = evaluate(&manifest, &on(&["x"]), &[], &[]);
    assert_eq!(blocked.reason(c), Some(Bypass::Blocked));
    assert!(blocked.may_use_cache(manifest.id("d").unwrap()));
}
