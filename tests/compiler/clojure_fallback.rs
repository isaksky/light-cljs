//! A `clojure.X` require that no source provides resolves to `cljs.X`, a
//! source's or a bundled library's, and both qualifiers name it
//! (docs/compiler.md, "Namespaces"), in clean and cached `--compile`
//! builds. An explicit `clojure.X` source still wins.
use crate::temp_dir::TempDir;
use light_cljs::compiler::analyze_namespace_entry_with_dependencies;
use light_cljs::dependency_manifest::{RequireForm, RequireTarget, SourceOrigin};
use light_cljs::{MacroHost, analyzer::AnalysisOptions};
use std::{fs, path::Path, process::Command};

/// `clojure.reader` falls back to the bundled `cljs.reader` and
/// `clojure.foo` to the source root's `cljs.foo`, through an alias, a
/// refer and the written qualifier.
const MAIN: &str = "(ns app.main (:require [clojure.reader :as r :refer [read-string]] [clojure.foo :as f :refer [v]]))
(js/console.log (pr-str [(r/read-string \"{:a 1}\") (read-string \"[2]\") (clojure.reader/read-string \":k\") f/v v clojure.foo/v]))";
const EXPECTED: &str = "[{:a 1} [2] :k 41 41 41]\n";

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

struct Build {
    js: String,
    hits: u64,
    misses: u64,
}

/// A `--compile` of `app.main` in `dir`, with the cache in `dir/cache`.
fn build(dir: &Path) -> Build {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(dir)
        .args(["--compile", "--entry", "app.main", "--source-root", "src"])
        .args(["--stats", "--cache-dir", "cache"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    let stat = |key: &str| {
        stderr
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(' ')?.parse().ok())
            .unwrap_or(0)
    };
    Build {
        js: String::from_utf8(output.stdout).unwrap(),
        hits: stat("cache.hits"),
        misses: stat("cache.misses"),
    }
}

fn run(js: &str) -> String {
    crate::canonical_json::node_stdout(js)
}

#[test]
fn clojure_requires_fall_back_to_bundled_and_source_cljs_libraries_in_clean_and_cached_builds() {
    let dir = TempDir::new("clojure-fallback");
    write(dir.path(), "src/cljs/foo.cljs", "(ns cljs.foo) (def v 41)");
    write(dir.path(), "src/app/main.cljs", MAIN);
    let cold = build(dir.path());
    assert_eq!(run(&cold.js), EXPECTED);
    assert!(cold.misses > 0);
    // A warm build installs every namespace and writes the same output.
    let warm = build(dir.path());
    assert_eq!((warm.hits, warm.misses), (cold.misses, 0));
    assert_eq!(warm.js, cold.js);
    // An explicit `clojure.reader` source then replaces the bundled
    // `cljs.reader`, also for the cached build, whose lookup of it saw no
    // file.
    write(
        dir.path(),
        "src/clojure/reader.cljs",
        "(ns clojure.reader) (defn read-string [s] (str \"own \" s))",
    );
    let shadowed = build(dir.path());
    assert!(shadowed.misses > 0);
    assert_eq!(
        run(&shadowed.js),
        "[\"own {:a 1}\" \"own [2]\" \"own :k\" 41 41 41]\n"
    );
    // So does an explicit `clojure.foo` source replace `cljs.foo`.
    write(
        dir.path(),
        "src/clojure/foo.cljs",
        "(ns clojure.foo) (def v 7)",
    );
    let own = build(dir.path());
    assert_eq!(
        run(&own.js),
        "[\"own {:a 1}\" \"own [2]\" \"own :k\" 7 7 7]\n"
    );
}

#[test]
fn a_bundled_rewrite_records_both_lookups_and_the_bundled_target() {
    let dir = TempDir::new("clojure-fallback-manifest");
    write(
        dir.path(),
        "app.cljs",
        "(ns app (:require [clojure.reader :as r])) (js/console.log (r/read-string \"1\") (cljs.reader/read-string \"2\"))",
    );
    let (_, _, manifest) = analyze_namespace_entry_with_dependencies(
        "app",
        std::slice::from_ref(dir.path()),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let app = manifest.record(manifest.id("app").unwrap());
    let require = app
        .requires
        .iter()
        .find(|r| r.written == "clojure.reader")
        .unwrap();
    assert_eq!(require.form, RequireForm::ClojureRewrite);
    let reader = manifest.id("cljs.reader").unwrap();
    assert_eq!(require.target, RequireTarget::Namespace(reader));
    let checked: Vec<_> = require
        .lookups
        .iter()
        .map(|&lookup| {
            let lookup = manifest.lookup(lookup);
            (lookup.namespace.as_str(), lookup.found.is_some())
        })
        .collect();
    assert_eq!(checked, [("clojure.reader", false), ("cljs.reader", false)]);
    assert_eq!(manifest.record(reader).source.origin, SourceOrigin::Bundled);
    assert!(manifest.id("clojure.reader").is_none());
}
