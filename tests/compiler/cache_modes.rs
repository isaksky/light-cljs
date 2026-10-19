//! The persistent cache across output modes (docs/compiler-cache.md,
//! "Validation and performance gates", the row "Dev, ordinary output,
//! advanced output, and lazy modules", and the separation of front-end and
//! emission keys in "Conservative keys and invalidation"). Each test runs
//! the compiler binary on one fixture: a cache-off build at the same commit
//! is the oracle of every mode, and a warm build must match it byte for
//! byte, source maps and `chunks.json` included, and run the same in Node
//! (the `--esm`/chunk driver `tests/support/chunked_execution.mjs` and the
//! dev and single-file driver `tests/support/dev_execution.mjs`).
//!
//! The artifacts hold analyzed, pre-emission namespaces, so an option that
//! only changes emission (`--optimize`, `--esm`, `--out-dir`, `--define`,
//! `--pseudo-names` and the source map flags) reuses the artifacts a build
//! in another mode published: every namespace hits. An option analysis
//! reads (`--elide-asserts`, and `--reloadable`, which turns on the warning
//! mode) is in the front-end key and misses once.

use crate::chunked_execution::run_directory;
use crate::dev_execution::run_dev;
use crate::json::Json;
use crate::temp_dir::TempDir;
use light_cljs::artifact_codec::{self, DecodeLimits};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Macros running a compile-time helper and generating names, a protocol
/// and a record, `goog-define`s, a `light.lazy/loadable` target that uses a
/// JavaScript package and an assertion. Script and dev builds read packages
/// from `globalThis.lightCljsPackages`, which `app.packages` fills; ES
/// modules import them from `node_modules`, which the tests install.
const FIXTURE: &[(&str, &str)] = &[
    (
        "app/util.cljs",
        "(ns app.util)\n(defn label [x] (str \"<\" x \">\"))\n",
    ),
    (
        "app/macros.cljs",
        "(ns app.macros (:require [app.util :as util]))\n\
         (defmacro labelled [x] (util/label x))\n\
         (defmacro twice [form] `(let [v# ~form] (+ v# v#)))\n",
    ),
    (
        "app/shapes.cljs",
        "(ns app.shapes)\n\
         (defprotocol Area (area [s]))\n\
         (defrecord Square [n] Area (area [_] (* n n)))\n",
    ),
    (
        "app/config.cljs",
        "(ns app.config)\n\
         (goog-define DEBUG false)\n\
         (goog-define FLAVOR \"plain\")\n\
         (defn describe [] (str FLAVOR (when DEBUG \" debug\")))\n",
    ),
    (
        "app/packages.cljs",
        "(ns app.packages)\n\
         (set! (.-lightCljsPackages js/globalThis)\n\
         \x20     (js-obj \"stub-package\" (js-obj \"twice\" (fn [x] (* 2 x)) \"label\" \"stub\")))\n",
    ),
    (
        "app/editor.cljs",
        "(ns app.editor\n\
         \x20 (:require [\"stub-package\" :as stub :refer [twice]]\n\
         \x20           [app.shapes :as shapes]))\n\
         (defn start [x]\n\
         \x20 (assert (number? x) \"a number\")\n\
         \x20 (str (twice x) \" \" (.-label stub) \" \" (shapes/area (shapes/->Square x))))\n",
    ),
    (
        "app/main.cljs",
        "(ns app.main\n\
         \x20 (:require [light.lazy :as lazy]\n\
         \x20           [app.packages]\n\
         \x20           [app.macros :refer [labelled twice]]\n\
         \x20           [app.shapes :as shapes]\n\
         \x20           [app.config :as config]))\n\
         (def editor (lazy/loadable app.editor/start))\n\
         (defn start []\n\
         \x20 (js/console.log (labelled \"m\") (twice 21) (shapes/area (shapes/->Square 3)) (config/describe))\n\
         \x20 (-> (lazy/load editor) (.then (fn [f] (js/console.log (f 2)) nil))))\n\
         start\n",
    ),
];

/// The namespaces of the fixture's program, `light.lazy` among them.
const NAMESPACES: u64 = 8;

/// `--optimize none`, which a build must name: the default is `prepare`.
const NONE: &[&str] = &["--optimize", "none"];

/// What the fixture prints with the default defines.
const PRINTED: &str = "<m> 42 9 plain\n4 stub 4\n";

const PACKAGE_JS: &str = "export const twice = (x) => 2 * x;\nexport const label = \"stub\";\n";
const PACKAGE_JSON: &str = r#"{"name": "stub-package", "type": "module", "main": "index.js"}"#;

struct Project {
    dir: TempDir,
}

/// One build: its stdout, its `--stats` report or error, the source map it
/// wrote and the files of its `--out-dir`.
struct Build {
    success: bool,
    js: String,
    stderr: String,
    map: Option<String>,
    files: Vec<(String, String)>,
}

impl Build {
    /// The value of the `--stats` line `key`, 0 when absent.
    fn stat(&self, key: &str) -> u64 {
        self.stderr
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(' ')?.parse().ok())
            .unwrap_or(0)
    }

    /// The build's error: its stderr without the `--stats` report.
    fn error(&self) -> &str {
        assert!(!self.success, "the build succeeds");
        &self.stderr
    }

    /// Require byte equality with the build `oracle` of the same mode.
    fn assert_same(&self, oracle: &Build, mode: &str) {
        assert!(self.success, "{mode}: {}", self.stderr);
        assert!(oracle.success, "{mode} oracle: {}", oracle.stderr);
        assert!(
            self.js == oracle.js,
            "{mode}: the output differs from the uncached build: {}",
            divergence(&self.js, &oracle.js)
        );
        assert!(
            self.map == oracle.map,
            "{mode}: the source map differs from the uncached build: {}",
            divergence(
                self.map.as_deref().unwrap_or(""),
                oracle.map.as_deref().unwrap_or("")
            )
        );
        assert_eq!(
            self.files.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            oracle
                .files
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            "{mode}"
        );
        for ((name, text), (_, expected)) in self.files.iter().zip(&oracle.files) {
            assert!(
                text == expected,
                "{mode}: {name} differs from the uncached build: {}",
                divergence(text, expected)
            );
        }
    }

    /// Require that every namespace was installed from the cache and none
    /// was analyzed or published.
    fn assert_all_hits(&self, mode: &str) {
        assert_eq!(
            self.stat("cache.hits"),
            NAMESPACES,
            "{mode}: {}",
            self.stderr
        );
        assert_eq!(self.stat("cache.misses"), 0, "{mode}: {}", self.stderr);
        assert_eq!(self.stat("cache.bypasses"), 0, "{mode}");
        assert_eq!(self.stat("cache.published"), 0, "{mode}");
        assert_eq!(self.stat("macro-calls"), 0, "{mode}");
    }

    /// The startup chunk of a chunked build's `chunks.json`.
    fn startup(&self) -> String {
        let (_, manifest) = self
            .files
            .iter()
            .find(|(name, _)| name == "chunks.json")
            .expect("a chunked build writes chunks.json");
        Json::parse(manifest).get("startup").str().to_owned()
    }

    /// Run the build in Node: a single script through the dev driver, which
    /// calls the start function the program's value is; an ES module, or a
    /// chunked build from its manifest's startup chunk, through the module
    /// driver with the fixture package installed; a dev build through the
    /// dev driver. Returns the driver's record.
    fn run(&self, project: &Project, layout: Layout) -> String {
        let dir = TempDir::new("cache-modes-run");
        let record = match layout {
            Layout::Script => {
                let file = dir.join("app.js");
                fs::write(&file, &self.js).unwrap();
                run_dev(&file, true, &[])
            }
            Layout::Module | Layout::Chunks => {
                let startup = match layout {
                    Layout::Module => {
                        fs::write(dir.join("app.main.js"), &self.js).unwrap();
                        "app.main.js".to_owned()
                    }
                    _ => {
                        for (name, text) in &self.files {
                            fs::write(dir.join(name), text).unwrap();
                        }
                        startup_chunk(&dir)
                    }
                };
                let package = dir.join("node_modules/stub-package");
                fs::create_dir_all(&package).unwrap();
                fs::write(package.join("index.js"), PACKAGE_JS).unwrap();
                fs::write(package.join("package.json"), PACKAGE_JSON).unwrap();
                run_directory(&dir, &startup)
            }
            Layout::Dev => {
                // The dev build's maps name the sources relative to its
                // directory: run it where it was written.
                let out = project.dir.join("out");
                fs::create_dir_all(&out).unwrap();
                for (name, text) in &self.files {
                    fs::write(out.join(name), text).unwrap();
                }
                run_dev(&out, true, &[])
            }
        };
        record.unwrap_or_else(|error| panic!("{error}"))
    }
}

/// How a mode's output is run.
#[derive(Clone, Copy, Debug)]
enum Layout {
    Script,
    Module,
    Chunks,
    Dev,
}

/// Where `text` first differs from `expected`, with some context.
fn divergence(text: &str, expected: &str) -> String {
    let at = text
        .bytes()
        .zip(expected.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let around = |s: &str| {
        let start = s.floor_char_boundary(at.saturating_sub(120));
        let end = s.floor_char_boundary((at + 120).min(s.len()));
        s[start..end].to_owned()
    };
    format!(
        "at byte {at}\n  cached:   {:?}\n  uncached: {:?}",
        around(text),
        around(expected)
    )
}

/// The startup chunk `chunks.json` in `dir` names.
fn startup_chunk(dir: &Path) -> String {
    let manifest = Json::parse(&fs::read_to_string(dir.join("chunks.json")).unwrap());
    manifest.get("startup").str().to_owned()
}

/// The stdout of a driver record.
fn printed(record: &str) -> String {
    Json::parse(record).get("stdout").str().to_owned()
}

impl Project {
    fn new() -> Self {
        let project = Self {
            dir: TempDir::new("cache-modes"),
        };
        for (path, text) in FIXTURE {
            project.write(path, text);
        }
        project
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.dir.join("src").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("cache")
    }

    /// Build with `flags` in the project directory, with `--stats` and, when
    /// `cached`, the project's cache. `out` and `out/app.js.map` are the
    /// places `--out-dir` and `--source-map` flags name; both are emptied
    /// first, so each build's files are its own.
    fn build(&self, flags: &[&str], cached: bool) -> Build {
        let out = self.dir.join("out");
        if out.exists() {
            fs::remove_dir_all(&out).unwrap();
        }
        fs::create_dir_all(&out).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
        command
            .current_dir(&self.dir)
            .args(["--compile", "--entry", "app.main", "--source-root", "src"])
            .arg("--stats")
            .args(flags);
        if cached {
            command.arg("--cache-dir").arg(self.cache());
        } else {
            command.arg("--no-cache");
        }
        let output = command.output().unwrap();
        let map = out.join("app.js.map");
        let map = map.exists().then(|| fs::read_to_string(&map).unwrap());
        let mut files = fs::read_dir(&out)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().into_string().unwrap())
            .filter(|name| name != "app.js.map")
            .map(|name| {
                let text = fs::read_to_string(out.join(&name)).unwrap();
                (name, text)
            })
            .collect::<Vec<_>>();
        files.sort();
        Build {
            success: output.status.success(),
            js: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
            map,
            files,
        }
    }

    /// Fill the cache with a script build at `none` (the default level is
    /// `prepare`).
    fn populate(&self) -> Build {
        let populating = self.build(NONE, true);
        assert!(populating.success, "{}", populating.stderr);
        assert_eq!(populating.stat("cache.misses"), NAMESPACES);
        assert_eq!(populating.stat("cache.published"), NAMESPACES);
        populating
    }

    /// The artifact files of the cache, by name, with their bytes.
    fn objects(&self) -> Vec<(String, Vec<u8>)> {
        let mut objects = fs::read_dir(self.cache().join("v1/objects"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().into_string().unwrap())
            .filter(|name| !name.starts_with(".tmp"))
            .map(|name| {
                let bytes = fs::read(self.cache().join("v1/objects").join(&name)).unwrap();
                (name, bytes)
            })
            .collect::<Vec<_>>();
        objects.sort();
        objects
    }

    /// Check one emission-only mode against a cache filled by a build in
    /// another mode: all hits, byte for byte the uncached output, and the
    /// same run in Node, printing `expected`.
    fn check_hits(&self, mode: &str, flags: &[&str], layout: Layout, expected: &str) {
        let oracle = self.build(flags, false);
        let warm = self.build(flags, true);
        warm.assert_same(&oracle, mode);
        warm.assert_all_hits(mode);
        let record = warm.run(self, layout);
        assert_eq!(record, oracle.run(self, layout), "{mode}");
        assert_eq!(printed(&record), expected, "{mode}: {record}");
    }
}

/// Single-file output at every level, as a script and as an ES module, with
/// each source map flag, `--define` and `--pseudo-names`: after one script
/// build at `none` fills the cache, every one of them installs every
/// namespace and emits what the uncached build does.
#[test]
fn single_file_modes_reuse_the_artifacts_of_another_mode() {
    let project = Project::new();
    let populating = project.populate();
    let oracle = project.build(NONE, false);
    populating.assert_same(&oracle, "populating");
    let defined = "<m> 42 9 cached debug\n4 stub 4\n";
    let modes: &[(&str, &[&str], Layout, &str)] = &[
        ("none", NONE, Layout::Script, PRINTED),
        (
            "prepare",
            &["--optimize", "prepare"],
            Layout::Script,
            PRINTED,
        ),
        (
            "advanced",
            &["--optimize", "advanced"],
            Layout::Script,
            PRINTED,
        ),
        (
            "esm none",
            &["--esm", "--optimize", "none"],
            Layout::Module,
            PRINTED,
        ),
        ("default level", &[], Layout::Script, PRINTED),
        (
            "esm prepare",
            &["--esm", "--optimize", "prepare"],
            Layout::Module,
            PRINTED,
        ),
        (
            "esm advanced",
            &["--esm", "--optimize", "advanced"],
            Layout::Module,
            PRINTED,
        ),
        (
            "source map",
            &["--source-map", "out/app.js.map"],
            Layout::Script,
            PRINTED,
        ),
        (
            "source map with sources, advanced",
            &[
                "--optimize",
                "advanced",
                "--source-map",
                "out/app.js.map",
                "--source-map-include-sources",
            ],
            Layout::Script,
            PRINTED,
        ),
        (
            "inline source map",
            &["--source-map-inline"],
            Layout::Script,
            PRINTED,
        ),
        (
            "inline source map with sources, esm prepare",
            &[
                "--esm",
                "--optimize",
                "prepare",
                "--source-map-inline",
                "--source-map-include-sources",
            ],
            Layout::Module,
            PRINTED,
        ),
        (
            "defines",
            &[
                "--define",
                "app.config/DEBUG=true",
                "--define",
                "app.config/FLAVOR=cached",
            ],
            Layout::Script,
            defined,
        ),
        (
            "defines, esm advanced",
            &[
                "--esm",
                "--optimize",
                "advanced",
                "--define",
                "app.config/DEBUG=true",
                "--define",
                "app.config/FLAVOR=cached",
            ],
            Layout::Module,
            defined,
        ),
        (
            "pseudo-names",
            &["--optimize", "advanced", "--pseudo-names"],
            Layout::Script,
            PRINTED,
        ),
        (
            "pseudo-names, esm with a source map",
            &[
                "--esm",
                "--optimize",
                "advanced",
                "--pseudo-names",
                "--source-map",
                "out/app.js.map",
            ],
            Layout::Module,
            PRINTED,
        ),
    ];
    for (mode, flags, layout, expected) in modes {
        project.check_hits(mode, flags, *layout, expected);
    }
}

/// `--out-dir` chunked builds at every level reuse a script build's
/// artifacts: the chunk files and `chunks.json` are the uncached build's,
/// and the chunks run as the single-file ES module does.
#[test]
fn chunked_builds_reuse_the_artifacts_of_a_script_build() {
    let project = Project::new();
    project.populate();
    for level in ["none", "prepare", "advanced"] {
        let mode = format!("chunks {level}");
        let flags = ["--optimize", level, "--out-dir", "out"];
        project.check_hits(&mode, &flags, Layout::Chunks, PRINTED);
        let chunked = project.build(&flags, true);
        assert!(
            chunked.files.len() > 2,
            "{mode}: the editor has its own chunk: {:?}",
            chunked
                .files
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        );
        let single = project.build(&["--optimize", level, "--esm"], true);
        assert_eq!(
            chunked.run(&project, Layout::Chunks).replacen(
                &format!("\"id\":\"{}\"", chunked.startup()),
                "\"id\":\"app.main.js\"",
                1
            ),
            single.run(&project, Layout::Module),
            "{mode}: the chunks run as the single file does"
        );
    }
}

/// The modes whose options analysis reads, so they are in the front-end
/// key: `--elide-asserts`, and the dev build, which turns on the warning
/// mode.
const ANALYSIS_MODES: &[(&str, &[&str], Layout)] = &[
    ("elide-asserts", &["--elide-asserts"], Layout::Script),
    (
        "elide-asserts, advanced",
        &["--elide-asserts", "--optimize", "advanced"],
        Layout::Script,
    ),
    (
        "dev build",
        &["--reloadable", "--out-dir", "out"],
        Layout::Dev,
    ),
];

/// Over the artifacts of a script build, the first build in each analysis
/// mode misses every namespace for its options and publishes beside them,
/// and the next hits them all; the script build still hits its own
/// artifacts afterwards. These builds analyze while the cache holds
/// artifacts, and match the uncached build byte for byte, the names of
/// their source maps included: a namespace's generated names are its own
/// scope's (light-cljs-332), whatever the cache holds.
#[test]
fn options_analysis_reads_miss_once_then_hit() {
    let project = Project::new();
    project.populate();
    for (index, (mode, flags, layout)) in ANALYSIS_MODES.iter().enumerate() {
        let oracle = project.build(flags, false);
        let first = project.build(flags, true);
        first.assert_same(&oracle, &format!("{mode}, first build"));
        // The second elide-asserts mode is emission-only against the first.
        if index == 1 {
            first.assert_all_hits(mode);
        } else {
            assert_eq!(first.stat("cache.hits"), 0, "{mode}: {}", first.stderr);
            assert_eq!(first.stat("cache.misses"), NAMESPACES, "{mode}");
            assert_eq!(first.stat("cache.miss.options"), NAMESPACES, "{mode}");
            assert_eq!(first.stat("cache.published"), NAMESPACES, "{mode}");
        }
        let warm = project.build(flags, true);
        warm.assert_same(&oracle, mode);
        warm.assert_all_hits(mode);
        let record = warm.run(&project, *layout);
        assert_eq!(record, oracle.run(&project, *layout), "{mode}");
        assert_eq!(printed(&record), PRINTED, "{mode}");
    }
    let script = project.build(NONE, true);
    script.assert_all_hits("script after the other modes");
    script.assert_same(&project.build(NONE, false), "script after the other modes");
}

/// Each analysis mode filling its own cache: the warm build, the dev
/// build's source maps included, is byte for byte the uncached one and runs
/// as it does, and the dev build runs as the `none` script of the same
/// sources.
#[test]
fn analysis_modes_are_byte_identical_from_their_own_cache() {
    for (mode, flags, layout) in ANALYSIS_MODES {
        let project = Project::new();
        let oracle = project.build(flags, false);
        let populating = project.build(flags, true);
        populating.assert_same(&oracle, &format!("{mode}, populating"));
        assert_eq!(
            populating.stat("cache.miss.no-artifact"),
            NAMESPACES,
            "{mode}"
        );
        assert_eq!(populating.stat("cache.published"), NAMESPACES, "{mode}");
        let warm = project.build(flags, true);
        warm.assert_same(&oracle, mode);
        warm.assert_all_hits(mode);
        let record = warm.run(&project, *layout);
        assert_eq!(record, oracle.run(&project, *layout), "{mode}");
        assert_eq!(printed(&record), PRINTED, "{mode}");
        if let Layout::Dev = layout {
            let script = project.build(NONE, false);
            assert_eq!(
                record,
                script.run(&project, Layout::Script),
                "the dev build runs as its oracle"
            );
        }
    }
}

/// Installed namespaces still go through the checks of the assembled
/// program: lazy-target validation when analysis finishes, and application
/// preparation's define check. In a script build and in a dev build, each
/// filling its own cache, editing only the lazy target's namespace leaves
/// the namespace holding the `loadable` a hit (a `LAZY` edge is not a
/// compilation dependency), so removing the target's function is reported
/// as the uncached build reports it, chunked builds included; so is a
/// define naming no `goog-define` over an all-hit build.
#[test]
fn hits_do_not_skip_target_validation_or_preparation() {
    let script: &[&str] = NONE;
    let dev: &[&str] = &["--reloadable", "--out-dir", "out"];
    for (populate, failing) in [
        (script, &[script, &["--out-dir", "out"]][..]),
        (dev, &[dev][..]),
    ] {
        let project = Project::new();
        assert!(project.build(populate, true).success);
        project
            .build(populate, true)
            .assert_all_hits(&format!("{populate:?}"));
        if populate == script {
            let missing = ["--define", "app.config/MISSING=1"];
            let oracle = project.build(&missing, false);
            let warm = project.build(&missing, true);
            assert!(
                oracle
                    .error()
                    .contains("no goog-define named app.config/MISSING"),
                "{}",
                oracle.stderr
            );
            assert_eq!(warm.error(), oracle.error());
        }
        // An edit to the target's namespace that keeps its function misses
        // only that namespace.
        project.write(
            "app/editor.cljs",
            &FIXTURE[5].1.replace("\" \" (.-label", "\"  \" (.-label"),
        );
        let edited = project.build(populate, true);
        assert_eq!(
            edited.stat("cache.hits"),
            NAMESPACES - 1,
            "{populate:?}: {}",
            edited.stderr
        );
        assert_eq!(edited.stat("cache.miss.source"), 1, "{}", edited.stderr);
        edited.assert_same(&project.build(populate, false), "edited editor");
        // Without the function, the hit on `app.main` still has its target
        // checked.
        project.write(
            "app/editor.cljs",
            &FIXTURE[5].1.replace("(defn start [x]", "(defn begin [x]"),
        );
        for flags in failing {
            let oracle = project.build(flags, false);
            let warm = project.build(flags, true);
            assert!(
                oracle.error().contains("app.editor/start"),
                "{flags:?}: {}",
                oracle.stderr
            );
            assert_eq!(warm.error(), oracle.error(), "{flags:?}");
        }
    }
}

/// Where the emission key goes: nowhere a front-end artifact is found by.
/// An artifact holds the namespace before emission and serves every mode,
/// so its manifest records the emission key as zeros, and builds in other
/// emission modes neither rewrite nor add artifacts.
#[test]
fn emission_modes_leave_the_published_artifacts_alone() {
    let project = Project::new();
    project.populate();
    let published = project.objects();
    assert_eq!(published.len() as u64, NAMESPACES);
    for (name, bytes) in &published {
        let manifest = artifact_codec::read_manifest(bytes, &DecodeLimits::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            manifest.fields.emission_key, [0; 32],
            "{}: a front-end artifact records no emission key",
            manifest.namespace
        );
    }
    for flags in [
        &["--optimize", "advanced", "--pseudo-names"][..],
        &[
            "--esm",
            "--source-map-inline",
            "--source-map-include-sources",
        ],
        &["--out-dir", "out"],
        &["--define", "app.config/DEBUG=true"],
    ] {
        let warm = project.build(flags, true);
        warm.assert_all_hits(&format!("{flags:?}"));
        assert!(
            project.objects() == published,
            "{flags:?}: the artifacts changed"
        );
    }
}
