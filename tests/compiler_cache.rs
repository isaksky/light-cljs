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

/// On the Pants build, a program that may write a core function's value
/// installs no artifact, since cached macro code may call that function
/// directly, and each miss says so instead of "no artifact was published".
/// It publishes nothing either: an identical rebuild would not look it up.
#[cfg(all(
    feature = "bbp-macros",
    not(feature = "v8-macros"),
    not(feature = "quickjs")
))]
#[test]
fn core_function_writes_name_their_miss() {
    let project = Project::new(&[
        ("util.cljs", "(ns util) (defn helper [x] (str \"h\" x))"),
        (
            "main.cljs",
            "(ns main (:require [util])) (set! cljs.core/inc (fn [_] 42)) (js/console.log (util/helper (inc 1)))",
        ),
    ]);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "h42\n");
    let populating = project.cached("main", &[]);
    let warm = project.cached("main", &[]);
    for build in [&populating, &warm] {
        assert_eq!(build.stat("cache.published"), 0, "{}", build.stats);
        assert_eq!(build.stat("cache.bytes-written"), 0, "{}", build.stats);
        assert_eq!(build.stat("cache.output.stored"), 0, "{}", build.stats);
        assert_eq!(build.js, oracle.js);
    }
    assert_eq!(warm.stat("cache.hits"), 0, "{}", warm.stats);
    assert_eq!(warm.stat("cache.miss.core-write"), 2, "{}", warm.stats);
    let reason = "miss core-write: the build may write the core function inc, which cached macro code may call directly";
    assert_eq!(
        warm.rows(),
        [format!("util {reason}"), format!("main {reason}")]
    );
    assert_eq!(warm.js, oracle.js);
}

/// A real core write the Pants build's scan must find before any lookup,
/// whatever form hides it: once a consumer adds one, the cached provider
/// whose helper calls the written function is analyzed again rather than
/// installed, and expansion sees the write. Each row is one shape: `set!`,
/// `binding`, `with-redefs` and a `js*` hole inside `#(...)` or a `fn`;
/// metadata on a binding vector or a place; multi-arity, named and `letfn`
/// functions; `#(...)` nested in a `fn`; syntax-quoted places and binding
/// vectors in a macro; reader conditionals around or inside a binding
/// vector; a threaded `set!`; a `..` property place; `aset` and
/// `unchecked-set` on a core function, also as a step of `->` or `doto`; a
/// three-operand `set!` in `->`; a quoted `aset` a macro builds a write
/// with; top-level `refer-clojure` renames; a discarded splice whose tail
/// the reader keeps; and a macro expanding to a core function's name as a
/// later `->` step, a place and a receiver.
#[cfg(all(
    feature = "bbp-macros",
    not(feature = "v8-macros"),
    not(feature = "quickjs")
))]
#[test]
fn anonymous_core_writes_reject_cached_providers() {
    // One edited consumer: its file, the forms before its macro, the
    // macro's body and what the macro returns, when the row pins it.
    struct CoreWriteRow {
        file: &'static str,
        defs: &'static str,
        redefine: &'static str,
        expected: Option<&'static str>,
    }
    const fn row(redefine: &'static str, expected: &'static str) -> CoreWriteRow {
        CoreWriteRow {
            file: "main.cljs",
            defs: "",
            redefine,
            expected: Some(expected),
        }
    }
    for CoreWriteRow {
        file,
        defs,
        redefine,
        expected,
    } in [
        row(
            "(let [before (util/probe)] (#(set! cljs.core/even? (fn [_] false))) [before (util/probe)])",
            "[true false]",
        ),
        row(
            "(#(with-redefs [cljs.core/even? (fn [_] false)] (util/probe)))",
            "false",
        ),
        row(
            "((fn [] (binding [cljs.core/even? (fn [_] false)] (util/probe))))",
            "false",
        ),
        row(
            "(let [before (util/probe)] (#(js* \"(~{} = function (_) { return false; })\" cljs.core/even?)) [before (util/probe)])",
            "[true false]",
        ),
        // Metadata on the binding vector, in `#(...)` and in a list.
        row(
            "(#(with-redefs ^:scan [cljs.core/even? (fn [_] false)] (util/probe)))",
            "false",
        ),
        row(
            "(binding ^{:scan true} [cljs.core/even? (fn [_] false)] (util/probe))",
            "false",
        ),
        // Metadata on the place.
        row(
            "(#(with-redefs [^:scan cljs.core/even? (fn [_] false)] (util/probe)))",
            "false",
        ),
        row(
            "((fn ([] (with-redefs [cljs.core/even? (fn [_] false)] (util/probe))) ([x] x)))",
            "false",
        ),
        row(
            "((fn named [] (binding [cljs.core/even? (fn [_] false)] (util/probe))))",
            "false",
        ),
        row(
            "(letfn [(f [] (with-redefs [cljs.core/even? (fn [_] false)] (util/probe)))] (f))",
            "false",
        ),
        row(
            "((fn [] (#(set! cljs.core/even? %) (fn [_] false)) (util/probe)))",
            "false",
        ),
        // A threaded `set!` whose place is the threaded value.
        row(
            "(let [before (util/probe)] (-> cljs.core/even? (set! (fn [_] false))) [before (util/probe)])",
            "[true false]",
        ),
        // Syntax-quoted binding vector and place in a macro the helper uses.
        CoreWriteRow {
            file: "main.cljs",
            defs: "(defmacro redef [bindings & body] `(with-redefs ~bindings ~@body)) \
                   (defn redef-probe [] (redef [cljs.core/even? (fn [_] false)] (util/probe)))",
            redefine: "(redef-probe)",
            expected: Some("false"),
        },
        CoreWriteRow {
            file: "main.cljs",
            defs: "(defmacro setter [place value] `(set! ~place ~value)) \
                   (defn redef-probe [] (let [before (util/probe)] (setter cljs.core/even? (fn [_] false)) [before (util/probe)]))",
            redefine: "(redef-probe)",
            expected: Some("[true false]"),
        },
        // Reader conditionals around and inside the binding vector. The
        // reader selects the branch before the scan runs; these rows keep
        // that so.
        CoreWriteRow {
            file: "main.cljc",
            defs: "",
            redefine: "(with-redefs #?(:cljs [cljs.core/even? (fn [_] false)] :clj []) (util/probe))",
            expected: Some("false"),
        },
        CoreWriteRow {
            file: "main.cljc",
            defs: "",
            redefine: "(with-redefs [#?@(:cljs [cljs.core/even? (fn [_] false)])] (util/probe))",
            expected: Some("false"),
        },
        // A property place reached through `..`.
        CoreWriteRow {
            file: "main.cljs",
            defs: "",
            redefine: "(let [before (util/sum)] (#(set! (.. cljs.core/+ -call) (fn [_ a b] 42))) [before (util/sum)])",
            expected: None,
        },
        // `aset` and `unchecked-set` on a core function, which the analyzer
        // indexes as resolved writes (light-cljs-586).
        row(
            "(let [before (util/sum)] (aset cljs.core/+ \"call\" (fn [_ a b] 42)) [before (util/sum)])",
            "[3 42]",
        ),
        row(
            "(let [before (util/sum)] (#(unchecked-set cljs.core/+ \"call\" %) (fn [_ a b] 42)) [before (util/sum)])",
            "[3 42]",
        ),
        // A step of `->` or `doto` whose place is the threaded value,
        // with and without its own place operand.
        row(
            "(let [before (util/sum)] (-> cljs.core/+ (aset \"call\" (fn [_ a b] 42))) [before (util/sum)])",
            "[3 42]",
        ),
        row(
            "(let [before (util/sum)] (-> cljs.core/+ (set! -call (fn [_ a b] 42))) [before (util/sum)])",
            "[3 42]",
        ),
        row(
            "(let [before (util/sum)] (doto cljs.core/+ (aset \"call\" (fn [_ a b] 42))) [before (util/sum)])",
            "[3 42]",
        ),
        // A core function renamed by a top-level namespace directive, which
        // namespace preparation replaces with nil before the scan.
        CoreWriteRow {
            file: "main.cljs",
            defs: "(refer-clojure :rename '{even? original-even?})",
            redefine: "(with-redefs [original-even? (fn [_] false)] (util/probe))",
            expected: Some("false"),
        },
        CoreWriteRow {
            file: "main.cljs",
            defs: "(do (refer-clojure :rename '{even? original-even?}))",
            redefine: "(let [before (util/probe)] (set! original-even? (fn [_] false)) [before (util/probe)])",
            expected: Some("[true false]"),
        },
        // A quoted `aset` a macro builds a write with.
        CoreWriteRow {
            file: "main.cljs",
            defs: "(defmacro put [o k v] (list 'aset o k v)) \
                   (defn redef-probe [] (let [before (util/sum)] (put cljs.core/+ \"call\" (fn [_ a b] 42)) [before (util/sum)]))",
            redefine: "(redef-probe)",
            expected: Some("[3 42]"),
        },
        // A discarded splice whose tail the reader keeps.
        CoreWriteRow {
            file: "main.cljc",
            defs: "",
            redefine: "[#_ #?@(:cljs [nil (with-redefs [cljs.core/even? (fn [_] false)] (util/probe))]) 0]",
            expected: Some("[false 0]"),
        },
        // A place or receiver a macro expands to a core function's name:
        // a later step of `->`, a place and a receiver of `set!` and `aset`.
        CoreWriteRow {
            file: "main.cljs",
            defs: "(defmacro core-place [_] 'cljs.core/even?)",
            redefine: "(let [before (util/probe)] (-> nil core-place (set! (fn [_] false))) [before (util/probe)])",
            expected: Some("[true false]"),
        },
        CoreWriteRow {
            file: "main.cljs",
            defs: "(defmacro core-place [_] 'cljs.core/even?)",
            redefine: "(let [before (util/probe)] (set! (core-place nil) (fn [_] false)) [before (util/probe)])",
            expected: Some("[true false]"),
        },
        CoreWriteRow {
            file: "main.cljs",
            defs: "(defmacro core-place [_] 'cljs.core/+)",
            redefine: "(let [before (util/sum)] (set! (.-call (core-place nil)) (fn [_ a b] 42)) [before (util/sum)])",
            expected: Some("[3 42]"),
        },
        CoreWriteRow {
            file: "main.cljs",
            defs: "(defmacro core-place [_] 'cljs.core/+)",
            redefine: "(let [before (util/sum)] (aset (core-place nil) \"call\" (fn [_ a b] 42)) [before (util/sum)])",
            expected: Some("[3 42]"),
        },
    ] {
        let project = Project::new(&[
            (
                "util.cljs",
                "(ns util) (defn probe [] (even? 2)) (defn sum [] (+ 1 2))",
            ),
            (
                "macros.cljs",
                "(ns macros (:require [util])) (defmacro check [] [(util/probe) (util/sum)])",
            ),
            (
                "main.cljs",
                "(ns main (:require [macros :refer [check]])) (js/console.log (pr-str (check)))",
            ),
        ]);
        project.cached("main", &[]);
        let warm = project.cached("main", &[]);
        assert_eq!(warm.stat("cache.hits"), 3, "{}", warm.stats);
        assert_eq!(warm.run(), "[true 3]\n");

        fs::remove_file(project.dir.join("src/main.cljs")).unwrap();
        project.write(
            file,
            &format!(
                "(ns main (:require [util] [macros :refer [check]])) {defs} \
                 (defmacro redefined [] {redefine}) \
                 (js/console.log (pr-str (check)) (pr-str (redefined)))"
            ),
        );
        let oracle = project.uncached("main");
        if let Some(expected) = expected {
            assert_eq!(oracle.run(), format!("[true 3] {expected}\n"), "{redefine}");
        }
        let edited = project.cached("main", &[]);
        assert_eq!(edited.stat("cache.hits"), 0, "{redefine}: {}", edited.stats);
        assert_eq!(
            edited.stat("cache.miss.core-write"),
            3,
            "{redefine}: {}",
            edited.stats
        );
        assert_eq!(
            edited.stat("cache.published"),
            0,
            "{redefine}: {}",
            edited.stats
        );
        assert_eq!(edited.js, oracle.js, "{redefine}");
    }
}

/// Places the Pants build's scan can read and that name no core function
/// mark no core write, so a rebuild with nothing changed still hits every
/// namespace: metadata on a binding vector, multi-arity, named and `letfn`
/// functions, a syntax-quoted binding vector whose places are literal, a
/// property receiver a method call computes, literal `js*` holes, and `aset`
/// and `unchecked-set` on a local, in `doto` (also of `(js-obj)`) and `->`,
/// through `apply` and on a receiver a method call computes.
#[cfg(all(
    feature = "bbp-macros",
    not(feature = "v8-macros"),
    not(feature = "quickjs")
))]
#[test]
fn non_core_places_keep_cached_artifacts() {
    let project = Project::new(&[
        (
            "util.cljs",
            "(ns util) (def ^:dynamic *scale* 1) (defn scaled [x] (* x *scale*)) \
             (defmacro with-scale [n & body] `(binding [util/*scale* ~n] ~@body))",
        ),
        (
            "macros.cljs",
            "(ns macros (:require [util])) \
             (defmacro scaled-all [& xs] (mapv #(with-redefs ^:scan [util/*scale* 10] (util/scaled %)) xs)) \
             (defmacro arities [] ((fn ([] (binding [util/*scale* 2] (util/scaled 1))) ([x] x)))) \
             (defmacro named [] ((fn scale [] (binding [util/*scale* 3] (util/scaled 1))))) \
             (defmacro letfns [] (letfn [(f [] (binding [util/*scale* 4] (util/scaled 1)))] (f)))",
        ),
        (
            "main.cljs",
            "(ns main (:require [macros :refer [scaled-all arities named letfns]] [util :refer [with-scale]])) \
             (def state #js {:n 0}) \
             (set! (.-n (.valueOf state)) (js* \"~{} + ~{}\" 1 #js [2])) \
             (def arr #js [0 0 0 0 0 0]) \
             (aset arr 0 1) (doto arr (aset 1 2)) (-> arr (aset 2 3)) (apply aset arr [3 4]) \
             (aset (.valueOf arr) 4 5) (#(unchecked-set arr 5 %) 6) (def o (doto (js-obj) (aset \"a\" 7))) \
             (js/console.log (pr-str (scaled-all 1 2)) (arities) (named) (letfns) \
               (with-scale 5 (util/scaled 1)) (.-n state) (pr-str (vec arr)) (.-a o))",
        ),
    ]);
    let oracle = project.uncached("main");
    assert_eq!(oracle.run(), "[10 20] 2 3 4 5 12 [1 2 3 4 5 6] 7\n");
    let populated = project.cached("main", &[]);
    assert_eq!(populated.stat("cache.published"), 3, "{}", populated.stats);
    let warm = project.cached("main", &[]);
    assert_eq!(warm.stat("cache.hits"), 3, "{}", warm.stats);
    assert!(warm.rows().is_empty(), "{:?}", warm.rows());
    assert_eq!(warm.js, oracle.js);
}

/// A write through a receiver a method call computes, `(set! (.-call
/// (.valueOf +)) f)` or `(aset (.valueOf +) "call" f)`, which no macro can
/// make, is not a core write on the Pants build: the analyzer's index of
/// resolved writes does not count it either, so
/// with or without the cache the build prepares the same macro code and
/// prints the same output, and the cached providers keep their hits
/// (light-cljs-586).
#[cfg(all(
    feature = "bbp-macros",
    not(feature = "v8-macros"),
    not(feature = "quickjs")
))]
#[test]
fn computed_core_receivers_match_the_uncached_build() {
    let consumer = "(ns main (:require [util] [macros :refer [check]])) {defs} \
                    (js/console.log (pr-str (check)) {calls})";
    let project = Project::new(&[
        ("util.cljs", "(ns util) (defn sum [] (+ 1 2))"),
        (
            "macros.cljs",
            "(ns macros (:require [util])) (defmacro check [] (util/sum))",
        ),
        (
            "main.cljs",
            &consumer.replace("{defs}", "").replace("{calls}", ""),
        ),
    ]);
    project.cached("main", &[]);
    assert_eq!(project.cached("main", &[]).stat("cache.hits"), 3);
    project.write(
        "main.cljs",
        &consumer
            .replace(
                "{defs}",
                "(defmacro by-property [] (let [before (util/sum)] \
                   (set! (.-call (.valueOf cljs.core/+)) (fn [_ a b] 42)) [before (util/sum)])) \
                 (defmacro by-index [] (let [before (util/sum)] \
                   (aset (.valueOf cljs.core/+) \"call\" (fn [_ a b] 43)) [before (util/sum)]))",
            )
            .replace("{calls}", "(pr-str (by-property)) (pr-str (by-index))"),
    );
    let oracle = project.uncached("main");
    let edited = project.cached("main", &[]);
    assert_eq!(edited.stat("cache.hits"), 2, "{}", edited.stats);
    assert_eq!(edited.rows(), ["main miss source: source changed"]);
    assert_eq!(edited.stat("cache.published"), 1, "{}", edited.stats);
    assert_eq!(edited.js, oracle.js);
    assert_eq!(edited.run(), oracle.run());
}

/// Forms the reader never reads mark no core write on the Pants build, so a
/// rebuild with nothing changed still hits every namespace: a `:clj` branch
/// of a reader conditional, whole or spliced, with an unquoted binding
/// vector, a core `set!` or a mutation operator used as a value, and a
/// discarded `#_` form (light-cljs-587).
#[cfg(all(
    feature = "bbp-macros",
    not(feature = "v8-macros"),
    not(feature = "quickjs")
))]
#[test]
fn inactive_cljc_writes_keep_cached_artifacts() {
    for (file, forms) in [
        (
            "main.cljc",
            "#?(:clj (defmacro b [bs & body] `(binding ~bs ~@body))) (js/console.log 1)",
        ),
        (
            "main.cljc",
            "#?(:clj (set! cljs.core/even? (fn [_] false)) :cljs (def x 1)) (js/console.log x)",
        ),
        (
            "main.cljc",
            "(js/console.log #?(:clj set! :default 1) [#?@(:clj [(map binding [])])])",
        ),
        (
            "main.cljs",
            "#_(set! cljs.core/even? (fn [_] false)) (js/console.log 1)",
        ),
    ] {
        let main = format!("(ns main (:require [macros])) {forms} (js/console.log (macros/check))");
        let project = Project::new(&[
            ("util.cljs", "(ns util) (defn probe [] (even? 2))"),
            (
                "macros.cljs",
                "(ns macros (:require [util])) (defmacro check [] (util/probe))",
            ),
            (file, &main),
        ]);
        let oracle = project.uncached("main");
        assert_eq!(oracle.run().lines().last(), Some("true"), "{main}");
        let populated = project.cached("main", &[]);
        assert_eq!(
            populated.stat("cache.published"),
            3,
            "{main}: {}",
            populated.stats
        );
        let warm = project.cached("main", &[]);
        assert_eq!(warm.stat("cache.hits"), 3, "{main}: {}", warm.stats);
        assert!(warm.rows().is_empty(), "{main}: {:?}", warm.rows());
        assert_eq!(warm.js, oracle.js, "{main}");
    }
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
