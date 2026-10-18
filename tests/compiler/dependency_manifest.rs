//! The dependency manifest of a namespace compilation: edges by kind, their
//! reverse closure, source resolution records and observations outside the
//! require closure (docs/compiler-cache.md, "Conservative keys and
//! invalidation").
use crate::temp_dir::TempDir;
use light_cljs::compiler::analyze_namespace_entry_with_dependencies;
use light_cljs::dependency_manifest::{
    Candidate, DependencyManifest, EdgeKinds, NamespaceId, RequireForm, RequireTarget, SourceKind,
    SourceOrigin,
};
use light_cljs::{MacroHost, analyzer::AnalysisOptions};
use std::{fs, path::PathBuf, process::Command};

struct Roots(Vec<TempDir>);

impl Roots {
    fn new(count: usize) -> Self {
        Self((0..count).map(|_| TempDir::new("cljs-manifest")).collect())
    }
    fn write(&self, root: usize, path: &str, source: &str) {
        let path = self.0[root].join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    /// The canonical path of `path` under `root`, as candidates name it.
    fn path(&self, root: usize, path: &str) -> PathBuf {
        fs::canonicalize(self.0[root].path()).unwrap().join(path)
    }
    fn paths(&self) -> Vec<PathBuf> {
        self.0.iter().map(|root| root.path().clone()).collect()
    }
    fn manifest(&self, entry: &str) -> (String, DependencyManifest) {
        let (js, manifest, _) = self.manifest_with(entry, AnalysisOptions::default());
        (js, manifest)
    }
    /// The output, manifest and warning kinds of `entry` under `options`.
    fn manifest_with(
        &self,
        entry: &str,
        options: AnalysisOptions,
    ) -> (String, DependencyManifest, Vec<&'static str>) {
        let mut warnings = Vec::new();
        let (program, _, manifest) = analyze_namespace_entry_with_dependencies(
            entry,
            &self.paths(),
            &mut MacroHost::new(),
            options,
            &mut warnings,
        )
        .unwrap();
        // Recording a manifest leaves the program as it is without one.
        let plain = light_cljs::compiler::analyze_namespace_entry(
            entry,
            &self.paths(),
            &mut MacroHost::new(),
            options,
            &mut Vec::new(),
        )
        .unwrap();
        let js = light_cljs::emit_js(&program);
        assert_eq!(js, light_cljs::emit_js(&plain));
        (js, manifest, warnings.iter().map(|w| w.kind).collect())
    }
}

fn id(manifest: &DependencyManifest, name: &str) -> NamespaceId {
    manifest
        .id(name)
        .unwrap_or_else(|| panic!("{name} is not in the manifest"))
}

/// The kinds of an edge apart from [`EdgeKinds::COARSE`], which every
/// namespace that ran a source macro has to every earlier one.
fn edge(manifest: &DependencyManifest, from: &str, to: &str) -> Option<EdgeKinds> {
    manifest
        .edge(id(manifest, from), id(manifest, to))
        .map(|kinds| EdgeKinds(kinds.0 & !EdgeKinds::COARSE.0))
        .filter(|kinds| kinds.0 != 0)
}

fn coarse_edge(manifest: &DependencyManifest, from: &str, to: &str) -> bool {
    manifest
        .edge(id(manifest, from), id(manifest, to))
        .is_some_and(|kinds| kinds.contains(EdgeKinds::COARSE))
}

fn names(manifest: &DependencyManifest, ids: &[NamespaceId]) -> Vec<String> {
    ids.iter()
        .map(|&id| manifest.record(id).name.clone())
        .collect()
}

fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", js])
        .output()
        .expect("compiler integration requires Node");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// `app` uses a macro of `m`, whose expansion calls `h` at compile time;
/// `app` requires `m` and the plain `r`, never `h`.
fn macro_fixture() -> Roots {
    let roots = Roots::new(1);
    roots.write(0, "h.cljs", "(ns h) (defn double [x] (* 2 x))");
    roots.write(
        0,
        "m.cljs",
        "(ns m (:require [h])) (def before 0) (defmacro twice [x] (h/double x))",
    );
    roots.write(0, "r.cljs", "(ns r) (def base 1)");
    roots.write(
        0,
        "app.cljs",
        "(ns app (:require [m :refer [twice]] [r])) (def value (+ r/base (twice 20)))",
    );
    roots.write(
        0,
        "main.cljs",
        "(ns main (:require [app])) (js/console.log app/value)",
    );
    roots
}

#[test]
fn edges_carry_runtime_macro_provider_and_helper_kinds() {
    let roots = macro_fixture();
    let (js, manifest) = roots.manifest("main");
    assert_eq!(execute(&js), "41\n");
    assert_eq!(
        edge(&manifest, "app", "m"),
        Some(EdgeKinds::RUNTIME | EdgeKinds::MACRO_PROVIDER)
    );
    assert_eq!(edge(&manifest, "app", "r"), Some(EdgeKinds::RUNTIME));
    // `h` is reached only through the macro: a helper, not a require.
    assert_eq!(edge(&manifest, "app", "h"), Some(EdgeKinds::HELPER));
    assert_eq!(edge(&manifest, "m", "h"), Some(EdgeKinds::RUNTIME));
    assert_eq!(edge(&manifest, "main", "app"), Some(EdgeKinds::RUNTIME));
    assert_eq!(edge(&manifest, "main", "m"), None);
    assert_eq!(
        names(&manifest, &manifest.closure(id(&manifest, "main"))),
        ["h", "m", "r", "app"]
    );
    // Every edge has its reverse.
    for record in 0..manifest.namespaces.len() {
        let from = NamespaceId(record as u32);
        for (to, kinds) in manifest.dependencies(from) {
            assert!(manifest.dependents(to).any(|edge| edge == (from, kinds)));
        }
    }
    // Only `app` ran a source macro.
    let coarse: Vec<_> = manifest
        .namespaces
        .iter()
        .filter(|record| record.coarse)
        .map(|record| record.name.as_str())
        .collect();
    assert_eq!(coarse, ["app"]);
    assert!(coarse_edge(&manifest, "app", "h") && !coarse_edge(&manifest, "main", "h"));
}

#[test]
fn reverse_closure_of_a_macro_namespace_reaches_every_consumer() {
    let roots = macro_fixture();
    let (_, manifest) = roots.manifest("main");
    assert_eq!(
        names(&manifest, &manifest.reverse_closure(id(&manifest, "m"))),
        ["app", "main"]
    );
    // A change to the helper invalidates the macro's consumers too.
    assert_eq!(
        names(&manifest, &manifest.reverse_closure(id(&manifest, "h"))),
        ["m", "app", "main"]
    );
    assert_eq!(
        names(&manifest, &manifest.reverse_closure(id(&manifest, "r"))),
        ["app", "main"]
    );
}

#[test]
fn cljs_takes_precedence_over_cljc_and_only_the_cljs_candidates_are_tried() {
    let roots = Roots::new(1);
    roots.write(0, "x/y.cljs", "(ns x.y) (def v 1)");
    roots.write(0, "x/y.cljc", "(ns x.y) (def v 2)");
    roots.write(0, "z/w.cljc", "(ns z.w) (def v #?(:cljs 3 :clj 4))");
    roots.write(0, "app.cljs", "(ns app (:require [x.y] [z.w]))");
    let (_, manifest) = roots.manifest("app");
    let both = manifest.record(id(&manifest, "x.y"));
    assert_eq!(both.source.kind, SourceKind::Cljs);
    assert_eq!(both.source.origin, SourceOrigin::Provider);
    assert_eq!(
        manifest.lookup(both.source.lookup).candidates,
        [Candidate {
            path: roots.path(0, "x/y.cljs"),
            present: true
        }]
    );
    let cljc = manifest.record(id(&manifest, "z.w"));
    assert_eq!(cljc.source.kind, SourceKind::Cljc);
    assert_eq!(
        manifest.lookup(cljc.source.lookup).candidates,
        [
            Candidate {
                path: roots.path(0, "z/w.cljs"),
                present: false
            },
            Candidate {
                path: roots.path(0, "z/w.cljc"),
                present: true
            },
        ]
    );
    assert_eq!(
        cljc.source.file,
        roots.path(0, "z/w.cljc").to_string_lossy()
    );
}

#[test]
fn an_absent_candidate_that_appears_shadows_and_changes_the_record() {
    let roots = Roots::new(2);
    roots.write(1, "x/y.cljc", "(ns x.y) (def v 1)");
    roots.write(0, "app.cljs", "(ns app (:require [x.y]))");
    let (_, before) = roots.manifest("app");
    let record = before.record(id(&before, "x.y"));
    let lookup = before.lookup(record.source.lookup);
    assert_eq!(lookup.namespace, "x.y");
    assert_eq!(
        lookup.candidates,
        [
            Candidate {
                path: roots.path(0, "x/y.cljs"),
                present: false
            },
            Candidate {
                path: roots.path(1, "x/y.cljs"),
                present: false
            },
            Candidate {
                path: roots.path(0, "x/y.cljc"),
                present: false
            },
            Candidate {
                path: roots.path(1, "x/y.cljc"),
                present: true
            },
        ]
    );
    // The file the record listed as absent appears and wins.
    roots.write(0, "x/y.cljs", "(ns x.y) (def v 2)");
    let (_, after) = roots.manifest("app");
    let shadowing = after.record(id(&after, "x.y"));
    assert_eq!(shadowing.source.kind, SourceKind::Cljs);
    assert_eq!(
        shadowing.source.file,
        roots.path(0, "x/y.cljs").to_string_lossy()
    );
    assert_eq!(
        after.lookup(shadowing.source.lookup).candidates,
        [
            Candidate {
                path: roots.path(0, "x/y.cljs"),
                present: true
            },
            Candidate {
                path: roots.path(1, "x/y.cljs"),
                present: false
            },
        ]
    );
    assert_ne!(before.lookups, after.lookups);
}

#[test]
fn bundled_fallback_records_the_absent_lookup_a_source_would_shadow() {
    let roots = Roots::new(1);
    roots.write(
        0,
        "app.cljs",
        "(ns app (:require [clojure.string :as s])) (js/console.log (s/upper-case \"a\"))",
    );
    let (_, manifest) = roots.manifest("app");
    let bundled = manifest.record(id(&manifest, "clojure.string"));
    assert_eq!(bundled.source.origin, SourceOrigin::Bundled);
    assert_eq!(bundled.source.file, "<first-party>/clojure/string.cljs");
    let lookup = manifest.lookup(bundled.source.lookup);
    assert_eq!(lookup.found, None);
    assert_eq!(
        lookup.candidates,
        [
            Candidate {
                path: roots.path(0, "clojure/string.cljs"),
                present: false
            },
            Candidate {
                path: roots.path(0, "clojure/string.cljc"),
                present: false
            },
        ]
    );
    // Before falling back, the loader looked for the `cljs.string` rewrite.
    let require = &manifest.record(id(&manifest, "app")).requires[0];
    assert_eq!(require.written, "clojure.string");
    assert_eq!(require.form, RequireForm::Declared);
    let checked: Vec<_> = require
        .lookups
        .iter()
        .map(|&lookup| manifest.lookup(lookup).namespace.as_str())
        .collect();
    assert_eq!(checked, ["clojure.string", "cljs.string"]);
    // A source on a root replaces the bundled library.
    roots.write(
        0,
        "clojure/string.cljs",
        "(ns clojure.string) (defn upper-case [s] s)",
    );
    let (_, shadowed) = roots.manifest("app");
    let provided = shadowed.record(id(&shadowed, "clojure.string"));
    assert_eq!(provided.source.origin, SourceOrigin::Provider);
    assert!(shadowed.lookup(provided.source.lookup).candidates[0].present);
}

#[test]
fn rewrites_host_namespaces_and_core_are_recorded_as_resolved() {
    let roots = Roots::new(1);
    roots.write(0, "cljs/thing.cljs", "(ns cljs.thing) (def v 1)");
    roots.write(
        0,
        "app.cljs",
        "(ns app (:require [clojure.thing :as t] [goog.object :as gobj] [cljs.core :as c]))
         (js/console.log t/v (gobj/get #js {:a 1} \"a\"))",
    );
    let (_, manifest) = roots.manifest("app");
    let app = manifest.record(id(&manifest, "app"));
    let thing = app
        .requires
        .iter()
        .find(|r| r.written == "clojure.thing")
        .unwrap();
    assert_eq!(thing.form, RequireForm::ClojureRewrite);
    assert_eq!(
        thing.target,
        RequireTarget::Namespace(id(&manifest, "cljs.thing"))
    );
    let goog = app
        .requires
        .iter()
        .find(|r| r.written == "goog.object")
        .unwrap();
    assert_eq!(goog.target, RequireTarget::Host);
    assert_eq!(manifest.lookup(goog.lookups[0]).found, None);
    assert!(
        app.requires
            .iter()
            .any(|r| r.written == "cljs.core" && r.target == RequireTarget::Core)
    );
    assert_eq!(
        edge(&manifest, "app", "cljs.thing"),
        Some(EdgeKinds::RUNTIME)
    );
}

#[test]
fn a_qualified_name_in_a_loaded_unrequired_namespace_is_an_observed_dependency() {
    let roots = Roots::new(1);
    roots.write(0, "x.cljs", "(ns x) (def value 7)");
    // `c` names `x/value` without requiring `x`, which `main` loaded first.
    roots.write(0, "c.cljs", "(ns c) (def doubled (* 2 x/value))");
    roots.write(
        0,
        "main.cljs",
        "(ns main (:require [x] [c])) (js/console.log c/doubled)",
    );
    let (js, manifest) = roots.manifest("main");
    assert_eq!(execute(&js), "14\n");
    assert_eq!(edge(&manifest, "c", "x"), Some(EdgeKinds::OBSERVED));
    let requires = EdgeKinds::RUNTIME | EdgeKinds::MACRO_PROVIDER | EdgeKinds::HELPER;
    assert!(
        manifest
            .closure_through(id(&manifest, "c"), requires)
            .is_empty()
    );
    assert_eq!(
        names(&manifest, &manifest.reverse_closure(id(&manifest, "x"))),
        ["c", "main"]
    );
}

#[test]
fn a_dotted_symbol_records_the_namespace_it_did_not_find() {
    let roots = Roots::new(1);
    roots.write(0, "app.cljs", "(ns app) (js/console.log (some? Math.PI))");
    let (_, manifest) = roots.manifest("app");
    // Were a namespace `Math` loaded first, `Math.PI` would name its Var.
    assert_eq!(manifest.record(id(&manifest, "app")).absent, ["Math"]);
}

#[test]
fn a_warning_mode_unknown_qualifier_is_absent_until_its_namespace_loads_first() {
    let warning_mode = AnalysisOptions {
        warning_mode: true,
        ..Default::default()
    };
    let roots = Roots::new(1);
    roots.write(0, "app.cljs", "(ns app) (def v missing.ns/value)");
    roots.write(0, "main.cljs", "(ns main (:require [app]))");
    let (_, before, warnings) = roots.manifest_with("main", warning_mode);
    assert!(warnings.contains(&"undeclared-ns"), "{warnings:?}");
    assert_eq!(before.record(id(&before, "app")).absent, ["missing.ns"]);
    // The namespace the record names as absent now loads before `app`.
    roots.write(0, "missing/ns.cljs", "(ns missing.ns) (def value 3)");
    roots.write(0, "main.cljs", "(ns main (:require [missing.ns] [app]))");
    let (_, after, warnings) = roots.manifest_with("main", warning_mode);
    assert!(!warnings.contains(&"undeclared-ns"), "{warnings:?}");
    assert!(after.record(id(&after, "app")).absent.is_empty());
    assert_eq!(edge(&after, "app", "missing.ns"), Some(EdgeKinds::OBSERVED));
}

#[test]
fn source_macros_see_extensions_from_namespaces_outside_the_require_closure() {
    let roots = Roots::new(1);
    roots.write(0, "p.cljs", "(ns p) (defprotocol Shout (shout [x]))");
    // `x` extends the protocol; nothing `c` requires reaches `x`.
    roots.write(
        0,
        "x.cljs",
        "(ns x (:require [p])) (extend-protocol p/Shout string (shout [s] (str s \"!\")))",
    );
    roots.write(
        0,
        "m.cljs",
        "(ns m (:require [p])) (defmacro loud [s] (p/shout s))",
    );
    roots.write(
        0,
        "c.cljs",
        "(ns c (:require [m :refer [loud]])) (def greeting (loud \"hi\"))",
    );
    roots.write(
        0,
        "main.cljs",
        "(ns main (:require [x] [c])) (js/console.log c/greeting)",
    );
    let (js, manifest) = roots.manifest("main");
    // The expansion used `x`'s extension.
    assert_eq!(execute(&js), "hi!\n");
    let c = id(&manifest, "c");
    let requires = EdgeKinds::RUNTIME | EdgeKinds::MACRO_PROVIDER | EdgeKinds::HELPER;
    assert!(
        !manifest
            .closure_through(c, requires)
            .contains(&id(&manifest, "x"))
    );
    assert!(manifest.record(c).coarse);
    assert!(coarse_edge(&manifest, "c", "x"));
    assert!(manifest.reverse_closure(id(&manifest, "x")).contains(&c));
    // `m` runs no source macro, so it depends only on what it requires.
    assert!(!manifest.record(id(&manifest, "m")).coarse);
    assert_eq!(manifest.edge(id(&manifest, "m"), id(&manifest, "x")), None);
}

#[test]
fn a_source_macro_consumer_depends_on_earlier_namespaces_without_declarations() {
    // `x` declares nothing a macro could see yet: giving it a first extension
    // or a macro would change `c`'s expansion through the declaration view
    // or `&env`, so `c` already depends on it.
    let roots = Roots::new(1);
    roots.write(0, "p.cljs", "(ns p) (defprotocol Marker (mark [x]))");
    roots.write(0, "x.cljs", "(ns x (:require [p])) (def plain 1)");
    roots.write(
        0,
        "m.cljs",
        "(ns m (:require [p])) (defmacro marked? [] (satisfies? p/Marker \"s\"))",
    );
    roots.write(
        0,
        "c.cljs",
        "(ns c (:require [m :refer [marked?]])) (def v (marked?))",
    );
    roots.write(0, "d.cljs", "(ns d) (def v 2)");
    roots.write(
        0,
        "main.cljs",
        "(ns main (:require [x] [c] [d])) (js/console.log c/v d/v)",
    );
    let (js, manifest) = roots.manifest("main");
    assert_eq!(execute(&js), "false 2\n");
    let c = id(&manifest, "c");
    assert!(manifest.record(c).coarse);
    assert_eq!(names(&manifest, &manifest.closure(c)), ["p", "x", "m"]);
    assert_eq!(
        names(&manifest, &manifest.reverse_closure(id(&manifest, "x"))),
        ["c", "main"]
    );
    // Namespaces that run no source macro are not coarse.
    for name in ["p", "x", "m", "d", "main"] {
        assert!(!manifest.record(id(&manifest, name)).coarse, "{name}");
    }
    assert!(!coarse_edge(&manifest, "d", "x"));
}

#[test]
fn a_fully_qualified_macro_of_a_loaded_namespace_is_a_macro_provider() {
    let roots = macro_fixture();
    // `q` expands `m/twice` without requiring `m`, which `main2` loaded first.
    roots.write(0, "q.cljs", "(ns q) (def value (m/twice 4))");
    roots.write(
        0,
        "main2.cljs",
        "(ns main2 (:require [m] [q])) (js/console.log q/value)",
    );
    let (js, manifest) = roots.manifest("main2");
    assert_eq!(execute(&js), "8\n");
    assert_eq!(
        edge(&manifest, "q", "m"),
        Some(EdgeKinds::OBSERVED | EdgeKinds::MACRO_PROVIDER)
    );
    // The provider's helper is reached through the observed macro.
    assert_eq!(edge(&manifest, "q", "h"), Some(EdgeKinds::HELPER));
    assert_eq!(
        names(&manifest, &manifest.reverse_closure(id(&manifest, "m"))),
        ["q", "main2"]
    );
}

#[test]
fn namespace_reflection_records_loaded_and_absent_namespaces() {
    let roots = Roots::new(1);
    // `empty` has no Vars, so nothing else names it.
    roots.write(0, "empty.cljs", "(ns empty)");
    roots.write(0, "later.cljs", "(ns later) (def v 1)");
    roots.write(
        0,
        "c.cljs",
        "(ns c) (def counts [(count (ns-publics 'empty)) (count (ns-publics 'later))])",
    );
    roots.write(
        0,
        "main.cljs",
        "(ns main (:require [empty] [c] [later])) (js/console.log (pr-str c/counts))",
    );
    let (js, manifest) = roots.manifest("main");
    assert_eq!(execute(&js), "[0 0]\n");
    assert_eq!(edge(&manifest, "c", "empty"), Some(EdgeKinds::OBSERVED));
    // `later` loads after `c`: loading it earlier would change the answer.
    assert_eq!(manifest.record(id(&manifest, "c")).absent, ["later"]);
    assert_eq!(edge(&manifest, "c", "later"), None);
    assert!(!manifest.record(id(&manifest, "c")).coarse);
}

#[test]
fn implicit_lazy_require_and_forward_lazy_target_stay_out_of_compilation_closures() {
    let roots = Roots::new(1);
    roots.write(
        0,
        "app.cljs",
        "(ns app) (def editor (light.lazy/loadable page/start)) (defn base [] 1) (fn [] editor)",
    );
    // The lazy target requires the namespace that loads it.
    roots.write(
        0,
        "page.cljs",
        "(ns page (:require [app])) (defn start [] (app/base))",
    );
    let (_, manifest) = roots.manifest("app");
    let app = id(&manifest, "app");
    let page = id(&manifest, "page");
    let lazy = manifest
        .record(app)
        .requires
        .iter()
        .find(|r| r.form == RequireForm::ImplicitLazy)
        .expect("implicit light.lazy require");
    assert_eq!(lazy.written, "light.lazy");
    assert_eq!(
        lazy.target,
        RequireTarget::Namespace(id(&manifest, "light.lazy"))
    );
    assert_eq!(
        edge(&manifest, "app", "light.lazy"),
        Some(EdgeKinds::RUNTIME)
    );
    assert_eq!(
        manifest.record(id(&manifest, "light.lazy")).source.origin,
        SourceOrigin::Bundled
    );
    assert!(page > app);
    assert_eq!(manifest.edge(app, page), Some(EdgeKinds::LAZY));
    assert_eq!(edge(&manifest, "page", "app"), Some(EdgeKinds::RUNTIME));
    assert!(!manifest.closure(app).contains(&page));
    assert!(manifest.closure(page).contains(&app));
    assert!(!manifest.reverse_closure(page).contains(&app));
    assert_eq!(
        manifest.lookup(manifest.record(page).source.lookup).found,
        Some(roots.path(0, "page.cljs").to_string_lossy().into_owned())
    );
}
