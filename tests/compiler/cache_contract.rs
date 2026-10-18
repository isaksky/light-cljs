//! Namespace artifacts installed in place of analysis, mixed with namespaces
//! compiled fresh (docs/compiler-cache.md, "Cache contract"). There is no
//! store or key yet: a test captures every namespace in one compilation and
//! installs the ones it chooses, among those the policy evaluator lets use
//! the cache, in a second compilation with a fresh `MacroHost`.
//!
//! [`check_mixed`] is the oracle check the macros, deferred, compile-time
//! unit, protocol/record and chunk suites run on their multi-namespace
//! fixtures: every mix of hits and misses it tries emits byte for byte what
//! the uncached compilation emits at `none`, `prepare` and `advanced`.
use crate::temp_dir::TempDir;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::cache_policy::{self, CacheSettings};
use light_cljs::compiler::{
    EntryRecorders, analyze_namespace_entry_recorded,
    analyze_namespace_entry_recorded_with_provider, analyze_namespace_entry_with_sources,
};
use light_cljs::dependency_manifest::DependencyManifest;
use light_cljs::namespace_artifact::NamespaceArtifact;
use light_cljs::source_provider::SourceProvider;
use light_cljs::{
    ApplicationOptions, CompileStats, MacroHost, OptimizationLevel, prepare_application,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Prepare,
    OptimizationLevel::Advanced,
];

/// Where a compilation's sources come from.
#[derive(Clone, Copy)]
pub(crate) enum Input<'a> {
    Roots(&'a [PathBuf]),
    Provider(&'a dyn SourceProvider),
}

impl Input<'_> {
    fn analyze(
        self,
        entry: &str,
        host: &mut MacroHost,
        recorders: EntryRecorders<'_>,
    ) -> Result<light_cljs::Program, light_cljs::NamespaceCompileError> {
        let options = AnalysisOptions::default();
        let warnings = &mut Vec::new();
        match self {
            Input::Roots(roots) => {
                analyze_namespace_entry_recorded(entry, roots, host, options, warnings, recorders)
            }
            Input::Provider(sources) => analyze_namespace_entry_recorded_with_provider(
                entry, sources, host, options, warnings, recorders,
            ),
        }
        .map(|(program, _)| program)
    }
}

/// Every namespace's artifact from one capturing compilation, and which of
/// them the policy evaluator lets use the cache.
pub(crate) struct Captured {
    pub namespaces: Vec<String>,
    pub artifacts: HashMap<String, NamespaceArtifact>,
    pub eligible: Vec<String>,
}

/// Capture every namespace of `entry`'s compilation, with the build's cache
/// on and `blockers` as its `--cache-blocker` set.
pub(crate) fn capture(input: Input<'_>, entry: &str, blockers: &[&str]) -> Option<Captured> {
    let mut host = MacroHost::new();
    let mut manifest = DependencyManifest::default();
    let mut artifacts = HashMap::new();
    let mut namespaces = Vec::new();
    let mut sink = |capture: light_cljs::namespace_artifact::NamespaceCapture| {
        namespaces.push(capture.namespace.clone());
        if let Ok(artifact) = capture.artifact {
            artifacts.insert(capture.namespace, artifact);
        }
    };
    input
        .analyze(
            entry,
            &mut host,
            EntryRecorders {
                manifest: Some(&mut manifest),
                capture: Some(&mut sink),
                ..EntryRecorders::default()
            },
        )
        .ok()?;
    let policy = cache_policy::evaluate(
        &manifest,
        &CacheSettings {
            enabled: true,
            blockers: blockers.iter().map(|name| name.to_string()).collect(),
        },
        &cache_policy::source_metadata(&manifest, &host),
        &[],
    );
    let eligible = namespaces
        .iter()
        .filter(|name| manifest.id(name).is_some_and(|id| policy.may_use_cache(id)))
        .cloned()
        .collect();
    Some(Captured {
        namespaces,
        artifacts,
        eligible,
    })
}

/// Compile `entry` in a fresh host, installing the eligible artifacts of the
/// namespaces `hits` names. Every installation must succeed.
pub(crate) fn compile_mixed(
    input: Input<'_>,
    entry: &str,
    captured: &Captured,
    hits: &[&str],
) -> (light_cljs::Program, CompileStats) {
    let chosen: HashMap<&str, &NamespaceArtifact> = captured
        .artifacts
        .iter()
        .filter(|(name, _)| hits.contains(&name.as_str()) && captured.eligible.contains(name))
        .map(|(name, artifact)| (name.as_str(), artifact))
        .collect();
    let lookup = |name: &str| chosen.get(name).copied();
    let mut stats = CompileStats::new();
    let program = input
        .analyze(
            entry,
            &mut MacroHost::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                install: Some(&lookup),
                ..EntryRecorders::default()
            },
        )
        .unwrap_or_else(|error| panic!("mixed compilation with hits {hits:?} failed: {error}"));
    let install = stats.install.clone().expect("install counts");
    assert_eq!(install.refused, Vec::new(), "refused artifacts");
    let mut expected = chosen
        .keys()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    let mut installed = install.installed.clone();
    expected.sort();
    installed.sort();
    assert_eq!(installed, expected, "installed namespaces");
    for name in &install.installed {
        assert_eq!(
            stats.macro_calls_in(name),
            Some(0),
            "installed namespace {name} executed macro calls"
        );
    }
    (program, stats)
}

/// What a program emits at each of [`LEVELS`]: one script, and the chunk
/// files of a program with lazy references.
fn outputs(program: &light_cljs::Program) -> Vec<String> {
    LEVELS
        .iter()
        .map(|&level| {
            let prepared = prepare_application(program, ApplicationOptions::new(level));
            let mut out = prepared.emit_js();
            if !program.lazy_namespaces.is_empty() {
                for file in prepared.emit_chunks().files {
                    out.push_str(&format!("\n// chunk {}\n{}", file.name, file.source));
                }
            }
            out
        })
        .collect()
}

/// The mixes of hits and misses [`check_mixed`] tries: every eligible
/// namespace, and alternate halves of them in source order.
fn mixes(captured: &Captured) -> Vec<Vec<&str>> {
    let eligible = captured
        .namespaces
        .iter()
        .filter(|name| captured.eligible.contains(name) && captured.artifacts.contains_key(*name))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut mixes = vec![eligible.clone()];
    for parity in [0, 1] {
        let half = eligible
            .iter()
            .enumerate()
            .filter(|(index, _)| index % 2 == parity)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>();
        if !half.is_empty() && half.len() < eligible.len() {
            mixes.push(half);
        }
    }
    mixes
}

/// Check that installing captured artifacts for any mix [`mixes`] tries
/// leaves the output of `entry`'s compilation byte for byte what the
/// uncached compilation emits, at `none`, `prepare` and `advanced`.
/// Byte-identical JavaScript runs as the uncached build does. A fixture
/// whose uncached compilation fails has nothing to compare.
pub(crate) fn check_mixed_input(input: Input<'_>, entry: &str) {
    let Ok(program) = input.analyze(entry, &mut MacroHost::new(), EntryRecorders::default()) else {
        return;
    };
    let expected = outputs(&program);
    let captured = capture(input, entry, &[]).expect("the capturing compilation fails");
    for hits in mixes(&captured) {
        let (program, _) = compile_mixed(input, entry, &captured, &hits);
        for ((level, expected), actual) in LEVELS.iter().zip(&expected).zip(outputs(&program)) {
            if *expected != actual {
                let line = expected
                    .lines()
                    .zip(actual.lines())
                    .position(|(a, b)| a != b)
                    .unwrap_or(0);
                panic!(
                    "{entry} with hits {hits:?} differs at {level:?} from line {}:\n\
                     uncached: {}\n\
                     mixed:    {}",
                    line + 1,
                    expected.lines().nth(line).unwrap_or(""),
                    actual.lines().nth(line).unwrap_or("")
                );
            }
        }
    }
}

/// [`check_mixed_input`] for sources under `root`.
pub(crate) fn check_mixed(root: &Path, entry: &str) {
    check_mixed_input(Input::Roots(&[root.to_path_buf()]), entry);
}

struct Sources(TempDir);

impl Sources {
    fn new(files: &[(&str, &str)]) -> Self {
        let sources = Self(TempDir::new("cljs-cache-contract"));
        for (path, source) in files {
            let path = sources.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        sources
    }
    fn roots(&self) -> Vec<PathBuf> {
        vec![self.0.path().to_path_buf()]
    }
}

fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", js])
        .output()
        .expect("compiler integration requires Node");
    assert!(
        output.status.success(),
        "{}\n{js}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run(program: &light_cljs::Program) -> String {
    execute(
        &prepare_application(program, ApplicationOptions::new(OptimizationLevel::None)).emit_js(),
    )
}

const COUNTER: [(&str, &str); 3] = [
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
        "(ns main (:require [dep :as d] [macros :refer [next-value]])) (js/console.log d/value (next-value))",
    ),
];

/// The counter example of "Cache contract": a hit skips its macro calls, so
/// a later miss sees a compile-time state the skipped call never changed.
#[test]
fn an_installed_namespace_skips_its_macro_calls() {
    let sources = Sources::new(&COUNTER);
    let roots = sources.roots();
    let (fresh, _) = analyze_namespace_entry_with_sources(
        "main",
        &roots,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(run(&fresh), "1 2\n");
    let captured = capture(Input::Roots(&roots), "main", &[]).unwrap();
    assert_eq!(captured.eligible, ["macros", "dep", "main"]);
    // `dep` installed: its call never ran, so `main`'s is the first.
    let (program, stats) = compile_mixed(Input::Roots(&roots), "main", &captured, &["dep"]);
    assert_eq!(run(&program), "1 1\n");
    assert_eq!(stats.macro_calls_in("dep"), Some(0));
    assert!(stats.macro_calls_in("main").unwrap() > 0);
    let report = stats.report();
    assert!(
        report.contains("\ncache.install.namespaces 1\n"),
        "{report}"
    );
    assert!(report.contains("\ncache.install.refused 0\n"), "{report}");
    // `macros` installed as well: `main`'s expansion demands its `counter`
    // helper unit and its macro function, registered but never run.
    let (program, stats) =
        compile_mixed(Input::Roots(&roots), "main", &captured, &["macros", "dep"]);
    assert_eq!(run(&program), "1 1\n");
    assert_eq!(stats.macro_calls_in("macros"), Some(0));
    assert_eq!(stats.macro_calls_in("dep"), Some(0));
    assert!(stats.deferred_units.executed > 0);
}

/// A blocker forces its compilation consumers to miss, so every macro call
/// runs again as in a fresh build.
#[test]
fn a_blocked_macro_provider_makes_its_consumers_miss() {
    let sources = Sources::new(&COUNTER);
    let roots = sources.roots();
    let captured = capture(Input::Roots(&roots), "main", &["macros"]).unwrap();
    assert_eq!(captured.eligible, Vec::<String>::new());
    let (program, stats) = compile_mixed(
        Input::Roots(&roots),
        "main",
        &captured,
        &["macros", "dep", "main"],
    );
    assert_eq!(run(&program), "1 2\n");
    assert_eq!(stats.install.unwrap().installed, Vec::<String>::new());
}

/// Installed namespaces execute none of their macro calls, while the misses
/// that use their macros and helpers do.
#[test]
fn installed_namespaces_execute_no_macro_calls() {
    let sources = Sources::new(&[
        (
            "lib/macros.cljs",
            "(ns lib.macros) (defn twice [x] (* 2 x)) (defmacro doubled [x] (twice x))",
        ),
        (
            "lib/uses.cljs",
            "(ns lib.uses (:require [lib.macros :refer [doubled]])) (def a (doubled 3)) (def b (doubled 4)) (when-let [x a] (def c (+ x b)))",
        ),
        (
            "app/main.cljs",
            "(ns app.main (:require [lib.uses :as u] [lib.macros :refer [doubled]])) (js/console.log u/a u/b u/c (doubled 5))",
        ),
    ]);
    let roots = sources.roots();
    let mut fresh = CompileStats::new();
    let (program, _) = light_cljs::compiler::analyze_namespace_entry_with_stats(
        "app.main",
        &roots,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
        &mut fresh,
    )
    .unwrap();
    assert_eq!(run(&program), "6 8 14 10\n");
    assert!(fresh.macro_calls_in("lib.uses").unwrap() > 0);
    let captured = capture(Input::Roots(&roots), "app.main", &[]).unwrap();
    for hits in [
        &["lib.uses"][..],
        &["lib.macros", "lib.uses"],
        &["lib.macros"],
    ] {
        let (program, stats) = compile_mixed(Input::Roots(&roots), "app.main", &captured, hits);
        assert_eq!(run(&program), "6 8 14 10\n", "{hits:?}");
        for hit in hits {
            assert_eq!(stats.macro_calls_in(hit), Some(0), "{hit}");
        }
        assert!(stats.macro_calls_in("app.main").unwrap() > 0);
        assert!(stats.macro_calls < fresh.macro_calls, "{hits:?}");
    }
    check_mixed(sources.0.path(), "app.main");
}

/// A miss whose macro calls a helper defined in an installed namespace
/// demands the helper's prepared unit, which the installation registered.
#[test]
fn a_miss_demands_a_helper_unit_of_an_installed_namespace() {
    let sources = Sources::new(&[
        (
            "helpers.cljs",
            "(ns helpers) (defn label [x] (str \"<\" x \">\")) (def table {:a 1 :b 2})",
        ),
        (
            "macros.cljs",
            "(ns macros (:require [helpers :as h])) (defmacro labelled [k] (h/label (get h/table k)))",
        ),
        (
            "main.cljs",
            "(ns main (:require [macros :refer [labelled]])) (js/console.log (labelled :b))",
        ),
    ]);
    let roots = sources.roots();
    let captured = capture(Input::Roots(&roots), "main", &[]).unwrap();
    for hits in [&["helpers"][..], &["helpers", "macros"]] {
        let (program, stats) = compile_mixed(Input::Roots(&roots), "main", &captured, hits);
        assert_eq!(run(&program), "<2>\n", "{hits:?}");
        assert!(stats.deferred_units.executed > 0, "{hits:?}");
    }
    check_mixed(sources.0.path(), "main");
}

/// An artifact that does not describe the source the loader resolved is
/// refused, with its reason, before it changes anything, and the namespace
/// is analyzed instead.
#[test]
fn a_refused_artifact_leaves_its_namespace_to_analysis() {
    let sources = Sources::new(&[
        ("leaf.cljs", "(ns leaf) (def x 1)"),
        (
            "main.cljs",
            "(ns main (:require [leaf])) (js/console.log leaf/x)",
        ),
    ]);
    let roots = sources.roots();
    let captured = capture(Input::Roots(&roots), "main", &[]).unwrap();
    fs::write(sources.0.join("leaf.cljs"), "(ns leaf) (def x 22)").unwrap();
    let lookup = |name: &str| (name == "leaf").then(|| &captured.artifacts["leaf"]);
    let mut stats = CompileStats::new();
    let program = Input::Roots(&roots)
        .analyze(
            "main",
            &mut MacroHost::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                install: Some(&lookup),
                ..EntryRecorders::default()
            },
        )
        .unwrap();
    assert_eq!(run(&program), "22\n");
    let install = stats.install.unwrap();
    assert_eq!(install.installed, Vec::<String>::new());
    assert_eq!(
        install.refused,
        [(
            "leaf".to_owned(),
            "the artifact describes another source file".to_owned()
        )]
    );
}

/// Macro-free leaf namespaces, protocols and records across namespaces.
#[test]
fn mixed_hits_and_misses_match_the_uncached_build() {
    let sources = Sources::new(&[
        (
            "shapes/core.cljs",
            "(ns shapes.core) (defprotocol Area (area [s])) (defrecord Square [side] Area (area [_] (* side side)))",
        ),
        (
            "shapes/circle.cljs",
            "(ns shapes.circle (:require [shapes.core :as c])) (deftype Circle [r] c/Area (area [_] (* 3 r r))) (extend-type string c/Area (area [s] (count s)))",
        ),
        (
            "util.cljs",
            "(ns util) (def ^:private secret 7) (defn add [a b] (+ a b secret)) (def items [1 2 3])",
        ),
        (
            "main.cljs",
            "(ns main (:require [shapes.core :as c] [shapes.circle :refer [->Circle]] [util :as u] [clojure.string :as s])) (js/console.log (c/area (c/->Square 3)) (c/area (->Circle 2)) (c/area \"four\") (u/add 1 2) (s/join \",\" u/items))",
        ),
    ]);
    let roots = sources.roots();
    let (program, _) = analyze_namespace_entry_with_sources(
        "main",
        &roots,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(run(&program), "9 12 4 10 1,2,3\n");
    check_mixed(sources.0.path(), "main");
}

/// The uncached program of `entry`.
fn uncached(roots: &[PathBuf], entry: &str) -> light_cljs::Program {
    analyze_namespace_entry_with_sources(
        entry,
        roots,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap()
    .0
}

/// What a program emits at `none`, with its source map.
fn mapped(program: &light_cljs::Program) -> (String, light_cljs::source_map::SourceMapping) {
    prepare_application(program, ApplicationOptions::new(OptimizationLevel::None)).emit_js_mapped()
}

const LIBRARY: (&str, &str) = (
    "lib.cljs",
    "(ns lib) (defn helper [x] (* 10 x)) (defn ^{:test (fn [] (assert (= 20 (helper 2))))} checked [x] (helper x)) (defmacro scaled [x] (helper x))",
);

/// An artifact installed after a namespace it was not captured with lands at
/// other source indices, global IDs and deferred-unit IDs, and its program
/// is still the uncached one: its executable `:test` metadata names its own
/// unit, and a miss's macro demands its helper.
#[test]
fn relocates_an_artifact_after_a_new_preceding_namespace() {
    let sources = Sources::new(&[
        LIBRARY,
        (
            "main.cljs",
            "(ns main (:require [lib :refer [scaled]])) (js/console.log (lib/checked 3) (scaled 4))",
        ),
    ]);
    let roots = sources.roots();
    let captured = capture(Input::Roots(&roots), "main", &[]).unwrap();
    let library = &captured.artifacts["lib"];
    assert!(
        library
            .units
            .units
            .iter()
            .any(|unit| matches!(unit.name, light_cljs::namespace_artifact::UnitName::Test(_))),
        "{}",
        library.describe()
    );
    fs::write(
        sources.0.join("other.cljs"),
        "(ns other) (defn ^{:test (fn [] true)} first-checked [] 1) (defmacro early [] 5) (def early-value (early))",
    )
    .unwrap();
    fs::write(
        sources.0.join("main.cljs"),
        "(ns main (:require [other] [lib :refer [scaled]])) (js/console.log other/early-value (lib/checked 3) (scaled 4) (some? (:test (meta #'lib/checked))))",
    )
    .unwrap();
    let expected = uncached(&roots, "main");
    assert_eq!(
        expected.namespaces.iter().position(|ns| ns == "lib"),
        Some(1)
    );
    let (program, stats) = compile_mixed(Input::Roots(&roots), "main", &captured, &["lib"]);
    assert_eq!(stats.install.unwrap().installed, ["lib"]);
    assert_eq!(outputs(&program), outputs(&expected));
    assert_eq!(mapped(&program), mapped(&expected));
    assert_eq!(run(&program), "5 30 40 true\n");
}

/// An artifact whose compile-time code relocations overlap is refused
/// before it changes anything, and its namespace is analyzed instead.
#[test]
fn overlapping_code_relocations_are_refused_before_installation() {
    let sources = Sources::new(&[
        LIBRARY,
        (
            "main.cljs",
            "(ns main (:require [lib :refer [scaled]])) (js/console.log (lib/checked 3) (scaled 4))",
        ),
    ]);
    let roots = sources.roots();
    let mut captured = capture(Input::Roots(&roots), "main", &[]).unwrap();
    let library = captured.artifacts.get_mut("lib").unwrap();
    let code = library
        .units
        .units
        .iter_mut()
        .map(|unit| &mut unit.code)
        .find(|code| !code.units.is_empty())
        .expect("a unit whose code names another");
    let first = code.units[0];
    code.units.insert(0, first);
    assert!(library.validate().is_err());
    let lookup = |name: &str| (name == "lib").then(|| &captured.artifacts["lib"]);
    let mut stats = CompileStats::new();
    let program = Input::Roots(&roots)
        .analyze(
            "main",
            &mut MacroHost::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                install: Some(&lookup),
                ..EntryRecorders::default()
            },
        )
        .unwrap();
    let install = stats.install.unwrap();
    assert_eq!(install.installed, Vec::<String>::new());
    assert_eq!(install.refused.len(), 1, "{:?}", install.refused);
    assert_eq!(outputs(&program), outputs(&uncached(&roots, "main")));
    assert_eq!(run(&program), "30 40\n");
}

/// A helper's registration names its namespace's reader context as it was
/// where the helper was defined, before a later `defmacro` of the same
/// name, so syntax-quote at compile time resolves as it does uncached.
#[test]
fn installed_helpers_keep_their_definition_reader_context() {
    let sources = Sources::new(&[
        (
            "lib/helpers.cljs",
            "(ns lib.helpers) (defn quoted [] (binding [*ns* (find-ns (quote lib.helpers))] (pr-str [(read-string \"`map\") `map]))) (defmacro map [x] x) (def tail 0)",
        ),
        (
            "main.cljs",
            "(ns main (:require [lib.helpers :as h])) (defmacro show [] (h/quoted)) (js/console.log (show))",
        ),
    ]);
    let roots = sources.roots();
    let expected = uncached(&roots, "main");
    let printed = run(&expected);
    let captured = capture(Input::Roots(&roots), "main", &[]).unwrap();
    let (program, _) = compile_mixed(Input::Roots(&roots), "main", &captured, &["lib.helpers"]);
    assert_eq!(run(&program), printed);
    assert_eq!(outputs(&program), outputs(&expected));
}
