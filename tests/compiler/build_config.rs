//! `light.build/config` and `light.build/depend-on!`: the build
//! configuration that `--config KEY=EDN` gives macros, the fingerprint of
//! the whole configuration that every namespace depends on, and the coarse
//! broad views, each recorded in the dependency manifest of every namespace
//! whose compilation used it (docs/compiler.md, "Compile-time services:
//! light.build"). The default build runs these macros in V8 and the
//! QuickJS-only build in QuickJS; both run this suite.
use crate::temp_dir::TempDir;
use light_cljs::compiler::analyze_namespace_entry_with_dependencies;
use light_cljs::dependency_manifest::{BroadView, DependencyManifest, ExternalInput, NamespaceId};
use light_cljs::{BuildConfiguration, MacroHost, analyzer::AnalysisOptions};
use std::{fs, process::Command};

struct Root(TempDir);

impl Root {
    fn new() -> Self {
        Self(TempDir::new("cljs-build-config"))
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    fn build(
        &self,
        entry: &str,
        configuration: &BuildConfiguration,
    ) -> Result<(String, DependencyManifest), String> {
        let mut host = MacroHost::new();
        host.set_build_configuration(configuration.clone());
        let (program, _, manifest) = analyze_namespace_entry_with_dependencies(
            entry,
            &[self.0.path().clone()],
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .map_err(|error| error.to_string())?;
        Ok((light_cljs::emit_js(&program), manifest))
    }
}

fn configuration(entries: &[(&str, &str)]) -> BuildConfiguration {
    let mut configuration = BuildConfiguration::new();
    for (key, value) in entries {
        configuration.insert(key, value).unwrap();
    }
    configuration
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

/// The configuration fingerprint namespace `name` depends on.
fn fingerprint(manifest: &DependencyManifest, name: &str) -> [u8; 32] {
    let hashes: Vec<[u8; 32]> = manifest
        .external_inputs(id(manifest, name))
        .filter_map(|input| match input {
            ExternalInput::Configuration { sha256 } => Some(*sha256),
            _ => None,
        })
        .collect();
    let [hash] = hashes[..] else {
        panic!("{name}: one configuration input expected, got {hashes:?}");
    };
    hash
}

/// The broad views namespace `name` depends on.
fn views(manifest: &DependencyManifest, name: &str) -> Vec<BroadView> {
    manifest
        .external_inputs(id(manifest, name))
        .filter_map(|input| match input {
            ExternalInput::View(view) => Some(*view),
            _ => None,
        })
        .collect()
}

const CONFIG: &str = r#"(ns lib.macros (:require [light.build :as b]))
(defmacro flavor [] (b/config :app/flavor))
(defmacro shown [key] (pr-str (b/config key)))
(defmacro kinds []
  (let [settings (b/config :app/settings)]
    (pr-str [(keyword? (b/config :app/flavor)) (map? settings) (vector? (:ports settings))
             (set? (:tags settings)) (list? (:path settings)) (nil? (b/config :app/missing))])))"#;

#[test]
fn config_answers_a_set_key_an_unset_key_and_a_collection_value() {
    let root = Root::new();
    root.write("lib/macros.cljs", CONFIG);
    root.write(
        "app/core.cljs",
        r#"(ns app.core (:require [lib.macros :refer [flavor shown kinds]]))
(str (flavor) " " (shown :app/settings) " " (shown :app/missing) " " (shown :app/n) " " (kinds))"#,
    );
    let configuration = configuration(&[
        ("app/flavor", ":dev"),
        (
            "app/settings",
            r#"{:ports [80 443], :tags #{"a"} :name "x \u0079" :on true :off false :none nil :path (1 -2.50)}"#,
        ),
        ("app/n", "3"),
    ]);
    let (js, _) = root.build("app.core", &configuration).unwrap();
    assert_eq!(
        execute(&js),
        r#":dev {:ports [80 443], :tags #{"a"}, :name "x y", :on true, :off false, :none nil, :path (1 -2.5)} nil 3 [true true true true true true]"#
    );
    // Without the flag every key is unset.
    let (js, _) = root.build("app.core", &BuildConfiguration::new()).unwrap();
    assert_eq!(
        execute(&js),
        " nil nil nil [false false false false false true]"
    );
    // Only a keyword names a key.
    root.write(
        "app/core.cljs",
        r#"(ns app.core (:require [lib.macros :refer [shown]])) (shown "app/flavor")"#,
    );
    let error = root.build("app.core", &configuration).unwrap_err();
    assert!(
        error.contains("light.build/config: expected a keyword, got \"app/flavor\""),
        "{error}"
    );
}

#[test]
fn every_namespace_depends_on_the_whole_configurations_fingerprint() {
    let root = Root::new();
    root.write("lib/macros.cljs", CONFIG);
    root.write("app/plain.cljs", "(ns app.plain) (def v 1)");
    root.write(
        "app/core.cljs",
        "(ns app.core (:require [app.plain] [lib.macros :refer [flavor]])) (str (flavor))",
    );
    let read = |entries: &[(&str, &str)]| {
        let configuration = configuration(entries);
        let (js, manifest) = root.build("app.core", &configuration).unwrap();
        for name in ["lib.macros", "app.plain", "app.core"] {
            assert_eq!(
                fingerprint(&manifest, name),
                configuration.fingerprint(),
                "{name}"
            );
        }
        // One input, interned, whichever namespaces depend on it.
        let count = manifest
            .inputs
            .iter()
            .filter(|input| matches!(input, ExternalInput::Configuration { .. }))
            .count();
        assert_eq!(count, 1);
        (execute(&js), fingerprint(&manifest, "app.plain"))
    };
    let (output, first) = read(&[("app/flavor", ":dev"), ("app/unread", "1")]);
    assert_eq!(output, ":dev");
    // A key no namespace reads still changes every namespace's fingerprint.
    let (output, changed) = read(&[("app/flavor", ":dev"), ("app/unread", "2")]);
    assert_eq!(output, ":dev");
    assert_ne!(first, changed);
    let (_, without) = read(&[("app/flavor", ":dev")]);
    assert_ne!(first, without);
    assert_ne!(changed, without);
    // The empty map has its own fingerprint, which a key, even one set to
    // nil, changes.
    let (output, empty) = read(&[]);
    assert_eq!(output, "");
    let (output, nil) = read(&[("app/unread", "nil")]);
    assert_eq!(output, "");
    assert_eq!(empty, BuildConfiguration::new().fingerprint());
    assert_ne!(empty, nil);
    // Insertion order is not part of the configuration.
    let ordered = configuration(&[("b/x", "1"), ("a/y", "2")]);
    let reversed = configuration(&[("a/y", "2"), ("b/x", "1")]);
    assert_eq!(ordered.fingerprint(), reversed.fingerprint());
    assert_eq!(ordered.text(), "{:a/y 2 :b/x 1}");
}

#[test]
fn configuration_values_are_edn_data_and_keys_are_given_once() {
    let mut configuration = BuildConfiguration::new();
    configuration
        .insert_text(r#"app/settings= { :a [1 , 2] #_ :skipped :b #{"x"} }"#)
        .unwrap();
    configuration.insert_text("flavor=:dev").unwrap();
    configuration.insert("app/n", "-3.5").unwrap();
    assert_eq!(
        configuration.get("app/settings"),
        Some(r#"{:a [1 2] :b #{"x"}}"#)
    );
    assert_eq!(configuration.get("flavor"), Some(":dev"));
    assert_eq!(configuration.get("app/n"), Some("-3.5"));
    assert_eq!(configuration.get("app/missing"), None);
    // Scalars are canonical: equal values have one text.
    for (value, canonical) in [
        ("1.50", "1.5"),
        ("1.0", "1"),
        ("+7", "7"),
        ("-0", "0"),
        ("1e3", "1000"),
        ("2.5E-3", "0.0025"),
        (r#""a\u0062\n\"\\""#, r#""ab\n\"\\""#),
        ("(1 [2.0])", "(1 [2])"),
    ] {
        let mut single = BuildConfiguration::new();
        single.insert("k", value).unwrap();
        assert_eq!(single.get("k"), Some(canonical), "{value}");
    }
    for (text, message) in [
        ("app/n=4", "the key app/n is given twice"),
        ("app/flavor", "expected KEY=EDN"),
        (":app/flavor=1", "written without its colon, as app/flavor"),
        ("app flavor=1", "invalid key \"app flavor\""),
        ("=1", "invalid key \"\""),
        ("app/a=", "the value is empty"),
        ("app/a=1 2", "holds more than one form"),
        ("app/a={:a 1", "cannot read the value"),
        ("app/a={:a 1 :a 2}", "repeats the key :a"),
        ("app/a=#{1 1}", "repeats the element 1"),
        // Duplicates are found by value, as the EDN reader compares.
        (r#"app/a={"a" 1 "\u0061" 2}"#, r#"repeats the key "a""#),
        ("app/a=#{1 1.0}", "repeats the element 1"),
        ("app/a=#{[1] (1)}", "repeats the element [1]"),
        (
            "app/a={{:a 1 :b 2} 1 {:b 2 :a 1} 2}",
            "repeats the key {:a 1 :b 2}",
        ),
        // Only numbers the EDN reader reads as JavaScript numbers.
        ("app/a=36rN", "\"36rN\" is not a configuration number"),
        ("app/a=1N", "is not a configuration number"),
        ("app/a=1M", "is not a configuration number"),
        ("app/a=0x10", "is not a configuration number"),
        ("app/a=1/2", "is not a configuration number"),
        ("app/a=01", "is not a configuration number"),
        ("app/a=1e999", "is not a configuration number"),
        ("app/a=dev", "\"dev\" is not a configuration value"),
        (
            "app/a=[::local]",
            "\"::local\" is not a configuration value",
        ),
        ("app/a=#inst \"2026-10-18\"", "is not a configuration value"),
        ("app/a='x", "is not a configuration value"),
        ("app/a=#(inc %)", "is not a configuration value"),
    ] {
        let error = configuration.insert_text(text).unwrap_err();
        assert!(error.contains(message), "{text}: {error}");
    }
    // A failed insert leaves the configuration as it was.
    assert_eq!(configuration.entries().count(), 3);
}

/// Test discovery: a macro that lists the namespaces of the build and their
/// declarations, run directly or through a helper unit.
#[test]
fn each_broad_view_is_recorded_on_the_namespace_that_asked_and_through_helpers() {
    let root = Root::new();
    root.write(
        "lib/macros.cljs",
        r#"(ns lib.macros (:require [light.build :as b]))
(defmacro discover [] (b/depend-on! :namespaces) (b/depend-on! :declarations) (b/depend-on! :config) nil)
(defmacro bogus [] (b/depend-on! :everything))"#,
    );
    root.write(
        "lib/helpers.cljs",
        r#"(ns lib.helpers (:require [light.build :as b]))
(defn declarations [] (b/depend-on! :declarations) :declared)
;; The application loads this namespace too, where light.build is an error.
(def inventory
  (when (exists? js/lightCljsBuild)
    (b/depend-on! :namespaces) (b/depend-on! :declarations) (b/depend-on! :config) :listed))"#,
    );
    root.write(
        "lib/reflect.cljs",
        r#"(ns lib.reflect (:require [lib.helpers :as h]))
(defmacro reflect [] (str (h/declarations) h/inventory))"#,
    );
    root.write("app/other.cljs", "(ns app.other) (def v 1)");
    root.write(
        "app/runner.cljs",
        "(ns app.runner (:require [app.other] [lib.macros :refer [discover]])) (discover)",
    );
    root.write(
        "app/a.cljs",
        "(ns app.a (:require [lib.reflect :refer [reflect]])) (def value (reflect))",
    );
    root.write(
        "app/main.cljs",
        "(ns app.main (:require [app.runner] [app.a :as a])) a/value",
    );
    let (js, manifest) = root.build("app.main", &BuildConfiguration::new()).unwrap();
    assert_eq!(execute(&js), ":declared:listed");
    assert_eq!(
        views(&manifest, "app.runner"),
        [
            BroadView::Namespaces,
            BroadView::Declarations,
            BroadView::Config
        ]
    );
    for name in ["lib.macros", "app.other"] {
        assert!(views(&manifest, name).is_empty(), "{name}");
    }
    // A helper value's views go to its namespace and every namespace whose
    // compilation depends on it, since a later one may reuse the value; a
    // helper function's call, to the namespace whose expansion called it.
    // `app.a`'s `def` is a deferred unit, being analyzed when its expansion
    // calls the helper, so the call is also conservatively a helper read of
    // `app.a`, which reaches `app.main`.
    for name in ["lib.helpers", "lib.reflect", "app.a", "app.main"] {
        let mut found = views(&manifest, name);
        found.sort_by_key(|view| *view as u8);
        assert_eq!(
            found,
            [
                BroadView::Namespaces,
                BroadView::Declarations,
                BroadView::Config
            ],
            "{name}"
        );
    }

    root.write(
        "app/main.cljs",
        "(ns app.main (:require [lib.macros :refer [bogus]])) (bogus)",
    );
    let error = root
        .build("app.main", &BuildConfiguration::new())
        .unwrap_err();
    assert!(
        error.contains(
            "light.build/depend-on!: expected :namespaces, :declarations or :config, got :everything"
        ),
        "{error}"
    );
}

/// A custom data reader reads the configuration and declares broad views
/// through `js/lightCljsBuild`, recorded on the namespace whose form it
/// reads, also when it reads a helper namespace's value, from which they
/// reach the namespaces that depend on it.
#[test]
fn a_custom_data_reader_reads_configuration_and_declares_views() {
    let root = Root::new();
    root.write(
        "lib/data.cljs",
        r#"(ns lib.data) (def table #app/flavored "rows") (defmacro table-macro [] table)"#,
    );
    root.write(
        "app/core.cljs",
        r#"(ns app.core (:require [app.plain] [lib.data :refer [table-macro]]))
(str #app/flavored "x" "," (table-macro))"#,
    );
    root.write("app/plain.cljs", "(ns app.plain) (def v 1)");
    let mut host = MacroHost::new();
    host.register_compiler_data_reader(
        light_cljs::compiler_data::Name::new(Some("app"), "flavored"),
        r#"(fn [text]
  (.dependOn js/lightCljsBuild "namespaces")
  (.dependOn js/lightCljsBuild "declarations")
  (.dependOn js/lightCljsBuild "config")
  (str text "@" (.config js/lightCljsBuild "app/flavor")))"#,
    )
    .unwrap();
    host.set_build_configuration(configuration(&[("app/flavor", ":dev")]));
    let (program, _, manifest) = analyze_namespace_entry_with_dependencies(
        "app.core",
        &[root.0.path().clone()],
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(execute(&light_cljs::emit_js(&program)), "x@:dev,rows@:dev");
    let all = [
        BroadView::Namespaces,
        BroadView::Declarations,
        BroadView::Config,
    ];
    for name in ["lib.data", "app.core"] {
        assert_eq!(views(&manifest, name), all, "{name}");
    }
    assert!(views(&manifest, "app.plain").is_empty());
}
