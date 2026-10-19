//! The persistent compiler cache of `--compile --entry --cache-dir`, across
//! processes (docs/compiler-cache.md, "Loading, diagnostics, and storage"
//! and "Validation and performance gates"). Each test runs the compiler
//! binary: a cache-off build as the oracle, a populating build, then warm
//! builds after one change, asserting hits, misses and bypasses through
//! `--stats`, the output and its execution.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;
use temp_dir::TempDir;

struct Project {
    dir: TempDir,
}

/// One build's output and `--stats` report.
struct Build {
    js: String,
    stats: String,
}

impl Build {
    /// The value of the `--stats` line `key`, 0 when absent.
    fn stat(&self, key: &str) -> u64 {
        self.stats
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(' ')?.parse().ok())
            .unwrap_or(0)
    }

    /// The `[cache-misses-and-bypasses]` rows.
    fn rows(&self) -> Vec<&str> {
        self.stats
            .split_once("[cache-misses-and-bypasses]\n")
            .map_or(Vec::new(), |(_, rows)| rows.lines().collect())
    }

    fn run(&self) -> String {
        execute(&self.js)
    }
}

fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["-e", js])
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "node fails: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

impl Project {
    fn new(files: &[(&str, &str)]) -> Self {
        let project = Self {
            dir: TempDir::new("cljs-compiler-cache"),
        };
        for (path, text) in files {
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

    fn command(&self, entry: &str, extra: &[&str], cached: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
        command
            .args(["--compile", "--entry", entry, "--source-root"])
            .arg(self.dir.join("src"))
            .arg("--stats");
        if cached {
            command.arg("--cache-dir").arg(self.cache());
        }
        command.args(extra);
        command
    }

    fn finish(output: Output) -> Build {
        let stats = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "the build fails: {stats}");
        Build {
            js: String::from_utf8(output.stdout).unwrap(),
            stats,
        }
    }

    /// A build without the cache: the oracle.
    fn uncached(&self, entry: &str) -> Build {
        Self::finish(self.command(entry, &[], false).output().unwrap())
    }

    fn cached(&self, entry: &str, extra: &[&str]) -> Build {
        Self::finish(self.command(entry, extra, true).output().unwrap())
    }

    /// The artifact files of the cache directory.
    fn objects(&self) -> Vec<PathBuf> {
        let mut objects: Vec<PathBuf> = fs::read_dir(self.cache().join("v1/objects"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| !name_of(path).starts_with(".tmp"))
            .collect();
        objects.sort();
        objects
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

/// A macro that runs a helper at compile time, a protocol and a record, a
/// gensym-generating macro and a plain namespace.
const FIXTURE: &[(&str, &str)] = &[
    ("util.cljs", "(ns util) (defn helper [x] (str \"h\" x))"),
    (
        "macros.cljs",
        "(ns macros (:require [util])) (defmacro shout [x] (util/helper x)) (defmacro twice [form] `(let [v# ~form] (+ v# v#)))",
    ),
    (
        "shapes.cljs",
        "(ns shapes) (defprotocol Area (area [s])) (defrecord Square [n] Area (area [_] (* n n)))",
    ),
    ("plain.cljs", "(ns plain) (def answer (+ 40 2))"),
    (
        "main.cljs",
        "(ns main (:require [macros :refer [shout twice]] [shapes :as s] [plain] [util])) (js/console.log (shout 1) (s/area (s/->Square 3)) (twice plain/answer) (util/helper 2))",
    ),
];

#[test]
fn warm_fresh_process_build() {
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "h1 9 84 h2\n");
    assert!(!oracle.stats.contains("cache.hits"));
    let populating = project.cached("main", &[]);
    assert_eq!(populating.js, oracle.js);
    assert_eq!(populating.stat("cache.hits"), 0);
    assert_eq!(populating.stat("cache.misses"), 5);
    assert_eq!(populating.stat("cache.miss.no-artifact"), 5);
    assert_eq!(populating.stat("cache.published"), 5);
    assert!(populating.stat("cache.bytes-written") > 0);
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 5, "{}", warm.stats);
    assert_eq!(warm.stat("cache.misses"), 0);
    assert_eq!(warm.stat("cache.published"), 0);
    assert_eq!(warm.stat("macro-calls"), 0);
    assert!(warm.stat("cache.bytes-read") > 0);
    assert!(warm.rows().is_empty(), "{:?}", warm.rows());
    assert_eq!(
        warm.js, oracle.js,
        "a warm build differs from the uncached one"
    );
}

const COUNTER: &[(&str, &str)] = &[
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

#[test]
fn counter_example_across_processes() {
    let project = Project::new(COUNTER);
    assert_eq!(project.cached("main", &[]).run(), "1 2\n");
    // Editing only `main`: the hit on `dep` skips its increment, the
    // documented consequence of the cache contract.
    project.write(
        "main.cljs",
        "(ns main (:require [dep :as d] [macros :refer [next-value]])) (js/console.log d/value (next-value)) ; edited",
    );
    let edited = project.cached("main", &[]);
    assert_eq!(edited.run(), "1 1\n");
    assert_eq!(edited.stat("cache.hits"), 2);
    assert_eq!(edited.rows(), ["main miss source: source changed"]);
    // A blocker on the effectful provider compiles it and its consumers.
    let blocked = project.cached("main", &["--cache-blocker", "macros"]);
    assert_eq!(blocked.run(), "1 2\n");
    assert_eq!(blocked.stat("cache.hits"), 0);
    assert_eq!(blocked.stat("cache.bypass.blocked"), 3);
    assert_eq!(
        blocked.rows(),
        [
            "macros bypass blocked macros",
            "dep bypass blocked macros > dep",
            "main bypass blocked macros > main",
        ]
    );
    assert_eq!(blocked.js, project.uncached("main").js);
}

/// The cache with several jobs (docs/compiler-concurrency.md, "As built
/// (light-cljs-340)"): from two caches populated alike, one build with
/// `--jobs 1` and one with `--jobs 4` of the same edit, which hit every
/// namespace but the edited one and its consumer, emit the same program,
/// and the parallel build's misses give their macro-free units to the pool.
#[test]
fn a_warm_hit_and_a_parallel_miss_match_one_job() {
    let project = Project::new(FIXTURE);
    let caches = [project.dir.join("cache-1"), project.dir.join("cache-4")];
    let build = |cache: &Path, jobs: &str| {
        let mut command = project.command("main", &["--jobs", jobs], false);
        command.arg("--cache-dir").arg(cache);
        Project::finish(command.output().unwrap())
    };
    for cache in &caches {
        assert_eq!(build(cache, "1").stat("cache.published"), 5);
    }
    project.write(
        "plain.cljs",
        "(ns plain) (def answer (+ 40 2)) (defn count-down [n] (loop [i n] (if (pos? i) (recur (dec i)) i))) (def table [1 2 3])",
    );
    let oracle = project.uncached("main");
    let serial = build(&caches[0], "1");
    let parallel = build(&caches[1], "4");
    for warm in [&serial, &parallel] {
        assert_eq!(warm.stat("cache.hits"), 3, "{}", warm.stats);
        assert_eq!(
            warm.rows(),
            [
                "plain miss source: source changed",
                "main miss dependencies: dependency plain changed"
            ]
        );
    }
    assert_eq!(parallel.js, serial.js);
    assert_eq!(serial.js, oracle.js);
    assert_eq!(parallel.run(), "h1 9 84 h2\n");
    // QuickJS-only builds have one slot, so no pool.
    let pooled = parallel.stat("deferred-units.pooled");
    if cfg!(feature = "v8-macros") {
        assert!(pooled > 0, "{}", parallel.stats);
    } else {
        assert_eq!(pooled, 0);
    }
}

/// Definitions whose units expand source macros, one of which runs a helper
/// at compile time, and another generates a name.
const MACRO_UNITS: &[(&str, &str)] = &[
    ("util.cljs", "(ns util) (defn helper [x] (str \"h\" x))"),
    (
        "macros.cljs",
        "(ns macros (:require [util])) (defmacro shout [x] (util/helper x)) (defmacro twice [form] `(let [v# ~form] (+ v# v#)))",
    ),
    (
        "app.cljs",
        "(ns app (:require [macros :refer [shout twice]])) (def a (shout 3)) (def b (twice 4)) (defn c [n] (twice (inc n))) (def d (vec (for [i (range 3)] (shout i)))) (defn f1 [] (shout 1)) (defn f2 [] (twice 2)) (defn f3 [] (shout 3)) (defn f4 [] (twice 4)) (defn f5 [] (shout 5)) (defn f6 [] (twice 6)) (defn f7 [] (shout 7)) (defn f8 [] (twice 8)) (defn f9 [] (shout 9)) (defn f10 [] (twice 10)) (defn f11 [] (shout 11)) (defn f12 [] (twice 12))",
    ),
    (
        "main.cljs",
        "(ns main (:require [app] [macros :refer [twice]])) (def e (twice app/b)) (js/console.log app/a app/b (app/c 2) (pr-str app/d) e)",
    ),
];

/// A cold cache with several jobs (light-cljs-374): capturing the misses'
/// artifacts leaves their macro-expanding units on the workers' isolates,
/// and the parallel build emits the uncached program and publishes the
/// artifacts a one-job build publishes, byte for byte, which a warm parallel
/// build then installs.
#[test]
fn a_cold_parallel_build_expands_units_on_workers_and_publishes_the_one_job_artifacts() {
    let project = Project::new(MACRO_UNITS);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "h3 8 6 [\"hi\" \"hi\" \"hi\"] 16\n");
    let build = |cache: &Path, jobs: &str| {
        // Room for four isolates, so several workers record at once.
        let extra = ["--jobs", jobs, "--macro-memory-limit", "268435456"];
        let mut command = project.command("main", &extra, false);
        command.arg("--cache-dir").arg(cache);
        Project::finish(command.output().unwrap())
    };
    let objects = |cache: &Path| {
        let mut objects: Vec<(String, Vec<u8>)> = fs::read_dir(cache.join("v1/objects"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| !name_of(path).starts_with(".tmp"))
            .map(|path| (name_of(&path), fs::read(&path).unwrap()))
            .collect();
        objects.sort();
        objects
    };
    let serial_cache = project.dir.join("cache-1");
    let serial = build(&serial_cache, "1");
    assert_eq!(serial.js, oracle.js);
    assert_eq!(serial.stat("cache.published"), 4);
    let mut isolates = 0;
    for round in 0..3 {
        let cache = project.dir.join(format!("cache-4-{round}"));
        let parallel = build(&cache, "4");
        assert_eq!(parallel.js, oracle.js, "round {round}");
        assert_eq!(parallel.stat("cache.published"), 4, "round {round}");
        assert_eq!(objects(&cache), objects(&serial_cache), "round {round}");
        // Units that expand macros went to the pool too, analyzed in the
        // workers' isolates; QuickJS-only builds have one slot, so no pool.
        let (pooled, macro_free) = (
            parallel.stat("deferred-units.pooled"),
            parallel.stat("deferred-units.macro-free"),
        );
        if cfg!(feature = "v8-macros") {
            assert!(pooled > macro_free, "{}", parallel.stats);
            isolates = isolates.max(parallel.stat("pool.worker-isolates"));
        } else {
            assert_eq!(pooled, 0);
        }
        let warm = build(&cache, "4");
        assert_eq!(warm.stat("cache.hits"), 4, "{}", warm.stats);
        assert_eq!(warm.js, oracle.js);
    }
    // Several workers recorded at once.
    if cfg!(feature = "v8-macros") {
        assert!(isolates > 1, "{isolates} worker isolates");
    }
}

#[test]
fn policy_changes_with_an_existing_artifact() {
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    project.cached("main", &[]);
    // A blocker takes effect although every artifact matches.
    let blocked = project.cached("main", &["--cache-blocker", "shapes"]);
    assert_eq!(blocked.js, oracle.js);
    assert_eq!(blocked.stat("cache.hits"), 3, "{}", blocked.stats);
    assert_eq!(
        blocked.rows(),
        [
            "shapes bypass blocked shapes",
            "main bypass blocked shapes > main"
        ]
    );
    // `^:light-cljs/no-cache` covers that namespace only: it is compiled on
    // every build and never written, and its consumers still hit once
    // its key is the one they recorded.
    project.write(
        "plain.cljs",
        "(ns ^:light-cljs/no-cache plain) (def answer (+ 40 2))",
    );
    let first = project.cached("main", &[]);
    assert_eq!(first.run(), "h1 9 84 h2\n");
    assert!(
        first.rows().contains(&"plain bypass no-cache"),
        "{:?}",
        first.rows()
    );
    let second = project.cached("main", &[]);
    assert_eq!(second.rows(), ["plain bypass no-cache"]);
    assert_eq!(second.stat("cache.hits"), 4);
    assert_eq!(second.stat("cache.published"), 0);
    // `^:dev/always` reaches every compilation consumer, on every build.
    project.write(
        "util.cljs",
        "(ns ^:dev/always util) (defn helper [x] (str \"h\" x))",
    );
    for _ in 0..2 {
        let always = project.cached("main", &[]);
        assert_eq!(always.run(), "h1 9 84 h2\n");
        assert_eq!(
            always.rows(),
            [
                "util bypass always util",
                "macros bypass always util > macros",
                "plain bypass no-cache",
                "main bypass always util > main",
            ]
        );
        assert_eq!(always.stat("cache.bypass.always"), 3, "{}", always.stats);
        assert_eq!(always.stat("cache.bypass.no-cache"), 1);
        assert_eq!(always.stat("cache.hits"), 1);
    }
}

#[test]
fn corrupted_artifact_is_a_miss_then_republished() {
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    project.cached("main", &[]);
    let objects = project.objects();
    assert_eq!(objects.len(), 5);
    // One flipped byte fails the content hash, one truncation the size.
    let mut bytes = fs::read(&objects[0]).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xff;
    fs::write(&objects[0], bytes).unwrap();
    let truncated = fs::read(&objects[1]).unwrap();
    fs::write(&objects[1], &truncated[..truncated.len() / 3]).unwrap();
    let damaged = project.cached("main", &[]);
    assert_eq!(damaged.js, oracle.js);
    assert!(
        damaged.stat("cache.miss.unreadable") >= 2,
        "{}",
        damaged.stats
    );
    assert!(damaged.stat("cache.published") >= 2);
    assert!(
        damaged
            .rows()
            .iter()
            .any(|row| row.contains("miss unreadable: the artifact fails its content hash")),
        "{:?}",
        damaged.rows()
    );
    let repaired = project.cached("main", &[]);
    assert_eq!(repaired.stat("cache.hits"), 5, "{}", repaired.stats);
    assert_eq!(repaired.js, oracle.js);
}

#[test]
fn two_concurrent_compiles_share_one_cache_directory() {
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    for _ in 0..3 {
        let children: Vec<_> = (0..2)
            .map(|_| {
                project
                    .command("main", &[], true)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for child in children {
            let build = Project::finish(child.wait_with_output().unwrap());
            assert_eq!(build.js, oracle.js);
            assert_eq!(build.stat("cache.unpublished"), 0, "{}", build.stats);
        }
    }
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 5, "{}", warm.stats);
    assert_eq!(warm.js, oracle.js);
    // Retention keeps the store bounded: one artifact per namespace and key.
    assert_eq!(project.objects().len(), 5);
}

#[test]
fn cache_flags_require_an_entry_and_change_nothing_without_one() {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--cache-dir", "x", "--text", "1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("require --entry"));
}

const INPUTS: &[(&str, &str)] = &[
    (
        "inputs.cljs",
        "(ns inputs (:require [light.build :as b])) (defmacro inline [path] (b/resource path)) (defmacro optional [path] (or (b/resource? path) \"none\")) (defmacro flavor [] (str (b/config :app/flavor)))",
    ),
    (
        "text.cljs",
        "(ns text (:require [inputs :refer [inline]])) (def value (inline \"data.txt\"))",
    ),
    (
        "extra.cljs",
        "(ns extra (:require [inputs :refer [optional]])) (def value (optional \"extra.txt\"))",
    ),
    (
        "flavor.cljs",
        "(ns flavor (:require [inputs :refer [flavor]])) (def value (flavor))",
    ),
    ("plain.cljs", "(ns plain) (def value 42)"),
    (
        "main.cljs",
        "(ns main (:require [text] [extra] [flavor] [plain])) (js/console.log text/value extra/value flavor/value plain/value)",
    ),
];

/// Resources, an optional resource appearing and the `--config` map reach
/// the namespaces that read them in a later process, through the records
/// the lookup rebuilds from stored manifests.
#[test]
fn persistent_inputs_invalidate_their_readers() {
    let project = Project::new(INPUTS);
    project.write("data.txt", "one");
    let config = ["--config", "app/flavor=:dev"];
    assert_eq!(project.cached("main", &config).run(), "one none :dev 42\n");
    assert_eq!(project.cached("main", &config).stat("cache.hits"), 7);
    project.write("data.txt", "two");
    let edited = project.cached("main", &config);
    assert_eq!(edited.run(), "two none :dev 42\n");
    assert!(
        edited
            .rows()
            .contains(&"text miss inputs: external input changed"),
        "{:?}",
        edited.rows()
    );
    // `extra` and `flavor` ran source macros after `text`: the coarse
    // fallback makes them depend on every earlier namespace.
    assert_eq!(edited.stat("cache.hits"), 3, "{}", edited.stats);
    assert_eq!(edited.stat("cache.miss.view"), 2);
    project.write("extra.txt", "here");
    let appeared = project.cached("main", &config);
    assert_eq!(appeared.run(), "two here :dev 42\n");
    assert!(
        appeared
            .rows()
            .contains(&"extra miss inputs: external input changed"),
        "{:?}",
        appeared.rows()
    );
    // The whole configuration map is in every key.
    let prod = ["--config", "app/flavor=:prod"];
    let reconfigured = project.cached("main", &prod);
    assert_eq!(reconfigured.run(), "two here :prod 42\n");
    assert_eq!(reconfigured.stat("cache.hits"), 0);
    assert_eq!(
        reconfigured.stat("cache.miss.config"),
        7,
        "{}",
        reconfigured.stats
    );
    let cache_off = Project::finish(project.command("main", &prod, false).output().unwrap());
    assert_eq!(cache_off.js, reconfigured.js);
}

/// A damaged index is a miss that names it, and the next publish replaces
/// it; a require removed is reported as the source change it is, before
/// the dependency it loses.
#[test]
fn damaged_index_and_removed_require_report_their_first_reason() {
    let project = Project::new(FIXTURE);
    project.cached("main", &[]);
    let index = project.cache().join("v1/index");
    for file in fs::read_dir(&index).unwrap().flatten() {
        fs::write(file.path(), b"damaged").unwrap();
    }
    let damaged = project.cached("main", &[]);
    assert_eq!(damaged.stat("cache.miss.index"), 5, "{}", damaged.stats);
    assert_eq!(project.cached("main", &[]).stat("cache.hits"), 5);
    project.write(
        "main.cljs",
        "(ns main (:require [macros :refer [shout twice]] [shapes :as s] [util])) (js/console.log (shout 1) (s/area (s/->Square 3)) (twice 42) (util/helper 2))",
    );
    let removed = project.cached("main", &[]);
    assert_eq!(removed.run(), "h1 9 84 h2\n");
    assert_eq!(removed.rows(), ["main miss source: source changed"]);
}
