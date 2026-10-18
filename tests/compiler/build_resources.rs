//! `light.build`: compile-time resource reads and declared dependencies,
//! from macros, helpers and data readers, each recorded in the dependency
//! manifest of every namespace whose compilation used it (docs/compiler.md,
//! "Compile-time services: light.build"). The default build runs these
//! macros in V8 and the QuickJS-only build in QuickJS; both run this suite.
use crate::temp_dir::TempDir;
use light_cljs::cache_policy::{self, Bypass, CacheSettings};
use light_cljs::compiler::{
    EntryRecorders, analyze_namespace_entry_recorded_with_provider,
    analyze_namespace_entry_with_dependencies,
};
use light_cljs::compiler_data::Name;
use light_cljs::dependency_manifest::{
    Candidate, DependencyManifest, EdgeKinds, ExternalInput, NamespaceId, ResourceDependency,
};
use light_cljs::source_provider::{
    LoadError, LoadRequest, ResourceInput, ResourceProvider, SourceInput, SourceProvider,
};
use light_cljs::{MacroHost, analyzer::AnalysisOptions};
use std::{fs, path::PathBuf, process::Command, rc::Rc};

struct Roots(Vec<TempDir>);

impl Roots {
    fn new(count: usize) -> Self {
        Self((0..count).map(|_| TempDir::new("cljs-build")).collect())
    }
    fn write(&self, root: usize, path: &str, source: &str) {
        let path = self.0[root].join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn path(&self, root: usize, path: &str) -> PathBuf {
        fs::canonicalize(self.0[root].path()).unwrap().join(path)
    }
    fn paths(&self) -> Vec<PathBuf> {
        self.0.iter().map(|root| root.path().clone()).collect()
    }
    fn build_with(
        &self,
        entry: &str,
        host: &mut MacroHost,
    ) -> Result<(String, DependencyManifest), String> {
        let (program, _, manifest) = analyze_namespace_entry_with_dependencies(
            entry,
            &self.paths(),
            host,
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .map_err(|error| error.to_string())?;
        Ok((light_cljs::emit_js(&program), manifest))
    }
    fn build(&self, entry: &str) -> Result<(String, DependencyManifest), String> {
        let (js, manifest) = self.build_with(entry, &mut MacroHost::new())?;
        // Recording a manifest leaves the output as it is without one.
        let plain = light_cljs::compiler::analyze_namespace_entry(
            entry,
            &self.paths(),
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(js, light_cljs::emit_js(&plain));
        Ok((js, manifest))
    }
}

fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", &format!("console.log({js});")])
        .output()
        .expect("compiler integration requires Node");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

fn id(manifest: &DependencyManifest, name: &str) -> NamespaceId {
    manifest
        .id(name)
        .unwrap_or_else(|| panic!("{name} is not in the manifest"))
}

/// The resources namespace `name` depends on, by path.
fn resources<'a>(manifest: &'a DependencyManifest, name: &str) -> Vec<&'a ResourceDependency> {
    manifest
        .external_inputs(id(manifest, name))
        .filter_map(|input| match input {
            ExternalInput::Resource(resource) => Some(resource),
            _ => None,
        })
        .collect()
}

fn resource_paths(manifest: &DependencyManifest, name: &str) -> Vec<String> {
    let mut paths: Vec<_> = resources(manifest, name)
        .into_iter()
        .map(|resource| resource.path.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

const INLINE: &str = r#"(ns lib.macros (:require [light.build :as b]))
(defmacro inline [path] (b/resource path))
(defmacro optional [path] (or (b/resource? path) "missing"))"#;

#[test]
fn a_macro_inlines_a_resource_and_records_its_text_and_candidates() {
    let roots = Roots::new(2);
    roots.write(0, "lib/macros.cljs", INLINE);
    roots.write(
        0,
        "app/core.cljs",
        r#"(ns app.core (:require [lib.macros :refer [inline]]))
(inline "app/data.txt")"#,
    );
    roots.write(1, "app/data.txt", "hello\nresource");
    let (js, manifest) = roots.build("app.core").unwrap();
    assert_eq!(execute(&js), "hello\nresource");
    let [read] = resources(&manifest, "app.core")[..] else {
        panic!("one read expected");
    };
    assert_eq!(read.path, "app/data.txt");
    assert!(read.stable);
    // Both roots are candidates: a file added to the first would be ambiguous.
    let candidates: Vec<_> = read
        .candidates
        .iter()
        .map(|c| (c.path.clone(), c.present))
        .collect();
    assert_eq!(
        candidates,
        [
            (roots.path(0, "app/data.txt"), false),
            (roots.path(1, "app/data.txt"), true)
        ]
    );
    let found = read.found.as_ref().unwrap();
    assert_eq!(
        found.identity.as_deref(),
        Some(roots.path(1, "app/data.txt").to_str().unwrap())
    );
    assert_eq!(found.bytes, 14);
    assert!(cache_policy::non_cacheable_inputs(&manifest).is_empty());
    // The macro provider read nothing while it was compiled.
    assert!(resources(&manifest, "lib.macros").is_empty());

    // The hash is of the text read: a change changes the record.
    roots.write(1, "app/data.txt", "changed");
    let (js, changed) = roots.build("app.core").unwrap();
    assert_eq!(execute(&js), "changed");
    let hash = |m: &DependencyManifest| resources(m, "app.core")[0].found.as_ref().unwrap().sha256;
    assert_ne!(hash(&manifest), hash(&changed));

    // An absent resource is an error that names every root tried.
    fs::remove_file(roots.path(1, "app/data.txt")).unwrap();
    let error = roots.build("app.core").unwrap_err();
    assert!(
        error.contains("light.build/resource: resource app/data.txt not found in any source root"),
        "{error}"
    );
    assert!(
        error.contains(roots.path(1, "app/data.txt").to_str().unwrap()),
        "{error}"
    );
}

#[test]
fn an_optional_resource_records_absence_until_it_appears() {
    let roots = Roots::new(1);
    roots.write(0, "lib/macros.cljs", INLINE);
    roots.write(
        0,
        "app/core.cljs",
        r#"(ns app.core (:require [lib.macros :refer [optional]]))
(optional "app/extra.txt")"#,
    );
    let (js, absent) = roots.build("app.core").unwrap();
    assert_eq!(execute(&js), "missing");
    let [read] = resources(&absent, "app.core")[..] else {
        panic!("one read expected");
    };
    assert!(read.found.is_none());
    assert_eq!(read.candidates.len(), 1);
    assert!(!read.candidates[0].present);

    roots.write(0, "app/extra.txt", "present");
    let (js, present) = roots.build("app.core").unwrap();
    assert_eq!(execute(&js), "present");
    let [read] = resources(&present, "app.core")[..] else {
        panic!("one read expected");
    };
    assert!(read.found.is_some());
    assert!(read.candidates[0].present);
    assert_ne!(
        absent.record(id(&absent, "app.core")).external_inputs,
        Vec::new()
    );
    assert_ne!(absent.inputs, present.inputs);
}

#[test]
fn a_path_in_two_roots_is_ambiguous_even_for_an_optional_read() {
    let roots = Roots::new(2);
    roots.write(0, "lib/macros.cljs", INLINE);
    roots.write(0, "app/data.txt", "one");
    roots.write(1, "app/data.txt", "two");
    for call in ["inline", "optional"] {
        roots.write(
            0,
            "app/core.cljs",
            &format!(
                r#"(ns app.core (:require [lib.macros :refer [{call}]]))
({call} "app/data.txt")"#
            ),
        );
        let error = roots.build("app.core").unwrap_err();
        assert!(error.contains("ambiguous resource app/data.txt"), "{error}");
        assert!(
            error.contains(roots.path(0, "app/data.txt").to_str().unwrap()),
            "{error}"
        );
        assert!(
            error.contains(roots.path(1, "app/data.txt").to_str().unwrap()),
            "{error}"
        );
    }
}

#[test]
fn a_caught_ambiguous_read_remains_a_dependency() {
    let roots = Roots::new(2);
    roots.write(
        0,
        "lib/macros.cljs",
        r#"(ns lib.macros (:require [light.build :as b]))
(defmacro guarded [path] (try (b/resource path) (catch :default _ "fallback")))"#,
    );
    roots.write(
        0,
        "app/core.cljs",
        r#"(ns app.core (:require [lib.macros :refer [guarded]])) (guarded "app/data.txt")"#,
    );
    roots.write(0, "app/data.txt", "one");
    roots.write(1, "app/data.txt", "two");
    let (js, manifest) = roots.build("app.core").unwrap();
    assert_eq!(execute(&js), "fallback");
    let [read] = resources(&manifest, "app.core")[..] else {
        panic!("one read expected");
    };
    assert!(read.stable && read.found.is_none());
    let present: Vec<_> = read.candidates.iter().map(|c| c.present).collect();
    assert_eq!(present, [true, true]);
    // Removing a copy changes the recorded candidates.
    fs::remove_file(roots.path(1, "app/data.txt")).unwrap();
    let (js, removed) = roots.build("app.core").unwrap();
    assert_eq!(execute(&js), "one");
    assert_ne!(manifest.inputs, removed.inputs);
}

#[test]
fn a_relative_path_resolves_against_the_expansion_sites_file() {
    let roots = Roots::new(1);
    roots.write(0, "lib/macros.cljs", INLINE);
    roots.write(0, "lib/data.txt", "the macro's own");
    roots.write(0, "my_app/data.txt", "next to the caller");
    roots.write(0, "shared/data.txt", "a sibling directory");
    roots.write(
        0,
        "my_app/core.cljs",
        r#"(ns my-app.core (:require [lib.macros :refer [inline]]))
(str (inline "./data.txt") "," (inline "../shared/data.txt"))"#,
    );
    let (js, manifest) = roots.build("my-app.core").unwrap();
    assert_eq!(execute(&js), "next to the caller,a sibling directory");
    assert_eq!(
        resource_paths(&manifest, "my-app.core"),
        ["my_app/data.txt", "shared/data.txt"]
    );
    // Leaving the roots, an absolute path and an empty segment are errors.
    for path in ["../../x.txt", "/my_app/data.txt", "my_app//data.txt"] {
        roots.write(
            0,
            "my_app/core.cljs",
            &format!(
                r#"(ns my-app.core (:require [lib.macros :refer [inline]]))
(inline "{path}")"#
            ),
        );
        let error = roots.build("my-app.core").unwrap_err();
        assert!(error.contains("light.build/resource"), "{path}: {error}");
    }
}

#[test]
fn a_helper_value_read_once_is_a_dependency_of_every_namespace_that_reuses_it() {
    let roots = Roots::new(1);
    roots.write(
        0,
        "lib/helpers.cljs",
        r#"(ns lib.helpers (:require [light.build :as b]))
(def reads (atom 0))
;; The application loads this namespace too, where light.build is an error.
(def table (when (exists? js/lightCljsBuild) (swap! reads inc) (b/resource "lib/table.txt")))"#,
    );
    roots.write(
        0,
        "lib/macros.cljs",
        r#"(ns lib.macros (:require [lib.helpers :as h]))
(defmacro table [] (str h/table " #" @h/reads))"#,
    );
    roots.write(
        0,
        "app/a.cljs",
        "(ns app.a (:require [lib.macros :refer [table]])) (def value (table))",
    );
    roots.write(
        0,
        "app/b.cljs",
        "(ns app.b (:require [lib.macros :refer [table]])) (def value (table))",
    );
    roots.write(0, "app/other.cljs", "(ns app.other) (def value 1)");
    roots.write(
        0,
        "app/main.cljs",
        r#"(ns app.main (:require [app.other] [app.a :as a] [app.b :as b])) (str a/value "," b/value)"#,
    );
    roots.write(0, "lib/table.txt", "rows");
    let (js, manifest) = roots.build("app.main").unwrap();
    // One initialization, during app.a's compilation, reused by app.b.
    assert_eq!(execute(&js), "rows #1,rows #1");
    for name in ["lib.helpers", "lib.macros", "app.a", "app.b", "app.main"] {
        assert_eq!(resource_paths(&manifest, name), ["lib/table.txt"], "{name}");
    }
    assert!(resource_paths(&manifest, "app.other").is_empty());
    // An artifact's manifest carries the namespace's inputs.
    let fields = light_cljs::artifact_codec::ManifestFields::of(
        &manifest,
        id(&manifest, "app.b"),
        Default::default(),
        Default::default(),
        [0; 32],
    );
    assert_eq!(
        fields.inputs,
        manifest
            .external_inputs(id(&manifest, "app.b"))
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(fields.inputs.len(), 1);

    // A helper's initializer runs outside any expansion: `./` is an error.
    roots.write(
        0,
        "lib/helpers.cljs",
        r#"(ns lib.helpers (:require [light.build :as b]))
(def reads (atom 0))
(def table (when (exists? js/lightCljsBuild) (b/resource "./table.txt")))"#,
    );
    let error = roots.build("app.main").unwrap_err();
    assert!(error.contains("has no expansion site"), "{error}");
}

#[test]
fn reads_in_nested_deferred_units_survive_their_released_analysis() {
    let roots = Roots::new(1);
    roots.write(0, "lib/macros.cljs", INLINE);
    // `inline` expands while `header`'s deferred unit is analyzed; `./`
    // resolves against lib/data.cljs, the unit's namespace.
    roots.write(
        0,
        "lib/data.cljs",
        r#"(ns lib.data (:require [light.build :as b] [lib.macros :refer [inline]]))
(def header (inline "./header.txt"))
(def table (when (exists? js/lightCljsBuild) (b/resource "lib/table.txt")))"#,
    );
    // `wrapped`'s initializer initializes `table`, nested.
    roots.write(
        0,
        "lib/helpers.cljs",
        r#"(ns lib.helpers (:require [lib.data :as d]))
(def wrapped (str d/header "[" d/table "]"))"#,
    );
    roots.write(
        0,
        "lib/use.cljs",
        r#"(ns lib.use (:require [lib.helpers :as h]))
(defmacro wrapped [] h/wrapped)"#,
    );
    roots.write(
        0,
        "app/main.cljs",
        "(ns app.main (:require [lib.use :refer [wrapped]])) (wrapped)",
    );
    roots.write(0, "lib/header.txt", "H");
    roots.write(0, "lib/table.txt", "rows");
    let (js, manifest) = roots.build("app.main").unwrap();
    assert_eq!(execute(&js), "H[rows]");
    for name in ["lib.data", "lib.helpers", "lib.use", "app.main"] {
        assert_eq!(
            resource_paths(&manifest, name),
            ["lib/header.txt", "lib/table.txt"],
            "{name}"
        );
    }
    assert!(resource_paths(&manifest, "lib.macros").is_empty());
}

#[test]
fn a_custom_data_reader_reads_for_the_namespace_whose_form_it_reads() {
    let roots = Roots::new(1);
    roots.write(
        0,
        "app/core.cljs",
        r#"(ns app.core (:require [app.plain]))
(str #app/inline "./data.txt" "," #app/inline "shared/x.txt")"#,
    );
    roots.write(0, "app/plain.cljs", "(ns app.plain) (def v 1)");
    roots.write(0, "app/data.txt", "local");
    roots.write(0, "shared/x.txt", "shared");
    let mut host = MacroHost::new();
    host.register_compiler_data_reader(
        Name::new(Some("app"), "inline"),
        "(fn [path] (.resource js/lightCljsBuild path))",
    )
    .unwrap();
    let (js, manifest) = roots.build_with("app.core", &mut host).unwrap();
    assert_eq!(execute(&js), "local,shared");
    assert_eq!(
        resource_paths(&manifest, "app.core"),
        ["app/data.txt", "shared/x.txt"]
    );
    assert!(resource_paths(&manifest, "app.plain").is_empty());
}

#[test]
fn depend_declares_resources_namespaces_and_providers_without_reading() {
    let roots = Roots::new(1);
    roots.write(0, "lib/data.cljs", "(ns lib.data) (def v 1)");
    roots.write(
        0,
        "lib/macros.cljs",
        r#"(ns lib.macros (:require [light.build :as b]))
(defmacro declared []
  [(b/depend! {:resource "app/schema.edn"})
   (b/depend! {:namespace 'lib.data})
   (b/depend! {:provider :git/head :fingerprint "0abc"})])
(defmacro bad [] (b/depend! {:resource "x" :namespace 'y}))"#,
    );
    roots.write(
        0,
        "app/core.cljs",
        "(ns app.core (:require [lib.data] [lib.macros :refer [declared]])) (pr-str (declared))",
    );
    let (js, manifest) = roots.build("app.core").unwrap();
    assert_eq!(execute(&js), "[nil nil nil]");
    let inputs: Vec<_> = manifest
        .external_inputs(id(&manifest, "app.core"))
        .collect();
    assert_eq!(inputs.len(), 3, "{inputs:?}");
    assert!(inputs.iter().any(|input| matches!(input,
        ExternalInput::Resource(r) if r.path == "app/schema.edn" && r.found.is_none())));
    assert!(inputs.contains(&&ExternalInput::Namespace("lib.data".into())));
    assert!(inputs.contains(&&ExternalInput::Provider {
        token: ":git/head".into(),
        fingerprint: "0abc".into()
    }));
    let edge = manifest
        .edge(id(&manifest, "app.core"), id(&manifest, "lib.data"))
        .unwrap();
    assert!(edge.contains(EdgeKinds::OBSERVED | EdgeKinds::RUNTIME));

    roots.write(
        0,
        "app/core.cljs",
        "(ns app.core (:require [lib.macros :refer [bad]])) (bad)",
    );
    let error = roots.build("app.core").unwrap_err();
    assert!(error.contains("light.build/depend!: expected"), "{error}");
}

/// Service arguments are well-formed text, which both engines' bridges carry
/// alike: a lone surrogate or a NUL is refused before any request, in a
/// fingerprint as in a path, and a surrogate pair is kept.
#[test]
fn service_arguments_with_lone_surrogates_are_refused_in_both_engines() {
    let roots = Roots::new(1);
    roots.write(
        0,
        "lib/macros.cljs",
        r#"(ns lib.macros (:require [light.build :as b]))
(defn- attempt [f] (try (f) "ok" (catch :default e (.-message e))))
(defmacro attempts []
  (let [lone (js/String.fromCharCode 55296)
        pair (js/String.fromCharCode 55357 56832)]
    [(attempt #(b/depend! {:provider "p" :fingerprint lone}))
     (attempt #(b/resource? (str "app/" lone ".txt")))
     (attempt #(b/resource? (str "app/" (js/String.fromCharCode 0))))
     (attempt #(b/depend! {:provider "p" :fingerprint pair}))]))"#,
    );
    roots.write(
        0,
        "app/core.cljs",
        "(ns app.core (:require [lib.macros :refer [attempts]])) (pr-str (attempts))",
    );
    let (js, manifest) = roots.build("app.core").unwrap();
    assert_eq!(
        execute(&js),
        "[\"light.build/depend!: a string with a lone surrogate\" \
         \"light.build/resource?: a string with a lone surrogate\" \
         \"light.build/resource?: a string with a NUL character\" \"ok\"]"
    );
    let inputs: Vec<_> = manifest
        .external_inputs(id(&manifest, "app.core"))
        .collect();
    assert_eq!(
        inputs,
        [&ExternalInput::Provider {
            token: "p".into(),
            fingerprint: "\u{1F600}".into()
        }]
    );
}

#[test]
fn application_code_cannot_call_light_build() {
    let roots = Roots::new(1);
    roots.write(
        0,
        "app/core.cljs",
        r#"(ns app.core (:require [light.build :as b]))
(try (b/resource "app/core.cljs") (catch :default e (.-message e)))"#,
    );
    let (js, manifest) = roots.build("app.core").unwrap();
    assert_eq!(
        execute(&js),
        "light.build/resource is only available at compile time: call it from a macro, \
         a helper a macro runs or a custom data reader"
    );
    assert!(manifest.inputs.is_empty());
}

/// An embedding provider with in-memory sources and resources it cannot
/// identify: no stable identity, so what reads them is non-cacheable.
struct Memory;

impl SourceProvider for Memory {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        let source = match String::from_utf16_lossy(request.namespace.name()).as_str() {
            "lib.macros" => INLINE,
            "app.core" => {
                r#"(ns app.core (:require [lib.macros :refer [inline]])) (inline "app/data.txt")"#
            }
            _ => return Ok(None),
        };
        Ok(Some(SourceInput::new("memory", source)))
    }
    fn resources(&self) -> Option<Rc<dyn ResourceProvider>> {
        Some(Rc::new(Memory))
    }
}

impl ResourceProvider for Memory {
    fn load_resource(
        &self,
        path: &str,
        tried: &mut Vec<Candidate>,
    ) -> Result<Option<ResourceInput>, LoadError> {
        tried.push(Candidate {
            path: format!("memory:{path}").into(),
            present: path == "app/data.txt",
        });
        Ok((path == "app/data.txt").then(|| ResourceInput {
            file: format!("memory:{path}"),
            identity: None,
            text: "from memory".into(),
        }))
    }
}

#[test]
fn a_provider_without_stable_identities_makes_its_readers_non_cacheable() {
    let mut manifest = DependencyManifest::default();
    let (program, _) = analyze_namespace_entry_recorded_with_provider(
        "app.core",
        &Memory,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            manifest: Some(&mut manifest),
            ..EntryRecorders::default()
        },
    )
    .unwrap();
    assert_eq!(execute(&light_cljs::emit_js(&program)), "from memory");
    let [read] = resources(&manifest, "app.core")[..] else {
        panic!("one read expected");
    };
    assert!(!read.stable);
    assert_eq!(read.found.as_ref().unwrap().identity, None);
    let inputs = cache_policy::non_cacheable_inputs(&manifest);
    let settings = CacheSettings {
        enabled: true,
        blockers: Vec::new(),
    };
    let policy = cache_policy::evaluate(&manifest, &settings, &[], &inputs);
    let core = id(&manifest, "app.core");
    assert_eq!(policy.reason(core), Some(Bypass::NonCacheableInput));
    assert!(policy.may_use_cache(id(&manifest, "lib.macros")));
    assert_eq!(
        policy.describe(&manifest, core).unwrap(),
        "input without a stable identity (resource app/data.txt from a provider without \
         stable identities) through app.core"
    );
}

/// Record a build of `entry` from `provider`: its output and manifest.
fn build_from(entry: &str, provider: &dyn SourceProvider) -> (String, DependencyManifest) {
    let mut manifest = DependencyManifest::default();
    let (program, _) = analyze_namespace_entry_recorded_with_provider(
        entry,
        provider,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            manifest: Some(&mut manifest),
            ..EntryRecorders::default()
        },
    )
    .unwrap();
    (light_cljs::emit_js(&program), manifest)
}

/// An embedding provider with its own layout: `app.core` is the file
/// `generated/core.cljs`, and its resources sit beside it.
struct Generated;

impl SourceProvider for Generated {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        Ok(
            match String::from_utf16_lossy(request.namespace.name()).as_str() {
                "lib.macros" => Some(SourceInput::new("lib/macros.cljs", INLINE)),
                "app.core" => Some(SourceInput::new(
                    "generated/core.cljs",
                    r#"(ns app.core (:require [lib.macros :refer [inline]])) (inline "./data.txt")"#,
                )),
                _ => None,
            },
        )
    }
    fn resources(&self) -> Option<Rc<dyn ResourceProvider>> {
        Some(Rc::new(Generated))
    }
}

impl ResourceProvider for Generated {
    fn load_resource(
        &self,
        path: &str,
        tried: &mut Vec<Candidate>,
    ) -> Result<Option<ResourceInput>, LoadError> {
        let present = path == "generated/data.txt";
        tried.push(Candidate {
            path: path.into(),
            present,
        });
        Ok(present.then(|| ResourceInput {
            file: path.into(),
            identity: Some(path.into()),
            text: "beside the generated file".into(),
        }))
    }
    fn stable(&self) -> bool {
        true
    }
    fn directory(&self, file: &str) -> Option<String> {
        Some(
            file.rsplit_once('/')
                .map_or("", |(directory, _)| directory)
                .into(),
        )
    }
}

#[test]
fn a_relative_path_resolves_against_the_directory_of_the_providers_file() {
    let (js, manifest) = build_from("app.core", &Generated);
    assert_eq!(execute(&js), "beside the generated file");
    assert_eq!(
        resource_paths(&manifest, "app.core"),
        ["generated/data.txt"]
    );
}

/// A provider with no resources: `light.build/resource` is an error a
/// macro can catch.
struct NoResources;

impl SourceProvider for NoResources {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        Ok(
            match String::from_utf16_lossy(request.namespace.name()).as_str() {
                "lib.macros" => Some(SourceInput::new(
                    "lib/macros.cljs",
                    r#"(ns lib.macros (:require [light.build :as b]))
(defmacro inline [path] (try (b/resource path) (catch :default _ "fallback")))"#,
                )),
                "app.core" => Some(SourceInput::new(
                    "app/core.cljs",
                    r#"(ns app.core (:require [lib.macros :refer [inline]])) (inline "app/data.txt")"#,
                )),
                _ => None,
            },
        )
    }
}

#[test]
fn a_caught_read_without_a_resource_provider_is_non_cacheable() {
    let (js, manifest) = build_from("app.core", &NoResources);
    assert_eq!(execute(&js), "fallback");
    let [read] = resources(&manifest, "app.core")[..] else {
        panic!("one read expected");
    };
    assert_eq!(read.path, "app/data.txt");
    assert!(!read.stable && read.found.is_none() && read.candidates.is_empty());
    let inputs = cache_policy::non_cacheable_inputs(&manifest);
    let settings = CacheSettings {
        enabled: true,
        blockers: Vec::new(),
    };
    let policy = cache_policy::evaluate(&manifest, &settings, &[], &inputs);
    assert_eq!(
        policy.reason(id(&manifest, "app.core")),
        Some(Bypass::NonCacheableInput)
    );
}
