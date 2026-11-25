//! The dev build (docs/dev-workflow.md, "The dev build" and "Phase 0"):
//! `--compile --entry NS --source-root DIR --reloadable --out-dir DIR`, run in
//! Node by tests/support/dev_execution.mjs, the driver that also runs its
//! `--optimize none` oracle and applies replaced units with the hook protocol.
//! The phase oracles check the dev builds of their programs as well
//! (tests/support/phase_oracle.rs).
use super::size_ratchet::ratchet_fixtures::FIXTURES;
use crate::dev_execution::{none_and_dev, run_dev};
use crate::reload_fixtures::{FIXTURES as RELOAD_FIXTURES, record};
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

/// `args` with `--no-cache` after `--compile` when they compile namespaces
/// and name no cache, so the default cache directory (`.light-cljs/cache`
/// under the current directory) carries nothing from one test to the next.
fn uncached<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut args = args.to_vec();
    let named = args
        .iter()
        .any(|arg| *arg == "--cache-dir" || *arg == "--no-cache");
    if args.contains(&"--entry")
        && !named
        && let Some(at) = args.iter().position(|arg| *arg == "--compile")
    {
        args.insert(at + 1, "--no-cache");
    }
    args
}

/// The output of the CLI for `args`, which must succeed without diagnostics.
fn light_cljs(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(uncached(args))
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
    let args = [
        "--compile",
        "--no-cache",
        "--entry",
        "app.main",
        "--source-root",
        root,
    ];
    assert_eq!(
        light_cljs(&[&args[..], &["--reloadable", "--out-dir", out]].concat()),
        ""
    );
}

/// A copy of examples/dev in `dir` whose app/view.cljs is `view`.
fn edited_example(dir: &Path, view: &str) -> PathBuf {
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    // The analyzer reports canonical source paths; `$TMPDIR` may sit behind a symlink.
    let root = root.canonicalize().unwrap();
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
    // The build's globals end in its key, the key of the dev build of no
    // light-cljs.edn whose entry is app.main (docs/value-layout.md,
    // section 1).
    let key = light_cljs::dev_build::dev_key(None, "app.main");
    let key = key.key();
    let runtime = fs::read_to_string(out.join("runtime.js")).unwrap();
    assert!(runtime.starts_with(&format!("globalThis.__lightRuntime${key} = ")));
    let view = fs::read_to_string(out.join("app.view.js")).unwrap();
    assert!(
        view.starts_with(&format!(
            "globalThis.__lightDev${key}.define(\"app.view\", [\"app.macros\", \"app.state\"], (__lightInteractiveRuntime) => {{ globalThis.__lightApplicationRoots${key} ??= Object.create(null); return (() => {{\n"
        )) && view.ends_with("\n; });\n//# sourceMappingURL=app.view.js.map\n"),
        "{view}"
    );
    // Units read and write the registry; nothing is sealed in a closure.
    assert!(!view.contains("let g"), "{view}");
    assert!(view.contains(&format!(
        "globalThis.__lightApplicationRoots${key}[\"app.view/render\"] = "
    )));
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
    // The analyzer reports canonical source paths; `$TMPDIR` may sit behind a symlink.
    let root = root.canonicalize().unwrap();
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

/// The reload fixtures (tests/support/reload_fixtures.rs) through the Phase
/// 0 driver: the build of each row's sources started, then the units of the
/// namespaces the row reloads, from a fresh dev build of the edited sources,
/// applied between the hooks, print the row's output.
#[test]
fn reload_fixtures_print_their_output_through_the_driver() {
    for fixture in RELOAD_FIXTURES {
        let dir = TempDir::new("dev-reload-fixture");
        let first = source_root(&dir.join("first"), fixture.files);
        let edited = source_root(&dir.join("edited"), fixture.files);
        fs::write(edited.join(fixture.edit.0), fixture.edit.1).unwrap();
        dev_build(&first, &dir.join("first/dev"));
        dev_build(&edited, &dir.join("edited/dev"));
        let units = fixture
            .reloads
            .iter()
            .map(|namespace| dir.join("edited/dev").join(format!("{namespace}.js")))
            .collect::<Vec<_>>();
        assert_eq!(
            run_dev(&dir.join("first/dev"), true, &units).unwrap(),
            record(fixture.stdout),
            "{}",
            fixture.name
        );
    }
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
        .args([
            "--compile",
            "--no-cache",
            "--entry",
            "index",
            "--source-root",
        ])
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

/// A page in Node for `index.js` (its path is the first argument): a
/// document stub whose script elements behave as a browser's with
/// `async = false`. Each appended script is requested at once ("request F"),
/// its fetch finishes after a delay that is longer the earlier it was
/// appended, so later files arrive first, and the scripts run in the order
/// they were appended as soon as each and those before it have arrived
/// ("script F"), each as the document's `currentScript`. A script whose
/// file is missing fires `error` and is skipped; one that throws reports the
/// throw as the page's `error` event while it is the `currentScript`; each
/// then fires `load`. The page prints how `__lightDevLoaded` settles,
/// then defines one more unit, as a reload would, and prints whether it ran.
const PAGE: &str = r#"
const { existsSync, readFileSync } = require("node:fs");
const { runInThisContext } = require("node:vm");
const { fileURLToPath, pathToFileURL } = require("node:url");
const listeners = new Set();
globalThis.addEventListener = (type, listener) => { if (type === "error") listeners.add(listener); };
globalThis.removeEventListener = (type, listener) => listeners.delete(listener);
const index = process.argv[1];
const name = file => file.slice(file.lastIndexOf("/") + 1);
const scripts = [];
let appended = 0;
const run = () => {
  while (scripts.length > 0 && scripts[0].arrived) {
    const script = scripts.shift();
    const file = fileURLToPath(script.src);
    if (!existsSync(file)) { script.onerror(); continue; }
    console.log("script " + name(file));
    const current = document.currentScript;
    document.currentScript = script;
    try { runInThisContext(readFileSync(file, "utf8"), { filename: file }); }
    catch (error) { for (const listener of [...listeners]) listener({ error, message: String(error) }); }
    finally { document.currentScript = current; }
    script.onload();
  }
};
globalThis.document = {
  currentScript: { src: pathToFileURL(index).href },
  baseURI: pathToFileURL(index).href,
  createElement: () => ({}),
  head: {
    append(script) {
      if (script.async !== false) throw new Error("a script that may run out of order");
      console.log("request " + name(script.src));
      scripts.push(script);
      setTimeout(() => { script.arrived = true; run(); }, 50 - 10 * appended++);
    },
  },
};
require(index);
globalThis.__lightDevLoaded.then(
  value => console.log("resolved " + value()),
  error => console.log("rejected " + error.message.replace(/ file:.*\//, " "))
).then(() => {
  globalThis.__lightDev.define("app.reloaded", [], () => console.log("reloaded runs"));
});
"#;

/// The lines [`PAGE`] prints for the dev build, built under `dir`, of three
/// namespaces: `app.main` requires `app.dep`, which requires `app.base`;
/// each logs as it runs, `app.dep` then runs `dep`, and `app.main`'s value
/// is `start`, which returns `started`. `edit` changes the build's
/// directory first.
fn page_lines(dir: &Path, dep: &str, edit: impl FnOnce(&Path)) -> String {
    let dep = format!("(ns app.dep (:require [app.base]))\n(js/console.log \"dep runs\")\n{dep}");
    let root = source_root(
        dir,
        &[
            (
                "app/base.cljs",
                "(ns app.base)\n(js/console.log \"base runs\")\n",
            ),
            ("app/dep.cljs", &dep),
            (
                "app/main.cljs",
                "(ns app.main (:require [app.dep]))\n(js/console.log \"main runs\")\n(defn start [] \"started\")\n",
            ),
        ],
    );
    let out = dir.join("dev");
    dev_build(&root, &out);
    edit(&out);
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-e")
        .arg(PAGE)
        .arg(out.join("index.js"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// In a page, `index.js` requests every file at once as a script element
/// with `async = false`, before any has arrived, and the files run in
/// dependency order although the later ones arrive first; the promise
/// resolves to the entry's value, and a unit defined afterwards runs.
#[test]
fn index_requests_every_file_at_once_and_runs_them_in_order() {
    let dir = TempDir::new("dev-page");
    let lines = page_lines(&dir, "", |_| {});
    assert_eq!(
        lines,
        "request runtime.js\nrequest client.js\nrequest app.base.js\nrequest app.dep.js\nrequest app.main.js\n\
         script runtime.js\nscript client.js\nscript app.base.js\nbase runs\nscript app.dep.js\ndep runs\n\
         script app.main.js\nmain runs\nresolved started\nreloaded runs\n"
    );
}

/// In a page, an error thrown while a unit runs rejects `__lightDevLoaded`
/// with that error, and no later unit runs, though its file was requested
/// and its script runs; once the promise has settled, a unit defined (as a
/// reload would) runs again.
#[test]
fn index_rejects_in_a_page_when_a_unit_throws() {
    let dir = TempDir::new("dev-page");
    let lines = page_lines(&dir, "(throw (js/Error. \"dep failed\"))\n", |_| {});
    assert_eq!(
        lines,
        "request runtime.js\nrequest client.js\nrequest app.base.js\nrequest app.dep.js\nrequest app.main.js\n\
         script runtime.js\nscript client.js\nscript app.base.js\nbase runs\nscript app.dep.js\ndep runs\n\
         script app.main.js\nrejected dep failed\nreloaded runs\n"
    );
}

/// In a page, a file that cannot load rejects `__lightDevLoaded`, and no
/// unit after it runs.
#[test]
fn index_rejects_in_a_page_when_a_file_cannot_load() {
    let dir = TempDir::new("dev-page");
    let lines = page_lines(&dir, "", |out| {
        fs::remove_file(out.join("app.base.js")).unwrap();
    });
    assert_eq!(
        lines,
        "request runtime.js\nrequest client.js\nrequest app.base.js\nrequest app.dep.js\nrequest app.main.js\n\
         script runtime.js\nscript client.js\nscript app.dep.js\nscript app.main.js\n\
         rejected cannot load app.base.js\nreloaded runs\n"
    );
}

/// In a page, the first failure is the one `__lightDevLoaded` rejects with:
/// a later file that cannot load does not replace a unit's error.
#[test]
fn index_rejects_with_the_first_failure_when_a_later_file_cannot_load() {
    let dir = TempDir::new("dev-page");
    let lines = page_lines(&dir, "(throw (js/Error. \"dep failed\"))\n", |out| {
        fs::remove_file(out.join("app.main.js")).unwrap();
    });
    assert_eq!(
        lines,
        "request runtime.js\nrequest client.js\nrequest app.base.js\nrequest app.dep.js\nrequest app.main.js\n\
         script runtime.js\nscript client.js\nscript app.base.js\nbase runs\nscript app.dep.js\ndep runs\n\
         rejected dep failed\nreloaded runs\n"
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
    let base = [
        "--compile",
        "--no-cache",
        "--entry",
        "app.main",
        "--source-root",
        root,
    ];
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
    // The analyzer reports canonical source paths; `$TMPDIR` may sit behind a symlink.
    let root = root.canonicalize().unwrap();
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
        map.starts_with(
            "{\"version\":3,\"file\":\"app.util.js\",\"sources\":[\"../src/app/util.cljs\"],\"names\":["
        ),
        "{map}"
    );
    let main_map = fs::read_to_string(out.join("app.main.js.map")).unwrap();
    assert!(
        main_map.starts_with(
            "{\"version\":3,\"file\":\"app.main.js\",\"sources\":[\"../src/app/main.cljs\"],\"names\":["
        ),
        "{main_map}"
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
    let util = light_cljs::canonical_path::canonicalize(root.join("app/util.cljs")).unwrap();
    assert_eq!(frame, format!("    at check ({}:5:12)", util.display()));
}

/// `--reloadable` requires `--out-dir`, and a dev build takes none of the
/// flags of a single-file build. `--out-dir` alone writes a chunked build
/// (tests/compiler/chunks.rs).
#[test]
fn reloadable_flags_are_checked() {
    let dir = TempDir::new("dev-flags");
    let out = dir.join("dev");
    let out = out.to_str().unwrap();
    let base = [
        "--compile",
        "--no-cache",
        "--entry",
        "app.main",
        "--source-root",
        EXAMPLE,
    ];
    for (extra, message) in [
        (&["--reloadable"][..], "--reloadable requires --out-dir DIR"),
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

/// Read every file in a dev build directory into sorted `(name, bytes)` pairs.
fn read_dev_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            let bytes = fs::read(entry.path()).unwrap();
            (name, bytes)
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// Parallel dev emission and streaming writes (`--jobs 8`) produce byte-for-byte
/// identical `.js`, `.js.map`, `manifest.json`, and `index.js` files to `--jobs 1`,
/// with compact per-unit `"sources"` entries and working Node source-map traces.
#[test]
fn parallel_dev_build_matches_single_slot_byte_for_byte() {
    let dir = TempDir::new("dev-parallel-determinism");
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    // The analyzer reports canonical source paths; `$TMPDIR` may sit behind a symlink.
    let root = root.canonicalize().unwrap();
    // 40 namespaces so coarse batching creates multiple batches across 8 slots.
    let count = 40usize;
    fs::write(
        root.join("app/macros.cljs"),
        "(ns app.macros)\n(defmacro tagged [tag x] `[~tag ~x])\n",
    )
    .unwrap();
    let mut requires = String::new();
    let mut calls = String::new();
    for i in 0..count {
        let dep = if i == 0 {
            "(:require [app.macros :refer [tagged]])".to_owned()
        } else {
            format!(
                "(:require [app.macros :refer [tagged]] [app.n{:02} :as prev])",
                i - 1
            )
        };
        let body = if i == 0 {
            "(defn step [x]\n  (when (neg? x)\n    (throw (js/Error. (str \"neg \" x))))\n  (tagged :n00 (inc x)))\n"
                .to_owned()
        } else {
            format!(
                "(defn step [x]\n  (let [[_ v] (prev/step x)]\n    (tagged :n{i:02} (+ v {i}))))\n"
            )
        };
        fs::write(
            root.join(format!("app/n{i:02}.cljs")),
            format!("(ns app.n{i:02}\n  {dep})\n{body}"),
        )
        .unwrap();
        requires.push_str(&format!("\n            [app.n{i:02}]"));
        if i == count - 1 {
            calls.push_str(&format!("(app.n{i:02}/step 1)\n"));
        }
    }
    fs::write(
        root.join("app/main.cljs"),
        format!(
            "(ns app.main\n  (:require{requires}))\n(defn ^:dev/after-load reload [] :ok)\n(js/console.log (str {calls}))\n"
        ),
    )
    .unwrap();

    // 1. Library API: emit_dev_build_on and write_dev_build_on across slots 1, 2, 8.
    let (mut program, sources) = analyze_namespace_entry_with_sources(
        "app.main",
        std::slice::from_ref(&root),
        &mut MacroHost::new(),
        AnalysisOptions {
            warning_mode: true,
            ..AnalysisOptions::default()
        },
        &mut Vec::new(),
    )
    .unwrap();
    program.flatten_leaves();
    let source_files: Vec<_> = sources
        .iter()
        .map(|(path, text)| {
            let rel = path.strip_prefix(&root).unwrap().to_str().unwrap();
            (format!("../src/{rel}"), text.as_str())
        })
        .collect();
    let source_refs: Vec<_> = source_files
        .iter()
        .map(|(url, text)| light_cljs::source_map::SourceFile { url, text })
        .collect();

    let s1 = light_cljs::scheduler::Slots::ONE;
    let s2 = light_cljs::scheduler::Slots::new(2).unwrap();
    let s8 = light_cljs::scheduler::Slots::new(8).unwrap();
    let emitted_1 = light_cljs::dev_build::emit_dev_build_on(&program, &source_refs, None, s1);
    let emitted_2 = light_cljs::dev_build::emit_dev_build_on(&program, &source_refs, None, s2);
    let emitted_8 = light_cljs::dev_build::emit_dev_build_on(&program, &source_refs, None, s8);
    assert_eq!(emitted_1, emitted_2);
    assert_eq!(emitted_1, emitted_8);

    let lib_out_1 = dir.join("lib-j1");
    let lib_out_8 = dir.join("lib-j8");
    light_cljs::dev_build::write_dev_build_on(&program, &source_refs, None, &lib_out_1, s1, true)
        .unwrap();
    light_cljs::dev_build::write_dev_build_on(&program, &source_refs, None, &lib_out_8, s8, true)
        .unwrap();
    let lib_files_1 = read_dev_dir(&lib_out_1);
    let lib_files_8 = read_dev_dir(&lib_out_8);
    assert_eq!(lib_files_1, lib_files_8);
    let mut emitted_sorted: Vec<_> = emitted_1
        .into_iter()
        .map(|f| (f.name, f.text.into_bytes()))
        .collect();
    emitted_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(lib_files_1, emitted_sorted);

    // 2. CLI end-to-end: --compile --reloadable --jobs 1 vs --jobs 8 (with --stats).
    let cli_out_1 = dir.join("cli-j1");
    let cli_out_8 = dir.join("cli-j8");
    for (jobs, out) in [("1", &cli_out_1), ("8", &cli_out_8)] {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args([
                "--compile",
                "--no-cache",
                "--entry",
                "app.main",
                "--source-root",
                root.to_str().unwrap(),
                "--reloadable",
                "--out-dir",
                out.to_str().unwrap(),
                "--jobs",
                jobs,
                "--stats",
            ])
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "--jobs {jobs}: {stderr}");
        let stat = |key: &str| -> f64 {
            stderr
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(' ')?;
                    (k == key).then(|| v.parse::<f64>().unwrap())
                })
                .unwrap_or_else(|| panic!("missing {key} in:\n{stderr}"))
        };
        let wall = stat("time.wall-ms");
        let emission = stat("time.emission-ms");
        let write = stat("time.write-ms");
        assert!(emission >= 0.0 && emission <= wall, "jobs={jobs}: {stderr}");
        assert!(write >= 0.0 && write <= wall, "jobs={jobs}: {stderr}");
    }
    let cli_files_1 = read_dev_dir(&cli_out_1);
    let cli_files_8 = read_dev_dir(&cli_out_8);
    assert_eq!(cli_files_1, cli_files_8);

    // Every unit map only lists the single source it references.
    for i in 0..count {
        let map = fs::read_to_string(cli_out_8.join(format!("app.n{i:02}.js.map"))).unwrap();
        let expected = format!(
            "{{\"version\":3,\"file\":\"app.n{i:02}.js\",\"sources\":[\"../src/app/n{i:02}.cljs\"],\"names\":["
        );
        assert!(map.starts_with(&expected), "app.n{i:02}.js.map: {map}");
    }

    // Running the --jobs 8 build in Node prints the expected result.
    let run = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(cli_out_8.join("index.js"))
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let expected_sum: usize = 2 + (1..count).sum::<usize>();
    assert_eq!(
        String::from_utf8(run.stdout).unwrap(),
        format!("[:n{:02} {expected_sum}]\n", count - 1)
    );
}

/// Dev emission workers use the compiler's 8 MiB stack, so namespaces at the
/// parser's 127-level nesting limit emit identically at `--jobs 1` and
/// `--jobs 8` even in unoptimized builds.
#[test]
fn parallel_dev_build_preserves_accepted_nesting() {
    let dir = TempDir::new("dev-parallel-nesting");
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    // The analyzer reports canonical source paths; `$TMPDIR` may sit behind a symlink.
    let root = root.canonicalize().unwrap();
    let deep_127 = format!("{}0{}", "(let* [] ".repeat(127), ")".repeat(127));
    let deep_126 = format!("{}42{}", "(let* [] ".repeat(126), ")".repeat(126));
    fs::write(
        root.join("app/deep.cljs"),
        format!("(ns app.deep)\n{deep_127}\n(def value {deep_126})\n"),
    )
    .unwrap();
    fs::write(
        root.join("app/main.cljs"),
        "(ns app.main (:require [app.deep :as d]))\n(js/console.log d/value)\n",
    )
    .unwrap();
    let out_1 = dir.join("j1");
    let out_8 = dir.join("j8");
    for (jobs, out) in [("1", &out_1), ("8", &out_8)] {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args([
                "--compile",
                "--no-cache",
                "--entry",
                "app.main",
                "--source-root",
                root.to_str().unwrap(),
                "--reloadable",
                "--out-dir",
                out.to_str().unwrap(),
                "--jobs",
                jobs,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "--jobs {jobs}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(read_dev_dir(&out_1), read_dev_dir(&out_8));
    let run = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(out_8.join("index.js"))
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    assert_eq!(String::from_utf8(run.stdout).unwrap(), "42\n");
}

/// A worker write failure during streaming dev output cancels remaining work,
/// drains cleanly, returns `DevBuildError::Write` naming the obstructed path,
/// and leaves a subsequent retry clean.
#[test]
fn streaming_dev_build_propagates_namespace_write_errors() {
    let dir = TempDir::new("dev-stream-write-error");
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    // The analyzer reports canonical source paths; `$TMPDIR` may sit behind a symlink.
    let root = root.canonicalize().unwrap();
    let count = 24usize;
    let mut requires = String::new();
    for i in 0..count {
        fs::write(
            root.join(format!("app/n{i:02}.cljs")),
            format!("(ns app.n{i:02})\n(defn f [x] (+ x {i}))\n"),
        )
        .unwrap();
        requires.push_str(&format!("\n            [app.n{i:02}]"));
    }
    fs::write(
        root.join("app/main.cljs"),
        format!("(ns app.main\n  (:require{requires}))\n(app.n00/f 1)\n"),
    )
    .unwrap();
    let (mut program, sources) = analyze_namespace_entry_with_sources(
        "app.main",
        std::slice::from_ref(&root),
        &mut MacroHost::new(),
        AnalysisOptions {
            warning_mode: true,
            ..AnalysisOptions::default()
        },
        &mut Vec::new(),
    )
    .unwrap();
    program.flatten_leaves();
    let source_files: Vec<_> = sources
        .iter()
        .map(|(path, text)| {
            let rel = path.strip_prefix(&root).unwrap().to_str().unwrap();
            (format!("../src/{rel}"), text.as_str())
        })
        .collect();
    let source_refs: Vec<_> = source_files
        .iter()
        .map(|(url, text)| light_cljs::source_map::SourceFile { url, text })
        .collect();

    let mut expected: Vec<_> = light_cljs::dev_build::emit_dev_build_on(
        &program,
        &source_refs,
        None,
        light_cljs::scheduler::Slots::ONE,
    )
    .into_iter()
    .map(|file| (file.name, file.text.into_bytes()))
    .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));

    for (label, slots) in [
        ("j1", light_cljs::scheduler::Slots::ONE),
        ("j8", light_cljs::scheduler::Slots::new(8).unwrap()),
    ] {
        let out = dir.join(label);
        let obstructed = out.join("app.n00.js.map");
        fs::create_dir_all(&obstructed).unwrap();
        let error = light_cljs::dev_build::write_dev_build_on(
            &program,
            &source_refs,
            None,
            &out,
            slots,
            false,
        )
        .unwrap_err();
        match error {
            light_cljs::dev_build::DevBuildError::Write { path, .. } => {
                assert_eq!(path, obstructed, "{label}");
            }
            other => panic!("{label}: expected Write error, got {other:?}"),
        }
        fs::remove_dir(&obstructed).unwrap();
        light_cljs::dev_build::write_dev_build_on(&program, &source_refs, None, &out, slots, false)
            .unwrap();
        assert_eq!(read_dev_dir(&out), expected, "{label}");
    }
}

/// Dev (`--reloadable`) builds at `--jobs 1, 2, 4, 8` and cold cached dev builds
/// (`--cache-dir`, where artifact capture disables `ParallelTopLevel`) emit
/// byte-identical output and never submit 1-unit per-form `warning_mode` drains
/// to `UnitPool`.
#[test]
fn dev_build_and_cached_warning_mode_avoid_single_unit_pool_ping_pong() {
    let dir = TempDir::new("dev-warning-mode-drain-pooling");
    let root = dir.join("src");
    fs::create_dir_all(root.join("app")).unwrap();
    // The analyzer reports canonical source paths; `$TMPDIR` may sit behind a symlink.
    let root = root.canonicalize().unwrap();
    let count = 8usize;
    let mut requires = String::new();
    let mut calls = String::new();
    for i in 0..count {
        fs::write(
            root.join(format!("app/s{i}.cljs")),
            format!(
                "(ns app.s{i})\n\
                 (defrecord Rec{i} [x])\n\
                 (when false (Rec{i}. 1 2))\n\
                 (defn step [n] (+ (.-x (->Rec{i} n)) {i}))\n"
            ),
        )
        .unwrap();
        requires.push_str(&format!("\n            [app.s{i}]"));
        calls.push_str(&format!(" (app.s{i}/step {i})"));
    }
    fs::write(
        root.join("app/main.cljs"),
        format!("(ns app.main\n  (:require{requires}))\n(js/console.log (pr-str [{calls}]))\n"),
    )
    .unwrap();

    let run_cli = |extra: &[&str], out: &Path| -> String {
        let mut args = vec![
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root.to_str().unwrap(),
            "--reloadable",
            "--out-dir",
            out.to_str().unwrap(),
            "--stats",
        ];
        args.extend_from_slice(extra);
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(&args)
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{args:?}: {stderr}");
        stderr
    };
    let stat_opt = |stderr: &str, key: &str| -> Option<f64> {
        stderr.lines().find_map(|line| {
            let (k, v) = line.split_once(' ')?;
            (k == key).then(|| v.parse::<f64>().unwrap())
        })
    };
    let warning_lines = |stderr: &str| -> Vec<String> {
        stderr
            .lines()
            .filter(|line| line.contains("warning:") || line.contains("Wrong number of args"))
            .map(ToOwned::to_owned)
            .collect()
    };

    // Uncached dev build across jobs 1, 2, 4, 8: byte-identical output and
    // warnings, and zero single-unit deferred pooling.
    let out_j1 = dir.join("nocache-j1");
    let stderr_j1 = run_cli(
        &[
            "--no-cache",
            "--macro-memory-limit",
            "268435456",
            "--jobs",
            "1",
        ],
        &out_j1,
    );
    let files_j1 = read_dev_dir(&out_j1);
    let warnings_j1 = warning_lines(&stderr_j1);
    assert_eq!(warnings_j1.len(), count, "{stderr_j1}");

    for jobs in ["2", "4", "8"] {
        let out = dir.join(format!("nocache-j{jobs}"));
        let stderr = run_cli(
            &[
                "--no-cache",
                "--macro-memory-limit",
                "268435456",
                "--jobs",
                jobs,
            ],
            &out,
        );
        assert_eq!(read_dev_dir(&out), files_j1, "nocache jobs={jobs}");
        assert_eq!(warning_lines(&stderr), warnings_j1, "nocache jobs={jobs}");
        assert_eq!(
            stat_opt(&stderr, "deferred-units.pooled").unwrap_or(0.0),
            0.0,
            "nocache jobs={jobs} should not pool 1-unit warning drains:\n{stderr}"
        );
    }

    // Cold cached dev build (`--cache-dir` disables ParallelTopLevel): byte-identical
    // to j=1 with zero pooled units and zero pool jobs at j=2 and j=8.
    for jobs in ["1", "2", "8"] {
        let out = dir.join(format!("cache-j{jobs}"));
        let cache = dir.join(format!("cache-dir-j{jobs}"));
        let stderr = run_cli(
            &[
                "--cache-dir",
                cache.to_str().unwrap(),
                "--macro-memory-limit",
                "268435456",
                "--jobs",
                jobs,
            ],
            &out,
        );
        assert_eq!(read_dev_dir(&out), files_j1, "cache jobs={jobs}");
        assert_eq!(warning_lines(&stderr), warnings_j1, "cache jobs={jobs}");
        if jobs != "1" {
            assert_eq!(
                stat_opt(&stderr, "deferred-units.pooled").unwrap_or(0.0),
                0.0,
                "cache jobs={jobs} should not pool 1-unit warning drains:\n{stderr}"
            );
            assert_eq!(
                stat_opt(&stderr, "pool.jobs").unwrap_or(0.0),
                0.0,
                "cache jobs={jobs} should not ping-pong units on pool:\n{stderr}"
            );
        }
    }
}

/// Dev builds keep `hostExt` and direct slot-first paths active by default,
/// yet `extend-type`, `specify!`, direct `(aset ICounted ...)`/`(aset -pr-writer ...)`,
/// and raw core-method slot clearing must all still take effect both at initial
/// load and when introduced by a reloaded unit.
#[test]
fn dev_fast_paths_observe_later_extensions_direct_host_writes_and_slot_clears() {
    let dir = TempDir::new("dev-fast-paths");
    let initial = source_root(
        &dir.join("v1"),
        &[
            (
                "app/ext.cljs",
                r#"(ns app.ext)
(defn snapshot []
  [(counted? 5)
   (pr-str 5)
   (pr-str {:a 1 :b [2 3]})
   (count [1 2 3])
   (vector? [1])
   (map? {:a 1})])
"#,
            ),
            (
                "app/main.cljs",
                r##"(ns app.main
  (:require [app.ext :as ext]))
(defn ^:dev/after-load report []
  (js/console.log (pr-str (ext/snapshot))))
(defn start []
  (report)
  (let [o (specify! #js {}
            IPrintWithWriter (-pr-writer [_ w _] (-write w "#spec"))
            ICounted (-count [_] 9))]
    (js/console.log (pr-str o) (count o) (counted? o))))
start
"##,
            ),
        ],
    );
    let out = dir.join("dev");
    dev_build(&initial, &out);

    // Reload 1: extend-type on host type `number` for ICounted and IPrintWithWriter.
    let v2 = source_root(
        &dir.join("v2"),
        &[
            (
                "app/ext.cljs",
                r#"(ns app.ext)
(extend-type number
  ICounted (-count [n] (+ n 10))
  IPrintWithWriter (-pr-writer [n w _] (-write w (str "N:" n))))
(defn snapshot []
  [(counted? 5) (count 5) (pr-str 5) (pr-str {:a 1 :b [2 3]})])
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.ext :as ext]))
(defn ^:dev/after-load report [] (js/console.log (pr-str (ext/snapshot))))
(defn start [] (report))
start
"#,
            ),
        ],
    );
    let out_v2 = dir.join("dev-v2");
    dev_build(&v2, &out_v2);

    assert_eq!(
        run_dev(&out, true, &[out_v2.join("app.ext.js")]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"[false \"5\" \"{:a 1, :b [2 3]}\" 3 true true]\n#spec 9 true\n[true N:15 \"N:5\" \"{:a N:1, :b [N:2 N:3]}\"]\n"}"#
    );

    // Reload 2: direct `(aset ICounted/IPrintWithWriter/-pr-writer "boolean" ...)`
    // without `extend-type`, plus raw slot clearing on PersistentVector.prototype.
    let v3 = source_root(
        &dir.join("v3"),
        &[
            (
                "app/ext.cljs",
                r#"(ns app.ext)
(aset ICounted "boolean" true)
(aset IPrintWithWriter "boolean" true)
(aset -pr-writer "boolean" (fn [b w _] (-write w (if b "YES" "NO"))))
(set! (.. PersistentVector -prototype -cljs$core$ICounted$_count$arity$1) nil)
(defn snapshot []
  [(counted? true) (pr-str true) (count [1 2 3])])
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.ext :as ext]))
(defn ^:dev/after-load report [] (js/console.log (pr-str (ext/snapshot))))
(defn start [] (report))
start
"#,
            ),
        ],
    );
    let out_v3 = dir.join("dev-v3");
    dev_build(&v3, &out_v3);

    assert_eq!(
        run_dev(&out, true, &[out_v3.join("app.ext.js")]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"[false \"5\" \"{:a 1, :b [2 3]}\" 3 true true]\n#spec 9 true\n[YES \"YES\" 3]\n"}"#
    );
}

/// Dev `defprotocol` dispatchers use `protocol_fast_path` (direct slot calls),
/// yet hot-reloaded protocol redefinitions, late `extend-type` on records and
/// host types (`string`, `nil`, `default`), `specify!`, and multi-arity /
/// arity-error dispatch must all keep working.
#[test]
fn dev_protocol_fast_path_preserves_reload_extend_type_specify_and_arities() {
    let dir = TempDir::new("dev-protocol-fast");
    let v1 = source_root(
        &dir.join("v1"),
        &[
            (
                "app/proto.cljs",
                r#"(ns app.proto)
(defprotocol P
  (one [this])
  (two [this] [this x]))
(defrecord Box [v]
  P
  (one [this] (:v this))
  (two [this] (:v this))
  (two [this x] (+ (:v this) x)))
(defonce saved-box (->Box 10))
(defn call-all []
  [(one saved-box) (two saved-box) (two saved-box 5)])
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.proto :as p]))
(defn ^:dev/after-load after [] (js/console.log (pr-str (p/call-all))))
(defn start []
  (let [s (specify! #js {}
            p/P
            (one [_] :spec-one)
            (two ([_] :spec-two-1) ([_ x] [:spec-two-2 x])))]
    (js/console.log (pr-str (p/call-all)) (pr-str [(p/one s) (p/two s) (p/two s 3)]))))
start
"#,
            ),
        ],
    );
    let out = dir.join("dev");
    dev_build(&v1, &out);

    // Reload: redefine protocol P, extend host types `string`, `nil`, and
    // `default`, and keep dispatching on the pre-reload `saved-box` instance.
    let v2 = source_root(
        &dir.join("v2"),
        &[
            (
                "app/proto.cljs",
                r#"(ns app.proto)
(defprotocol P
  (one [this])
  (two [this] [this x]))
(defrecord Box [v]
  P
  (one [this] (* 2 (:v this)))
  (two [this] (* 2 (:v this)))
  (two [this x] (+ (* 2 (:v this)) x)))
(extend-type string
  P
  (one [s] (str "S:" s))
  (two ([s] s) ([s x] (str s ":" x))))
(extend-type nil
  P
  (one [_] :nil-one)
  (two ([_] :nil-1) ([_ x] [:nil-2 x])))
(extend-type default
  P
  (one [x] [:def x])
  (two ([x] [:def1 x]) ([x y] [:def2 x y])))
(defonce saved-box (->Box 10))
(defn call-all []
  [(one saved-box) (two saved-box 5)
   (one (->Box 10)) (two (->Box 10) 5)
   (one "hi") (two "hi" 7)
   (one nil) (two nil 9)
   (one 42)
   (try (two saved-box 1 2) :no-err (catch :default _ :arity-err))])
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.proto :as p]))
(defn ^:dev/after-load after [] (js/console.log (pr-str (p/call-all))))
(defn start [] (after))
start
"#,
            ),
        ],
    );
    let out_v2 = dir.join("dev-v2");
    dev_build(&v2, &out_v2);

    assert_eq!(
        run_dev(&out, true, &[out_v2.join("app.proto.js")]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"[10 10 15] [:spec-one :spec-two-1 [:spec-two-2 3]]\n[10 15 20 25 \"S:hi\" \"hi:7\" :nil-one [:nil-2 9] [:def 42] :arity-err]\n"}"#
    );
}

/// Dev `satisfies?` and `implements?` fast paths (both inline user-protocol
/// marker checks and cached `coreInfo` mask/hostExt checks) must match
/// ClojureScript semantics across core protocols, user protocols, `specify!`,
/// late `extend-type`, and direct `(aset ICounted ...)` mutations on reload.
#[test]
fn dev_satisfies_and_implements_fast_paths_preserve_core_user_and_reload_semantics() {
    let dir = TempDir::new("dev-satisfies-fast");
    let v1 = source_root(
        &dir.join("v1"),
        &[
            (
                "app/pred.cljs",
                r#"(ns app.pred)
(defprotocol Tag (tag [this]))
(defrecord Item [x] Tag (tag [_] :item))
(defonce saved-item (->Item 1))
(defn check []
  [(satisfies? Tag saved-item) (implements? Tag saved-item)
   (satisfies? Tag "abc") (implements? Tag "abc")
   (satisfies? ICounted [1]) (implements? ICounted [1])
   (satisfies? ICounted nil) (implements? ICounted nil)
   (satisfies? ICounted 5) (implements? ICounted 5)
   (satisfies? IWatchable (atom 1)) (satisfies? IWatchable [1])
   (satisfies? IDeref (atom 1)) (satisfies? IDeref :k)
   (satisfies? IMeta (fn [])) (satisfies? Fn (fn []))
   (satisfies? ISeqable {:a 1}) (satisfies? ISeq {:a 1})])
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.pred :as p]))
(defn ^:dev/after-load after [] (js/console.log (pr-str (p/check))))
(defn start []
  (let [s (specify! #js {} p/Tag (tag [_] :s))]
    (js/console.log (pr-str (p/check)) (satisfies? p/Tag s) (implements? p/Tag s))))
start
"#,
            ),
        ],
    );
    let out = dir.join("dev");
    dev_build(&v1, &out);

    let v2 = source_root(
        &dir.join("v2"),
        &[
            (
                "app/pred.cljs",
                r#"(ns app.pred)
(defprotocol Tag (tag [this]))
(defrecord Item [x] Tag (tag [_] :item2))
(extend-type string Tag (tag [_] :str))
(extend-type number IWatchable (-add-watch [this _ _] this))
(aset ICounted "number" true)
(defonce saved-item (->Item 1))
(defn check []
  [(satisfies? Tag saved-item) (implements? Tag saved-item)
   (satisfies? Tag "abc") (implements? Tag "abc")
   (satisfies? ICounted 5) (implements? ICounted 5)
   (satisfies? IWatchable 5) (implements? IWatchable 5)
   (satisfies? IWatchable [1])])
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.pred :as p]))
(defn ^:dev/after-load after [] (js/console.log (pr-str (p/check))))
(defn start [] (after))
start
"#,
            ),
        ],
    );
    let out_v2 = dir.join("dev-v2");
    dev_build(&v2, &out_v2);

    assert_eq!(
        run_dev(&out, true, &[out_v2.join("app.pred.js")]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"[true true false false true true true false false false true false true false true true true false] true true\n[true true true false true false true false false]\n"}"#
    );
}

/// Dev units hoist each distinct keyword literal to one `const kN` per
/// namespace/unit, keeping keyword identity across hot reload and renaming
/// compiler names cleanly when raw `js*` mentions `k0`.
#[test]
fn dev_keyword_hoisting_deduplicates_per_namespace_and_preserves_reload_and_hygiene() {
    let dir = TempDir::new("dev-kw-hoist");
    let v1 = source_root(
        &dir.join("v1"),
        &[
            (
                "app/kw.cljs",
                r#"(ns app.kw)
(defonce saved-kw :app.kw/alpha)
(defn pair [] [:app.kw/alpha :beta :app.kw/alpha])
(defn raw-k0 []
  (let [k0 7]
    [(js* "((k0) => k0 + 1)(~{})" k0) :beta]))
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.kw :as kw]))
(defn ^:dev/after-load after []
  (js/console.log (identical? kw/saved-kw (first (kw/pair))) (pr-str (kw/pair)) (pr-str (kw/raw-k0))))
(defn start [] (after))
start
"#,
            ),
        ],
    );
    let out = dir.join("dev");
    dev_build(&v1, &out);

    let kw_js = fs::read_to_string(out.join("app.kw.js")).unwrap();
    assert_eq!(
        kw_js.matches("rt.keyword(\"app.kw\", \"alpha\")").count(),
        1,
        "expected :app.kw/alpha hoisted once:\n{kw_js}"
    );
    assert_eq!(
        kw_js.matches("rt.keyword(null, \"beta\")").count(),
        1,
        "expected :beta hoisted once:\n{kw_js}"
    );

    let v2 = source_root(
        &dir.join("v2"),
        &[
            (
                "app/kw.cljs",
                r#"(ns app.kw)
(defonce saved-kw :app.kw/alpha)
(defn pair [] [:app.kw/alpha :gamma :beta])
(defn raw-k0 []
  (let [k0 9]
    [(js* "((k0) => k0 + 1)(~{})" k0) :beta]))
"#,
            ),
            (
                "app/main.cljs",
                r#"(ns app.main (:require [app.kw :as kw]))
(defn ^:dev/after-load after []
  (js/console.log (identical? kw/saved-kw (first (kw/pair))) (pr-str (kw/pair)) (pr-str (kw/raw-k0))))
(defn start [] (after))
start
"#,
            ),
        ],
    );
    let out_v2 = dir.join("dev-v2");
    dev_build(&v2, &out_v2);

    assert_eq!(
        run_dev(&out, true, &[out_v2.join("app.kw.js")]).unwrap(),
        r#"{"id":"program","outcome":{"status":"returned","value":{"$value":"undefined"}},"trace":[],"stdout":"true [:app.kw/alpha :beta :app.kw/alpha] [8 :beta]\ntrue [:app.kw/alpha :gamma :beta] [10 :beta]\n"}"#
    );
}
