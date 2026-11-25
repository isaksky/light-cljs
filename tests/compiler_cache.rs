//! The persistent compiler cache of `--compile --entry --cache-dir`, across
//! processes (docs/compiler-cache.md, "Loading, diagnostics, and storage"
//! and "Validation and performance gates"). Each test runs the compiler
//! binary: a cache-off build as the oracle, a populating build, then warm
//! builds after one change, asserting hits, misses and bypasses through
//! `--stats`, the output and its execution.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/node.rs"]
#[allow(dead_code)]
mod node;
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
    let output = node::eval(js).expect("node runs");
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
        } else if !extra.contains(&"--cache-dir") {
            command.arg("--no-cache");
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

/// The runtime key is the output writer's (docs/value-layout.md, section 1,
/// light-cljs-1025): a namespace's artifact holds the key's placeholder, so
/// a library namespace's artifact serves builds whose outputs have other
/// keys, and a cached build writes the uncached build's bytes, its key
/// included.
#[test]
fn artifacts_hold_no_runtime_key() {
    let main = |n: u32| {
        format!(
            "(ns main (:require [shapes :as s])) (js/console.log (s/area (s/->Square {n})) (s/counted (s/->Square 1)) (s/counted (js-obj)))"
        )
    };
    let project = Project::new(&[
        (
            "shapes.cljs",
            "(ns shapes) (defprotocol Area (area [s])) (defrecord Square [n] Area (area [_] (* n n)))
             (defn counted [x] (boolean (.-cljs$core$ICounted$ x)))",
        ),
        ("main.cljs", &main(3)),
    ]);
    let key = |build: &Build| {
        assert!(!build.js.contains("$LKEY"), "{}", build.js);
        light_cljs::runtime_key::RuntimeKey::written(&build.js)
            .expect("the output registers its key")
            .key()
            .to_owned()
    };
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "9 true false\n");
    let populating = project.cached("main", &[]);
    assert_eq!(populating.js, oracle.js);
    assert_eq!(key(&populating), key(&oracle));
    assert_eq!(populating.stat("cache.published"), 2);
    // Another constant in `main`: another output and key, and `shapes`'s
    // artifact, stored with the first key, hits.
    project.write("main.cljs", &main(4));
    let edited = project.uncached("main");
    assert_eq!(edited.run(), "16 true false\n");
    assert_ne!(key(&edited), key(&oracle));
    let mixed = project.cached("main", &[]);
    assert_eq!(mixed.stat("cache.hits"), 1, "{}", mixed.stats);
    assert_eq!(mixed.rows(), ["main miss source: source changed"]);
    assert_eq!(mixed.js, edited.js);
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 2, "{}", warm.stats);
    assert_eq!(warm.js, edited.js);
}

#[test]
fn dynamic_core_bindings_reuse_cached_artifacts() {
    let project = Project::new(&[
        (
            "macros.cljs",
            "(ns macros) (defn current-setting [] *print-length*) (defmacro setting [n] [(current-setting) (binding [*print-length* n] (current-setting)) (current-setting)])",
        ),
        (
            "main.cljs",
            "(ns main (:require [macros :refer [setting]])) (js/console.log (pr-str (setting 3)) (binding [*print-length* 3] *print-length*))",
        ),
    ]);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "[nil 3 nil] 3\n");
    let populated = project.cached("main", &[]);
    assert_eq!(populated.js, oracle.js);
    assert_eq!(populated.stat("cache.published"), 2, "{}", populated.stats);
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 2, "{}", warm.stats);
    assert_eq!(warm.stat("cache.misses"), 0, "{}", warm.stats);
    assert_eq!(warm.stat("cache.published"), 0, "{}", warm.stats);
    assert_eq!(warm.stat("macro-calls"), 0, "{}", warm.stats);
    assert!(warm.rows().is_empty(), "{:?}", warm.rows());
    assert_eq!(warm.js, oracle.js);
    assert_eq!(warm.run(), "[nil 3 nil] 3\n");

    // Miss only the consumer so expansion executes the provider's restored
    // macro and helper factories, including reads before and after binding.
    project.write(
        "main.cljs",
        "(ns main (:require [macros :refer [setting]])) (js/console.log (pr-str (setting 3)) (pr-str (setting 1)) (binding [*print-length* 3] *print-length*))",
    );
    let edited = project.cached("main", &[]);
    assert_eq!(edited.stat("cache.hits"), 1, "{}", edited.stats);
    assert_eq!(edited.stat("cache.misses"), 1, "{}", edited.stats);
    assert_eq!(edited.stat("cache.published"), 1, "{}", edited.stats);
    assert_eq!(edited.stat("macro-calls"), 2, "{}", edited.stats);
    assert_eq!(edited.rows(), ["main miss source: source changed"]);
    let oracle = project.uncached("main");
    let printed = edited.run();
    assert_eq!(printed, "[nil 3 nil] [nil 1 nil] 3\n");
    assert_eq!(printed, oracle.run());
    assert_eq!(edited.js, oracle.js);
}

/// `set!` and `binding` at the head of an anonymous function literal are
/// calls like any other, not first-class uses of the operator, so on the
/// Pants build they must not mark every core function written and turn off
/// every lookup (light-cljs-571: the 200k generated project's rebuild with
/// nothing changed hit nothing).
#[test]
fn mutations_in_anonymous_functions_reuse_cached_artifacts() {
    let project = Project::new(&[
        (
            "util.cljs",
            "(ns util) (def ^:dynamic *scale* 1) (defn helper [x] (str \"h\" x)) (defn scaled [x] (* x *scale*))",
        ),
        (
            "macros.cljs",
            "(ns macros (:require [util])) (defmacro shout [x] (util/helper x)) (defmacro scaled-all [& xs] (mapv #(binding [util/*scale* 10] (util/scaled %)) xs))",
        ),
        (
            "main.cljs",
            "(ns main (:require [macros :refer [shout scaled-all]] [util])) (def state #js {:n 0}) (def bump #(set! (.-n state) (+ (.-n state) %))) (bump 5) (js/console.log (shout 1) (pr-str (scaled-all 1 2)) (.-n state) (pr-str (map #(binding [util/*scale* 3] (util/scaled %)) [1 2])))",
        ),
    ]);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "h1 [10 20] 5 (3 6)\n");
    let populated = project.cached("main", &[]);
    assert_eq!(populated.js, oracle.js);
    assert_eq!(populated.stat("cache.published"), 3, "{}", populated.stats);
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 3, "{}", warm.stats);
    assert_eq!(warm.stat("cache.misses"), 0, "{}", warm.stats);
    assert_eq!(warm.stat("cache.published"), 0, "{}", warm.stats);
    assert_eq!(warm.stat("macro-calls"), 0, "{}", warm.stats);
    assert!(warm.rows().is_empty(), "{:?}", warm.rows());
    assert_eq!(warm.js, oracle.js);

    // A rebuild after a one-file edit misses only the edited namespace and
    // runs its macros against the installed provider.
    project.write(
        "main.cljs",
        "(ns main (:require [macros :refer [shout scaled-all]] [util])) (def state #js {:n 0}) (def bump #(set! (.-n state) (+ (.-n state) %))) (bump 7) (js/console.log (shout 2) (pr-str (scaled-all 3)) (.-n state) (pr-str (map #(binding [util/*scale* 3] (util/scaled %)) [1 2])))",
    );
    let edited = project.cached("main", &[]);
    assert_eq!(edited.stat("cache.hits"), 2, "{}", edited.stats);
    assert_eq!(edited.rows(), ["main miss source: source changed"]);
    assert_eq!(edited.run(), "h2 [30] 7 (3 6)\n");
    assert_eq!(edited.js, project.uncached("main").js);
}

/// Computed mutation symbols leave no lexical core-write hint. Installing the
/// writer must restore its resolved writes before preparing later factories,
/// including property/index targets and writes in the artifact's deferred bodies.
#[test]
fn generated_core_writes_survive_cached_artifacts() {
    for jobs in ["1", "4"] {
        for deferred in [false, true] {
            for (mutation, body, expected) in [
                (
                    "(list (symbol \"set!\") (symbol \"cljs.core/inc\") '(fn [_] 42))",
                    "(let [before (inc 2)] (writer/install) [before (inc 2)])",
                    "[3 42]\n",
                ),
                (
                    "(list (symbol \"binding\") [(symbol \"cljs.core/inc\") '(fn [_] 42)] '(inc 2))",
                    "[(inc 2) (writer/install) (inc 2)]",
                    "[3 42 3]\n",
                ),
                (
                    "(list (symbol \"set!\") (list (symbol \".-call\") (symbol \"cljs.core/+\")) '(fn [_ a b] 42))",
                    "(let [before (+ 1 2)] (writer/install) [before (+ 1 2)])",
                    "[3 42]\n",
                ),
                (
                    // set! of an aget form is intentionally rejected; aset
                    // produces the assignment's JsIndex target directly.
                    "(list (symbol \"aset\") (symbol \"cljs.core/+\") \"call\" '(fn [_ a b] 42))",
                    "(let [before (+ 1 2)] (writer/install) [before (+ 1 2)])",
                    "[3 42]\n",
                ),
            ] {
                let definition = if deferred {
                    "(defn install [] (write-core))"
                } else {
                    "(if true (def install (fn [] (write-core))) nil)"
                };
                let writer = format!(
                    "(ns writer) (defmacro write-core [] {mutation}) {definition} \
                     (defmacro ready [] (fn? install))"
                );
                let main = format!(
                    "(ns main (:require [writer])) (writer/ready) (defmacro probe [] {body}) \
                     (js/console.log (pr-str (probe)))"
                );
                // Explicitly demand the installer's function value before
                // preparing probe, without executing the mutation itself.
                let project = Project::new(&[("writer.cljs", &writer), ("main.cljs", &main)]);
                let options = ["--jobs", jobs];
                let cold =
                    Project::finish(project.command("main", &options, false).output().unwrap());
                assert_eq!(cold.run(), expected, "{definition}: jobs {jobs}");
                let populated = project.cached("main", &options);
                assert!(
                    populated.js == cold.js,
                    "populated {definition}: jobs {jobs}"
                );
                assert_eq!(populated.stat("cache.hits"), 0, "{}", populated.stats);
                let warm = project.cached("main", &options);
                assert!(warm.stat("cache.hits") > 0, "{}", warm.stats);
                assert_eq!(warm.run(), expected);
                assert!(warm.js == cold.js, "warm {definition}: jobs {jobs}");

                // Keep the writer cached and force the consumer's macro to run
                // again, using the restored write index for its new factory.
                project.write("main.cljs", &format!("{main} ; touched consumer"));
                let edited = project.cached("main", &options);
                assert_eq!(edited.stat("cache.hits"), 1, "{}", edited.stats);
                assert!(edited.stat("macro-calls") > 0, "{}", edited.stats);
                assert_eq!(edited.run(), expected, "edited {definition}: jobs {jobs}");
                assert!(edited.js == cold.js, "edited {definition}: jobs {jobs}");
            }
        }
    }
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
        let cache = cache.to_str().unwrap();
        let mut command = project.command("main", &["--jobs", jobs, "--cache-dir", cache], false);
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
    let pooled = parallel.stat("deferred-units.pooled");
    assert!(pooled > 0, "{}", parallel.stats);
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
        let cache = cache.to_str().unwrap();
        let extra = ["--jobs", jobs, "--macro-memory-limit", "268435456"];
        let extra = [&extra[..], &["--cache-dir", cache]].concat();
        let mut command = project.command("main", &extra, false);
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
        // workers' isolates (QuickJS runtimes in QuickJS-only builds).
        let (pooled, macro_free) = (
            parallel.stat("deferred-units.pooled"),
            parallel.stat("deferred-units.macro-free"),
        );
        assert!(pooled > macro_free, "{}", parallel.stats);
        isolates = isolates.max(parallel.stat("pool.worker-isolates"));
        let warm = build(&cache, "4");
        assert_eq!(warm.stat("cache.hits"), 4, "{}", warm.stats);
        assert_eq!(warm.js, oracle.js);
    }
    // Several workers recorded at once.
    assert!(isolates > 1, "{isolates} worker isolates");
}

/// Cold cache-fill capture must not enter the compile-time macro engine for
/// units that never invoke a JavaScript macro (pure/macro-free units and units
/// expanding only native core macros), while still capturing `gensym` and
/// namespace-registry side effects identically across `--jobs 1` and `--jobs 4`
/// (light-cljs-1102).
#[test]
fn cold_cache_capture_gates_host_entries_on_actual_engine_use() {
    let project = Project::new(&[
        (
            "macros.cljs",
            r#"(ns macros)
(defmacro named [x]
  (let [s (gensym "g")]
    `(let [~s ~x] (+ ~s 1))))
(defmacro with-ns [sym x]
  (create-ns sym)
  `[~(some? (find-ns sym)) ~x])"#,
        ),
        (
            "app.cljs",
            r#"(ns app (:require [macros :refer [named with-ns]]))
(defn p0 [x] (+ x 1))
(defn p1 [x] (+ x 2))
(defn p2 [x] (+ x 3))
(defn p3 [x] (+ x 4))
(defn p4 [x] (+ x 5))
(defn p5 [x] (+ x 6))
(defn n0 [x] (when x (and true x)))
(defn n1 [x] (when x (or false x)))
(defn n2 [x] (cond (pos? x) x :else 0))
(defn m0 [x] (named x))
(defn m1 [x] (with-ns app.dyn.one x))
(defn m2 [x] (named (m0 x)))
(defn m3 [x] (with-ns app.dyn.two (named x)))"#,
        ),
        (
            "main.cljs",
            r#"(ns main (:require [app]))
(js/console.log (app/p0 1) (app/p5 1) (app/n0 3) (app/n2 4) (app/m0 5) (pr-str (app/m1 6)) (app/m2 7) (pr-str (app/m3 8)))"#,
        ),
    ]);
    let nocache_j1 = project.uncached("main");
    assert_eq!(nocache_j1.run(), "2 7 3 4 6 [true 6] 9 [true 9]\n");
    let nocache_j4 = Project::finish(
        project
            .command(
                "main",
                &["--jobs", "4", "--macro-memory-limit", "268435456"],
                false,
            )
            .output()
            .unwrap(),
    );
    assert_eq!(nocache_j4.js, nocache_j1.js);

    let build = |cache: &Path, jobs: &str| {
        let cache = cache.to_str().unwrap();
        let extra = [
            "--jobs",
            jobs,
            "--macro-memory-limit",
            "268435456",
            "--cache-dir",
            cache,
        ];
        Project::finish(project.command("main", &extra, false).output().unwrap())
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

    let c1 = project.dir.join("cache-j1");
    let cold_j1 = build(&c1, "1");
    assert_eq!(cold_j1.js, nocache_j1.js);
    let c4 = project.dir.join("cache-j4");
    let cold_j4 = build(&c4, "4");
    assert_eq!(cold_j4.js, nocache_j1.js);
    assert_eq!(objects(&c4), objects(&c1));

    {
        let nc1_host = nocache_j1.stat("trace.units.host");
        let nc4_host = nocache_j4.stat("trace.units.host");
        let j1_host = cold_j1.stat("trace.units.host");
        let j4_host = cold_j4.stat("trace.units.host");
        let analyzed = cold_j1.stat("deferred-units.analyzed");
        assert!(
            nc1_host > 0 && nc1_host < analyzed,
            "expected subset of units to enter host: nc1_host={nc1_host} analyzed={analyzed}"
        );
        assert_eq!(
            j1_host, nc1_host,
            "cold --jobs 1 should only enter host for macro-invoking units:\n{}",
            cold_j1.stats
        );
        assert_eq!(
            j4_host, nc4_host,
            "cold --jobs 4 should only enter host for macro-invoking units:\n{}",
            cold_j4.stats
        );
        assert_eq!(
            j4_host, 4,
            "only m0..m3 invoke JS macros:\n{}",
            cold_j4.stats
        );
    }

    let warm_j4 = build(&c4, "4");
    assert_eq!(warm_j4.stat("cache.hits"), 3, "{}", warm_j4.stats);
    assert_eq!(warm_j4.js, nocache_j1.js);
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

/// A namespace that ran a source macro depends on every earlier namespace
/// (the coarse fallback), but its manifest lists only its exact edges and
/// the `coarse` flag, so its size does not grow with its position in the
/// program (light-cljs-1000). A lookup adds the coarse edges back: a
/// blocker reaches it through one, and an edit to an earlier namespace it
/// does not require still misses its view.
#[test]
fn coarse_edges_are_the_manifest_flag_not_a_list() {
    use light_cljs::artifact_codec::{DecodeLimits, read_manifest};
    use light_cljs::dependency_manifest::EdgeKinds;
    let project = Project::new(&[
        ("early.cljs", "(ns early) (def x 1)"),
        (
            "macros.cljs",
            "(ns macros) (defmacro twice [form] `(* 2 ~form))",
        ),
        (
            "consumer.cljs",
            "(ns consumer (:require [macros :refer [twice]])) (def y (twice 21))",
        ),
        (
            "main.cljs",
            "(ns main (:require [early] [consumer])) (js/console.log early/x consumer/y)",
        ),
    ]);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "1 42\n");
    assert_eq!(project.cached("main", &[]).js, oracle.js);
    let manifests: Vec<_> = project
        .objects()
        .iter()
        .map(|path| read_manifest(&fs::read(path).unwrap(), &DecodeLimits::default()).unwrap())
        .collect();
    let consumer = manifests
        .iter()
        .find(|manifest| manifest.namespace == "consumer")
        .expect("consumer is published");
    assert!(consumer.fields.coarse);
    let listed: Vec<&str> = consumer
        .fields
        .dependencies
        .iter()
        .map(|dependency| dependency.namespace.as_str())
        .collect();
    assert_eq!(listed, ["macros"]);
    for manifest in &manifests {
        for dependency in &manifest.fields.dependencies {
            assert!(
                !EdgeKinds(dependency.kinds).intersects(EdgeKinds::COARSE),
                "{} lists a coarse edge to {}",
                manifest.namespace,
                dependency.namespace
            );
        }
    }
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 4, "{}", warm.stats);
    assert_eq!(warm.js, oracle.js);
    // `consumer` does not require `early`: the blocker reaches it through
    // its coarse edge, rebuilt from the flag of the artifact that matched.
    let blocked = project.cached("main", &["--cache-blocker", "early"]);
    assert_eq!(blocked.js, oracle.js);
    assert_eq!(
        blocked.rows(),
        [
            "early bypass blocked early",
            "consumer bypass blocked early > consumer",
            "main bypass blocked early > main",
        ]
    );
    // An edit that declares nothing a source macro could see leaves the
    // view alone (light-cljs-347).
    project.write("early.cljs", "(ns early) (def x 2)");
    let edited = project.cached("main", &[]);
    assert_eq!(edited.run(), "2 42\n");
    assert_eq!(
        edited.rows(),
        [
            "early miss source: source changed",
            "main miss dependencies: dependency early changed",
        ]
    );
    assert_eq!(edited.js, project.uncached("main").js);
    // A protocol it starts to register is part of the view.
    project.write("early.cljs", "(ns early) (defprotocol P (m [_])) (def x 2)");
    let declared = project.cached("main", &[]);
    assert_eq!(declared.run(), "2 42\n");
    assert_eq!(
        declared.rows(),
        [
            "early miss source: source changed",
            "consumer miss view: earlier namespaces changed",
            "main miss dependencies: dependency early changed",
        ]
    );
    assert_eq!(declared.js, project.uncached("main").js);
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

/// A stale candidate, tried first because it is newer, costs only its
/// manifest: the key is compared before the rest of the artifact is read
/// and hashed.
#[test]
fn a_stale_candidate_is_read_only_to_its_manifest() {
    use light_cljs::artifact_codec::manifest_end;
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    project.cached("main", &[]);
    let first = project.objects();
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 5, "{}", warm.stats);
    let whole: u64 = first
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();
    assert_eq!(warm.stat("cache.bytes-read"), whole);
    project.write("plain.cljs", "(ns plain) (def answer (+ 40 3))");
    project.cached("main", &[]);
    let stale: Vec<PathBuf> = project
        .objects()
        .into_iter()
        .filter(|path| !first.contains(path))
        .collect();
    assert!(!stale.is_empty());
    // Back to the first sources: each namespace the edit republished tries
    // the edit's artifact first, misses on its key, then hits the first.
    project.write("plain.cljs", FIXTURE[3].1);
    let back = project.cached("main", &[]);
    assert_eq!(back.js, oracle.js);
    assert_eq!(back.stat("cache.hits"), 5, "{}", back.stats);
    let manifests: u64 = stale
        .iter()
        .map(|path| manifest_end(&fs::read(path).unwrap()).unwrap())
        .sum();
    let stale_whole: u64 = stale
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();
    assert!(manifests < stale_whole);
    assert_eq!(
        back.stat("cache.bytes-read"),
        whole + manifests,
        "{}",
        back.stats
    );
}

/// An artifact damaged so that its manifest and section hashes still check
/// (re-sealed), under the key a build looks up, fails its content hash and is
/// never installed.
#[test]
fn a_matching_candidate_that_fails_its_content_hash_is_a_miss() {
    use light_cljs::artifact_codec::{DecodeLimits, read_manifest, seal};
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    project.cached("main", &[]);
    let objects = project.objects();
    let mut bytes = fs::read(&objects[0]).unwrap();
    let namespace = read_manifest(&bytes, &DecodeLimits::default())
        .unwrap()
        .namespace;
    *bytes.last_mut().unwrap() ^= 0xff;
    seal(&mut bytes).unwrap();
    assert!(read_manifest(&bytes, &DecodeLimits::default()).is_ok());
    fs::write(&objects[0], &bytes).unwrap();
    let damaged = project.cached("main", &[]);
    assert_eq!(damaged.js, oracle.js);
    assert_eq!(damaged.run(), "h1 9 84 h2\n");
    assert_eq!(
        damaged.stat("cache.miss.unreadable"),
        1,
        "{}",
        damaged.stats
    );
    let row = format!("{namespace} miss unreadable: the artifact fails its content hash");
    assert!(
        damaged.rows().iter().any(|line| line.contains(&row)),
        "{row} in {:?}",
        damaged.rows()
    );
    let repaired = project.cached("main", &[]);
    assert_eq!(repaired.stat("cache.hits"), 5, "{}", repaired.stats);
    assert_eq!(repaired.js, oracle.js);
    // Damage inside the manifest fails its own hash before any key is
    // compared; the miss still names the content hash, as a whole read did.
    let objects = project.objects();
    let mut bytes = fs::read(&objects[1]).unwrap();
    let namespace = read_manifest(&bytes, &DecodeLimits::default())
        .unwrap()
        .namespace;
    let end = light_cljs::artifact_codec::manifest_end(&bytes).unwrap() as usize;
    bytes[end - 1] ^= 0xff;
    fs::write(&objects[1], &bytes).unwrap();
    let damaged = project.cached("main", &[]);
    assert_eq!(damaged.js, oracle.js);
    let row = format!("{namespace} miss unreadable: the artifact fails its content hash");
    assert!(
        damaged.rows().iter().any(|line| line.contains(&row)),
        "{row} in {:?}",
        damaged.rows()
    );
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

/// The cache is on by default (light-cljs-328): a build that names no cache
/// keeps it in `.light-cljs/cache` under the current directory, a second
/// build there is warm, `--no-cache` neither reads nor writes it, and
/// `--no-cache` with `--cache-dir` is refused.
#[test]
fn the_cache_is_on_by_default_under_the_current_directory() {
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    let default = project.dir.join(".light-cljs/cache");
    assert!(!default.exists(), "--no-cache wrote the default cache");
    let build = |extra: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .current_dir(&*project.dir)
            .args(["--compile", "--entry", "main", "--source-root", "src"])
            .arg("--stats")
            .args(extra)
            .output()
            .unwrap();
        Project::finish(output)
    };
    let populating = build(&[]);
    assert_eq!(populating.js, oracle.js);
    assert_eq!(
        populating.stat("cache.published"),
        5,
        "{}",
        populating.stats
    );
    assert!(default.join("v1/objects").is_dir());
    let warm = build(&[]);
    assert_eq!(warm.stat("cache.hits"), 5, "{}", warm.stats);
    assert_eq!(warm.stat("macro-calls"), 0);
    assert_eq!(warm.js, oracle.js);
    let objects = || fs::read_dir(default.join("v1/objects")).unwrap().count();
    let before = objects();
    // After the namespace flags too.
    let fresh = build(&["--no-cache"]);
    assert!(!fresh.stats.contains("cache.hits"), "{}", fresh.stats);
    assert_eq!(fresh.js, oracle.js);
    assert_eq!(objects(), before);
    // A dev build keeps its own artifacts in the same default directory.
    let dev = project.dir.join("dev");
    let reloadable = build(&["--reloadable", "--out-dir", dev.to_str().unwrap()]);
    assert_eq!(
        reloadable.stat("cache.published"),
        5,
        "{}",
        reloadable.stats
    );
    assert!(objects() > before);
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(&*project.dir)
        .args([
            "--compile",
            "--no-cache",
            "--entry",
            "main",
            "--source-root",
        ])
        .args(["src", "--cache-dir", "elsewhere"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("--no-cache and --cache-dir exclude each other"),
        "{stderr}"
    );
    assert!(!project.dir.join("elsewhere").exists());
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

#[test]
fn parallel_resource_and_configuration_reads_match_one_thread() {
    let project = Project::new(&[
        (
            "macros.cljs",
            "(ns macros (:require [light.build :as b])) (defmacro input [] (str (b/resource \"./data.txt\") (b/config :app/flavor)))",
        ),
        (
            "main.cljs",
            "(ns main (:require [app])) (js/console.log (app/value0))",
        ),
    ]);
    project.write("data.txt", "one");
    let definitions: String = (0..16)
        .map(|i| format!("(defn value{i} [] (input)) "))
        .collect();
    project.write(
        "app.cljs",
        &format!("(ns app (:require [macros :refer [input]])) {definitions}"),
    );
    // V8 workers each reserve 64 MiB; allow four isolates so resource
    // expansions actually run on workers in both builds.
    let config = [
        "--config",
        "app/flavor=:dev",
        "--macro-memory-limit",
        "268435456",
    ];
    let oracle = Project::finish(project.command("main", &config, false).output().unwrap());
    assert_eq!(oracle.run(), "one:dev\n");
    let options = [
        "--jobs",
        "4",
        "--config",
        "app/flavor=:dev",
        "--macro-memory-limit",
        "268435456",
    ];
    let cold = project.cached("main", &options);
    assert_eq!(cold.js, oracle.js);
    {
        assert!(cold.stat("pool.worker-isolates") > 0, "{}", cold.stats);
    }
    let warm = project.cached("main", &options);
    assert_eq!(warm.stat("cache.hits"), 4, "{}", warm.stats);
    assert_eq!(warm.js, oracle.js);
    project.write("data.txt", "two");
    let edited = project.cached("main", &options);
    assert_eq!(edited.run(), "two:dev\n");
    assert!(
        edited
            .rows()
            .contains(&"app miss inputs: external input changed"),
        "{}",
        edited.stats
    );
    let oracle = Project::finish(project.command("main", &config, false).output().unwrap());
    assert_eq!(edited.js, oracle.js);
}

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
    // `extra` and `flavor` ran source macros after `text`, which declares
    // nothing a source macro could see, so they hit (light-cljs-347).
    assert_eq!(edited.stat("cache.hits"), 5, "{}", edited.stats);
    assert_eq!(edited.stat("cache.miss.view"), 0);
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

/// A guarded optional read cached while its resource was absent misses
/// once a directory takes that path, since the read now fails and the
/// guard's other branch runs; an ambiguity the read caught and that has not
/// changed is still a hit.
#[test]
fn cached_optional_resource_absence_becoming_directory() {
    let project = Project::new(&[
        (
            "guard.cljs",
            "(ns guard (:require [light.build :as b])) (defmacro optional [path] (try (or (b/resource? path) \"absent\") (catch :default _ \"failed\")))",
        ),
        (
            "main.cljs",
            "(ns main (:require [guard :refer [optional]])) (js/console.log (optional \"data.txt\"))",
        ),
    ]);
    assert_eq!(project.cached("main", &[]).run(), "absent\n");
    assert_eq!(project.cached("main", &[]).stat("cache.hits"), 3);
    let directory = project.dir.join("src/data.txt");
    fs::create_dir(&directory).unwrap();
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "failed\n");
    // The stored absence is no longer what the lookup finds, and the read
    // that fails instead cannot be keyed: `main` recompiles unpublished.
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 2, "{}", warm.stats);
    assert_eq!(warm.rows(), ["main bypass non-cacheable-input main"]);
    assert_eq!(warm.js, oracle.js);
    fs::remove_dir(&directory).unwrap();
    // The same path in a second root: the caught ambiguity is keyed by its
    // candidates, and a warm build that finds them unchanged reuses it.
    let other = project.dir.join("other");
    fs::create_dir_all(&other).unwrap();
    project.write("data.txt", "one");
    fs::write(other.join("data.txt"), "two").unwrap();
    let roots = ["--source-root", other.to_str().unwrap()];
    let oracle = Project::finish(project.command("main", &roots, false).output().unwrap());
    assert_eq!(oracle.run(), "failed\n");
    assert_eq!(project.cached("main", &roots).js, oracle.js);
    let warm = project.cached("main", &roots);
    assert_eq!(warm.stat("cache.hits"), 3, "{}", warm.stats);
    assert_eq!(warm.js, oracle.js);
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

/// Parallel cache publishing (`--jobs 8`) populates the cache cleanly across
/// enough namespaces to span multiple commit batches, produces an identical
/// warm rebuild, leaves zero temporary files, and handles read-only cache
/// directories with a single warning.
#[test]
fn parallel_cache_publishing_populates_warms_and_handles_read_only() {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut requires = String::new();
    let mut sum = String::from("(+");
    // 80 leaf namespaces (> PUBLISH_BATCH_SIZE = 64) plus entry = 81 total.
    for i in 0..80 {
        let path = format!("leaf{i}.cljs");
        let body = format!(
            "(ns leaf{i})\n(defn f{i} [x] (+ x {i}))\n(def v{i} (f{i} {}))\n",
            i * 3
        );
        files.push((path, body));
        requires.push_str(&format!(" [leaf{i}]"));
        sum.push_str(&format!(" leaf{i}/v{i}"));
    }
    sum.push(')');
    files.push((
        "main.cljs".into(),
        format!("(ns main (:require{requires}))\n(js/console.log {sum})\n"),
    ));
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
    let project = Project::new(&refs);
    let oracle = project.uncached("main");
    let expected = oracle.run();

    let cold = project.cached("main", &["--jobs", "8"]);
    assert_eq!(cold.run(), expected);
    assert_eq!(cold.stat("cache.hits"), 0, "{}", cold.stats);
    assert_eq!(cold.stat("cache.published"), 81, "{}", cold.stats);
    assert_eq!(cold.stat("cache.unpublished"), 0, "{}", cold.stats);
    assert_eq!(project.objects().len(), 81);
    for sub in ["v1/objects", "v1/index"] {
        let temps: Vec<_> = fs::read_dir(project.cache().join(sub))
            .unwrap()
            .flatten()
            .filter(|e| name_of(&e.path()).starts_with(".tmp-"))
            .collect();
        assert!(temps.is_empty(), "leaked temporaries in {sub}: {temps:?}");
    }

    // Warm rebuild hits every namespace and reuses whole output.
    let warm = project.cached("main", &["--jobs", "8"]);
    assert_eq!(warm.run(), expected);
    assert_eq!(warm.stat("cache.hits"), 81, "{}", warm.stats);
    assert_eq!(warm.stat("cache.published"), 0, "{}", warm.stats);
    assert_eq!(warm.stat("cache.output.reused"), 1, "{}", warm.stats);

    // Read-only cache directory under `--jobs 8`: succeeds with exactly one
    // warning and zero leaked temporaries.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let ro_cache = project.dir.join("ro-cache");
        fs::create_dir_all(&ro_cache).unwrap();
        fs::set_permissions(&ro_cache, fs::Permissions::from_mode(0o555)).unwrap();
        let output = project
            .command(
                "main",
                &["--jobs", "8", "--cache-dir", ro_cache.to_str().unwrap()],
                false,
            )
            .output()
            .unwrap();
        let _ = fs::set_permissions(&ro_cache, fs::Permissions::from_mode(0o755));
        let build = Project::finish(output);
        assert_eq!(build.run(), expected);
        assert_eq!(build.stat("cache.published"), 0, "{}", build.stats);
        assert_eq!(build.stat("cache.unpublished"), 81, "{}", build.stats);
        let warnings: Vec<_> = build
            .stats
            .lines()
            .filter(|line| line.starts_with("warning: cannot write the compiler cache"))
            .collect();
        assert_eq!(warnings.len(), 1, "expected 1 warning:\n{}", build.stats);
    }
}

/// Verify that with `--jobs > 1`, an `owned_concurrent` provider runs publish
/// revalidation off the caller thread (on a cache staging worker), whereas a
/// serial provider (`owned_concurrent() == None`) runs revalidation on the
/// caller thread and still refuses changed inputs.
#[test]
fn parallel_publish_revalidates_only_owned_concurrent_providers_off_thread() {
    use light_cljs::MacroHost;
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compile_cache::{CacheOptions, CompileCache};
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded_with_provider};
    use light_cljs::dependency_manifest::Candidate;
    use light_cljs::scheduler::Slots;
    use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput, SourceProvider};
    use std::sync::{Arc, Mutex};
    use std::thread::ThreadId;

    #[derive(Clone)]
    struct TrackedConcurrent {
        sources: Arc<Vec<(String, String)>>,
        caller: ThreadId,
        from_owned_concurrent: bool,
        off_thread_revalidations: Arc<Mutex<Vec<String>>>,
    }

    impl SourceProvider for TrackedConcurrent {
        fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
            let name = String::from_utf16_lossy(request.namespace.name());
            if self.from_owned_concurrent
                && std::thread::current().id() != self.caller
                && std::thread::current()
                    .name()
                    .is_some_and(|n| n.starts_with("light-cljs-worker-"))
            {
                self.off_thread_revalidations
                    .lock()
                    .unwrap()
                    .push(name.clone());
            }
            Ok(self
                .sources
                .iter()
                .find(|(n, _)| n == &name)
                .map(|(n, s)| SourceInput::new(format!("{n}.cljs"), s.clone())))
        }
        fn load_traced(
            &self,
            request: &LoadRequest,
            _tried: &mut Vec<Candidate>,
        ) -> Result<Option<SourceInput>, LoadError> {
            self.load(request)
        }
        fn concurrent(&self) -> Option<&(dyn SourceProvider + Send + Sync)> {
            Some(self)
        }
        fn owned_concurrent(&self) -> Option<Arc<dyn SourceProvider + Send + Sync>> {
            let mut owned = self.clone();
            owned.from_owned_concurrent = true;
            Some(Arc::new(owned))
        }
    }

    struct TrackedSerial {
        sources: Vec<(String, String)>,
        caller: ThreadId,
        loads_seen: std::cell::RefCell<usize>,
        caller_revalidations: std::cell::RefCell<usize>,
        mutate_after_first_load: bool,
    }

    impl SourceProvider for TrackedSerial {
        fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
            assert_eq!(
                std::thread::current().id(),
                self.caller,
                "serial provider must only be called on caller thread"
            );
            let name = String::from_utf16_lossy(request.namespace.name());
            let mut count = self.loads_seen.borrow_mut();
            *count += 1;
            let is_revalidation = *count > self.sources.len();
            if is_revalidation {
                *self.caller_revalidations.borrow_mut() += 1;
            }
            Ok(self.sources.iter().find(|(n, _)| n == &name).map(|(n, s)| {
                let text = if is_revalidation && self.mutate_after_first_load && n == "app.leaf" {
                    "(ns app.leaf) (def v 999)".to_owned()
                } else {
                    s.clone()
                };
                SourceInput::new(format!("{n}.cljs"), text)
            }))
        }
    }

    let sources = vec![
        ("app.leaf".to_owned(), "(ns app.leaf) (def v 1)".to_owned()),
        (
            "app.main".to_owned(),
            "(ns app.main (:require [app.leaf :as l])) (+ l/v 1)".to_owned(),
        ),
    ];
    let caller = std::thread::current().id();
    let slots = Slots::new(4).unwrap();

    // 1. Owned concurrent provider: revalidation runs off-thread on a cache
    //    staging worker and publishes both namespaces.
    let dir1 = TempDir::new("parallel-reval-concurrent");
    let concurrent = TrackedConcurrent {
        sources: Arc::new(sources.clone()),
        caller,
        from_owned_concurrent: false,
        off_thread_revalidations: Arc::new(Mutex::new(Vec::new())),
    };
    let mut host = MacroHost::new();
    let options = AnalysisOptions::default();
    let cache_opts = CacheOptions {
        dir: dir1.join("cache"),
        ..CacheOptions::default()
    };
    let mut cache = CompileCache::open(&cache_opts, &host, options, "app.main");
    analyze_namespace_entry_recorded_with_provider(
        "app.main",
        &concurrent,
        &mut host,
        options,
        &mut Vec::new(),
        EntryRecorders {
            cache: Some(&mut cache),
            slots,
            ..Default::default()
        },
    )
    .unwrap();
    cache.finish();
    let counts = cache.take_counts();
    assert_eq!(counts.published, 2);
    assert!(counts.unpublished.is_empty());
    let off_thread = concurrent.off_thread_revalidations.lock().unwrap();
    assert!(
        off_thread.contains(&"app.leaf".to_owned()) && off_thread.contains(&"app.main".to_owned()),
        "expected off-thread revalidation on staging workers, got: {off_thread:?}"
    );

    // 2. Serial provider: revalidation runs on the caller thread and refuses
    //    `app.leaf` when its source changes between compile and publish.
    let dir2 = TempDir::new("parallel-reval-serial");
    let serial = TrackedSerial {
        sources,
        caller,
        loads_seen: std::cell::RefCell::new(0),
        caller_revalidations: std::cell::RefCell::new(0),
        mutate_after_first_load: true,
    };
    let mut host = MacroHost::new();
    let cache_opts = CacheOptions {
        dir: dir2.join("cache"),
        ..CacheOptions::default()
    };
    let mut cache = CompileCache::open(&cache_opts, &host, options, "app.main");
    analyze_namespace_entry_recorded_with_provider(
        "app.main",
        &serial,
        &mut host,
        options,
        &mut Vec::new(),
        EntryRecorders {
            cache: Some(&mut cache),
            slots,
            ..Default::default()
        },
    )
    .unwrap();
    cache.finish();
    let counts = cache.take_counts();
    assert!(*serial.caller_revalidations.borrow() >= 2);
    assert_eq!(counts.published, 1, "only unchanged app.main publishes");
    assert_eq!(counts.unpublished.len(), 1);
    assert_eq!(counts.unpublished[0].0, "app.leaf");
    assert!(
        counts.unpublished[0].1.contains("source changed"),
        "{:?}",
        counts.unpublished
    );
}

#[test]
fn cache_publishing_respects_two_cpu_slots() {
    use light_cljs::MacroHost;
    use light_cljs::analyzer::AnalysisOptions;
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    use light_cljs::compile_cache::CacheCpuKind;
    use light_cljs::compile_cache::{CacheOptions, CompileCache};
    use light_cljs::compile_stats::CompileStats;
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded_with_provider};
    use light_cljs::dependency_manifest::Candidate;
    use light_cljs::scheduler::Slots;
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    use light_cljs::scheduler::{Limits, Perturbation};
    use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput, SourceProvider};
    use std::collections::HashSet;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread::ThreadId;
    use std::time::Duration;

    // More than `PUBLISH_BATCH_SIZE` (64) + `DRAIN_LAG` (6) so at least one
    // full batch commits (index decode/encode/SHA-256) mid-build before
    // analysis finishes.
    const NS_COUNT: usize = 76;

    #[derive(Clone)]
    struct SlotInstrumentedProvider {
        sources: Arc<Vec<(String, String)>>,
        caller: ThreadId,
        from_owned_concurrent: bool,
        off_thread_ids: Arc<Mutex<HashSet<ThreadId>>>,
        off_thread_names: Arc<Mutex<Vec<String>>>,
        off_thread_stages: Arc<AtomicUsize>,
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        pool_log: Arc<Mutex<Vec<(usize, u64)>>>,
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        concurrent_pool_starts: Arc<AtomicUsize>,
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        deferred_at_stage_start: Arc<Mutex<Vec<usize>>>,
    }

    impl SourceProvider for SlotInstrumentedProvider {
        fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
            let name = String::from_utf16_lossy(request.namespace.name());
            if self.from_owned_concurrent {
                let tid = std::thread::current().id();
                if tid != self.caller {
                    let stage_idx = self.off_thread_stages.fetch_add(1, Ordering::SeqCst);
                    self.off_thread_ids.lock().unwrap().insert(tid);
                    self.off_thread_names.lock().unwrap().push(
                        std::thread::current()
                            .name()
                            .unwrap_or("<unnamed>")
                            .to_owned(),
                    );
                    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
                    {
                        let (log_before, deferred_before) = {
                            let log = self.pool_log.lock().unwrap();
                            let deferred = log.iter().filter(|(_, k)| *k < (1u64 << 60)).count();
                            (log.len(), deferred)
                        };
                        self.deferred_at_stage_start
                            .lock()
                            .unwrap()
                            .push(deferred_before);
                        if stage_idx < 3 {
                            std::thread::sleep(Duration::from_millis(12));
                        }
                        let log_after = self.pool_log.lock().unwrap().len();
                        self.concurrent_pool_starts
                            .fetch_add(log_after.saturating_sub(log_before), Ordering::SeqCst);
                    }
                    #[cfg(not(any(debug_assertions, feature = "scheduling-stress")))]
                    let _ = stage_idx;
                }
            }
            Ok(self
                .sources
                .iter()
                .find(|(n, _)| n == &name)
                .map(|(n, s)| SourceInput::new(format!("{n}.cljs"), s.clone())))
        }
        fn load_traced(
            &self,
            request: &LoadRequest,
            _tried: &mut Vec<Candidate>,
        ) -> Result<Option<SourceInput>, LoadError> {
            self.load(request)
        }
        fn concurrent(&self) -> Option<&(dyn SourceProvider + Send + Sync)> {
            Some(self)
        }
        fn owned_concurrent(&self) -> Option<Arc<dyn SourceProvider + Send + Sync>> {
            let mut owned = self.clone();
            owned.from_owned_concurrent = true;
            Some(Arc::new(owned))
        }
    }

    let mut sources = Vec::with_capacity(NS_COUNT);
    for i in 0..NS_COUNT {
        let mut src = if i == 0 {
            "(ns app.n0)\n(defmacro bump [x] (list '+ x 1))\n".to_owned()
        } else {
            format!(
                "(ns app.n{i} (:require [app.n0 :as m] [app.n{} :as prev]))\n",
                i - 1
            )
        };
        for f in 0..4 {
            if i > 0 && f == 0 {
                src.push_str(&format!("(defn f{f} [x] (m/bump (+ x {i} (* x 2))))\n"));
            } else {
                src.push_str(&format!("(defn f{f} [x] (+ x {i} {f} (* x 2)))\n"));
            }
        }
        if i == 0 {
            src.push_str("(def v (f0 1))\n");
        } else {
            src.push_str("(def v (+ (prev/f0 prev/v) (f3 2)))\n");
        }
        sources.push((format!("app.n{i}"), src));
    }
    let entry = format!("app.n{}", NS_COUNT - 1);
    let slots = Slots::new(2).unwrap();
    let dir = TempDir::new("cache-two-cpu-slots");
    let cache_dir = dir.join("cache");
    let caller = std::thread::current().id();

    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    let perturbation = Perturbation {
        seed: 1,
        limits: Some(Limits {
            phase_quantum: 2,
            ..Limits::default()
        }),
        ..Default::default()
    };
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    let pool_log = Arc::clone(&perturbation.log);

    let provider = SlotInstrumentedProvider {
        sources: Arc::new(sources),
        caller,
        from_owned_concurrent: false,
        off_thread_ids: Arc::new(Mutex::new(HashSet::new())),
        off_thread_names: Arc::new(Mutex::new(Vec::new())),
        off_thread_stages: Arc::new(AtomicUsize::new(0)),
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        pool_log: Arc::clone(&pool_log),
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        concurrent_pool_starts: Arc::new(AtomicUsize::new(0)),
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        deferred_at_stage_start: Arc::new(Mutex::new(Vec::new())),
    };

    let mut host = MacroHost::new();
    // Leave room for the single `--jobs 2` worker to hold a compile-time
    // engine isolate so the test exercises both macro-expanding deferred units
    // and engine-less cache CPU work sharing that one worker slot.
    host.evaluator_options.memory_limit = 256 << 20;
    let options = AnalysisOptions::default();
    let cache_opts = CacheOptions {
        dir: cache_dir.clone(),
        ..CacheOptions::default()
    };
    let mut cache = CompileCache::open(&cache_opts, &host, options, &entry);

    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    let (
        artifact_cpu_count,
        index_cpu_count,
        cpu_worker_ids,
        cpu_thread_names,
        cpu_concurrent_starts,
        index_deferred_before,
        artifact_deferred_before,
    ) = {
        let artifact_cpu_count = Arc::new(AtomicUsize::new(0));
        let index_cpu_count = Arc::new(AtomicUsize::new(0));
        let cpu_worker_ids = Arc::new(Mutex::new(HashSet::new()));
        let cpu_thread_names = Arc::new(Mutex::new(Vec::new()));
        let cpu_concurrent_starts = Arc::new(AtomicUsize::new(0));
        let index_deferred_before = Arc::new(Mutex::new(Vec::new()));
        let artifact_deferred_before = Arc::new(Mutex::new(Vec::new()));

        let a_cnt = Arc::clone(&artifact_cpu_count);
        let i_cnt = Arc::clone(&index_cpu_count);
        let w_ids = Arc::clone(&cpu_worker_ids);
        let t_names = Arc::clone(&cpu_thread_names);
        let conc = Arc::clone(&cpu_concurrent_starts);
        let idx_def = Arc::clone(&index_deferred_before);
        let art_def = Arc::clone(&artifact_deferred_before);
        let log = Arc::clone(&pool_log);

        cache.set_cpu_work_hook(Some(Arc::new(move |kind| {
            let tid = std::thread::current().id();
            let name = std::thread::current()
                .name()
                .unwrap_or("<unnamed>")
                .to_owned();
            t_names.lock().unwrap().push((kind, name));
            let count = match kind {
                CacheCpuKind::ArtifactEncodeHash => a_cnt.fetch_add(1, Ordering::SeqCst),
                CacheCpuKind::IndexEncodeHash => i_cnt.fetch_add(1, Ordering::SeqCst),
            };
            let (log_before, deferred_before) = {
                let guard = log.lock().unwrap();
                let deferred = guard.iter().filter(|(_, k)| *k < (1u64 << 60)).count();
                (guard.len(), deferred)
            };
            match kind {
                CacheCpuKind::ArtifactEncodeHash => art_def
                    .lock()
                    .unwrap()
                    .push((tid != caller, deferred_before)),
                CacheCpuKind::IndexEncodeHash => idx_def
                    .lock()
                    .unwrap()
                    .push((tid != caller, deferred_before)),
            }
            if tid != caller {
                w_ids.lock().unwrap().insert(tid);
                if count < 3 {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let log_after = log.lock().unwrap().len();
                conc.fetch_add(log_after.saturating_sub(log_before), Ordering::SeqCst);
            }
        })));
        (
            artifact_cpu_count,
            index_cpu_count,
            cpu_worker_ids,
            cpu_thread_names,
            cpu_concurrent_starts,
            index_deferred_before,
            artifact_deferred_before,
        )
    };

    let mut stats = CompileStats::default();
    analyze_namespace_entry_recorded_with_provider(
        &entry,
        &provider,
        &mut host,
        options,
        &mut Vec::new(),
        EntryRecorders {
            stats: Some(&mut stats),
            cache: Some(&mut cache),
            slots,
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
            perturbation: Some(perturbation),
            ..Default::default()
        },
    )
    .unwrap();
    cache.finish();
    let counts = cache.take_counts();
    assert_eq!(counts.published, NS_COUNT);
    assert!(counts.unpublished.is_empty(), "{:?}", counts.unpublished);
    assert!(
        stats.deferred_units.pooled > 0,
        "expected deferred units to run on the pool"
    );

    let off_thread_stages = provider.off_thread_stages.load(Ordering::SeqCst);
    assert!(
        off_thread_stages > 0,
        "expected cache staging to execute on the pool worker thread"
    );
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    {
        let concurrent = provider.concurrent_pool_starts.load(Ordering::SeqCst);
        assert_eq!(
            concurrent, 0,
            "deferred analysis jobs started concurrently ({concurrent}) while cache revalidation held a worker slot under --jobs 2"
        );
        let cpu_concurrent = cpu_concurrent_starts.load(Ordering::SeqCst);
        assert_eq!(
            cpu_concurrent, 0,
            "deferred analysis jobs started concurrently ({cpu_concurrent}) while cache artifact/index encode+hash held a worker slot under --jobs 2"
        );
        assert_eq!(
            artifact_cpu_count.load(Ordering::SeqCst),
            NS_COUNT,
            "expected every namespace's artifact encode+hash to be observed"
        );
        assert!(
            index_cpu_count.load(Ordering::SeqCst) >= 64,
            "expected commit-side index encode+hash work to be observed"
        );
        {
            let names = cpu_thread_names.lock().unwrap();
            assert!(
                names
                    .iter()
                    .all(|(_, name)| name != "light-cljs-cache-commit"),
                "commit-side or artifact CPU work ran on the I/O committer thread: {:?}",
                names
                    .iter()
                    .filter(|(_, n)| n == "light-cljs-cache-commit")
                    .collect::<Vec<_>>()
            );
            let worker_ids = cpu_worker_ids.lock().unwrap();
            assert_eq!(
                worker_ids.len(),
                1,
                "at --jobs 2, at most one worker thread may run cache CPU work, got {}",
                worker_ids.len()
            );
            let stage_ids = provider.off_thread_ids.lock().unwrap();
            assert_eq!(
                *worker_ids, *stage_ids,
                "artifact/index CPU work and revalidation must share the same single pool worker"
            );
        }
        let log = pool_log.lock().unwrap();
        let total_deferred = log.iter().filter(|(_, k)| *k < (1u64 << 60)).count();
        let total_cache_pool_jobs = log.iter().filter(|(_, k)| *k >= (1u64 << 60)).count();
        assert!(total_deferred > 0, "expected deferred jobs in pool log");
        assert!(
            total_cache_pool_jobs > 0,
            "expected cache staging/commit CPU jobs in pool log"
        );
        assert!(
            log.iter().all(|(worker, _)| *worker == 0),
            "expected all pool jobs on the single --jobs 2 worker"
        );
        let art_snaps = artifact_deferred_before.lock().unwrap();
        assert!(
            art_snaps
                .iter()
                .any(|&(off_thread, before)| off_thread && before > 0 && before < total_deferred),
            "expected off-thread artifact encode+hash to overlap with deferred analysis (total_deferred={total_deferred})"
        );
        let idx_snaps = index_deferred_before.lock().unwrap();
        assert!(
            idx_snaps
                .iter()
                .any(|&(_, before)| before > 0 && before < total_deferred),
            "expected a commit batch (index encode+hash) to run before deferred analysis ended (total_deferred={total_deferred})"
        );
    }

    {
        let off_thread_ids = provider.off_thread_ids.lock().unwrap();
        assert_eq!(
            off_thread_ids.len(),
            1,
            "at --jobs 2, at most one worker thread may run cache staging, got {} threads",
            off_thread_ids.len()
        );
        let off_thread_names = provider.off_thread_names.lock().unwrap();
        assert!(
            off_thread_names
                .iter()
                .all(|name| name.starts_with("light-cljs-worker-")),
            "expected cache staging to run on the scheduler worker thread, got: {off_thread_names:?}"
        );
    }

    // Warm rebuild hits every published namespace.
    let mut host2 = MacroHost::new();
    host2.evaluator_options.memory_limit = 256 << 20;
    let mut cache2 = CompileCache::open(&cache_opts, &host2, options, &entry);
    analyze_namespace_entry_recorded_with_provider(
        &entry,
        &provider,
        &mut host2,
        options,
        &mut Vec::new(),
        EntryRecorders {
            cache: Some(&mut cache2),
            slots,
            ..Default::default()
        },
    )
    .unwrap();
    cache2.finish();
    let warm = cache2.take_counts();
    assert_eq!(warm.hits, NS_COUNT);
    assert!(warm.misses.is_empty(), "{:?}", warm.misses);
    assert_eq!(warm.published, 0);
}

#[cfg(any(debug_assertions, feature = "scheduling-stress"))]
#[test]
fn cache_staging_io_does_not_block_deferred_analysis() {
    use light_cljs::MacroHost;
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compile_cache::{CacheOptions, CompileCache};
    use light_cljs::compile_stats::CompileStats;
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded_with_provider};
    use light_cljs::dependency_manifest::Candidate;
    use light_cljs::scheduler::Slots;
    use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput, SourceProvider};
    use std::sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};

    const NS_COUNT: usize = 12;

    #[derive(Clone)]
    struct MemProvider {
        sources: Arc<Vec<(String, String)>>,
    }

    impl SourceProvider for MemProvider {
        fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
            let name = String::from_utf16_lossy(request.namespace.name());
            Ok(self
                .sources
                .iter()
                .find(|(n, _)| n == &name)
                .map(|(n, s)| SourceInput::new(format!("{n}.cljs"), s.clone())))
        }
        fn load_traced(
            &self,
            request: &LoadRequest,
            _tried: &mut Vec<Candidate>,
        ) -> Result<Option<SourceInput>, LoadError> {
            self.load(request)
        }
        fn concurrent(&self) -> Option<&(dyn SourceProvider + Send + Sync)> {
            Some(self)
        }
        fn owned_concurrent(&self) -> Option<Arc<dyn SourceProvider + Send + Sync>> {
            Some(Arc::new(self.clone()))
        }
    }

    let mut sources = Vec::with_capacity(NS_COUNT);
    for i in 0..NS_COUNT {
        let mut src = if i == 0 {
            "(ns app.io0)\n(defmacro bump [x] (list '+ x 1))\n".to_owned()
        } else {
            format!(
                "(ns app.io{i} (:require [app.io0 :as m] [app.io{} :as prev]))\n",
                i - 1
            )
        };
        for f in 0..6 {
            if i > 0 && f == 0 {
                src.push_str(&format!("(defn f{f} [x] (m/bump (+ x {i} (* x 2))))\n"));
            } else {
                src.push_str(&format!("(defn f{f} [x] (+ x {i} {f} (* x 2)))\n"));
            }
        }
        if i == 0 {
            src.push_str("(def v (f0 1))\n");
        } else {
            src.push_str("(def v (+ (prev/f0 prev/v) (f5 2)))\n");
        }
        sources.push((format!("app.io{i}"), src));
    }
    let entry = format!("app.io{}", NS_COUNT - 1);
    let provider = MemProvider {
        sources: Arc::new(sources),
    };
    let dir = TempDir::new("cache-staging-io-nonblocking");
    let cache_dir = dir.join("cache");

    let mut host = MacroHost::new();
    host.evaluator_options.memory_limit = 256 << 20;
    let options = AnalysisOptions::default();
    let cache_opts = CacheOptions {
        dir: cache_dir,
        ..CacheOptions::default()
    };
    let mut cache = CompileCache::open(&cache_opts, &host, options, &entry);

    // Gate the very first temporary object file write and verify it runs on
    // the I/O committer thread, never blocking the sole `--jobs 2` pool worker
    // from finishing all later macro-expanding deferred units.
    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let hooked_once = Arc::new(AtomicBool::new(false));
    let write_on_committer = Arc::new(AtomicBool::new(false));
    let hook_gate = Arc::clone(&gate);
    let hook_once = Arc::clone(&hooked_once);
    let hook_committer = Arc::clone(&write_on_committer);
    cache.set_write_temporary_hook(Some(Arc::new(move |dir| {
        if dir.file_name().and_then(|n| n.to_str()) == Some("objects")
            && !hook_once.swap(true, Ordering::SeqCst)
        {
            hook_committer.store(
                std::thread::current().name() == Some("light-cljs-cache-commit"),
                Ordering::SeqCst,
            );
            let (mu, cv) = &*hook_gate;
            let mut st = mu.lock().unwrap();
            st.0 = true;
            cv.notify_all();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !st.1 && Instant::now() < deadline {
                let (next, _) = cv.wait_timeout(st, Duration::from_millis(50)).unwrap();
                st = next;
            }
        }
    })));

    let mut stats = CompileStats::default();
    analyze_namespace_entry_recorded_with_provider(
        &entry,
        &provider,
        &mut host,
        options,
        &mut Vec::new(),
        EntryRecorders {
            stats: Some(&mut stats),
            cache: Some(&mut cache),
            slots: Slots::new(2).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();

    // All deferred units (including later macro-expanding units on the sole
    // `--jobs 2` pool worker) finished while the first staged temp-file write
    // is still gated on the I/O committer thread.
    assert!(
        stats.deferred_units.pooled > 0,
        "expected pooled deferred units to finish while staging I/O was gated"
    );
    {
        let (mu, cv) = &*gate;
        let mut st = mu.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !st.0 && Instant::now() < deadline {
            let (next, _) = cv.wait_timeout(st, Duration::from_millis(20)).unwrap();
            st = next;
        }
        assert!(st.0, "expected first staged object write to enter gate");
        assert!(
            write_on_committer.load(Ordering::SeqCst),
            "staged object write must run on light-cljs-cache-commit, not on a pool worker"
        );
        // Release the gated write now that analysis and all pooled deferred
        // units have already completed.
        st.1 = true;
        cv.notify_all();
    }

    cache.finish();
    let counts = cache.take_counts();
    assert_eq!(counts.published, NS_COUNT);
    assert!(counts.unpublished.is_empty(), "{:?}", counts.unpublished);
}

#[test]
fn warm_rebuild_with_jobs_8_has_zero_engine_entries() {
    let project = Project::new(FIXTURE);
    let oracle = project.uncached("main");
    let args = ["--jobs", "8", "--macro-memory-limit", "268435456"];
    let cold = project.cached("main", &args);
    assert_eq!(cold.js, oracle.js);
    assert_eq!(cold.stat("cache.published"), 5, "{}", cold.stats);

    let warm = project.cached("main", &args);
    assert_eq!(warm.js, oracle.js);
    assert_eq!(warm.run(), "h1 9 84 h2\n");
    assert_eq!(warm.stat("cache.hits"), 5, "{}", warm.stats);
    assert_eq!(warm.stat("cache.published"), 0, "{}", warm.stats);
    assert!(
        warm.stats
            .lines()
            .any(|line| line == "trace.engine.entries 0"),
        "100% warm cache rebuild at --jobs 8 with worker isolates eligible must emit literal trace.engine.entries 0:\n{}",
        warm.stats
    );
}

#[test]
fn sliding_window_batch_lookup_recovers_from_mid_window_corruption() {
    use light_cljs::artifact_codec::{DecodeLimits, manifest_end, read_manifest, seal};

    let files = [
        ("n0.cljs", "(ns n0) (def v0 1)"),
        ("n1.cljs", "(ns n1 (:require [n0])) (def v1 (+ n0/v0 2))"),
        ("n2.cljs", "(ns n2 (:require [n1])) (def v2 (+ n1/v1 3))"),
        ("n3.cljs", "(ns n3 (:require [n2])) (def v3 (+ n2/v2 4))"),
        ("n4.cljs", "(ns n4 (:require [n3])) (def v4 (+ n3/v3 5))"),
        ("n5.cljs", "(ns n5 (:require [n4])) (def v5 (+ n4/v4 6))"),
        ("n6.cljs", "(ns n6 (:require [n5])) (def v6 (+ n5/v5 7))"),
        (
            "main.cljs",
            "(ns main (:require [n6])) (js/console.log n6/v6)",
        ),
    ];
    let project = Project::new(&files);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "28\n");
    let cold = project.cached("main", &["--jobs", "8"]);
    assert_eq!(cold.js, oracle.js);
    assert_eq!(cold.stat("cache.published"), 8, "{}", cold.stats);

    let warm = project.cached("main", &["--jobs", "8"]);
    assert_eq!(warm.js, oracle.js);
    assert_eq!(warm.stat("cache.hits"), 8, "{}", warm.stats);
    assert_eq!(warm.stat("trace.engine.entries"), 0, "{}", warm.stats);

    // Corrupt a mid-window artifact (`n3`) and re-seal its manifest & section
    // hashes so only full-file content-hash verification catches it.
    let n3_object = project
        .objects()
        .into_iter()
        .find(|path| {
            read_manifest(&fs::read(path).unwrap(), &DecodeLimits::default())
                .unwrap()
                .namespace
                == "n3"
        })
        .expect("n3 artifact exists");
    let mut bytes = fs::read(&n3_object).unwrap();
    *bytes.last_mut().unwrap() ^= 0xff;
    seal(&mut bytes).unwrap();
    fs::write(&n3_object, &bytes).unwrap();

    let recovered_whole: u64 = project
        .objects()
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();
    let recovered = project.cached("main", &["--jobs", "8"]);
    assert_eq!(recovered.js, oracle.js);
    assert_eq!(recovered.run(), "28\n");
    assert_eq!(
        recovered.stat("cache.miss.unreadable"),
        1,
        "{}",
        recovered.stats
    );
    assert_eq!(
        recovered.stat("cache.bytes-read"),
        recovered_whole,
        "mid-window corruption must count completed Stage C reads once and reuse matching later prefetched hits without re-reading:\n{}",
        recovered.stats
    );
    assert!(
        recovered
            .rows()
            .iter()
            .any(|row| row.contains("n3 miss unreadable: the artifact fails its content hash")),
        "{:?}",
        recovered.rows()
    );

    // After recovery, the repaired cache hits 8/8 at --jobs 8.
    let repaired = project.cached("main", &["--jobs", "8"]);
    assert_eq!(repaired.js, oracle.js);
    assert_eq!(repaired.stat("cache.hits"), 8, "{}", repaired.stats);

    // Stale candidates under --jobs 8 still read only their manifest prefix.
    let first = project.objects();
    let whole: u64 = first
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();
    project.write("n4.cljs", "(ns n4 (:require [n3])) (def v4 (+ n3/v3 50))");
    project.cached("main", &["--jobs", "8"]);
    let stale: Vec<PathBuf> = project
        .objects()
        .into_iter()
        .filter(|path| !first.contains(path))
        .collect();
    assert!(!stale.is_empty());
    project.write("n4.cljs", "(ns n4 (:require [n3])) (def v4 (+ n3/v3 5))");
    let back = project.cached("main", &["--jobs", "8"]);
    assert_eq!(back.js, oracle.js);
    assert_eq!(back.stat("cache.hits"), 8, "{}", back.stats);
    let manifests: u64 = stale
        .iter()
        .map(|path| manifest_end(&fs::read(path).unwrap()).unwrap())
        .sum();
    assert_eq!(
        back.stat("cache.bytes-read"),
        whole + manifests,
        "{}",
        back.stats
    );
}

#[test]
fn warm_rebuild_skips_trim_when_within_retention_and_trims_when_exceeding() {
    let project = Project::new(FIXTURE);
    // Populate two versions of `plain` and `main` (default retention = 3).
    project.cached("main", &["--jobs", "4"]);
    project.write("plain.cljs", "(ns plain) (def answer (+ 40 3))");
    project.cached("main", &["--jobs", "4"]);
    let count_two_versions = project.objects().len();
    assert!(
        count_two_versions > 5,
        "expected > 5 objects across two versions, got {count_two_versions}"
    );

    // Place a sentinel orphan object in `v1/objects/`. A `Store::trim` run
    // removes unindexed objects, so its survival proves `trim` was skipped.
    let sentinel = project
        .cache()
        .join("v1/objects")
        .join("0000000000000000000000000000000000000000000000000000000000000001");
    fs::write(&sentinel, b"orphan-sentinel!").unwrap();

    // Warm rebuild already within `--cache-retention 3` skips `trim`.
    let within = project.cached("main", &["--jobs", "4", "--cache-retention", "3"]);
    assert_eq!(within.stat("cache.hits"), 5, "{}", within.stats);
    assert_eq!(within.stat("cache.published"), 0, "{}", within.stats);
    assert!(
        sentinel.exists(),
        "0-publish warm build within retention must skip Store::trim"
    );

    // Warm rebuild with smaller `--cache-retention 1` still runs `trim` and
    // evicts both the older version's objects and the sentinel orphan.
    let tighter = project.cached("main", &["--jobs", "4", "--cache-retention", "1"]);
    assert_eq!(tighter.stat("cache.hits"), 5, "{}", tighter.stats);
    assert_eq!(tighter.stat("cache.published"), 0, "{}", tighter.stats);
    assert!(
        !sentinel.exists(),
        "0-publish warm build with tighter --cache-retention must still run Store::trim"
    );
    assert_eq!(
        project.objects().len(),
        5,
        "tighter --cache-retention 1 must trim down to 1 artifact per namespace"
    );
}

#[test]
fn warm_batch_trims_missing_candidate_before_older_hit() {
    use light_cljs::artifact_codec::{DecodeLimits, read_manifest};

    let project = Project::new(FIXTURE);
    let first = project.cached("main", &["--jobs", "4"]);
    assert_eq!(first.stat("cache.published"), 5, "{}", first.stats);
    let first_objects = project.objects();

    // Publish a second revision of `plain` (and `main`), then delete the newer
    // `plain` object while leaving its index entry intact.
    project.write("plain.cljs", "(ns plain) (def answer (+ 40 3))");
    let second = project.cached("main", &["--jobs", "4"]);
    assert_eq!(second.stat("cache.published"), 2, "{}", second.stats);
    let newer_plain = project
        .objects()
        .into_iter()
        .find(|path| {
            !first_objects.contains(path)
                && read_manifest(&fs::read(path).unwrap(), &DecodeLimits::default())
                    .unwrap()
                    .namespace
                    == "plain"
        })
        .expect("newer plain object exists");
    fs::remove_file(&newer_plain).unwrap();

    // Revert to the older `plain` sources. The batched rebuild sees the missing
    // newer candidate, hits the older artifact, publishes nothing, and must
    // still run `Store::trim` so the dangling index entry is cleaned up.
    project.write("plain.cljs", "(ns plain) (def answer (+ 40 2))");
    let store = light_cljs::artifact_store::Store::open(project.cache());
    assert_eq!(store.candidates("plain").len(), 2);

    let rebuilt = project.cached("main", &["--jobs", "4"]);
    assert_eq!(rebuilt.js, first.js);
    assert_eq!(rebuilt.stat("cache.hits"), 5, "{}", rebuilt.stats);
    assert_eq!(rebuilt.stat("cache.published"), 0, "{}", rebuilt.stats);
    assert_eq!(
        store.candidates("plain").len(),
        1,
        "missing newer candidate before older hit must still trigger trim to drop the dangling index entry"
    );
}

#[test]
fn batched_config_wide_miss_counts_and_reuses_lookahead_reads() {
    use light_cljs::artifact_codec::manifest_end;

    let files = [
        ("n0.cljs", "(ns n0) (def v0 1)"),
        ("n1.cljs", "(ns n1 (:require [n0])) (def v1 (+ n0/v0 2))"),
        ("n2.cljs", "(ns n2 (:require [n1])) (def v2 (+ n1/v1 3))"),
        ("n3.cljs", "(ns n3 (:require [n2])) (def v3 (+ n2/v2 4))"),
        (
            "main.cljs",
            "(ns main (:require [n3])) (js/console.log n3/v3)",
        ),
    ];
    let project = Project::new(&files);
    let cold = project.cached("main", &["--jobs", "4"]);
    assert_eq!(cold.stat("cache.published"), 5, "{}", cold.stats);

    let manifests: u64 = project
        .objects()
        .iter()
        .map(|path| manifest_end(&fs::read(path).unwrap()).unwrap())
        .sum();

    // Change a build option so every namespace misses on candidate 0.
    // Stage A lookahead reads each namespace's manifest prefix once and
    // retains unread lookahead entries across misses so total `bytes-read`
    // matches the exact sum of one manifest prefix per namespace.
    let changed = project.cached("main", &["--jobs", "4", "--elide-asserts"]);
    assert_eq!(changed.stat("cache.hits"), 0, "{}", changed.stats);
    assert_eq!(changed.stat("cache.misses"), 5, "{}", changed.stats);
    assert_eq!(
        changed.stat("cache.bytes-read"),
        manifests,
        "config-wide miss must count and reuse lookahead prefix reads exactly once:\n{}",
        changed.stats
    );
}

#[test]
fn lookup_window_bounds_older_candidate_working_set() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::artifact_codec::{DecodeLimits, manifest_end, read_manifest};
    use light_cljs::compile_cache::{CacheOptions, CompileCache};
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded_with_provider};
    use light_cljs::macro_host::MacroHost;
    use light_cljs::scheduler::Slots;
    use light_cljs::source_provider::FileSourceProvider;
    use std::sync::{Arc, Mutex};

    let dir = TempDir::new("lookup-window-older-working-set");
    let src_dir = dir.join("src");
    let cache_dir = dir.join("cache");
    fs::create_dir_all(&src_dir).unwrap();

    let payload = "x".repeat(4096);
    let write_rev = |large: bool| {
        for i in 0..7usize {
            let req = if i == 0 {
                String::new()
            } else {
                format!(" (:require [n{}])", i - 1)
            };
            let body = if large {
                format!("(ns n{i}{req}) (def s \"{payload}-{i}\") (def v {i})")
            } else {
                format!("(ns n{i}{req}) (def v {i})")
            };
            fs::write(src_dir.join(format!("n{i}.cljs")), body).unwrap();
        }
        let main_body = if large {
            format!("(ns main (:require [n6])) (def s \"{payload}-main\") (def out n6/v)")
        } else {
            "(ns main (:require [n6])) (def out n6/v)".to_owned()
        };
        fs::write(src_dir.join("main.cljs"), main_body).unwrap();
    };

    let run_build = |cache: &mut CompileCache| {
        let provider = FileSourceProvider::new(vec![src_dir.clone()]);
        let mut host = MacroHost::new();
        let mut warnings = Vec::new();
        analyze_namespace_entry_recorded_with_provider(
            "main",
            &provider,
            &mut host,
            AnalysisOptions::default(),
            &mut warnings,
            EntryRecorders {
                cache: Some(cache),
                slots: Slots::new(4).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
    };

    let cache_opts = CacheOptions {
        dir: cache_dir.clone(),
        ..CacheOptions::default()
    };

    // Revision 1: 8 large namespaces.
    write_rev(true);
    {
        let host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(4).unwrap(),
        );
        run_build(&mut cache);
        cache.finish();
        assert_eq!(cache.take_counts().published, 8);
    }
    let large_objects: Vec<PathBuf> = fs::read_dir(cache_dir.join("v1/objects"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !name_of(p).starts_with(".tmp"))
        .collect();
    assert_eq!(large_objects.len(), 8);
    let large_whole: u64 = large_objects
        .iter()
        .map(|p| fs::metadata(p).unwrap().len())
        .sum();
    let max_large_len: u64 = large_objects
        .iter()
        .map(|p| fs::metadata(p).unwrap().len())
        .max()
        .unwrap();

    // Revision 2: 8 small namespaces (newest index entries).
    write_rev(false);
    {
        let host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(4).unwrap(),
        );
        run_build(&mut cache);
        cache.finish();
        assert_eq!(cache.take_counts().published, 8);
    }
    let small_objects: Vec<PathBuf> = fs::read_dir(cache_dir.join("v1/objects"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !name_of(p).starts_with(".tmp") && !large_objects.contains(p))
        .collect();
    assert_eq!(small_objects.len(), 8);
    let small_total: u64 = small_objects
        .iter()
        .map(|p| fs::metadata(p).unwrap().len())
        .sum();
    let small_manifests: u64 = small_objects
        .iter()
        .map(|p| manifest_end(&fs::read(p).unwrap()).unwrap())
        .sum();

    // Revert to revision 1 (large older candidates behind small newest entries).
    write_rev(true);

    // Case 1: encoded cap admits all 8 small newest entries in Stage A, while
    // Stage B charges the selected older Reading::length() and splits Stage C
    // into bounded batches; tight shared decoded cap also exercises deferred
    // single-artifact fallback without spurious undecodable misses.
    let encoded_cap = (max_large_len * 2 + 512).max(small_total + 1024);
    assert!(
        large_whole > encoded_cap,
        "large_whole ({large_whole}) must exceed encoded_cap ({encoded_cap})"
    );
    let batches: Arc<Mutex<Vec<(usize, u64)>>> = Arc::new(Mutex::new(Vec::new()));
    let batches_hook = Arc::clone(&batches);
    let deferred_residents: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let deferred_hook = Arc::clone(&deferred_residents);
    {
        let host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(4).unwrap(),
        );
        cache.set_lookup_window_limits(encoded_cap, 12 * 1024);
        cache.set_stage_c_batch_hook(Some(Arc::new(move |len, bytes| {
            batches_hook.lock().unwrap().push((len, bytes));
        })));
        cache.set_deferred_decode_hook(Some(Arc::new(move |resident_decoded| {
            deferred_hook.lock().unwrap().push(resident_decoded);
        })));
        run_build(&mut cache);
        cache.finish();
        let counts = cache.take_counts();
        assert_eq!(counts.hits, 8);
        assert_eq!(counts.published, 0);
        assert_eq!(counts.bytes_read, small_manifests + large_whole);
    }
    let recorded_deferred = deferred_residents.lock().unwrap().clone();
    assert!(
        !recorded_deferred.is_empty(),
        "tight shared decoded cap must trigger Stage D deferred decoding"
    );
    assert!(
        recorded_deferred.iter().all(|&resident| resident == 0),
        "deferred oversized artifact must decode with no later decoded hits resident, saw {recorded_deferred:?}"
    );
    let recorded = batches.lock().unwrap().clone();
    assert!(
        recorded.len() >= 2,
        "expected older large candidates to split into multiple Stage C batches, got {recorded:?}"
    );
    for &(len, bytes) in &recorded {
        if len > 1 {
            assert!(
                bytes <= encoded_cap,
                "multi-item Stage C batch exceeded encoded cap: len={len}, bytes={bytes}, cap={encoded_cap}"
            );
        }
    }

    // Case 2: an oversized older candidate (> encoded_cap, while multiple
    // small newest entries still fit within encoded_cap for Stage A) runs
    // alone in Stage C under the existing per-artifact DecodeLimits and still
    // hits cleanly.
    let single_cap = (max_large_len - 1).max(small_total + 512);
    assert!(
        small_total < single_cap && max_large_len > single_cap,
        "small_total ({small_total}) < single_cap ({single_cap}) < max_large_len ({max_large_len})"
    );
    let single_batches: Arc<Mutex<Vec<(usize, u64)>>> = Arc::new(Mutex::new(Vec::new()));
    let single_hook = Arc::clone(&single_batches);
    {
        let host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(4).unwrap(),
        );
        cache.set_lookup_window_limits(single_cap, 1024);
        cache.set_stage_c_batch_hook(Some(Arc::new(move |len, bytes| {
            single_hook.lock().unwrap().push((len, bytes));
        })));
        run_build(&mut cache);
        cache.finish();
        let counts = cache.take_counts();
        assert_eq!(counts.hits, 8);
        assert_eq!(counts.published, 0);
        assert_eq!(counts.bytes_read, small_manifests + large_whole);
    }
    let recorded_single = single_batches.lock().unwrap().clone();
    assert_eq!(
        recorded_single.len(),
        8,
        "every oversized older candidate must run alone in Stage C, got {recorded_single:?}"
    );
    assert!(
        recorded_single.iter().all(|&(len, _)| len == 1),
        "oversized candidates must run alone in Stage C, got {recorded_single:?}"
    );
    // Keep unused import check clean when reading manifest metadata.
    assert_eq!(
        read_manifest(
            &fs::read(&large_objects[0]).unwrap(),
            &DecodeLimits::default()
        )
        .unwrap()
        .length,
        fs::metadata(&large_objects[0]).unwrap().len()
    );
}

#[test]
fn cache_lookup_respects_two_cpu_slots() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compile_cache::{CacheOptions, CompileCache};
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded_with_provider};
    use light_cljs::macro_host::MacroHost;
    use light_cljs::scheduler::Slots;
    use light_cljs::source_provider::FileSourceProvider;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    let dir = TempDir::new("cache-lookup-two-slots");
    let src_dir = dir.join("src");
    let cache_dir = dir.join("cache");
    fs::create_dir_all(&src_dir).unwrap();

    // `m0` is required before `h1..h4` by `main` and has macro-free deferred
    // units; `h1..h4` do not depend on `m0`, so modifying `m0` triggers a miss
    // that starts the `--jobs 2` UnitPool followed by a 4-namespace cache hit
    // window.
    let write_sources = |m0_val: i32| {
        fs::write(
            src_dir.join("m0.cljs"),
            format!(
                "(ns m0)\n(defn f0 [x] (+ x {m0_val}))\n(defn f1 [x] (* x 2))\n(defn f2 [x] (- x 1))"
            ),
        )
        .unwrap();
        for i in 1..=4usize {
            fs::write(
                src_dir.join(format!("h{i}.cljs")),
                format!("(ns h{i}) (defn g{i} [x] (+ x {i}))"),
            )
            .unwrap();
        }
        fs::write(
            src_dir.join("main.cljs"),
            "(ns main (:require [m0] [h1] [h2] [h3] [h4])) (def out (+ (m0/f0 1) (h1/g1 1) (h2/g2 1) (h3/g3 1) (h4/g4 1)))",
        )
        .unwrap();
    };

    let cache_opts = CacheOptions {
        dir: cache_dir,
        ..CacheOptions::default()
    };

    // Cold fill.
    write_sources(10);
    {
        let provider = FileSourceProvider::new(vec![src_dir.clone()]);
        let mut host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(2).unwrap(),
        );
        analyze_namespace_entry_recorded_with_provider(
            "main",
            &provider,
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new(),
            EntryRecorders {
                cache: Some(&mut cache),
                slots: Slots::new(2).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        cache.finish();
        assert_eq!(cache.take_counts().published, 6);
    }

    // Mutate `m0` so it misses and boots the `--jobs 2` UnitPool, followed by
    // the cached hit window `h1..h4`. Track Stage A and Stage C threads
    // separately: Stage A for `m0..` runs before `UnitPool` starts (using the
    // pre-pool MapPool, which is shut down before `UnitPool` starts), while
    // Stage C for `h1..h4` runs after `UnitPool` starts and must use only the
    // active `UnitPool` worker plus coordinator (<= 2 slots, no `light-cljs-map-*`).
    write_sources(20);
    type ThreadSet = HashSet<(std::thread::ThreadId, Option<String>)>;
    let stage_c_threads: Arc<Mutex<ThreadSet>> = Arc::new(Mutex::new(HashSet::new()));
    let hook_threads = Arc::clone(&stage_c_threads);
    {
        let provider = FileSourceProvider::new(vec![src_dir.clone()]);
        let mut host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(2).unwrap(),
        );
        cache.set_lookup_work_hook(Some(Arc::new(move |stage| {
            if stage == "stage-c" {
                let cur = std::thread::current();
                hook_threads
                    .lock()
                    .unwrap()
                    .insert((cur.id(), cur.name().map(str::to_owned)));
            }
        })));
        analyze_namespace_entry_recorded_with_provider(
            "main",
            &provider,
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new(),
            EntryRecorders {
                cache: Some(&mut cache),
                slots: Slots::new(2).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        cache.finish();
        let counts = cache.take_counts();
        assert_eq!(counts.hits, 4);
        assert_eq!(counts.misses.len(), 2);
    }

    let threads = stage_c_threads.lock().unwrap();
    assert!(!threads.is_empty(), "Stage C must have executed for h1..h4");
    assert!(
        threads.len() <= 2,
        "at --jobs 2, Stage C after UnitPool startup must run on at most 2 CPU slots, saw {threads:?}"
    );
    for (_, name) in threads.iter() {
        assert!(
            !name
                .as_deref()
                .is_some_and(|n| n.starts_with("light-cljs-map-")),
            "after UnitPool starts, cache lookup must share UnitPool slots rather than MapPool: {threads:?}"
        );
    }
}

#[test]
fn batched_cache_decode_cancellation_stops_the_build() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compile_cache::{CacheOptions, CompileCache};
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded_with_provider};
    use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
    use light_cljs::macro_host::MacroHost;
    use light_cljs::scheduler::Slots;
    use light_cljs::source_provider::FileSourceProvider;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let dir = TempDir::new("batched-stage-c-cancel");
    let src_dir = dir.join("src");
    let cache_dir = dir.join("cache");
    fs::create_dir_all(&src_dir).unwrap();
    for i in 0..5usize {
        let req = if i == 0 {
            String::new()
        } else {
            format!(" (:require [n{}])", i - 1)
        };
        fs::write(
            src_dir.join(format!("n{i}.cljs")),
            format!("(ns n{i}{req}) (def v {i})"),
        )
        .unwrap();
    }
    fs::write(
        src_dir.join("main.cljs"),
        "(ns main (:require [n4])) (def out n4/v)",
    )
    .unwrap();

    let cache_opts = CacheOptions {
        dir: cache_dir,
        ..CacheOptions::default()
    };

    // Cold fill.
    {
        let provider = FileSourceProvider::new(vec![src_dir.clone()]);
        let mut host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(4).unwrap(),
        );
        analyze_namespace_entry_recorded_with_provider(
            "main",
            &provider,
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new(),
            EntryRecorders {
                cache: Some(&mut cache),
                slots: Slots::new(4).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        cache.finish();
        assert_eq!(cache.take_counts().published, 6);
    }

    // Cancel as soon as Stage C starts decoding the batched warm hits.
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let control = evaluator.control();
    let cancel_handle = control.clone();
    let fired = Arc::new(AtomicBool::new(false));
    let fired_hook = Arc::clone(&fired);

    let provider = FileSourceProvider::new(vec![src_dir.clone()]);
    let mut host = MacroHost::new();
    let mut cache = CompileCache::open_with_slots(
        &cache_opts,
        &host,
        AnalysisOptions::default(),
        "main",
        Slots::new(4).unwrap(),
    );
    cache.set_lookup_work_hook(Some(Arc::new(move |stage| {
        if stage == "stage-c" && !fired_hook.swap(true, Ordering::SeqCst) {
            cancel_handle.cancel();
        }
    })));
    let result = analyze_namespace_entry_recorded_with_provider(
        "main",
        &provider,
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            control: Some(&control),
            cache: Some(&mut cache),
            slots: Slots::new(4).unwrap(),
            ..Default::default()
        },
    );
    let err = result.expect_err("Stage C cancellation must stop the build");
    assert!(
        err.to_string().to_lowercase().contains("cancel"),
        "expected cancellation error, got: {err}"
    );
    assert!(fired.load(Ordering::SeqCst), "Stage C hook must have fired");
    let counts = cache.take_counts();
    assert_eq!(counts.published, 0, "cancelled build must publish nothing");
    assert!(
        !counts
            .misses
            .iter()
            .any(|(_, kind, _)| *kind == "undecodable" || *kind == "unreadable"),
        "cancellation must not record a corruption miss: {:?}",
        counts.misses
    );
}

#[test]
fn prefetched_resource_change_after_earlier_miss() {
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compile_cache::{CacheOptions, CompileCache};
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded_with_provider};
    use light_cljs::dependency_manifest::Candidate;
    use light_cljs::emitter::emit_js;
    use light_cljs::macro_host::MacroHost;
    use light_cljs::scheduler::Slots;
    use light_cljs::source_provider::{
        FileSourceProvider, LoadError, LoadRequest, ResourceInput, ResourceProvider, SourceInput,
        SourceProvider,
    };
    use std::rc::Rc;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[derive(Clone)]
    struct FlippingResourceProvider {
        inner: FileSourceProvider,
        data_a: PathBuf,
        data_b: PathBuf,
        flipped_a: Arc<AtomicBool>,
        flipped_b: Arc<AtomicBool>,
    }

    struct FlippingResources {
        inner: Rc<dyn ResourceProvider>,
        data_a: PathBuf,
        data_b: PathBuf,
        flipped_a: Arc<AtomicBool>,
        flipped_b: Arc<AtomicBool>,
    }

    impl ResourceProvider for FlippingResources {
        fn load_resource(
            &self,
            path: &str,
            tried: &mut Vec<Candidate>,
        ) -> Result<Option<ResourceInput>, LoadError> {
            let res = self.inner.load_resource(path, tried)?;
            if path == "data_a.txt" && !self.flipped_a.swap(true, Ordering::SeqCst) {
                fs::write(&self.data_a, "two-a").unwrap();
            } else if path == "data_b.txt" && !self.flipped_b.swap(true, Ordering::SeqCst) {
                fs::write(&self.data_b, "two-b").unwrap();
            }
            Ok(res)
        }

        fn stable(&self) -> bool {
            self.inner.stable()
        }

        fn directory(&self, file: &str) -> Option<String> {
            self.inner.directory(file)
        }
    }

    impl SourceProvider for FlippingResourceProvider {
        fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
            self.inner.load(request)
        }

        fn load_traced(
            &self,
            request: &LoadRequest,
            tried: &mut Vec<Candidate>,
        ) -> Result<Option<SourceInput>, LoadError> {
            self.inner.load_traced(request, tried)
        }

        fn locate_traced(
            &self,
            request: &LoadRequest,
            tried: &mut Vec<Candidate>,
        ) -> Result<Option<String>, LoadError> {
            self.inner.locate_traced(request, tried)
        }

        fn resources(&self) -> Option<Rc<dyn ResourceProvider>> {
            Some(Rc::new(FlippingResources {
                inner: self.inner.resources()?,
                data_a: self.data_a.clone(),
                data_b: self.data_b.clone(),
                flipped_a: Arc::clone(&self.flipped_a),
                flipped_b: Arc::clone(&self.flipped_b),
            }))
        }

        fn concurrent(&self) -> Option<&(dyn SourceProvider + Send + Sync)> {
            Some(self)
        }

        fn owned_concurrent(&self) -> Option<Arc<dyn SourceProvider + Send + Sync>> {
            Some(Arc::new(self.clone()))
        }
    }

    let dir = TempDir::new("prefetched-resource-change");
    let src_dir = dir.join("src");
    let cache_dir = dir.join("cache");
    fs::create_dir_all(&src_dir).unwrap();
    let data_a = src_dir.join("data_a.txt");
    let data_b = src_dir.join("data_b.txt");
    fs::write(&data_a, "one-a").unwrap();
    fs::write(&data_b, "one-b").unwrap();
    fs::write(
        src_dir.join("inputs.cljs"),
        "(ns inputs (:require [light.build :as b])) (defmacro inline [path] (b/resource path))",
    )
    .unwrap();
    fs::write(src_dir.join("early.cljs"), "(ns early) (def v 1)").unwrap();
    // `reader_a` immediately follows `early` (tests Stage B lookahead reuse at
    // offset 0 after `early` misses). `plain` hits, then `reader_b` follows in
    // the same Stage C window (tests Stage D frontier recheck at offset > 0).
    fs::write(
        src_dir.join("reader_a.cljs"),
        "(ns reader-a (:require [inputs :refer [inline]])) (def value (inline \"data_a.txt\"))",
    )
    .unwrap();
    fs::write(src_dir.join("plain.cljs"), "(ns plain) (def x 7)").unwrap();
    fs::write(
        src_dir.join("reader_b.cljs"),
        "(ns reader-b (:require [inputs :refer [inline]])) (def value (inline \"data_b.txt\"))",
    )
    .unwrap();
    fs::write(
        src_dir.join("main.cljs"),
        "(ns main (:require [inputs] [early] [reader-a] [plain] [reader-b])) (def out [early/v reader-a/value plain/x reader-b/value])",
    )
    .unwrap();

    let cache_opts = CacheOptions {
        dir: cache_dir,
        ..CacheOptions::default()
    };

    // Cold fill with data_a = "one-a", data_b = "one-b".
    {
        let provider = FileSourceProvider::new(vec![src_dir.clone()]);
        let mut host = MacroHost::new();
        let mut cache = CompileCache::open_with_slots(
            &cache_opts,
            &host,
            AnalysisOptions::default(),
            "main",
            Slots::new(4).unwrap(),
        );
        let (program, _) = analyze_namespace_entry_recorded_with_provider(
            "main",
            &provider,
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new(),
            EntryRecorders {
                cache: Some(&mut cache),
                slots: Slots::new(4).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        cache.finish();
        assert_eq!(cache.take_counts().published, 7);
        let js = emit_js(&program);
        assert!(js.contains("one-a") && js.contains("one-b"));
    }

    // Mutate `early` so it misses while `reader-a` and `reader-b` are
    // prefetched in Stage A while their resources still hold "one-a"/"one-b",
    // then immediately change to "two-a"/"two-b" before their frontiers.
    fs::write(src_dir.join("early.cljs"), "(ns early) (def v 2)").unwrap();
    let provider = FlippingResourceProvider {
        inner: FileSourceProvider::new(vec![src_dir.clone()]),
        data_a: data_a.clone(),
        data_b: data_b.clone(),
        flipped_a: Arc::new(AtomicBool::new(false)),
        flipped_b: Arc::new(AtomicBool::new(false)),
    };
    let mut host = MacroHost::new();
    let mut cache = CompileCache::open_with_slots(
        &cache_opts,
        &host,
        AnalysisOptions::default(),
        "main",
        Slots::new(4).unwrap(),
    );
    let (program, _) = analyze_namespace_entry_recorded_with_provider(
        "main",
        &provider,
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            cache: Some(&mut cache),
            slots: Slots::new(4).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    cache.finish();
    let counts = cache.take_counts();
    let js = emit_js(&program);

    assert!(provider.flipped_a.load(Ordering::SeqCst));
    assert!(provider.flipped_b.load(Ordering::SeqCst));
    assert!(
        counts
            .misses
            .iter()
            .any(|(ns, kind, _)| ns == "reader-a" && *kind == "inputs"),
        "expected inputs miss for reader-a at frontier, got {:?}",
        counts.misses
    );
    assert!(
        counts
            .misses
            .iter()
            .any(|(ns, kind, _)| ns == "reader-b" && *kind == "inputs"),
        "expected inputs miss for reader-b at frontier, got {:?}",
        counts.misses
    );
    assert!(
        js.contains("two-a") && !js.contains("one-a"),
        "reader-a output must reflect updated resource: {js}"
    );
    assert!(
        js.contains("two-b") && !js.contains("one-b"),
        "reader-b output must reflect updated resource: {js}"
    );
}

#[test]
fn warm_build_trims_dangling_index_outside_entry_graph() {
    use light_cljs::artifact_codec::{DecodeLimits, read_manifest};

    let project = Project::new(&[
        ("plain.cljs", "(ns plain) (def answer 42)"),
        (
            "entry_a.cljs",
            "(ns entry-a (:require [plain])) (js/console.log plain/answer)",
        ),
        ("other.cljs", "(ns other) (def value 99)"),
        (
            "entry_b.cljs",
            "(ns entry-b (:require [other])) (js/console.log other/value)",
        ),
    ]);

    let first_a = project.cached("entry-a", &["--jobs", "4"]);
    assert_eq!(first_a.stat("cache.published"), 2, "{}", first_a.stats);
    let first_b = project.cached("entry-b", &["--jobs", "4"]);
    assert_eq!(first_b.stat("cache.published"), 2, "{}", first_b.stats);

    // Delete `other`'s object file while leaving its index entry intact.
    let other_obj = project
        .objects()
        .into_iter()
        .find(|path| {
            read_manifest(&fs::read(path).unwrap(), &DecodeLimits::default())
                .unwrap()
                .namespace
                == "other"
        })
        .expect("other object exists");
    fs::remove_file(&other_obj).unwrap();

    let store = light_cljs::artifact_store::Store::open(project.cache());
    assert_eq!(store.candidates("other").len(), 1);

    // A zero-publish warm rebuild of disjoint `entry-a` never visits `other`,
    // yet must detect the missing object in the unvisited index and trim it.
    let warm_a = project.cached("entry-a", &["--jobs", "4"]);
    assert_eq!(warm_a.js, first_a.js);
    assert_eq!(warm_a.stat("cache.hits"), 2, "{}", warm_a.stats);
    assert_eq!(warm_a.stat("cache.published"), 0, "{}", warm_a.stats);
    assert!(
        store.candidates("other").is_empty(),
        "warm build must trim dangling index entry for unvisited namespace `other`"
    );
}
