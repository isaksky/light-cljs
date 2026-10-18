//! Namespace cache keys (docs/compiler-cache.md, "Conservative keys and
//! invalidation"): each input of a namespace's front-end key changes it and
//! names itself as the first mismatch, an edit outside the closure or an
//! emission option changes no front-end key, and re-validation after a
//! compilation notices inputs that changed meanwhile.
use crate::temp_dir::TempDir;
use light_cljs::MacroHost;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::application::{ApplicationOptions, OptimizationLevel};
use light_cljs::cache_key::{
    self, EmissionOptions, FrontEndKey, KeyEnvironment, Mismatch, NamespaceKeys, OutputLayout,
    SourceMapOutput,
};
use light_cljs::compiler::{SourceFiles, analyze_namespace_entry_with_dependencies};
use light_cljs::compiler_data::Name;
use light_cljs::dependency_manifest::{DependencyManifest, NamespaceId};
use light_cljs::source_provider::FileSourceProvider;
use std::{fs, path::PathBuf};

struct Project(Vec<TempDir>);

/// One build's manifest, sources and keys.
struct Build {
    manifest: DependencyManifest,
    files: SourceFiles,
    keys: NamespaceKeys,
}

impl Build {
    fn key(&self, name: &str) -> &FrontEndKey {
        self.keys.key(self.id(name))
    }
    fn id(&self, name: &str) -> NamespaceId {
        self.manifest
            .id(name)
            .unwrap_or_else(|| panic!("{name} is not in the build"))
    }
    fn names(&self) -> Vec<String> {
        self.manifest
            .namespaces
            .iter()
            .map(|record| record.name.clone())
            .collect()
    }
    /// The first mismatch of each namespace's key in `current` against
    /// this build's, by name.
    fn mismatches(&self, current: &Build) -> Vec<(String, Option<Mismatch>)> {
        current
            .names()
            .into_iter()
            .map(|name| {
                let mismatch = self.key(&name).mismatch(current.key(&name));
                (name, mismatch)
            })
            .collect()
    }
    /// The keys again, from the manifest as it now stands.
    fn rekey(&mut self, environment: &KeyEnvironment) {
        self.keys = NamespaceKeys::compute(environment, &self.manifest, &self.files, ENTRY);
    }
}

const ENTRY: &str = "app.main";

impl Project {
    /// `app.main` requires `app.a`, which nothing else needs, then `app.b`,
    /// which expands `lib.macros/loud`, whose expansion runs the helper
    /// `lib.helpers/shout` and reads `lib/suffix.txt`, then `app.util`.
    /// `app.other` is in the roots but outside the closure.
    fn new() -> Self {
        let project = Self(vec![TempDir::new("cache-keys"), TempDir::new("cache-keys")]);
        project.write(0, "app/a.cljs", "(ns app.a) (def a 1)");
        project.write(
            0,
            "lib/helpers.cljs",
            r#"(ns lib.helpers) (defn shout [s] (str s "!"))"#,
        );
        project.write(
            0,
            "lib/macros.cljs",
            r#"(ns lib.macros (:require [lib.helpers :as h] [light.build :as b]))
(defmacro loud [s] (str (h/shout s) (b/resource "lib/suffix.txt")))"#,
        );
        project.write(1, "lib/suffix.txt", "?");
        project.write(
            0,
            "app/b.cljs",
            r#"(ns app.b (:require [lib.macros :refer [loud]])) (def b (loud "hi"))"#,
        );
        project.write(0, "app/util.cljs", "(ns app.util) (defn twice [x] (* 2 x))");
        project.write(
            0,
            "app/main.cljs",
            r#"(ns app.main (:require [app.a :as a] [app.b :as b] [app.util :as u]))
(str a/a b/b (u/twice 2))"#,
        );
        project.write(0, "app/other.cljs", "(ns app.other) (def other 1)");
        project
    }
    fn write(&self, root: usize, path: &str, text: &str) {
        let path = self.0[root].join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn path(&self, root: usize, path: &str) -> PathBuf {
        self.0[root].join(path)
    }
    fn roots(&self) -> Vec<PathBuf> {
        self.0.iter().map(|root| root.path().clone()).collect()
    }
    fn provider(&self) -> FileSourceProvider {
        FileSourceProvider::new(
            self.roots()
                .iter()
                .map(|root| fs::canonicalize(root).unwrap())
                .collect(),
        )
    }
    fn build_with(&self, host: &mut MacroHost, options: AnalysisOptions) -> Build {
        let environment = KeyEnvironment::new(host, options);
        let (_, files, manifest) = analyze_namespace_entry_with_dependencies(
            ENTRY,
            &self.roots(),
            host,
            options,
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("the build failed: {error}"));
        let keys = NamespaceKeys::compute(&environment, &manifest, &files, ENTRY);
        Build {
            manifest,
            files,
            keys,
        }
    }
    fn build(&self) -> Build {
        self.build_with(&mut MacroHost::new(), AnalysisOptions::default())
    }
}

fn only(mismatches: &[(String, Option<Mismatch>)]) -> Vec<(&str, String)> {
    mismatches
        .iter()
        .filter_map(|(name, mismatch)| Some((name.as_str(), mismatch.as_ref()?.to_string())))
        .collect()
}

#[test]
fn keys_are_stable_and_follow_source_order() {
    let project = Project::new();
    let base = project.build();
    assert_eq!(
        base.names(),
        [
            "app.a",
            "lib.helpers",
            "light.build",
            "lib.macros",
            "app.b",
            "app.util",
            "app.main"
        ]
    );
    // A fresh process would compute the same keys: nothing but content and
    // configuration goes in.
    let again = project.build();
    assert!(only(&base.mismatches(&again)).is_empty());
    let digests: Vec<_> = base
        .names()
        .iter()
        .map(|name| base.key(name).digest)
        .collect();
    let mut distinct = digests.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        digests.len(),
        "each namespace has its own key"
    );
    // The source-macro user is coarse, and its dependency list leaves the
    // coarse edges to the view.
    assert!(base.manifest.record(base.id("app.b")).coarse);
    let dependencies: Vec<_> = base
        .key("app.b")
        .dependencies
        .iter()
        .map(|dependency| dependency.namespace.as_str())
        .collect();
    assert_eq!(dependencies, ["lib.helpers", "light.build", "lib.macros"]);
}

#[test]
fn a_source_edit_invalidates_the_namespace_and_its_consumers() {
    let project = Project::new();
    let base = project.build();
    project.write(0, "app/util.cljs", "(ns app.util) (defn twice [x] (+ x x))");
    assert_eq!(
        only(&base.mismatches(&project.build())),
        [
            ("app.util", "source changed".into()),
            ("app.main", "dependency app.util changed".into()),
        ]
    );
}

#[test]
fn a_helper_implementation_edit_invalidates_every_consumer() {
    let project = Project::new();
    let base = project.build();
    // The same signature, another implementation.
    project.write(
        0,
        "lib/helpers.cljs",
        r#"(ns lib.helpers) (defn shout [s] (str s "!!"))"#,
    );
    let edited = project.build();
    let helper = base.id("lib.helpers");
    let consumers = base.manifest.reverse_closure(helper);
    assert_eq!(
        consumers
            .iter()
            .map(|&id| base.manifest.record(id).name.as_str())
            .collect::<Vec<_>>(),
        ["lib.macros", "app.b", "app.main"]
    );
    for &id in &consumers {
        let name = &base.manifest.record(id).name;
        assert_ne!(base.keys.key(id).digest, edited.key(name).digest, "{name}");
    }
    assert_eq!(
        only(&base.mismatches(&edited)),
        [
            ("lib.helpers", "source changed".into()),
            ("lib.macros", "dependency lib.helpers changed".into()),
            ("app.b", "dependency lib.helpers changed".into()),
            ("app.main", "dependency app.b changed".into()),
        ]
    );
}

#[test]
fn an_earlier_namespace_changes_only_a_coarse_view() {
    let project = Project::new();
    let base = project.build();
    // app.a is no dependency of app.b, but it loads first, and app.b ran a
    // source macro, which sees every earlier namespace's declarations.
    project.write(0, "app/a.cljs", "(ns app.a) (def a 2)");
    assert_eq!(
        only(&base.mismatches(&project.build())),
        [
            ("app.a", "source changed".into()),
            ("app.b", "earlier namespaces changed".into()),
            ("app.main", "dependency app.a changed".into()),
        ]
    );
}

#[test]
fn a_resource_change_and_its_appearance_change_the_reading_key() {
    let project = Project::new();
    let base = project.build();
    project.write(1, "lib/suffix.txt", "?!");
    let edited = project.build();
    assert_eq!(
        only(&base.mismatches(&edited)),
        [
            ("app.b", "external input changed".into()),
            ("app.main", "dependency app.b changed".into()),
        ]
    );
    // A copy in the other root makes the path ambiguous, and the build fails;
    // removing it again restores the keys.
    project.write(0, "lib/suffix.txt", "?!");
    let mut host = MacroHost::new();
    assert!(
        analyze_namespace_entry_with_dependencies(
            ENTRY,
            &project.roots(),
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new()
        )
        .is_err()
    );
    fs::remove_file(project.path(0, "lib/suffix.txt")).unwrap();
    assert!(only(&edited.mismatches(&project.build())).is_empty());
}

#[test]
fn an_edit_outside_the_closure_changes_no_key() {
    let project = Project::new();
    let base = project.build();
    project.write(0, "app/other.cljs", "(ns app.other) (def other 2)");
    project.write(0, "app/unrelated.txt", "text no macro reads");
    assert!(only(&base.mismatches(&project.build())).is_empty());
}

#[test]
fn each_analysis_option_changes_every_key() {
    let project = Project::new();
    let base = project.build();
    let every = |build: &Build, reason: &str| {
        let mismatches = base.mismatches(build);
        assert_eq!(mismatches.len(), base.names().len());
        for (name, mismatch) in mismatches {
            assert_eq!(
                mismatch.map(|m| m.to_string()).as_deref(),
                Some(reason),
                "{name}"
            );
        }
    };
    let warnings = AnalysisOptions {
        warning_mode: true,
        ..AnalysisOptions::default()
    };
    every(
        &project.build_with(&mut MacroHost::new(), warnings),
        "analysis options changed",
    );
    let mut host = MacroHost::new();
    host.set_assertions(false);
    every(
        &project.build_with(&mut host, AnalysisOptions::default()),
        "analysis options changed",
    );
    let mut host = MacroHost::new();
    host.opaque_packages = true;
    every(
        &project.build_with(&mut host, AnalysisOptions::default()),
        "analysis options changed",
    );
    let mut host = MacroHost::new();
    host.limits.expansions += 1;
    every(
        &project.build_with(&mut host, AnalysisOptions::default()),
        "analysis options changed",
    );
    let mut host = MacroHost::new();
    host.register_compiler_data_reader(Name::new(Some("app"), "upper"), "(fn [s] s)")
        .unwrap();
    every(
        &project.build_with(&mut host, AnalysisOptions::default()),
        "analysis options changed",
    );
    let mut host = MacroHost::new();
    host.register_compiler_default_data_reader("(fn [tag value] value)")
        .unwrap();
    every(
        &project.build_with(&mut host, AnalysisOptions::default()),
        "analysis options changed",
    );
    for options in [
        AnalysisOptions {
            checked_arrays: true,
            ..AnalysisOptions::default()
        },
        AnalysisOptions {
            array_checks: light_cljs::analyzer::CheckedArrays::Warn,
            ..AnalysisOptions::default()
        },
    ] {
        every(
            &project.build_with(&mut MacroHost::new(), options),
            "analysis options changed",
        );
    }
    let hosts: [fn(&mut MacroHost); 4] = [
        |host| host.limits.generated_nodes += 1,
        |host| host.limits.expansion_depth += 1,
        // A macro can catch a stack overflow and return normally.
        |host| host.evaluator_options.stack_limit *= 2,
        |host| host.evaluator_options.memory_limit *= 2,
    ];
    for change in hosts {
        let mut host = MacroHost::new();
        change(&mut host);
        every(
            &project.build_with(&mut host, AnalysisOptions::default()),
            "analysis options changed",
        );
    }
    // Another implementation of a registered reader.
    let mut replaced = MacroHost::new();
    replaced
        .register_compiler_data_reader(Name::new(Some("app"), "upper"), "(fn [s] s)")
        .unwrap();
    let mut first = MacroHost::new();
    first
        .register_compiler_data_reader(Name::new(Some("app"), "upper"), "(fn [s] s)")
        .unwrap();
    let first = project.build_with(&mut first, AnalysisOptions::default());
    replaced
        .register_compiler_data_reader(Name::new(Some("app"), "upper"), "(fn [s] [s])")
        .unwrap();
    assert!(
        first
            .mismatches(&project.build_with(&mut replaced, AnalysisOptions::default()))
            .iter()
            .all(|(_, mismatch)| mismatch == &Some(Mismatch::Options))
    );
    // The deadline and poll budget terminate work, which is never stored.
    let mut host = MacroHost::new();
    host.evaluator_options.timeout *= 2;
    host.evaluator_options.poll_budget *= 2;
    assert!(
        only(&base.mismatches(&project.build_with(&mut host, AnalysisOptions::default())))
            .is_empty()
    );

    // The configuration and the compiler.
    let environment = KeyEnvironment::new(&MacroHost::new(), AnalysisOptions::default());
    let mut configured = project.build();
    configured.rekey(&environment.with_config([3; 32]));
    every(&configured, "config changed");
    let mut other = project.build();
    other.rekey(&KeyEnvironment {
        compiler: [4; 32],
        ..environment
    });
    every(&other, "compiler changed");
    assert_eq!(environment.config, cache_key::empty_config());
}

#[test]
fn resolution_and_view_records_change_the_key() {
    let project = Project::new();
    let base = project.build();
    let environment = KeyEnvironment::new(&MacroHost::new(), AnalysisOptions::default());
    // A candidate that appeared would shadow the file found.
    let mut shadowed = project.build();
    let util = shadowed.id("app.util");
    let lookup = shadowed.manifest.record(util).source.lookup;
    shadowed.manifest.lookups[lookup.0 as usize].candidates[1].present = true;
    shadowed.rekey(&environment);
    assert_eq!(
        only(&base.mismatches(&shadowed)),
        [
            ("app.util", "resolution changed".into()),
            ("app.main", "dependency app.util changed".into()),
        ]
    );
    // A require resolved another way.
    let mut rewritten = project.build();
    let main = rewritten.id("app.main");
    rewritten.manifest.namespaces[main.0 as usize].requires[0].written = "clojure.a".into();
    rewritten.rekey(&environment);
    assert_eq!(
        only(&base.mismatches(&rewritten)),
        [("app.main", "resolution changed".into())]
    );
    // A name looked for and not found, which an earlier namespace now has.
    let mut absent = project.build();
    absent.manifest.namespaces[util.0 as usize]
        .absent
        .push("app.nowhere".into());
    absent.rekey(&environment);
    assert_eq!(
        only(&base.mismatches(&absent)),
        [
            ("app.util", "earlier namespaces changed".into()),
            ("app.main", "dependency app.util changed".into()),
        ]
    );
}

#[test]
fn emission_options_change_only_the_emission_key() {
    let project = Project::new();
    let base = project.build();
    let environment = KeyEnvironment::new(&MacroHost::new(), AnalysisOptions::default());
    let key = base.key("app.main");
    let application = ApplicationOptions::new(OptimizationLevel::None);
    let plain = EmissionOptions {
        application: &application,
        layout: OutputLayout::Script,
        source_map: SourceMapOutput::None,
        include_sources: false,
        modules: None,
    };
    let emission =
        |options: &EmissionOptions<'_>| cache_key::emission_key(&environment, key, options);
    let mut keys = vec![emission(&plain)];
    let advanced = ApplicationOptions::new(OptimizationLevel::Advanced);
    let mut pseudo = ApplicationOptions::new(OptimizationLevel::None);
    pseudo.pseudo_names = true;
    let mut defined = ApplicationOptions::new(OptimizationLevel::None);
    defined.defines.push(light_cljs::defines::Define {
        name: "app.main/DEBUG".into(),
        value: light_cljs::defines::DefineValue::Boolean(true),
    });
    let mut other_value = defined.clone();
    other_value.defines[0].value = light_cljs::defines::DefineValue::Boolean(false);
    let mut exported = ApplicationOptions::new(OptimizationLevel::None);
    exported.exports.push(light_cljs::analyzer::GlobalId(0));
    let mut no_inlining = ApplicationOptions::new(OptimizationLevel::None);
    no_inlining.no_inlining = true;
    let mut no_folding = ApplicationOptions::new(OptimizationLevel::None);
    no_folding.no_folding = true;
    for application in [
        &advanced,
        &pseudo,
        &defined,
        &other_value,
        &exported,
        &no_inlining,
        &no_folding,
    ] {
        keys.push(emission(&EmissionOptions {
            application,
            ..plain.clone()
        }));
    }
    for layout in [
        OutputLayout::Module,
        OutputLayout::Chunks,
        OutputLayout::Reloadable,
    ] {
        keys.push(emission(&EmissionOptions {
            layout,
            ..plain.clone()
        }));
    }
    for source_map in [
        SourceMapOutput::File("out.js.map".into()),
        SourceMapOutput::Inline,
    ] {
        keys.push(emission(&EmissionOptions {
            source_map,
            ..plain.clone()
        }));
    }
    keys.push(emission(&EmissionOptions {
        include_sources: true,
        ..plain.clone()
    }));
    for modules in [[1; 32], [2; 32]] {
        keys.push(emission(&EmissionOptions {
            layout: OutputLayout::Chunks,
            modules: Some(modules),
            ..plain.clone()
        }));
    }
    keys.push(cache_key::emission_key(
        &environment,
        base.key("app.util"),
        &plain,
    ));
    let count = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(
        keys.len(),
        count,
        "each emission option changes the emission key"
    );
    // The front-end key takes no emission option at all: the same build
    // keys the same under every one of them.
    assert_eq!(base.key("app.main"), project.build().key("app.main"));
}

#[test]
fn revalidation_notices_inputs_that_changed_after_reading() {
    let project = Project::new();
    let build = project.build();
    let provider = project.provider();
    let check = |name: &str| {
        let id = build.id(name);
        cache_key::revalidate(
            &build.manifest,
            id,
            &build.files[id.0 as usize].1,
            &provider,
        )
    };
    for name in build.names() {
        assert_eq!(check(&name), Ok(()), "{name}");
    }
    project.write(0, "app/util.cljs", "(ns app.util) (defn twice [x] (+ x x))");
    assert_eq!(check("app.util"), Err(Mismatch::Source));
    assert_eq!(check("app.main"), Ok(()));
    project.write(1, "lib/suffix.txt", "changed");
    assert_eq!(check("app.b"), Err(Mismatch::Inputs));
    assert_eq!(check("lib.macros"), Ok(()));
    // A file that would shadow app.a: its lookup would now go elsewhere.
    project.write(1, "app/a.cljs", "(ns app.a) (def a 1)");
    assert_eq!(check("app.a"), Err(Mismatch::Resolution));
}

/// Answers by the namespace that asks: `app.dep` has one text when
/// `app.main` requires it and another for anyone else.
struct Routed {
    from_main: std::cell::RefCell<String>,
}

impl light_cljs::source_provider::SourceProvider for Routed {
    fn load(
        &self,
        request: &light_cljs::source_provider::LoadRequest,
    ) -> Result<
        Option<light_cljs::source_provider::SourceInput>,
        light_cljs::source_provider::LoadError,
    > {
        let text = |name: &[u16]| String::from_utf16_lossy(name);
        let source = match (
            text(request.namespace.name()).as_str(),
            text(request.requesting_namespace.name()).as_str(),
        ) {
            ("app.main", _) => "(ns app.main (:require [app.dep :as d])) d/value".to_owned(),
            ("app.dep", "app.main") => self.from_main.borrow().clone(),
            ("app.dep", _) => "(ns app.dep) (def value :elsewhere)".to_owned(),
            _ => return Ok(None),
        };
        let file = format!("{}.cljs", text(request.namespace.name()));
        Ok(Some(light_cljs::source_provider::SourceInput::new(
            file, source,
        )))
    }
}

#[test]
fn revalidation_asks_as_the_loader_asked() {
    let provider = Routed {
        from_main: std::cell::RefCell::new("(ns app.dep) (def value :main)".into()),
    };
    let mut manifest = DependencyManifest::default();
    let (_, files) = light_cljs::compiler::analyze_namespace_entry_recorded_with_provider(
        ENTRY,
        &provider,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
        light_cljs::compiler::EntryRecorders {
            manifest: Some(&mut manifest),
            ..Default::default()
        },
    )
    .unwrap();
    let dep = manifest.id("app.dep").unwrap();
    let lookup = manifest.lookup(manifest.record(dep).source.lookup);
    assert_eq!(lookup.requester, "app.main");
    let check = || cache_key::revalidate(&manifest, dep, &files[dep.0 as usize].1, &provider);
    assert_eq!(check(), Ok(()));
    // Only the route the loader took changed.
    *provider.from_main.borrow_mut() = "(ns app.dep) (def value :changed)".into();
    assert_eq!(check(), Err(Mismatch::Source));

    // Who asked is part of the resolution key.
    let environment = KeyEnvironment::new(&MacroHost::new(), AnalysisOptions::default());
    let before = NamespaceKeys::compute(&environment, &manifest, &files, ENTRY);
    let mut moved = manifest.clone();
    let id = moved.record(dep).source.lookup;
    moved.lookups[id.0 as usize].requester = "app.other".into();
    let after = NamespaceKeys::compute(&environment, &moved, &files, ENTRY);
    assert_eq!(
        before.key(dep).mismatch(after.key(dep)),
        Some(Mismatch::Resolution)
    );
}
