//! The dev build (docs/dev-workflow.md, "The dev build" and "Phase 0"):
//! `--compile --entry NS --source-root DIR --reloadable --out-dir DIR`, run in
//! Node by tests/support/dev_execution.mjs, the driver that also runs its
//! `--optimize none` oracle and applies replaced units with the hook protocol.
//! The phase oracles check the dev builds of their programs as well
//! (tests/support/phase_oracle.rs).
use super::size_ratchet::ratchet_fixtures::FIXTURES;
use crate::dev_execution::{none_and_dev, run_dev};
use crate::temp_dir::TempDir;
use light_cljs::MacroHost;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry_with_sources;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/dev");

/// What examples/dev prints when the host calls `start`, its program value:
/// the same as a dev build and at `none`.
const EXAMPLE_RECORD: &str = r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"start\nclicks: [1]\n"}"#;

/// The manifest of the example's dev build.
const EXAMPLE_MANIFEST: &str = r#"{
  "entry": "app.main",
  "namespaces": [
    {"name": "app.state", "file": "app.state.js", "requires": [], "macros": false},
    {"name": "app.macros", "file": "app.macros.js", "requires": [], "macros": true},
    {"name": "app.view", "file": "app.view.js", "requires": ["app.macros", "app.state"], "macros": false},
    {"name": "app.main", "file": "app.main.js", "requires": ["app.state", "app.view"], "macros": false}
  ],
  "before-load": [
    {"name": "app.main/stop", "async": false}
  ],
  "after-load": [
    {"name": "app.main/reload", "async": false}
  ]
}
"#;

/// The output of the CLI for `args`, which must succeed without diagnostics.
fn light_cljs(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The CLI's dev build of `app.main` from the source root `root`, in `out`.
fn dev_build(root: &Path, out: &Path) {
    let [root, out] = [root, out].map(|path| path.to_str().unwrap());
    let args = ["--compile", "--entry", "app.main", "--source-root", root];
    assert_eq!(
        light_cljs(&[&args[..], &["--reloadable", "--out-dir", out]].concat()),
        ""
    );
}

/// A copy of examples/dev in `dir` whose app/view.cljs is `view`.
fn edited_example(dir: &Path, view: &str) -> PathBuf {
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    for name in ["state", "macros", "view", "main"] {
        let file = format!("app/{name}.cljs");
        fs::copy(Path::new(EXAMPLE).join(&file), root.join(&file)).unwrap();
    }
    fs::write(root.join("app/view.cljs"), view).unwrap();
    root
}

fn example_view() -> String {
    fs::read_to_string(Path::new(EXAMPLE).join("app/view.cljs")).unwrap()
}

/// The CLI writes the runtime once, one unit and source map per namespace in
/// dependency order, the stub client, the manifest and index.js; started by
/// the driver, the build prints what the `none` build prints. index.js loads
/// it in Node too.
#[test]
fn example_dev_build_prints_what_the_none_build_prints() {
    let dir = TempDir::new("dev-example");
    let none = dir.join("none.js");
    let out = dir.join("dev");
    fs::write(
        &none,
        light_cljs(&[
            "--compile",
            "--optimize",
            "none",
            "--entry",
            "app.main",
            "--source-root",
            EXAMPLE,
        ]),
    )
    .unwrap();
    dev_build(Path::new(EXAMPLE), &out);
    let mut files = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(
        files,
        [
            "app.macros.js",
            "app.macros.js.map",
            "app.main.js",
            "app.main.js.map",
            "app.state.js",
            "app.state.js.map",
            "app.view.js",
            "app.view.js.map",
            "client.js",
            "index.js",
            "manifest.json",
            "runtime.js",
        ]
    );
    assert_eq!(
        fs::read_to_string(out.join("manifest.json")).unwrap(),
        EXAMPLE_MANIFEST
    );
    let runtime = fs::read_to_string(out.join("runtime.js")).unwrap();
    assert!(runtime.starts_with("globalThis.__lightRuntime = "));
    let view = fs::read_to_string(out.join("app.view.js")).unwrap();
    assert!(
        view.starts_with(
            "globalThis.__lightDev.define(\"app.view\", [\"app.macros\", \"app.state\"], (__lightInteractiveRuntime) => { globalThis.__lightApplicationRoots ??= Object.create(null); return (() => {\n"
        ) && view.ends_with("\n; });\n//# sourceMappingURL=app.view.js.map\n"),
        "{view}"
    );
    // Units read and write the registry; nothing is sealed in a closure.
    assert!(!view.contains("let g"), "{view}");
    assert!(view.contains("globalThis.__lightApplicationRoots[\"app.view/render\"] = "));
    assert_eq!(run_dev(&none, true, &[]).unwrap(), EXAMPLE_RECORD);
    assert_eq!(run_dev(&out, true, &[]).unwrap(), EXAMPLE_RECORD);
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-e")
        .arg(format!(
            "require({:?}); globalThis.__lightDevLoaded.then(start => start());",
            out.join("index.js")
        ))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "start\nclicks: [1]\n"
    );
}

/// In a dev build the namespaces examples/features reaches only through
/// `light.lazy/loadable` load after the entry, as in a single file, and the
/// entry's value is still the program's: its `start` loads both features.
#[test]
fn features_example_dev_build_prints_what_the_none_build_prints() {
    let dir = TempDir::new("dev-features");
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");
    let (program, sources) = analyze_namespace_entry_with_sources(
        "app.main",
        &[PathBuf::from(root)],
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let [none, dev] = none_and_dev(&program, &sources, &dir, true);
    let none = none.unwrap();
    assert!(
        none.contains("menu: Editor | Reports\\neditor: 4 documents"),
        "{none}"
    );
    assert_eq!(dev.unwrap(), none);
}

/// A unit of `app.view` compiled from an edited source and applied with the
/// hook protocol changes the next render; running the `app.state` unit again
/// keeps the `defonce` atom, so the clicks go on from the first render.
#[test]
fn a_replaced_unit_changes_the_next_render_and_keeps_the_state() {
    let dir = TempDir::new("dev-edit");
    let out = dir.join("dev");
    dev_build(Path::new(EXAMPLE), &out);
    let view = example_view().replace(
        "(js/console.log (describe card))",
        "(js/console.log \"render:\" (describe card))",
    );
    assert_ne!(view, example_view());
    let edited = dir.join("edited");
    dev_build(&edited_example(&dir, &view), &edited);
    assert_ne!(
        fs::read_to_string(edited.join("app.view.js")).unwrap(),
        fs::read_to_string(out.join("app.view.js")).unwrap()
    );
    let units = [out.join("app.state.js"), edited.join("app.view.js")];
    assert_eq!(
        run_dev(&out, true, &units).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"start\nclicks: [1]\nreloading\nrender: clicks: [2]\n"}"#
    );
}

/// A unit that redefines the record keeps the instances of the old definition
/// usable: their fields, and the protocol methods they were extended with,
/// though they are not instances of the new type.
#[test]
fn a_replaced_record_definition_keeps_old_instances_usable() {
    let dir = TempDir::new("dev-record");
    let out = dir.join("dev");
    dev_build(Path::new(EXAMPLE), &out);
    let view = r#"(ns app.view
  (:require [app.macros :refer [labeled]]
            [app.state :as state]))

(defprotocol Describe
  (describe [this]))

(defrecord Card [label clicks unit])

(extend-protocol Describe
  Card
  (describe [card] (labeled (:label card) (:clicks card) (:unit card))))

(defn render []
  (let [old (:card @state/state)
        card (->Card "clicks" (:clicks @state/state) "x")]
    (js/console.log "old:" (describe old) (:clicks old) (record? old) (instance? Card old))
    (swap! state/state assoc :card card)
    (js/console.log (describe card))))
"#;
    let edited = dir.join("edited");
    dev_build(&edited_example(&dir, view), &edited);
    assert_eq!(
        run_dev(&out, true, &[edited.join("app.view.js")]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"start\nclicks: [1]\nreloading\nold: clicks: [1] 1 true false\nclicks: [2x]\n"}"#
    );
}

/// The manifest lists the hooks in definition order, also when a declaration
/// came first, marking the `-async` ones, and the driver runs the
/// before-load hooks in reverse order and the after-load hooks in order
/// around the units, waiting for each async hook's continuation.
#[test]
fn hooks_run_in_order_around_the_applied_units() {
    let dir = TempDir::new("dev-hooks");
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/main.cljs"),
        r#"(ns app.main)
(declare start-second stop-second)
(defn ^:dev/before-load stop-first [] (js/console.log "stop first"))
(defn ^:dev/before-load-async stop-second [done]
  (js/console.log "stop second")
  (js/setTimeout done 0))
(defn ^:dev/after-load-async start-first [done]
  (js/setTimeout (fn [] (js/console.log "start first") (done)) 0))
(defn ^{:dev/after-load true} start-second [] (js/console.log "start second"))
(defn ^{:dev/after-load false} not-a-hook [] (js/console.log "not a hook"))
(js/console.log "app.main runs")
"#,
    )
    .unwrap();
    let out = dir.join("dev");
    dev_build(&root, &out);
    let manifest = fs::read_to_string(out.join("manifest.json")).unwrap();
    assert!(
        manifest.ends_with(
            r#"  "before-load": [
    {"name": "app.main/stop-first", "async": false},
    {"name": "app.main/stop-second", "async": true}
  ],
  "after-load": [
    {"name": "app.main/start-first", "async": true},
    {"name": "app.main/start-second", "async": false}
  ]
}
"#
        ),
        "{manifest}"
    );
    assert_eq!(
        run_dev(&out, false, &[out.join("app.main.js")]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"app.main runs\nstop second\nstop first\napp.main runs\nstart first\nstart second\n"}"#
    );
}

/// Writes `sources` (path under the source root and text) into `dir`/src
/// and returns the root.
fn source_root(dir: &Path, sources: &[(&str, &str)]) -> PathBuf {
    let root = dir.join("src");
    for (path, text) in sources {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    root
}

/// The records of the `none` build and the dev build of `entry` from
/// `root`, which must be equal, built and run under `dir`.
fn none_matches_dev(entry: &str, root: &Path, dir: &Path, start: bool) -> String {
    let (program, sources) = analyze_namespace_entry_with_sources(
        entry,
        &[root.to_path_buf()],
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let [none, dev] = none_and_dev(&program, &sources, &dir.join("builds"), start);
    let none = none.unwrap();
    assert_eq!(dev.unwrap(), none);
    none
}

/// Namespaces named `runtime`, `client` and `index` get the unit files
/// `runtime$.js`, `client$.js` and `index$.js`, which no namespace's file
/// can be, instead of replacing the build's own files.
#[test]
fn namespaces_named_like_build_files_keep_their_own_unit_files() {
    let dir = TempDir::new("dev-names");
    let root = source_root(
        &dir,
        &[
            ("runtime.cljs", "(ns runtime)\n(def value 1)\n"),
            ("client.cljs", "(ns client)\n(def value 2)\n"),
            (
                "index.cljs",
                "(ns index (:require [runtime] [client]))\n(js/console.log (+ runtime/value client/value))\n",
            ),
        ],
    );
    let record = none_matches_dev("index", &root, &dir, false);
    assert!(record.contains(r#""stdout":"3\n""#), "{record}");
    let mut files = fs::read_dir(dir.join("builds/dev"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.ends_with(".map"))
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(
        files,
        [
            "client$.js",
            "client.js",
            "index$.js",
            "index.js",
            "manifest.json",
            "runtime$.js",
            "runtime.js",
        ]
    );
    // The CLI warns of single-segment namespaces in a dev build.
    let out = dir.join("cli");
    let build = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--entry", "index", "--source-root"])
        .arg(&root)
        .args(["--reloadable", "--out-dir"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(build.status.success(), "{build:?}");
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(out.join("index.js"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "3\n");
}

/// Single-file output registers every namespace before its first form runs,
/// so a namespace that loads first can already find the namespaces after it;
/// the dev build's first unit registers them all too.
#[test]
fn namespaces_loading_first_find_the_later_ones_as_at_none() {
    let dir = TempDir::new("dev-reflection");
    let root = source_root(
        &dir,
        &[
            (
                "app/dep.cljs",
                "(ns app.dep)\n(js/console.log (boolean (find-ns 'app.main)) (boolean (find-ns 'app.dep)) (str *ns*))\n(defn value [] 1)\n",
            ),
            (
                "app/main.cljs",
                "(ns app.main (:require [app.dep :as dep]))\n(js/console.log (boolean (find-ns 'app.main)) (str *ns*) (dep/value))\n",
            ),
        ],
    );
    let record = none_matches_dev("app.main", &root, &dir, false);
    assert!(
        record.contains(r#""stdout":"true true app.dep\ntrue app.main 1\n""#),
        "{record}"
    );
}

/// In a page, `index.js` loads each file as a script element, and an error
/// thrown while one runs rejects `__lightDevLoaded` and leaves the later files
/// unloaded. A small document stub in Node stands in for the page: it runs
/// each script, reports a throw as the page's `error` event, then fires
/// `load`.
#[test]
fn index_rejects_in_a_page_when_a_unit_throws() {
    let dir = TempDir::new("dev-page");
    let root = source_root(
        &dir,
        &[
            (
                "app/dep.cljs",
                "(ns app.dep)\n(js/console.log \"dep runs\")\n(throw (js/Error. \"dep failed\"))\n",
            ),
            (
                "app/main.cljs",
                "(ns app.main (:require [app.dep]))\n(js/console.log \"main runs\")\n",
            ),
        ],
    );
    let out = dir.join("dev");
    dev_build(&root, &out);
    let page = r#"
const { readFileSync } = require("node:fs");
const { runInThisContext } = require("node:vm");
const { fileURLToPath, pathToFileURL } = require("node:url");
const listeners = new Set();
globalThis.addEventListener = (type, listener) => { if (type === "error") listeners.add(listener); };
globalThis.removeEventListener = (type, listener) => listeners.delete(listener);
const index = process.argv[1];
globalThis.document = {
  currentScript: { src: pathToFileURL(index).href },
  baseURI: pathToFileURL(index).href,
  createElement: () => ({}),
  head: {
    append(script) {
      setTimeout(() => {
        const file = fileURLToPath(script.src);
        console.log("script " + file.slice(file.lastIndexOf("/") + 1));
        try { runInThisContext(readFileSync(file, "utf8"), { filename: file }); }
        catch (error) { for (const listener of [...listeners]) listener({ error, message: String(error) }); }
        script.onload();
      }, 0);
    },
  },
};
require(index);
globalThis.__lightDevLoaded.then(
  value => console.log("resolved " + value),
  error => console.log("rejected " + error.message));
"#;
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-e")
        .arg(page)
        .arg(out.join("index.js"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "script runtime.js\nscript client.js\nscript app.dep.js\ndep runs\nrejected dep failed\n"
    );
}

/// The CLI analyzes a dev build in warning mode (docs/dev-workflow.md,
/// "Dev-mode contract"): an undeclared Var is a warning there and an error
/// in an ordinary build.
#[test]
fn dev_builds_analyze_in_warning_mode() {
    let dir = TempDir::new("dev-warnings");
    let root = source_root(
        &dir,
        &[(
            "app/main.cljs",
            "(ns app.main)\n(defn later [] (missing 1))\n(js/console.log \"ok\")\n",
        )],
    );
    let root = root.to_str().unwrap();
    let out = dir.join("dev");
    let base = ["--compile", "--entry", "app.main", "--source-root", root];
    let strict = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(base)
        .output()
        .unwrap();
    let error = String::from_utf8(strict.stderr).unwrap();
    assert!(
        !strict.status.success() && error.contains("missing"),
        "{error}"
    );
    let dev = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(base)
        .args(["--reloadable", "--out-dir", out.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8(dev.stderr).unwrap();
    assert!(dev.status.success(), "{stderr}");
    assert_eq!(
        stderr,
        "WARNING: Use of undeclared Var app.main/missing [undeclared-var] (source 0, bytes 30..37)\n"
    );
    assert_eq!(
        run_dev(&out, false, &[]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"ok\n"}"#
    );
}

/// Each size ratchet fixture, as the namespace `fixture`, prints the same as
/// a dev build and at `none`.
#[test]
fn ratchet_fixtures_print_what_the_none_build_prints() {
    let dir = TempDir::new("dev-fixtures");
    std::thread::scope(|scope| {
        for chunk in FIXTURES.chunks(FIXTURES.len().div_ceil(4)) {
            let dir = &dir;
            scope.spawn(move || {
                for fixture in chunk {
                    let root = dir.join(fixture.name);
                    fs::create_dir_all(&root).unwrap();
                    fs::write(
                        root.join("fixture.cljs"),
                        format!("(ns fixture)\n{}\n", fixture.source),
                    )
                    .unwrap();
                    let (program, sources) = analyze_namespace_entry_with_sources(
                        "fixture",
                        std::slice::from_ref(&root),
                        &mut MacroHost::new(),
                        AnalysisOptions::default(),
                        &mut Vec::new(),
                    )
                    .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));
                    let [none, dev] = none_and_dev(&program, &sources, &root.join("builds"), false);
                    let none = none.unwrap_or_else(|error| panic!("{}: {error}", fixture.name));
                    assert_eq!(dev.unwrap(), none, "{}", fixture.name);
                }
            });
        }
    });
}

/// `node --enable-source-maps` reports a throw inside a unit at its `.cljs`
/// line and column, under the function's source name.
#[test]
fn unit_source_maps_point_node_stack_traces_at_the_sources() {
    let dir = TempDir::new("dev-maps");
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/util.cljs"),
        "(ns app.util)\n\n(defn check [x]\n  (when (neg? x)\n    (throw (js/Error. (str \"negative \" x))))\n  x)\n",
    )
    .unwrap();
    fs::write(
        root.join("app/main.cljs"),
        "(ns app.main\n  (:require [app.util :as u]))\n\n(u/check 1)\n(u/check -2)\n",
    )
    .unwrap();
    let out = dir.join("dev");
    dev_build(&root, &out);
    let map = fs::read_to_string(out.join("app.util.js.map")).unwrap();
    assert!(
        map.starts_with("{\"version\":3,\"file\":\"app.util.js\",\"sources\":[\"../src/app/util.cljs\",\"../src/app/main.cljs\"],\"names\":["),
        "{map}"
    );
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("--enable-source-maps")
        .arg(out.join("index.js"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let frame = stderr
        .lines()
        .find(|line| line.starts_with("    at "))
        .unwrap_or_else(|| panic!("no stack trace in\n{stderr}"));
    let util = fs::canonicalize(root.join("app/util.cljs")).unwrap();
    assert_eq!(frame, format!("    at check ({}:5:12)", util.display()));
}

/// `--reloadable` and `--out-dir` go together, and a dev build takes none of
/// the flags of a single-file build.
#[test]
fn reloadable_flags_are_checked() {
    let dir = TempDir::new("dev-flags");
    let out = dir.join("dev");
    let out = out.to_str().unwrap();
    let base = ["--compile", "--entry", "app.main", "--source-root", EXAMPLE];
    for (extra, message) in [
        (&["--reloadable"][..], "--reloadable requires --out-dir DIR"),
        (&["--out-dir", out][..], "--out-dir requires --reloadable"),
        (
            &["--reloadable", "--out-dir", out, "--esm"][..],
            "it excludes --esm",
        ),
        (
            &["--optimize", "none", "--reloadable", "--out-dir", out][..],
            "it excludes --esm, --optimize",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(base.iter().chain(extra))
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            !output.status.success() && stderr.contains(message),
            "{extra:?}: {stderr}"
        );
    }
    assert!(!Path::new(out).exists());
}
