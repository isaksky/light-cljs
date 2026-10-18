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
use light_cljs::analyzer::{AnalysisOptions, AnalysisWarning, CheckedArrays};
use light_cljs::artifact_codec::{self, DecodeLimits, ManifestFields};
use light_cljs::cache_policy::{self, CacheSettings};
use light_cljs::compiler::{
    EntryRecorders, analyze_namespace_entry_recorded,
    analyze_namespace_entry_recorded_with_provider, analyze_namespace_entry_with_sources,
};
use light_cljs::dependency_manifest::DependencyManifest;
use light_cljs::evaluator::ExecutionControl;
use light_cljs::generated_names;
use light_cljs::namespace_artifact::NamespaceArtifact;
use light_cljs::source_provider::SourceProvider;
use light_cljs::{
    ApplicationOptions, CompileStats, MacroHost, OptimizationLevel, prepare_application,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
        self.analyze_with(entry, host, AnalysisOptions::default(), recorders)
            .map(|(program, _)| program)
    }

    /// [`Self::analyze`] under `options`, with the compilation's warnings.
    fn analyze_with(
        self,
        entry: &str,
        host: &mut MacroHost,
        options: AnalysisOptions,
        recorders: EntryRecorders<'_>,
    ) -> Result<(light_cljs::Program, Vec<AnalysisWarning>), light_cljs::NamespaceCompileError>
    {
        let mut warnings = Vec::new();
        match self {
            Input::Roots(roots) => analyze_namespace_entry_recorded(
                entry,
                roots,
                host,
                options,
                &mut warnings,
                recorders,
            ),
            Input::Provider(sources) => analyze_namespace_entry_recorded_with_provider(
                entry,
                sources,
                host,
                options,
                &mut warnings,
                recorders,
            ),
        }
        .map(|(program, _)| (program, warnings))
    }
}

/// Every namespace's artifact from one capturing compilation, and which of
/// them the policy evaluator lets use the cache.
pub(crate) struct Captured {
    pub namespaces: Vec<String>,
    /// Each artifact after its round trip through the binary encoding
    /// ([`round_trip`]), which is what a store would install.
    pub artifacts: HashMap<String, NamespaceArtifact>,
    /// The artifacts as captured, in memory.
    pub originals: HashMap<String, NamespaceArtifact>,
    pub eligible: Vec<String>,
    /// The capturing compilation's program, whose generated names its
    /// artifacts retain.
    pub program: light_cljs::Program,
}

/// Capture every namespace of `entry`'s compilation, with the build's cache
/// on and `blockers` as its `--cache-blocker` set.
pub(crate) fn capture(input: Input<'_>, entry: &str, blockers: &[&str]) -> Option<Captured> {
    capture_with(input, entry, blockers, AnalysisOptions::default())
}

/// [`capture`] under `options`.
fn capture_with(
    input: Input<'_>,
    entry: &str,
    blockers: &[&str],
    options: AnalysisOptions,
) -> Option<Captured> {
    capture_in(input, entry, blockers, options, &mut MacroHost::new())
}

/// [`capture_with`] in `host`.
fn capture_in(
    input: Input<'_>,
    entry: &str,
    blockers: &[&str],
    options: AnalysisOptions,
    host: &mut MacroHost,
) -> Option<Captured> {
    let mut manifest = DependencyManifest::default();
    let mut artifacts = HashMap::new();
    let mut namespaces = Vec::new();
    let mut sink = |capture: light_cljs::namespace_artifact::NamespaceCapture| {
        namespaces.push(capture.namespace.clone());
        if let Ok(artifact) = capture.artifact {
            artifacts.insert(capture.namespace, artifact);
        }
    };
    let (program, _) = input
        .analyze_with(
            entry,
            host,
            options,
            EntryRecorders {
                manifest: Some(&mut manifest),
                capture: Some(&mut sink),
                ..EntryRecorders::default()
            },
        )
        .map_err(|error| eprintln!("the capturing compilation failed: {error}"))
        .ok()?;
    let policy = cache_policy::evaluate(
        &manifest,
        &CacheSettings {
            enabled: true,
            blockers: blockers.iter().map(|name| name.to_string()).collect(),
        },
        &cache_policy::source_metadata(&manifest, host),
        &cache_policy::non_cacheable_inputs(&manifest),
    );
    let eligible = namespaces
        .iter()
        .filter(|name| manifest.id(name).is_some_and(|id| policy.may_use_cache(id)))
        .cloned()
        .collect();
    let metadata = cache_policy::source_metadata(&manifest, host);
    let decoded = artifacts
        .iter()
        .map(|(name, artifact)| {
            let id = manifest
                .id(name)
                .expect("a captured namespace is in the manifest");
            let key = light_cljs::cache_key::FrontEndKey {
                digest: [1; 32],
                ..Default::default()
            };
            let fields = ManifestFields::of(&manifest, id, metadata[id.0 as usize], key, [2; 32]);
            (name.clone(), round_trip(artifact, &fields))
        })
        .collect();
    Some(Captured {
        namespaces,
        artifacts: decoded,
        originals: artifacts,
        eligible,
        program,
    })
}

/// `artifact` encoded and decoded again, after checking that nothing
/// changed: the decoded artifact prints as the original does and encodes
/// to the same bytes, and its manifest reads back as written.
pub(crate) fn round_trip(
    artifact: &NamespaceArtifact,
    fields: &ManifestFields,
) -> NamespaceArtifact {
    let name = &artifact.header.namespace;
    let bytes = artifact_codec::encode(artifact, fields)
        .unwrap_or_else(|error| panic!("{name} does not encode: {error}"));
    let limits = DecodeLimits::default();
    let manifest = artifact_codec::read_manifest(&bytes, &limits)
        .unwrap_or_else(|error| panic!("{name}'s manifest does not read: {error}"));
    assert_eq!(&manifest.fields, fields, "{name}'s manifest fields");
    assert_eq!(&manifest.namespace, name);
    assert_eq!(manifest.requires, artifact.declarations.requires);
    assert_eq!(manifest.references, artifact.references.namespaces);
    let (_, decoded) = artifact_codec::decode(&bytes, &limits, &|| false)
        .unwrap_or_else(|error| panic!("{name} does not decode: {error}"));
    assert_eq!(
        format!("{decoded:?}"),
        format!("{artifact:?}"),
        "{name} changed in its round trip"
    );
    assert!(
        artifact_codec::encode(&decoded, fields).unwrap() == bytes,
        "{name} encodes differently after its round trip"
    );
    decoded
}

/// Compile `entry` in a fresh host, installing the eligible artifacts of the
/// namespaces `hits` names, with their generated names reserved from the
/// start. Every installation must succeed.
pub(crate) fn compile_mixed(
    input: Input<'_>,
    entry: &str,
    captured: &Captured,
    hits: &[&str],
) -> (light_cljs::Program, CompileStats) {
    let (program, stats, _) =
        compile_mixed_with(input, entry, captured, hits, AnalysisOptions::default());
    (program, stats)
}

/// The artifacts of `captured` that `hits` names and the policy lets use
/// the cache.
fn chosen<'a>(captured: &'a Captured, hits: &[&str]) -> HashMap<&'a str, &'a NamespaceArtifact> {
    chosen_from(captured, &captured.artifacts, hits)
}

/// [`chosen`] of `artifacts`, decoded ones or the originals.
fn chosen_from<'a>(
    captured: &Captured,
    artifacts: &'a HashMap<String, NamespaceArtifact>,
    hits: &[&str],
) -> HashMap<&'a str, &'a NamespaceArtifact> {
    artifacts
        .iter()
        .filter(|(name, _)| hits.contains(&name.as_str()) && captured.eligible.contains(name))
        .map(|(name, artifact)| (name.as_str(), artifact))
        .collect()
}

/// [`compile_mixed`] under `options`, with the compilation's warnings.
fn compile_mixed_with(
    input: Input<'_>,
    entry: &str,
    captured: &Captured,
    hits: &[&str],
    options: AnalysisOptions,
) -> (light_cljs::Program, CompileStats, Vec<AnalysisWarning>) {
    compile_mixed_from(input, entry, captured, &captured.artifacts, hits, options)
}

/// [`compile_mixed_with`] installing `artifacts`, decoded ones or the
/// originals.
fn compile_mixed_from(
    input: Input<'_>,
    entry: &str,
    captured: &Captured,
    artifacts: &HashMap<String, NamespaceArtifact>,
    hits: &[&str],
    options: AnalysisOptions,
) -> (light_cljs::Program, CompileStats, Vec<AnalysisWarning>) {
    let chosen = chosen_from(captured, artifacts, hits);
    let mut host = MacroHost::new();
    let (program, stats, warnings) =
        compile_installing_with(input, entry, &chosen, &mut host, options, None)
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
    (program, stats, warnings)
}

/// Compile `entry` in a fresh host, offering the artifacts of `chosen` for
/// installation, with their generated names reserved before the first
/// namespace when `reserve` is set, as a store would.
fn compile_installing(
    input: Input<'_>,
    entry: &str,
    chosen: &HashMap<&str, &NamespaceArtifact>,
    reserve: bool,
) -> Result<(light_cljs::Program, CompileStats), light_cljs::NamespaceCompileError> {
    let lookup = |name: &str| chosen.get(name).copied();
    let reservation = reserve
        .then(|| generated_names::Reservation::of(chosen.values().map(|artifact| &artifact.names)));
    let mut stats = CompileStats::new();
    let program = input.analyze(
        entry,
        &mut MacroHost::new(),
        EntryRecorders {
            stats: Some(&mut stats),
            install: Some(&lookup),
            reserve: reservation.as_ref(),
            ..EntryRecorders::default()
        },
    )?;
    Ok((program, stats))
}

/// [`compile_installing`] in `host`, under `options`, with names reserved,
/// cancelled by `control`; with the compilation's warnings.
#[allow(clippy::type_complexity)]
fn compile_installing_with(
    input: Input<'_>,
    entry: &str,
    chosen: &HashMap<&str, &NamespaceArtifact>,
    host: &mut MacroHost,
    options: AnalysisOptions,
    control: Option<&ExecutionControl>,
) -> Result<(light_cljs::Program, CompileStats, Vec<AnalysisWarning>), String> {
    let lookup = |name: &str| chosen.get(name).copied();
    let reservation =
        generated_names::Reservation::of(chosen.values().map(|artifact| &artifact.names));
    let mut stats = CompileStats::new();
    let (program, warnings) = input
        .analyze_with(
            entry,
            host,
            options,
            EntryRecorders {
                stats: Some(&mut stats),
                install: Some(&lookup),
                reserve: Some(&reservation),
                control,
                ..EntryRecorders::default()
            },
        )
        .map_err(|error| error.to_string())?;
    Ok((program, stats, warnings))
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
/// uncached compilation emits, at `none`, `prepare` and `advanced`, with
/// the same warnings in the same order. Byte-identical JavaScript runs as
/// the uncached build does. A fixture whose uncached compilation fails has
/// nothing to compare.
///
/// The artifacts installed are the decoded ones ([`round_trip`]); with every
/// eligible namespace a hit, the in-memory artifacts are installed as well,
/// and both emit the same.
pub(crate) fn check_mixed_input(input: Input<'_>, entry: &str) {
    let options = AnalysisOptions::default();
    let Ok((program, uncached_warnings)) = input.analyze_with(
        entry,
        &mut MacroHost::new(),
        options,
        EntryRecorders::default(),
    ) else {
        return;
    };
    let expected = outputs(&program);
    let captured = capture(input, entry, &[]).expect("the capturing compilation fails");
    for (index, hits) in mixes(&captured).into_iter().enumerate() {
        let (program, _, warnings) = compile_mixed_with(input, entry, &captured, &hits, options);
        assert_eq!(warnings, uncached_warnings, "{entry} with hits {hits:?}");
        let actual = outputs(&program);
        if index == 0 {
            let (in_memory, _, _) =
                compile_mixed_from(input, entry, &captured, &captured.originals, &hits, options);
            assert!(
                outputs(&in_memory) == actual,
                "{entry}: installing decoded artifacts differs from installing them in memory"
            );
        }
        for ((level, expected), actual) in LEVELS.iter().zip(&expected).zip(actual) {
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

// Generated names (docs/compiler-cache.md, "Generated names").

/// Whether a local's name is one the compiler or a macro generated.
fn generated(name: &str) -> bool {
    name.ends_with("__auto__") || name.ends_with('#') || name.starts_with("G__")
}

/// The distinct generated names each source binds, by the program's binding
/// table, keyed by source index.
fn generated_locals(program: &light_cljs::Program) -> BTreeMap<usize, BTreeSet<String>> {
    let mut names = BTreeMap::<usize, BTreeSet<String>>::new();
    for local in program.locals.iter().filter(|local| generated(&local.name)) {
        names
            .entry(local.source_index)
            .or_default()
            .insert(local.name.clone());
    }
    names
}

/// A generated name stands for one identity, such as one `x#` of one syntax
/// quote. Two identities that come out with one name merge in the binding
/// table and can capture each other's references in a scope. So the
/// program binds as many distinct generated names, source by source and in
/// all, as the uncached build does; and since locals are emitted by binding
/// ID, its output at `none` is the uncached output when no generated name
/// escapes as data.
fn assert_hygienic(program: &light_cljs::Program, uncached: &light_cljs::Program, context: &str) {
    let counts = |program| {
        generated_locals(program)
            .into_iter()
            .map(|(source, names)| (source, names.len()))
            .collect::<Vec<_>>()
    };
    let all = |program| {
        generated_locals(program)
            .into_values()
            .flatten()
            .collect::<BTreeSet<_>>()
            .len()
    };
    assert!(all(uncached) > 0, "the fixture binds no generated name");
    assert_eq!(
        counts(program),
        counts(uncached),
        "{context}: names by source"
    );
    assert_eq!(all(program), all(uncached), "{context}: names in all");
}

/// Every subset of the eligible namespaces, in source order.
fn every_mix(captured: &Captured) -> Vec<Vec<&str>> {
    let eligible = captured
        .namespaces
        .iter()
        .filter(|name| captured.eligible.contains(name) && captured.artifacts.contains_key(*name))
        .map(String::as_str)
        .collect::<Vec<_>>();
    (0..1usize << eligible.len())
        .map(|mask| {
            eligible
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, name)| *name)
                .collect()
        })
        .collect()
}

fn none_output(program: &light_cljs::Program) -> String {
    prepare_application(program, ApplicationOptions::new(OptimizationLevel::None)).emit_js()
}

/// Two namespaces whose macros, functions and `binding` all generate names:
/// syntax-quote `x#`, `#()` parameters inside and outside syntax quotes,
/// destructuring locals and `binding` locals. `main` nests one's macro
/// expansion inside the other's, so equal names would capture a reference.
const AUTO_NAMES: [(&str, &str); 3] = [
    (
        "p.cljs",
        "(ns p) (defmacro wrap [f v] `(let [x# ~v] (~f x#))) (defmacro call3 [f] `(#(~f %) 3)) (defn pd [{:keys [a b]} [c & d]] (#(+ %1 %2 a b c (count d)) 1 2)) (def ^:dynamic *depth* 0) (defn depth [] *depth*) (def bound (binding [*depth* 5] (depth)))",
    ),
    (
        "q.cljs",
        "(ns q) (defmacro outer [& body] `(let [x# 10] ~@body)) (defmacro add100 [v] `(#(+ % ~v) 100)) (defmacro sum-pair [pair] `(let [[a# b#] ~pair] (+ a# b#))) (defn qd [[x y] {:keys [z]}] (#(* % x y z) 2))",
    ),
    (
        "main.cljs",
        "(ns main (:require [p :refer [wrap call3]] [q :refer [outer add100 sum-pair]])) (js/console.log (wrap outer 3) (call3 add100) (sum-pair [1 2]) (p/pd {:a 1 :b 2} [3 4]) (q/qd [1 2] {:z 3}) p/bound)",
    ),
];

/// Every mix of hits and misses keeps generated names apart, also when the
/// compilation stands for a new process: its allocators start where the
/// capture's did, or where one namespace's analysis left them, so a miss
/// allocates the very numbers a hit retains unless they are reserved.
#[test]
fn generated_names_stay_apart_in_every_mix_of_hits_and_misses() {
    let sources = Sources::new(&AUTO_NAMES);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let expected = uncached(&roots, "main");
    let printed = run(&expected);
    assert_eq!(printed, "3 103 3 10 12 5\n");
    let start = generated_names::current();
    let captured = capture(input, "main", &[]).unwrap();
    assert_eq!(captured.eligible, ["p", "q", "main"]);
    let restarts = std::iter::once(start)
        .chain(
            captured
                .namespaces
                .iter()
                .map(|name| captured.artifacts[name].names.to),
        )
        .collect::<Vec<_>>();
    for hits in every_mix(&captured) {
        for restart in &restarts {
            generated_names::rewind(restart);
            let context = format!("hits {hits:?} from {restart:?}");
            let (program, _) = compile_mixed(input, "main", &captured, &hits);
            assert_hygienic(&program, &expected, &context);
            assert_eq!(none_output(&program), none_output(&expected), "{context}");
            assert_eq!(run(&program), printed, "{context}");
        }
    }
    check_mixed(sources.0.path(), "main");
}

/// Two namespaces whose constants are gensyms the compile-time runtime made:
/// a symbol and a string. Each compilation's runtime counts from zero.
const GENSYM_DATA: [(&str, &str); 4] = [
    (
        "macros.cljs",
        "(ns macros) (defmacro uniq [] (list 'quote (gensym \"k\"))) (defmacro uniq-name [] (str (gensym)))",
    ),
    (
        "a.cljs",
        "(ns a (:require [macros :refer [uniq uniq-name]])) (def s (uniq)) (def n (uniq-name))",
    ),
    (
        "b.cljs",
        "(ns b (:require [macros :refer [uniq uniq-name]])) (def s (uniq)) (def n (uniq-name))",
    ),
    (
        "main.cljs",
        "(ns main (:require [a] [b])) (js/console.log (pr-str [a/s a/n b/s b/n]) (= a/s b/s) (= a/n b/n))",
    ),
];

/// The gensyms `main` prints, `a`'s then `b`'s, and whether each pair is
/// equal.
fn printed_gensyms(printed: &str) -> (Vec<String>, String) {
    let (names, equal) = printed[1..].split_once("] ").expect(printed);
    (
        names
            .split(' ')
            .map(|name| name.trim_matches('"').to_owned())
            .collect(),
        equal.trim_end().to_owned(),
    )
}

/// A gensym escaping as data keeps, in a hit, the name it was captured with,
/// unnormalized, and a miss's gensym never takes it.
#[test]
fn a_gensym_escaping_as_data_keeps_its_name_and_no_miss_takes_it() {
    let sources = Sources::new(&GENSYM_DATA);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let (_, equal) = printed_gensyms(&run(&uncached(&roots, "main")));
    assert_eq!(equal, "false false");
    let captured = capture(input, "main", &[]).unwrap();
    assert_eq!(captured.eligible, ["macros", "a", "b", "main"]);
    // What the hits retain: the capture's names, which need not be the
    // uncached build's.
    let (names, equal) = printed_gensyms(&run(&captured.program));
    assert_eq!(equal, "false false");
    for hits in every_mix(&captured) {
        let (program, _) = compile_mixed(input, "main", &captured, &hits);
        let (mixed, equal) = printed_gensyms(&run(&program));
        assert_eq!(equal, "false false", "hits {hits:?}: {mixed:?}");
        assert_eq!(
            mixed.iter().collect::<BTreeSet<_>>().len(),
            4,
            "hits {hits:?}: {mixed:?}"
        );
        for (index, namespace) in ["a", "b"].iter().enumerate() {
            if hits.contains(namespace) {
                assert_eq!(
                    mixed[2 * index..2 * index + 2],
                    names[2 * index..2 * index + 2],
                    "hits {hits:?}"
                );
            }
        }
    }
}

/// Offer the artifacts of `hits` without reserving their names before the
/// first namespace.
fn compile_unreserved(
    input: Input<'_>,
    entry: &str,
    captured: &Captured,
    hits: &[&str],
) -> (light_cljs::Program, CompileStats) {
    let chosen = hits
        .iter()
        .map(|name| (*name, &captured.artifacts[*name]))
        .collect();
    compile_installing(input, entry, &chosen, false).unwrap()
}

/// A namespace compiled fresh before a later hit, which allocates more names
/// than when the hit was captured, reaches the hit's numbers in a new
/// process. The hit's names are reserved before the first namespace, so the
/// fresh ones stay apart. Without that reservation, a hit whose window the
/// fresh namespace allocated from is refused and compiled fresh, and gensyms
/// compare equal.
#[test]
fn a_hit_installed_after_a_fresh_namespace_keeps_its_names_apart() {
    let sources = Sources::new(&AUTO_NAMES);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    uncached(&roots, "main");
    let start = generated_names::current();
    let captured = capture(input, "main", &[]).unwrap();
    // Two more syntax-quote names and two more `#()` before `wrap`.
    fs::write(
        sources.0.join("p.cljs"),
        AUTO_NAMES[0].1.replacen(
            "(ns p)",
            "(ns p) (defmacro pad [] `[a# b#]) (def pads [#(inc %) #(dec %)])",
            1,
        ),
    )
    .unwrap();
    let expected = uncached(&roots, "main");
    let printed = run(&expected);
    assert_eq!(printed, "3 103 3 10 12 5\n");
    generated_names::rewind(&start);
    let (program, stats) = compile_unreserved(input, "main", &captured, &["q"]);
    let install = stats.install.unwrap();
    assert_eq!(install.installed, Vec::<String>::new());
    assert_eq!(
        install.refused,
        [(
            "q".to_owned(),
            "its syntax-quote names overlap ones the compilation already holds".to_owned()
        )]
    );
    assert_eq!(run(&program), printed);
    generated_names::rewind(&start);
    let (program, _) = compile_mixed(input, "main", &captured, &["q"]);
    assert_hygienic(&program, &expected, "q after a fresh p");
    assert_eq!(none_output(&program), none_output(&expected));
    assert_eq!(run(&program), printed);

    let sources = Sources::new(&GENSYM_DATA);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let captured = capture(input, "main", &[]).unwrap();
    let (names, _) = printed_gensyms(&run(&captured.program));
    fs::write(
        sources.0.join("a.cljs"),
        GENSYM_DATA[1]
            .1
            .replacen("(def s", "(def e1 (uniq)) (def e2 (uniq)) (def s", 1),
    )
    .unwrap();
    let hits = ["macros", "b"];
    let (program, _) = compile_unreserved(input, "main", &captured, &hits);
    let (_, equal) = printed_gensyms(&run(&program));
    assert_ne!(equal, "false false", "the fixture does not collide");
    let (program, _) = compile_mixed(input, "main", &captured, &hits);
    let (mixed, equal) = printed_gensyms(&run(&program));
    assert_eq!(equal, "false false", "{mixed:?}");
    assert_eq!(mixed[2..], names[2..]);
}

/// Artifacts captured in separate compilations can retain the same
/// generated names for different identities. The second of two such
/// artifacts is refused, by its overlapping window or its gensym spelling,
/// and its namespace is compiled fresh.
#[test]
fn independently_captured_artifacts_that_share_names_are_not_both_installed() {
    let mut files = AUTO_NAMES.to_vec();
    files.push(("pmain.cljs", "(ns pmain (:require [p]))"));
    files.push(("qmain.cljs", "(ns qmain (:require [q]))"));
    let sources = Sources::new(&files);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let expected = run(&uncached(&roots, "main"));
    let start = generated_names::current();
    let p = capture(input, "pmain", &[]).unwrap();
    generated_names::rewind(&start);
    let q = capture(input, "qmain", &[]).unwrap();
    let chosen = HashMap::from([("p", &p.artifacts["p"]), ("q", &q.artifacts["q"])]);
    let (program, stats) = compile_installing(input, "main", &chosen, true).unwrap();
    let install = stats.install.unwrap();
    assert_eq!(install.installed, ["p"]);
    assert_eq!(
        install.refused,
        [(
            "q".to_owned(),
            "its syntax-quote names overlap ones the compilation already holds".to_owned()
        )]
    );
    assert_eq!(run(&program), expected);

    let mut files = GENSYM_DATA.to_vec();
    files.push(("amain.cljs", "(ns amain (:require [a]))"));
    files.push(("bmain.cljs", "(ns bmain (:require [b]))"));
    let sources = Sources::new(&files);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let a = capture(input, "amain", &[]).unwrap();
    let b = capture(input, "bmain", &[]).unwrap();
    assert_eq!(
        a.artifacts["a"].names.gensyms,
        b.artifacts["b"].names.gensyms
    );
    let chosen = HashMap::from([("a", &a.artifacts["a"]), ("b", &b.artifacts["b"])]);
    let (program, stats) = compile_installing(input, "main", &chosen, true).unwrap();
    let install = stats.install.unwrap();
    assert_eq!(install.installed, ["a"]);
    assert_eq!(install.refused.len(), 1, "{:?}", install.refused);
    assert!(
        install.refused[0].1.starts_with("its gensym "),
        "{:?}",
        install.refused
    );
    let (names, equal) = printed_gensyms(&run(&program));
    assert_eq!(equal, "false false", "{names:?}");
}

/// A hit installed after the compile-time runtime has started reserves its
/// gensyms in the running runtime, so a later miss does not return them.
#[test]
fn installing_a_hit_reserves_its_gensyms_in_a_running_runtime() {
    let sources = Sources::new(&[
        GENSYM_DATA[0],
        GENSYM_DATA[1],
        GENSYM_DATA[2],
        (
            "c.cljs",
            "(ns c (:require [macros :refer [uniq uniq-name]])) (def s (uniq)) (def n (uniq-name))",
        ),
        (
            "main.cljs",
            "(ns main (:require [a] [b] [c])) (js/console.log (pr-str [a/s a/n b/s b/n c/s c/n]) (= b/s c/s) (= b/n c/n))",
        ),
    ]);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let captured = capture(input, "main", &[]).unwrap();
    let (names, equal) = printed_gensyms(&run(&captured.program));
    assert_eq!(equal, "false false");
    // A fresh runtime counts from zero, and `a`, which starts it, now
    // generates two names more, so that `c` reaches `b`'s numbers.
    fs::write(
        sources.0.join("a.cljs"),
        GENSYM_DATA[1]
            .1
            .replacen("(def s", "(def e1 (uniq)) (def e2 (uniq-name)) (def s", 1),
    )
    .unwrap();
    let (program, stats) = compile_unreserved(input, "main", &captured, &["b"]);
    assert_eq!(stats.install.unwrap().installed, ["b"]);
    let (mixed, equal) = printed_gensyms(&run(&program));
    assert_eq!(equal, "false false", "{mixed:?}");
    assert_eq!(mixed[2..4], names[2..4]);
    assert_eq!(mixed.iter().collect::<BTreeSet<_>>().len(), 6, "{mixed:?}");
}

/// `gensym` appends its counter to any prefix, so a prefix ending in a digit
/// spells what another prefix spells at another count: `x1` at 1 and `x` at
/// 11 are both `x11`. A hit's spellings are reserved as spelled, so a miss
/// skips them as the uncached build does.
#[test]
fn gensym_prefixes_ending_in_digits_do_not_take_a_hits_spelling() {
    let sources = Sources::new(&[
        (
            "macros.cljs",
            "(ns macros) (defmacro one [] (list 'quote (gensym \"x1\"))) (defmacro many [] (list 'quote (vec (repeatedly 200 #(gensym \"x\")))))",
        ),
        (
            "a.cljs",
            "(ns a (:require [macros :refer [one]])) (def s (one))",
        ),
        (
            "b.cljs",
            "(ns b (:require [macros :refer [many]])) (def xs (many))",
        ),
        (
            "main.cljs",
            "(ns main (:require [a] [b])) (js/console.log (pr-str a/s) (boolean (some #{a/s} b/xs)))",
        ),
    ]);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    assert!(run(&uncached(&roots, "main")).ends_with(" false\n"));
    let captured = capture(input, "main", &[]).unwrap();
    let printed = run(&captured.program);
    assert!(printed.ends_with(" false\n"), "{printed}");
    for hits in [&["a"][..], &["macros", "a"]] {
        let (program, _) = compile_mixed(input, "main", &captured, hits);
        assert_eq!(run(&program), printed, "hits {hits:?}");
    }
}

/// Names a macro derives from a gensym, such as munged JavaScript names,
/// stay apart: a fresh `gensym` numbers above every hit's, so `x-` and `x_`
/// never munge to one name, as in the uncached build. Spellings are kept as
/// UTF-16 code units, lone surrogates included.
#[test]
fn names_derived_from_gensyms_stay_apart_and_spellings_keep_their_code_units() {
    let sources = Sources::new(&[
        (
            "macros.cljs",
            "(ns macros) (defmacro mx [prefix] (str (munge (gensym prefix)))) (defmacro odd [] (list 'quote (gensym \"\\uD800\")))",
        ),
        (
            "a.cljs",
            "(ns a (:require [macros :refer [mx odd]])) (def s (mx \"x-\")) (def o (odd))",
        ),
        (
            "b.cljs",
            "(ns b (:require [macros :refer [mx]])) (def s (mx \"x_\"))",
        ),
        (
            "main.cljs",
            "(ns main (:require [a] [b])) (js/console.log a/s b/s (= a/s b/s) (.charCodeAt (name a/o) 0))",
        ),
    ]);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    assert!(run(&uncached(&roots, "main")).ends_with(" false 55296\n"));
    let captured = capture(input, "main", &[]).unwrap();
    let printed = run(&captured.program);
    assert!(
        captured.artifacts["a"]
            .names
            .gensyms
            .iter()
            .any(|name| name.first() == Some(&0xD800)),
        "{:?}",
        captured.artifacts["a"].names
    );
    let (program, _) = compile_mixed(input, "main", &captured, &["a"]);
    let mixed = run(&program);
    assert!(mixed.ends_with(" false 55296\n"), "{mixed}");
    assert_eq!(mixed.split(' ').next(), printed.split(' ').next());
}

/// Generated-name marks that run backward or out of range are refused with
/// the artifact, before they move an allocator, and left out of the early
/// reservation, so later allocations still work.
#[test]
fn invalid_generated_name_marks_are_refused() {
    let sources = Sources::new(&AUTO_NAMES);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let expected = run(&uncached(&roots, "main"));
    let captured = capture(input, "main", &[]).unwrap();
    for (corrupt, reason) in [
        (
            (|names: &mut light_cljs::namespace_artifact::GeneratedNames| {
                names.to.binding = u64::MAX
            }) as fn(&mut _),
            "its binding mark is out of range",
        ),
        (
            |names| names.from.syntax_quote = names.to.syntax_quote + 1,
            "its syntax-quote window runs backward",
        ),
        (
            |names| names.gensym_to = u64::MAX,
            "its gensym mark is out of range",
        ),
    ] {
        let mut artifact = captured.artifacts["p"].clone();
        corrupt(&mut artifact.names);
        let chosen = HashMap::from([("p", &artifact)]);
        let (program, stats) = compile_installing(input, "main", &chosen, true).unwrap();
        let install = stats.install.unwrap();
        assert_eq!(install.refused, [("p".to_owned(), reason.to_owned())]);
        assert!(generated_names::current().binding < 1 << 48);
        assert_eq!(run(&program), expected);
    }
}

/// Warnings in every namespace: a single-segment namespace's header warning,
/// undeclared Vars in bodies, in a macro's expansion and in a compile-time
/// helper, and calls across namespaces.
const WARNED: [(&str, &str); 4] = [
    ("util.cljs", "(ns util) (defn f [x] (+ x missing-a))"),
    (
        "lib/macros.cljs",
        "(ns lib.macros) (defn helper [] (when false missing-h)) (defmacro uses-missing [] (helper) 'missing-m)",
    ),
    (
        "lib/b.cljs",
        "(ns lib.b (:require [util :as u] [lib.macros :refer [uses-missing]])) (defn g [] (u/f 1 2)) (def y missing-b) (def z (uses-missing))",
    ),
    (
        "app/main.cljs",
        "(ns app.main (:require [lib.b :as b] [util :as u] [lib.macros :refer [uses-missing]])) (js/console.log (u/f 1 2 3) (uses-missing) missing-c)",
    ),
];

fn warning_mode() -> AnalysisOptions {
    AnalysisOptions {
        warning_mode: true,
        ..AnalysisOptions::default()
    }
}

/// Installed namespaces restore their warnings with their spans, and every
/// mix of hits and misses reports the uncached build's warnings, in its
/// order.
#[test]
fn warnings_of_installed_and_fresh_namespaces_keep_the_uncached_order() {
    let sources = Sources::new(&WARNED);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let options = warning_mode();
    let (program, expected) = input
        .analyze_with(
            "app.main",
            &mut MacroHost::new(),
            options,
            EntryRecorders::default(),
        )
        .unwrap();
    let kinds = expected
        .iter()
        .map(|warning| warning.kind)
        .collect::<BTreeSet<_>>();
    for kind in ["single-segment-namespace", "undeclared-var"] {
        assert!(kinds.contains(kind), "{kind} in {expected:#?}");
    }
    let warned = expected
        .iter()
        .map(|warning| warning.source_index)
        .collect::<BTreeSet<_>>();
    assert!(warned.len() >= 3, "{expected:#?}");
    let captured = capture_with(input, "app.main", &[], options).unwrap();
    assert_eq!(captured.artifacts.len(), 4);
    let b = WARNED[2].1;
    let at = |text: &str| {
        let start = b.find(text).unwrap() as u32;
        (start, start + text.len() as u32)
    };
    let recorded = captured.artifacts["lib.b"]
        .diagnostics
        .warnings
        .iter()
        .map(|warning| (warning.kind.as_str(), warning.span))
        .collect::<Vec<_>>();
    assert!(
        recorded.contains(&("undeclared-var", at("missing-b"))),
        "{recorded:?}"
    );
    let expected_output = none_output(&program);
    for hits in every_mix(&captured) {
        let (program, _, warnings) =
            compile_mixed_with(input, "app.main", &captured, &hits, options);
        assert_eq!(warnings, expected, "hits {hits:?}");
        assert_eq!(none_output(&program), expected_output, "hits {hits:?}");
    }
}

/// An artifact captured under `--warnings` is refused by a strict build,
/// which analyzes the namespace and fails as the uncached strict build
/// does; an artifact without warnings is refused all the same, by its
/// policy.
#[test]
fn an_artifact_captured_in_warning_mode_is_not_installed_in_a_strict_build() {
    let sources = Sources::new(&[
        ("lib/clean.cljs", "(ns lib.clean) (defn f [] 1)"),
        ("lib/a.cljs", "(ns lib.a) (defn f [] undeclared-thing)"),
        (
            "app/clean.cljs",
            "(ns app.clean (:require [lib.clean :as c])) (js/console.log (c/f))",
        ),
        (
            "app/main.cljs",
            "(ns app.main (:require [lib.a :as a])) (js/console.log (a/f))",
        ),
    ]);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let strict = AnalysisOptions::default();
    let captured = capture_with(input, "app.main", &[], warning_mode()).unwrap();
    let uncached = input
        .analyze_with(
            "app.main",
            &mut MacroHost::new(),
            strict,
            EntryRecorders::default(),
        )
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(uncached.contains("undeclared-thing"), "{uncached}");
    let hits = chosen(&captured, &["lib.a", "app.main"]);
    let mixed = compile_installing_with(
        input,
        "app.main",
        &hits,
        &mut MacroHost::new(),
        strict,
        None,
    )
    .map(|_| ())
    .unwrap_err();
    assert_eq!(mixed, uncached);
    // In warning mode the same artifacts install, with their warning.
    let (_, stats, warnings) = compile_installing_with(
        input,
        "app.main",
        &hits,
        &mut MacroHost::new(),
        warning_mode(),
        None,
    )
    .unwrap();
    assert_eq!(stats.install.unwrap().installed, ["lib.a", "app.main"]);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.message.contains("undeclared-thing")),
        "{warnings:?}"
    );
    // A clean namespace is refused by its policy alone.
    let captured = capture_with(input, "app.clean", &[], warning_mode()).unwrap();
    let hits = chosen(&captured, &["lib.clean"]);
    let (program, stats, _) = compile_installing_with(
        input,
        "app.clean",
        &hits,
        &mut MacroHost::new(),
        strict,
        None,
    )
    .unwrap();
    let install = stats.install.unwrap();
    assert_eq!(install.installed, Vec::<String>::new());
    assert_eq!(install.refused.len(), 1);
    assert!(
        install.refused[0].1.contains("warning_mode: true"),
        "{:?}",
        install.refused
    );
    assert_eq!(run(&program), "1\n");
}

/// Nested expansions of a macro whose helper is a compile-time unit.
const EXPANDED: [(&str, &str); 3] = [
    (
        "lib/macros.cljs",
        "(ns lib.macros) (defn wrap [x] (list 'inc x)) (defmacro twice [x] (list '* 2 (wrap x)))",
    ),
    (
        "lib/a.cljs",
        "(ns lib.a (:require [lib.macros :refer [twice]])) (def a (twice 1)) (def b (twice (twice 2))) (defn f [x] (when x (twice x)))",
    ),
    (
        "app/main.cljs",
        "(ns app.main (:require [lib.a :as a] [lib.macros :refer [twice]])) (js/console.log a/a a/b (a/f 3) (twice 5))",
    ),
];

/// The outcome of compiling `entry` with `limits`, installing `chosen`:
/// the output at `none` and the warnings, or the error.
fn limited(
    input: Input<'_>,
    entry: &str,
    chosen: &HashMap<&str, &NamespaceArtifact>,
    limits: light_cljs::MacroLimits,
) -> Result<String, String> {
    let mut host = MacroHost::new();
    host.limits = limits;
    compile_installing_with(
        input,
        entry,
        chosen,
        &mut host,
        AnalysisOptions::default(),
        None,
    )
    .map(|(program, _, warnings)| format!("{}\n{warnings:?}", none_output(&program)))
}

/// A macro whose expansion is a sequence of a lazy shape: exporting it
/// charges the node budget for each cell it walks, about twice the nodes
/// the expansion has.
const WIDE: [(&str, &str); 3] = [
    (
        "lib/macros.cljs",
        "(ns lib.macros) (defmacro wide [n] (cons 'do (repeat n 1)))",
    ),
    (
        "lib/a.cljs",
        "(ns lib.a (:require [lib.macros :refer [wide]])) (def a (wide 50))",
    ),
    (
        "app/main.cljs",
        "(ns app.main (:require [lib.a :as a] [lib.macros :refer [wide]])) (js/console.log a/a (wide 3))",
    ),
];

/// Lowering `--macro-expansions`, `--macro-nodes` or the expansion depth
/// below what cached namespaces recorded fails exactly as the uncached
/// build does: a hit the remaining budget cannot admit is bypassed and its
/// analysis fails, and an admitted hit charges its work once, so a later
/// miss fails where it fails uncached. Every mix of hits is tried at each
/// threshold the artifacts' accounting implies, and either side of it,
/// for nested expansions and for a wide one whose export costs more than
/// the nodes it generates.
#[test]
fn lowered_macro_limits_fail_as_the_uncached_build_does() {
    let sources = Sources::new(&EXPANDED);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let captured = check_lowered_limits(input, "4 14 8 12\n");
    let a = captured.artifacts["lib.a"].accounting.unwrap();
    assert!(a.expansions >= 4, "{a:?}");
    assert!(a.macro_depth >= 2, "{a:?}");
    let sources = Sources::new(&WIDE);
    let roots = sources.roots();
    let captured = check_lowered_limits(Input::Roots(&roots), "1 1\n");
    let a = captured.artifacts["lib.a"].accounting.unwrap();
    assert!(a.node_peak >= a.generated_nodes + 40, "{a:?}");
    let sources = Sources::new(&WIDE_LAZY);
    let roots = sources.roots();
    check_lowered_limits(Input::Roots(&roots), "1\n");
}

/// A lazy expansion whose realization, while it is exported, expands
/// another macro, which encodes its own forms in the middle of the outer
/// encoding.
const WIDE_LAZY: [(&str, &str); 4] = [
    (
        "lib/inner.cljs",
        "(ns lib.inner) (defmacro one [] (list 'do 1))",
    ),
    (
        "lib/macros.cljs",
        "(ns lib.macros (:require [lib.inner])) (defmacro wide [] (cons 'do (map (fn [_] (macroexpand '(lib.inner/one))) (range 200))))",
    ),
    (
        "lib/a.cljs",
        "(ns lib.a (:require [lib.macros :refer [wide]])) (def a (wide))",
    ),
    (
        "app/main.cljs",
        "(ns app.main (:require [lib.a :as a])) (js/console.log a/a)",
    ),
];

/// A core macro that does not read its environment gets none when the
/// environment would fit in half the remaining node budget, and the whole
/// environment otherwise, whose list-heavy namespace metadata then costs
/// its conversion more: a namespace needs the budget that elision did.
#[test]
fn elided_list_environments_preserve_node_limits() {
    let zeros = vec!["0"; 1000].join(" ");
    let a = format!("(ns lib.a {{:padding ({zeros})}}) (def a (when true 1))");
    let files = [
        ("lib/b.cljs", "(ns lib.b) (def b 2)"),
        ("lib/a.cljs", a.as_str()),
        (
            "app/main.cljs",
            "(ns app.main (:require [lib.b :as b] [lib.a :as a])) (js/console.log a/a b/b)",
        ),
    ];
    let sources = Sources::new(&files);
    let roots = sources.roots();
    let captured = check_lowered_limits(Input::Roots(&roots), "1 2\n");
    let a = captured.artifacts["lib.a"].accounting.unwrap();
    assert!(a.node_peak >= 2000, "{a:?}");
}

/// The analysis options with each static and runtime array-check mode.
fn array_policies() -> Vec<AnalysisOptions> {
    let mut policies = Vec::new();
    for checked_arrays in [false, true] {
        for array_checks in [
            CheckedArrays::Off,
            CheckedArrays::Warn,
            CheckedArrays::Error,
        ] {
            policies.push(AnalysisOptions {
                checked_arrays,
                array_checks,
                ..AnalysisOptions::default()
            });
        }
    }
    policies
}

/// The array-check modes change the IR and the warnings, so an artifact
/// captured under one is refused, before anything changes, in a build
/// under another, which then compiles as uncached; under its own it
/// installs and compiles as uncached too.
#[test]
fn array_analysis_policies_are_not_interchangeable() {
    let sources = Sources::new(&[
        (
            "lib/a.cljs",
            "(ns lib.a) (defn f [] (aget #js {} \"a\")) (defn g [x] (aget x 0))",
        ),
        (
            "app/main.cljs",
            "(ns app.main (:require [lib.a :as a])) (js/console.log (a/g #js [7]))",
        ),
    ]);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let policies = array_policies();
    let none = HashMap::new();
    let outcome = |chosen: &HashMap<&str, &NamespaceArtifact>, options| {
        compile_installing_with(
            input,
            "app.main",
            chosen,
            &mut MacroHost::new(),
            options,
            None,
        )
        .map(|(program, stats, warnings)| (outputs(&program), warnings, stats.install.unwrap()))
    };
    let uncached = policies
        .iter()
        .map(|&options| {
            let (outputs, warnings, _) = outcome(&none, options).unwrap();
            (outputs, warnings)
        })
        .collect::<Vec<_>>();
    // The modes change the output or the warnings: each runtime mode, and
    // the static diagnostics without one (a runtime mode enables them too).
    let distinct = [0, 1, 2, 3].map(|i| &uncached[i]);
    for (i, a) in distinct.iter().enumerate() {
        for b in &distinct[i + 1..] {
            assert_ne!(a, b);
        }
    }
    for &captured_under in &policies {
        let captured = capture_with(input, "app.main", &[], captured_under).unwrap();
        let hits = chosen(&captured, &["lib.a", "app.main"]);
        assert_eq!(hits.len(), 2);
        for (&options, expected) in policies.iter().zip(&uncached) {
            let (outputs, warnings, install) = outcome(&hits, options).unwrap();
            assert_eq!(
                &(outputs, warnings),
                expected,
                "{captured_under:?} in {options:?}"
            );
            if options == captured_under {
                assert_eq!(install.installed, ["lib.a", "app.main"]);
                assert!(install.refused.is_empty());
            } else {
                assert!(
                    install.installed.is_empty(),
                    "{captured_under:?} in {options:?}"
                );
                assert_eq!(install.refused.len(), 2);
                assert!(
                    install
                        .refused
                        .iter()
                        .all(|(_, reason)| reason.starts_with("it was compiled under")),
                    "{:?}",
                    install.refused
                );
            }
        }
    }
}

/// [`lowered_macro_limits_fail_as_the_uncached_build_does`] for the
/// fixture of `input`, whose uncached build prints `printed`: the captured
/// artifacts.
fn check_lowered_limits(input: Input<'_>, printed: &str) -> Captured {
    let none = HashMap::new();
    let defaults = light_cljs::MacroLimits::default();
    let fresh = limited(input, "app.main", &none, defaults).unwrap();
    assert_eq!(execute(fresh.rsplit_once('\n').unwrap().0), printed);
    let captured = capture(input, "app.main", &[]).unwrap();
    assert_eq!(captured.eligible.len(), captured.namespaces.len());
    assert_eq!(captured.artifacts.len(), captured.namespaces.len());
    let accounting = |name: &str| captured.artifacts[name].accounting.unwrap();
    let a = accounting("lib.a");
    assert!(
        a.node_peak >= a.generated_nodes && a.generated_nodes > 0,
        "{a:?}"
    );
    // Each namespace's recorded need on top of what the namespaces before
    // it used, and one either side.
    let mut expansions = BTreeSet::from([0, 1]);
    let mut nodes = BTreeSet::from([0, 1]);
    let (mut expansions_before, mut nodes_before) = (0, 0);
    for name in &captured.namespaces {
        let work = accounting(name);
        let need = expansions_before + work.expansions;
        expansions.extend([need.saturating_sub(1), need, need + 1]);
        for need in [
            nodes_before + work.node_peak,
            nodes_before + work.generated_nodes,
        ] {
            nodes.extend([need.saturating_sub(1), need, need + 1]);
        }
        expansions_before += work.expansions;
        nodes_before += work.generated_nodes;
    }
    let depth = captured
        .artifacts
        .values()
        .map(|artifact| artifact.accounting.unwrap().macro_depth)
        .max()
        .unwrap() as usize;
    let mut limits = Vec::new();
    limits.extend(expansions.iter().map(|&n| light_cljs::MacroLimits {
        expansions: n as usize,
        ..defaults
    }));
    limits.extend(nodes.iter().map(|&n| light_cljs::MacroLimits {
        generated_nodes: n as usize,
        ..defaults
    }));
    limits.extend((0..=depth + 1).map(|depth| light_cljs::MacroLimits {
        expansion_depth: depth,
        ..defaults
    }));
    let mut failed = 0;
    for limits in limits {
        let expected = limited(input, "app.main", &none, limits);
        failed += usize::from(expected.is_err());
        for hits in every_mix(&captured) {
            let chosen = chosen(&captured, &hits);
            assert_eq!(
                limited(input, "app.main", &chosen, limits),
                expected,
                "{limits:?} with hits {hits:?}"
            );
        }
    }
    assert!(failed > 3, "{failed}");
    // An artifact without accounting is refused before installation.
    let mut unaccounted = captured.artifacts["lib.a"].clone();
    unaccounted.accounting = None;
    let chosen = HashMap::from([("lib.a", &unaccounted)]);
    let (_, stats, _) = compile_installing_with(
        input,
        "app.main",
        &chosen,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        stats.install.unwrap().refused,
        [(
            "lib.a".to_owned(),
            "it records no accounting of its work".to_owned()
        )]
    );
    captured
}

/// A compiler data reader's input and result each take at most the node
/// limit itself, not what is left of it, so a namespace whose reader took
/// more nodes than a lowered `--macro-nodes` is bypassed and fails as the
/// uncached build does, though the nodes it generated would fit.
#[test]
fn a_data_readers_conversions_need_the_node_limit_they_took() {
    let items = (0..60).map(|i| i.to_string()).collect::<Vec<_>>();
    let a = format!("(ns lib.a) (def a #test/count [{}])", items.join(" "));
    let files = [
        ("lib/a.cljs", a.as_str()),
        (
            "app/main.cljs",
            "(ns app.main (:require [lib.a :as a])) (js/console.log a/a)",
        ),
    ];
    let sources = Sources::new(&files);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let host = |limits| {
        let mut host = MacroHost::new();
        host.register_compiler_data_reader(
            light_cljs::compiler_data::Name::new(Some("test"), "count"),
            "(fn [v] (count v))",
        )
        .unwrap();
        host.limits = limits;
        host
    };
    let defaults = light_cljs::MacroLimits::default();
    let captured = capture_in(
        input,
        "app.main",
        &[],
        AnalysisOptions::default(),
        &mut host(defaults),
    )
    .unwrap();
    assert!(captured.eligible.contains(&"lib.a".to_owned()));
    let a = captured.artifacts["lib.a"].accounting.unwrap();
    assert!(a.conversion_peak > 60 && a.node_peak < 60, "{a:?}");
    let mut failed = 0;
    for nodes in [a.conversion_peak - 1, a.conversion_peak] {
        let limits = light_cljs::MacroLimits {
            generated_nodes: nodes as usize,
            ..defaults
        };
        let outcome = |chosen: &HashMap<&str, &NamespaceArtifact>| {
            compile_installing_with(
                input,
                "app.main",
                chosen,
                &mut host(limits),
                AnalysisOptions::default(),
                None,
            )
            .map(|(program, stats, _)| (none_output(&program), stats.install.unwrap()))
        };
        let uncached = outcome(&HashMap::new());
        let cached = outcome(&chosen(&captured, &["lib.a"]));
        match uncached {
            Err(error) => {
                failed += 1;
                assert_eq!(cached.unwrap_err(), error);
            }
            Ok((output, _)) => {
                let (cached_output, install) = cached.unwrap();
                assert_eq!(cached_output, output);
                assert_eq!(install.installed, ["lib.a"]);
                assert_eq!(execute(&output), "60\n");
            }
        }
    }
    assert_eq!(failed, 1);
}

/// Cancelling the compilation while a namespace installs stops it there:
/// nothing after it is installed, or analyzed in its place, and the
/// cancelled compilation captures no further artifact. The cancellation
/// arrives after each of the checks installation makes in turn, those
/// while it converts and declares the namespace included, until it arrives
/// only once the namespace is installed.
#[test]
fn cancellation_during_installation_stops_it() {
    let sources = Sources::new(&EXPANDED);
    let roots = sources.roots();
    let input = Input::Roots(&roots);
    let captured = capture(input, "app.main", &[]).unwrap();
    let reservation = generated_names::Reservation::of(
        captured.artifacts.values().map(|artifact| &artifact.names),
    );
    let mut stopped = 0;
    for passed in 0.. {
        let control = light_cljs::evaluator::Evaluator::new(Default::default())
            .unwrap()
            .control();
        let asked = std::cell::RefCell::new(Vec::new());
        let lookup = |name: &str| {
            asked.borrow_mut().push(name.to_owned());
            if name == "lib.a" {
                control.cancel_after_checks(passed);
            }
            (name != "lib.macros").then(|| &captured.artifacts[name])
        };
        let mut artifacts = Vec::new();
        let mut sink = |capture: light_cljs::namespace_artifact::NamespaceCapture| {
            artifacts.push((capture.namespace, capture.artifact.is_ok()));
        };
        let error = input
            .analyze(
                "app.main",
                &mut MacroHost::new(),
                EntryRecorders {
                    install: Some(&lookup),
                    reserve: Some(&reservation),
                    capture: Some(&mut sink),
                    control: Some(&control),
                    ..EntryRecorders::default()
                },
            )
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert_eq!(artifacts, [("lib.macros".to_owned(), true)], "{passed}");
        assert_eq!(*asked.borrow(), ["lib.macros", "lib.a"], "{passed}");
        if error.contains("installing lib.a: JavaScript session cancelled") {
            stopped += 1;
            continue;
        }
        // Past installation's checks, the compilation stops at the next
        // namespace, with lib.a installed: neither failed nor analyzed.
        assert!(
            error.contains("compiling app.main: JavaScript session cancelled"),
            "{passed}: {error}"
        );
        break;
    }
    // Before it begins, while it converts, and before and after it declares.
    assert!(stopped >= 4, "{stopped}");
    let control = light_cljs::evaluator::Evaluator::new(Default::default())
        .unwrap()
        .control();
    control.cancel();
    // A compilation cancelled before it begins stops at its first namespace.
    let mut artifacts = Vec::new();
    let mut sink = |capture: light_cljs::namespace_artifact::NamespaceCapture| {
        artifacts.push(capture.namespace);
    };
    let error = input
        .analyze(
            "app.main",
            &mut MacroHost::new(),
            EntryRecorders {
                capture: Some(&mut sink),
                control: Some(&control),
                ..EntryRecorders::default()
            },
        )
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("compiling lib.macros: JavaScript session cancelled"),
        "{error}"
    );
    assert!(artifacts.is_empty());
}
