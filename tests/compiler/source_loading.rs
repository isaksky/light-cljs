//! Loading sources through the parse queue (docs/compiler-concurrency.md,
//! "Source discovery and readiness"): the source order, entry value,
//! diagnostics and dependency manifest of a pooled load, at one slot and at
//! four, equal those of the depth-first walk that reads each source where
//! it reaches it, which is how a provider without a concurrent variant is
//! still called, on the calling thread.
use crate::temp_dir::TempDir;
use light_cljs::analyzer::{AnalysisOptions, AnalysisWarning};
use light_cljs::compiler::{
    EntryRecorders, analyze_namespace_entry_recorded,
    analyze_namespace_entry_recorded_with_provider,
};
use light_cljs::dependency_manifest::{Candidate, DependencyManifest};
use light_cljs::scheduler::Slots;
use light_cljs::source_provider::{
    FileSourceProvider, LoadError, LoadRequest, ResourceProvider, SourceInput, SourceProvider,
};
use light_cljs::{CompileStats, MacroHost, NamespaceCompileError, emit_js};
use std::{cell::RefCell, fs, path::PathBuf, rc::Rc, sync::Mutex};

struct Roots(Vec<TempDir>);

impl Roots {
    fn new(count: usize) -> Self {
        Self((0..count).map(|_| TempDir::new("cljs-loading")).collect())
    }
    fn write(&self, root: usize, path: &str, source: &str) -> &Self {
        let path = self.0[root].join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        self
    }
    fn paths(&self) -> Vec<PathBuf> {
        self.0.iter().map(|root| root.path().clone()).collect()
    }
    fn canonical(&self) -> Vec<PathBuf> {
        self.0
            .iter()
            .map(|root| fs::canonicalize(root.path()).unwrap())
            .collect()
    }
}

/// Source roots without the concurrent variant: the loader calls them where
/// its depth-first walk reaches each namespace, as before the parse queue.
struct Serial(FileSourceProvider);

impl SourceProvider for Serial {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        self.0.load(request)
    }
    fn load_traced(
        &self,
        request: &LoadRequest,
        tried: &mut Vec<Candidate>,
    ) -> Result<Option<SourceInput>, LoadError> {
        self.0.load_traced(request, tried)
    }
    fn resources(&self) -> Option<Rc<dyn ResourceProvider>> {
        self.0.resources()
    }
}

/// What a compilation made: its JavaScript, its source files in source
/// order, its manifest and warnings; or its error with the warnings before it.
type Made = Result<
    (
        String,
        Vec<PathBuf>,
        DependencyManifest,
        Vec<AnalysisWarning>,
    ),
    (NamespaceCompileError, Vec<AnalysisWarning>),
>;

fn compile(roots: &Roots, entry: &str, slots: Option<usize>) -> Made {
    let mut warnings = Vec::new();
    let mut manifest = DependencyManifest::default();
    let options = AnalysisOptions {
        warning_mode: true,
        ..Default::default()
    };
    let recorders = EntryRecorders {
        manifest: Some(&mut manifest),
        slots: Slots::new(slots.unwrap_or(1)).unwrap(),
        ..Default::default()
    };
    let mut macros = MacroHost::new();
    let analyzed = match slots {
        Some(_) => analyze_namespace_entry_recorded(
            entry,
            &roots.paths(),
            &mut macros,
            options,
            &mut warnings,
            recorders,
        ),
        None => analyze_namespace_entry_recorded_with_provider(
            entry,
            &Serial(FileSourceProvider::new(roots.canonical())),
            &mut macros,
            options,
            &mut warnings,
            recorders,
        ),
    };
    match analyzed {
        Ok((program, files)) => Ok((
            emit_js(&program),
            files.into_iter().map(|(path, _)| path).collect(),
            manifest,
            warnings,
        )),
        Err(error) => Err((error, warnings)),
    }
}

/// The compilation of `entry` by the depth-first walk, checked to equal the
/// pooled one at one slot and at four.
fn loaded(roots: &Roots, entry: &str) -> Made {
    let serial = compile(roots, entry, None);
    for slots in [1, 4] {
        // Several rounds, so that completion order varies.
        for _ in 0..4 {
            assert_eq!(compile(roots, entry, Some(slots)), serial, "{slots} slots");
        }
    }
    serial
}

/// The source files of a compilation, by name under its roots.
fn order(made: &Made) -> Vec<String> {
    let (_, files, _, _) = made.as_ref().expect("compiles");
    files
        .iter()
        .map(|path| {
            let path = path.to_string_lossy();
            let at = path.find("/app/").or_else(|| path.find("<first-party>"));
            path[at.unwrap_or(0)..].to_owned()
        })
        .collect()
}

fn error(made: &Made) -> &NamespaceCompileError {
    &made.as_ref().expect_err("fails").0
}

#[test]
fn a_diamond_loads_its_shared_dependency_once_in_depth_first_order() {
    let roots = Roots::new(1);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main (:require [app.left :as l] [app.right :as r]))\n[(l/f) (r/f)]\n",
        )
        .write(
            0,
            "app/left.cljs",
            "(ns app.left (:require [app.base :as b]))\n(defn f [] (b/g :left))\n",
        )
        .write(
            0,
            "app/right.cljs",
            "(ns app.right (:require [app.base :as b] [app.tail]))\n(defn f [] (b/g :right))\n",
        )
        .write(
            0,
            "app/base.cljs",
            "(ns app.base (:require [app.leaf :as leaf]))\n(defn g [k] [k leaf/x])\n",
        )
        .write(0, "app/leaf.cljs", "(ns app.leaf)\n(def x 1)\n")
        .write(0, "app/tail.cljs", "(ns app.tail)\n(def y 2)\n");
    let made = loaded(&roots, "app.main");
    assert_eq!(
        order(&made),
        [
            "/app/leaf.cljs",
            "/app/base.cljs",
            "/app/left.cljs",
            "/app/tail.cljs",
            "/app/right.cljs",
            "/app/main.cljs"
        ]
    );
    // `--stats` times each source's job, in source order, at four slots.
    let mut stats = CompileStats::default();
    light_cljs::compiler::analyze_namespace_entry_recorded(
        "app.main",
        &roots.paths(),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            stats: Some(&mut stats),
            slots: Slots::new(4).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    let trace = stats.trace.expect("a trace");
    assert_eq!(trace.namespaces.len(), 6);
    assert!(trace.namespaces.iter().all(|ns| !ns.load.is_zero()));
    assert!(
        !trace
            .stage(light_cljs::compile_trace::Stage::Load)
            .time
            .is_zero()
    );
}

#[test]
fn a_static_cycle_reports_its_path_from_the_walk_whatever_finished_first() {
    let roots = Roots::new(1);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main (:require [app.ok] [app.a] [app.missing]))\n1\n",
        )
        .write(0, "app/ok.cljs", "(ns app.ok)\n")
        .write(0, "app/a.cljs", "(ns app.a (:require [app.b]))\n")
        .write(0, "app/b.cljs", "(ns app.b (:require [app.c]))\n")
        .write(0, "app/c.cljs", "(ns app.c (:require [app.a]))\n");
    let made = loaded(&roots, "app.main");
    let failure = error(&made);
    assert_eq!(
        failure.message,
        "namespace dependency cycle: app.a -> app.b -> app.c -> app.a"
    );
    assert!(failure.path.as_ref().unwrap().ends_with("app/c.cljs"));
    // The walk meets the missing source first when it comes first.
    roots.write(
        0,
        "app/main.cljs",
        "(ns app.main (:require [app.ok] [app.missing] [app.a]))\n1\n",
    );
    let made = loaded(&roots, "app.main");
    let failure = error(&made);
    assert_eq!(
        failure.message,
        "namespace app.missing not found: expected app/missing.cljs or .cljc in explicit source roots"
    );
    assert!(failure.path.as_ref().unwrap().ends_with("app/main.cljs"));
    assert_eq!(failure.line_column, Some((1, 34)));
}

#[test]
fn a_parse_error_deep_in_the_graph_is_reported_where_the_walk_meets_it() {
    let roots = Roots::new(1);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main (:require [app.a] [app.bad]))\n1\n",
        )
        .write(0, "app/a.cljs", "(ns app.a (:require [app.worse]))\n")
        .write(0, "app/worse.cljs", "(ns app.worse)\n(def x [1 2)\n")
        .write(0, "app/bad.cljs", "(ns app.bad)\n(def y \"open\n");
    let made = loaded(&roots, "app.main");
    let failure = error(&made);
    assert!(failure.path.as_ref().unwrap().ends_with("app/worse.cljs"));
    assert_eq!(failure.line_column.map(|(line, _)| line), Some(2));
    // A source that names another namespace than its file.
    roots.write(0, "app/a.cljs", "(ns app.other)\n");
    let made = loaded(&roots, "app.main");
    assert_eq!(
        error_message(&made),
        "declared namespace app.other does not match requested namespace app.a"
    );
}

fn error_message(made: &Made) -> &str {
    &error(made).message
}

#[test]
fn extension_precedence_and_ambiguity_across_roots() {
    let roots = Roots::new(2);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main (:require [app.both :as both] [app.portable :as p]))\n[both/x p/y]\n",
        )
        .write(0, "app/both.cljs", "(ns app.both)\n(def x :cljs)\n")
        .write(0, "app/both.cljc", "(ns app.both)\n(def x :cljc)\n")
        .write(
            1,
            "app/portable.cljc",
            "(ns app.portable)\n(def y #?(:clj :jvm :cljs :js))\n",
        );
    let made = loaded(&roots, "app.main");
    assert_eq!(
        order(&made),
        ["/app/both.cljs", "/app/portable.cljc", "/app/main.cljs"]
    );
    roots.write(1, "app/both.cljs", "(ns app.both)\n(def x :other)\n");
    let made = loaded(&roots, "app.main");
    assert!(
        error_message(&made).starts_with("ambiguous namespace app.both: "),
        "{made:?}"
    );
}

#[test]
fn host_first_party_and_rewritten_requires_resolve_as_before() {
    let roots = Roots::new(1);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main (:require [goog.string :as gstring] [goog.explicit :as ge] \
             [clojure.string :as str] [clojure.extra :as extra]))\n\
             [(gstring/trim \" x \") ge/v (str/upper-case \"a\") extra/z]\n",
        )
        .write(0, "goog/explicit.cljs", "(ns goog.explicit)\n(def v 1)\n")
        .write(0, "cljs/extra.cljs", "(ns cljs.extra)\n(def z 2)\n");
    let made = loaded(&roots, "app.main");
    let names = order(&made);
    assert_eq!(names.len(), 4, "{names:?}");
    assert!(names[0].ends_with("goog/explicit.cljs"), "{names:?}");
    assert_eq!(names[1], "<first-party>/clojure/string.cljs");
    assert!(names[2].ends_with("cljs/extra.cljs"), "{names:?}");
    assert_eq!(names[3], "/app/main.cljs");
}

#[test]
fn raw_directives_after_the_ns_form_are_edges() {
    let roots = Roots::new(1);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main)\n(def a 1)\n(require '[app.late :as late])\n\
             (do (require 'app.later))\n[a late/x]\n",
        )
        .write(
            0,
            "app/late.cljs",
            "(ns app.late (:require [app.shared]))\n(def x 2)\n",
        )
        .write(
            0,
            "app/later.cljs",
            "(ns app.later (:require [app.shared]))\n",
        )
        .write(0, "app/shared.cljs", "(ns app.shared)\n");
    let made = loaded(&roots, "app.main");
    assert_eq!(
        order(&made),
        [
            "/app/shared.cljs",
            "/app/late.cljs",
            "/app/later.cljs",
            "/app/main.cljs"
        ]
    );
}

#[test]
fn a_lazy_target_that_requires_its_requester_appends_its_closure() {
    let roots = Roots::new(1);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main (:require [app.util :as util]))\n\
             (def page (light.lazy/loadable app.page/start))\n\
             (fn [] (util/twice \"main\"))\n",
        )
        .write(0, "app/util.cljs", "(ns app.util)\n(defn twice [s] (str s s))\n")
        .write(
            0,
            "app/page.cljs",
            "(ns app.page (:require [app.main :as main] [app.util :as util] [app.extra :as extra]))\n\
             (defn start [] [(util/twice extra/text) main/page])\n",
        )
        .write(
            0,
            "app/extra.cljs",
            "(ns app.extra (:require [app.deep]))\n(def text \"x\")\n",
        )
        .write(0, "app/deep.cljs", "(ns app.deep)\n");
    let made = loaded(&roots, "app.main");
    assert_eq!(
        order(&made),
        [
            "/app/util.cljs",
            "<first-party>/light/lazy.cljs",
            "/app/main.cljs",
            "/app/deep.cljs",
            "/app/extra.cljs",
            "/app/page.cljs"
        ]
    );
    // A lazy target's own missing dependency is reported at its require.
    roots.write(
        0,
        "app/extra.cljs",
        "(ns app.extra (:require [app.gone]))\n",
    );
    let made = loaded(&roots, "app.main");
    assert!(
        error(&made)
            .path
            .as_ref()
            .unwrap()
            .ends_with("app/extra.cljs")
    );
    assert!(error_message(&made).starts_with("namespace app.gone not found"));
}

#[test]
fn a_thread_affine_provider_is_called_on_the_calling_thread_in_walk_order() {
    let files: &[(&str, &str)] = &[
        (
            "app.main",
            "(ns app.main (:require [app.b] [app.a] [clojure.set]))\n1\n",
        ),
        ("app.a", "(ns app.a (:require [app.c]))\n"),
        ("app.b", "(ns app.b (:require [app.c] [app.a]))\n"),
        ("app.c", "(ns app.c)\n"),
    ];
    // Not `Send`: the loader may not move it to a worker.
    let calls = Rc::new(RefCell::new(Vec::new()));
    let caller = std::thread::current().id();
    let provider = {
        let calls = calls.clone();
        move |request: &LoadRequest| {
            assert_eq!(std::thread::current().id(), caller);
            let name = String::from_utf16_lossy(request.namespace.name());
            let requester = String::from_utf16_lossy(request.requesting_namespace.name());
            calls.borrow_mut().push(format!("{name}<{requester}"));
            Ok(files
                .iter()
                .find(|(file, _)| *file == name)
                .map(|(file, source)| SourceInput::new(format!("{file}.cljs"), *source)))
        }
    };
    for slots in [1, 4] {
        calls.borrow_mut().clear();
        analyze_namespace_entry_recorded_with_provider(
            "app.main",
            &provider,
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
            EntryRecorders {
                slots: Slots::new(slots).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            *calls.borrow(),
            [
                "app.main<app.main",
                "clojure.set<app.main",
                "cljs.set<app.main",
                "app.b<app.main",
                "app.c<app.b",
                "app.a<app.b",
                "clojure.set<app.main",
            ]
        );
    }
}

/// Source roots that opt into concurrent loading, recording the thread of
/// each request.
struct Counted {
    roots: FileSourceProvider,
    threads: Mutex<Vec<(String, Option<String>)>>,
}

impl SourceProvider for Counted {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        let name = String::from_utf16_lossy(request.namespace.name());
        let thread = std::thread::current().name().map(str::to_owned);
        self.threads.lock().unwrap().push((name, thread));
        self.roots.load(request)
    }
    fn concurrent(&self) -> Option<&(dyn SourceProvider + Send + Sync)> {
        Some(self)
    }
}

#[test]
fn a_concurrent_provider_is_asked_once_per_namespace_on_the_workers() {
    let roots = Roots::new(1);
    roots.write(
        0,
        "app/main.cljs",
        "(ns app.main (:require [app.n0] [app.n1] [app.n2] [app.n3] [app.n4] [app.n5]))\n",
    );
    for index in 0..6 {
        roots.write(
            0,
            &format!("app/n{index}.cljs"),
            &format!("(ns app.n{index} (:require [app.shared]))\n"),
        );
    }
    roots.write(0, "app/shared.cljs", "(ns app.shared)\n");
    for slots in [1, 4] {
        let provider = Counted {
            roots: FileSourceProvider::new(roots.canonical()),
            threads: Mutex::new(Vec::new()),
        };
        let (_, files) = analyze_namespace_entry_recorded_with_provider(
            "app.main",
            &provider,
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
            EntryRecorders {
                slots: Slots::new(slots).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(files.len(), 8);
        assert!(files[0].0.ends_with("app/shared.cljs"));
        let threads = provider.threads.into_inner().unwrap();
        let mut names: Vec<_> = threads.iter().map(|(name, _)| name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), threads.len(), "each asked once: {threads:?}");
        let caller = std::thread::current().name().map(str::to_owned);
        for (name, thread) in &threads {
            // QuickJS-only builds keep every parse on the calling thread.
            if slots == 1 || !cfg!(feature = "v8-macros") {
                assert_eq!(thread, &caller, "{name}");
            } else {
                let thread = thread.as_deref().unwrap_or_default();
                assert!(thread.starts_with("light-cljs-worker-"), "{name}: {thread}");
            }
        }
    }
}

#[test]
fn a_static_source_error_stops_the_build_before_any_application_macro_runs() {
    let roots = Roots::new(1);
    roots
        .write(
            0,
            "app/main.cljs",
            "(ns app.main (:require [app.early] [app.later]))\n1\n",
        )
        .write(
            0,
            "app/early.cljs",
            "(ns app.early)\n(defmacro boom [] (throw (js/Error. \"effect-ran\")))\n(boom)\n",
        )
        .write(
            0,
            "app/later.cljs",
            "(ns app.later (:require [app.gone]))\n",
        );
    let made = loaded(&roots, "app.main");
    assert!(
        error_message(&made).starts_with("namespace app.gone not found"),
        "{made:?}"
    );
    // The fixture's macro does run once the graph is whole.
    roots.write(0, "app/gone.cljs", "(ns app.gone)\n");
    let made = loaded(&roots, "app.main");
    assert!(error_message(&made).contains("effect-ran"), "{made:?}");
}
