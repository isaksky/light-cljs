//! The cache contract matrix of docs/compiler-cache.md, "Validation and
//! performance gates", against the real `--compile --cache-dir` path, one
//! process per build. Each test is named after its row: it populates the
//! cache in one process, changes one input, builds again in another and
//! asserts the hits, misses and bypasses `--stats` reports with their
//! reasons, then the output and its execution against a cache-off build of
//! the same inputs. The custom data reader, which the command line cannot
//! register, runs as a child process of this binary ([`child`], driven by
//! `LIGHT_CACHE_MATRIX_CHILD`) through `analyze_namespace_entry_cached`.
//!
//! A namespace that runs a source macro depends on every earlier namespace
//! (the coarse fallback, "Conservative keys and invalidation"), so the
//! fixtures put the namespaces an input change must not reach before the
//! ones it reaches, or keep them free of source macros, and a test that
//! shows the fallback says so.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

#[path = "support/node.rs"]
#[allow(dead_code)]
mod node;
#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;
use temp_dir::TempDir;

const CHILD: &str = "LIGHT_CACHE_MATRIX_CHILD";

/// A project of source roots `src` and `src2`, in that order, and a cache.
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

    /// The `[cache-misses-and-bypasses]` rows: misses, then bypasses, each
    /// in source order.
    fn rows(&self) -> Vec<&str> {
        self.stats
            .split_once("[cache-misses-and-bypasses]\n")
            .map_or(Vec::new(), |(_, rows)| rows.lines().collect())
    }

    fn run(&self) -> String {
        execute(&self.js)
    }

    /// Every namespace hit: no miss, no bypass, no macro executed.
    #[track_caller]
    fn assert_all_hits(&self, namespaces: u64) {
        assert_eq!(self.stat("cache.hits"), namespaces, "{}", self.stats);
        assert!(self.rows().is_empty(), "{:?}", self.rows());
        assert_eq!(self.stat("macro-calls"), 0, "{}", self.stats);
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
            dir: TempDir::new("cljs-cache-matrix"),
        };
        fs::create_dir_all(project.dir.join("src2")).unwrap();
        for (path, text) in files {
            project.write(path, text);
        }
        project
    }

    /// Write `path`, under `src` unless it starts with `src2/`.
    fn write(&self, path: &str, text: &str) {
        let path = self.path(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn remove(&self, path: &str) {
        fs::remove_file(self.path(path)).unwrap();
    }

    fn path(&self, path: &str) -> PathBuf {
        if path.starts_with("src2/") {
            self.dir.join(path)
        } else {
            self.dir.join("src").join(path)
        }
    }

    fn roots(&self) -> [PathBuf; 2] {
        [self.dir.join("src"), self.dir.join("src2")]
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("cache")
    }

    /// The cache's published artifacts, by name, with their bytes.
    fn objects(&self) -> Vec<(String, Vec<u8>)> {
        let mut objects: Vec<_> = fs::read_dir(self.cache().join("v1/objects"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .map(|path| {
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    path,
                )
            })
            .filter(|(name, _)| !name.starts_with(".tmp"))
            .map(|(name, path)| (name, fs::read(path).unwrap()))
            .collect();
        objects.sort();
        objects
    }

    fn command(&self, entry: &str, extra: &[&str], cached: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
        command.args(["--compile", "--entry", entry]);
        for root in self.roots() {
            command.arg("--source-root").arg(root);
        }
        command.arg("--stats");
        if cached {
            command.arg("--cache-dir").arg(self.cache());
        } else {
            command.arg("--no-cache");
        }
        command.args(extra);
        command
    }

    fn output(&self, entry: &str, extra: &[&str], cached: bool) -> Result<Build, String> {
        let output = self.command(entry, extra, cached).output().unwrap();
        finish(output)
    }

    /// A build with the cache: the candidate.
    #[track_caller]
    fn cached(&self, entry: &str, extra: &[&str]) -> Build {
        self.output(entry, extra, true)
            .unwrap_or_else(|error| panic!("the cached build fails: {error}"))
    }

    /// A build without the cache: the oracle.
    #[track_caller]
    fn uncached(&self, entry: &str, extra: &[&str]) -> Build {
        self.output(entry, extra, false)
            .unwrap_or_else(|error| panic!("the uncached build fails: {error}"))
    }

    /// A cached build, checked against the cache-off build of the same
    /// inputs: the same bytes and the same execution, which it returns.
    #[track_caller]
    fn checked(&self, entry: &str, extra: &[&str]) -> (Build, String) {
        let cached = self.cached(entry, extra);
        let oracle = self.uncached(entry, extra);
        assert_eq!(
            cached.js, oracle.js,
            "the cached build differs from the uncached one\n{}",
            cached.stats
        );
        let printed = cached.run();
        assert_eq!(printed, oracle.run());
        (cached, printed)
    }
}

fn finish(output: Output) -> Result<Build, String> {
    let stats = String::from_utf8(output.stderr).unwrap();
    if !output.status.success() {
        return Err(stats);
    }
    Ok(Build {
        js: String::from_utf8(output.stdout).unwrap(),
        stats,
    })
}

// -- Timestamp macro ----------------------------------------------------------

const TIMESTAMP: &[(&str, &str)] = &[
    (
        "stamp.cljs",
        "(ns stamp (:require [light.build :as b])) (defmacro now [] (str (.now js/Date) \"-\" (.random js/Math))) (defmacro built [] (str (b/config :build/time)))",
    ),
    (
        "clock.cljs",
        "(ns clock (:require [stamp :refer [now built]])) (def at (now)) (def keyed (built))",
    ),
    (
        "fresh.cljs",
        "(ns ^:light-cljs/no-cache fresh (:require [stamp :refer [now]])) (def at (now))",
    ),
    ("plain.cljs", "(ns plain) (def value 1)"),
    (
        "main.cljs",
        "(ns main (:require [clock] [fresh] [plain])) (js/console.log clock/at clock/keyed fresh/at plain/value)",
    ),
];

/// `main`'s printed `clock/at`, `clock/keyed` and `fresh/at`.
fn stamps(build: &Build) -> (String, String, String) {
    let printed = build.run();
    let fields: Vec<&str> = printed.split_whitespace().collect();
    assert_eq!(fields.len(), 4, "{printed}");
    (fields[0].into(), fields[1].into(), fields[2].into())
}

/// An undeclared `js/Date` read keeps its cached expansion, the documented
/// cost of the contract; the build time passed as `--config` refreshes it
/// when the value changes; `^:light-cljs/no-cache` refreshes it on every
/// build.
#[test]
fn timestamp_macro() {
    let project = Project::new(TIMESTAMP);
    let time = |value: &str| ["--config".to_owned(), format!("build/time={value}")];
    let one = time("1");
    let one = [one[0].as_str(), one[1].as_str()];
    let populating = project.cached("main", &one);
    let (at, keyed, fresh) = stamps(&populating);
    assert_eq!(keyed, "1");
    let warm = project.cached("main", &one);
    assert_eq!(warm.rows(), ["fresh bypass no-cache"]);
    // `light.build` is a namespace of the build too.
    assert_eq!(warm.stat("cache.hits"), 5, "{}", warm.stats);
    let (warm_at, warm_keyed, warm_fresh) = stamps(&warm);
    assert_eq!((&warm_at, &warm_keyed), (&at, &keyed), "the hit is kept");
    assert_ne!(warm_fresh, fresh, "no-cache refreshes");
    // An unrelated edit compiles `main` again; `clock` still hits.
    project.write(
        "main.cljs",
        "(ns main (:require [clock] [fresh] [plain])) (js/console.log clock/at clock/keyed fresh/at plain/value) ; edited",
    );
    let edited = project.cached("main", &one);
    assert_eq!(
        edited.rows(),
        ["main miss source: source changed", "fresh bypass no-cache"]
    );
    let (edited_at, _, edited_fresh) = stamps(&edited);
    assert_eq!(edited_at, at);
    assert_ne!(edited_fresh, warm_fresh);
    // A new build time is a new configuration: every namespace misses, and
    // the undeclared read refreshes with the declared one.
    let two = time("2");
    let two = [two[0].as_str(), two[1].as_str()];
    let rebuilt = project.cached("main", &two);
    assert_eq!(rebuilt.stat("cache.hits"), 0, "{}", rebuilt.stats);
    assert_eq!(rebuilt.stat("cache.miss.config"), 5, "{}", rebuilt.stats);
    let (rebuilt_at, rebuilt_keyed, _) = stamps(&rebuilt);
    assert_eq!(rebuilt_keyed, "2");
    assert_ne!(rebuilt_at, at);
    // The same value again is the same key: the hit returns.
    let again = project.cached("main", &two);
    assert_eq!(again.stat("cache.hits"), 5, "{}", again.stats);
    assert_eq!(stamps(&again).0, rebuilt_at);
}

// -- Resource change ----------------------------------------------------------

/// `inputs` reads resources for its macros; `before` runs one of them
/// first and must keep hitting; `plain` runs none.
fn resource_project(consumer: &str) -> Project {
    Project::new(&[
        (
            "inputs.cljs",
            "(ns inputs (:require [light.build :as b])) (defmacro inline [path] (b/resource path)) (defmacro optional [path] (or (b/resource? path) \"none\"))",
        ),
        (
            "before.cljs",
            "(ns before (:require [inputs :refer [inline]])) (def value (inline \"data/before.txt\"))",
        ),
        ("data/before.txt", "before"),
        ("consumer.cljs", consumer),
        (
            "after.cljs",
            "(ns after (:require [inputs :refer [inline]])) (def value (inline \"data/before.txt\"))",
        ),
        ("plain.cljs", "(ns plain) (def value 42)"),
        (
            "main.cljs",
            "(ns main (:require [before] [consumer] [after] [plain])) (js/console.log before/value consumer/value plain/value)",
        ),
    ])
}

/// The rows of a change that reaches `consumer`'s own input: it and its
/// runtime consumer `main` miss. `after` runs a source macro later, but
/// `consumer` declares nothing a source macro could see, so it hits: not a
/// reader of `consumer`'s input, its manifest does not list it
/// (light-cljs-347).
const CONSUMER_MISSES: [&str; 2] = [
    "consumer miss inputs: external input changed",
    "main miss dependencies: dependency consumer changed",
];

/// For `b/resource`, for `b/resource?` going from absent to present, and
/// for a new file shadowing a resource in an earlier root, exactly the
/// consumers miss.
#[test]
fn resource_change() {
    // `b/resource`.
    let project = resource_project(
        "(ns consumer (:require [inputs :refer [inline]])) (def value (inline \"data/text.txt\"))",
    );
    project.write("data/text.txt", "one");
    let (_, printed) = project.checked("main", &[]);
    assert_eq!(printed, "before one 42\n");
    // `light.build`, `inputs`, `before`, `consumer`, `after`, `plain`,
    // `main`.
    project.cached("main", &[]).assert_all_hits(7);
    project.write("data/text.txt", "two");
    let (changed, printed) = project.checked("main", &[]);
    assert_eq!(printed, "before two 42\n");
    assert_eq!(changed.rows(), CONSUMER_MISSES);
    assert_eq!(changed.stat("cache.hits"), 5);

    // `b/resource?`, absent and then present.
    let project = resource_project(
        "(ns consumer (:require [inputs :refer [optional]])) (def value (optional \"data/extra.txt\"))",
    );
    let (_, printed) = project.checked("main", &[]);
    assert_eq!(printed, "before none 42\n");
    project.cached("main", &[]).assert_all_hits(7);
    project.write("src2/data/extra.txt", "here");
    let (appeared, printed) = project.checked("main", &[]);
    assert_eq!(printed, "before here 42\n");
    assert_eq!(appeared.rows(), CONSUMER_MISSES);
    assert_eq!(appeared.stat("cache.hits"), 5);

    // A resource in the later root, then a file of that path in the earlier
    // root: a path in two roots is an error, which the cached build reports
    // as the uncached one does rather than hitting; once the later copy
    // goes, the earlier root's file is the one read.
    let project = resource_project(
        "(ns consumer (:require [inputs :refer [inline]])) (def value (inline \"data/shared.txt\"))",
    );
    project.write("src2/data/shared.txt", "later");
    let (_, printed) = project.checked("main", &[]);
    assert_eq!(printed, "before later 42\n");
    project.cached("main", &[]).assert_all_hits(7);
    project.write("data/shared.txt", "earlier");
    let error = project.output("main", &[], true).err().expect("ambiguous");
    let oracle = project.output("main", &[], false).err().expect("ambiguous");
    assert!(error.contains("data/shared.txt"), "{error}");
    assert!(error.contains("ambiguous"), "{error}");
    assert_eq!(error.lines().next(), oracle.lines().next());
    project.remove("src2/data/shared.txt");
    let (shadowed, printed) = project.checked("main", &[]);
    assert_eq!(printed, "before earlier 42\n");
    assert_eq!(shadowed.rows(), CONSUMER_MISSES);
    assert_eq!(shadowed.stat("cache.hits"), 5);
}

// -- Config change ------------------------------------------------------------

const PLAIN_FIXTURE: &[(&str, &str)] = &[
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

/// Every namespace misses, as the first implementation's whole-map
/// fingerprint dictates, even though none reads a key. A later precise
/// version changes these counts on purpose.
#[test]
fn config_change() {
    let project = Project::new(PLAIN_FIXTURE);
    let (_, printed) = project.checked("main", &["--config", "app/n=1"]);
    assert_eq!(printed, "h1 9 84 h2\n");
    project
        .cached("main", &["--config", "app/n=1"])
        .assert_all_hits(5);
    for config in [
        &["--config", "app/n=2"][..],
        &["--config", "app/n=2", "--config", "app/other=:x"],
    ] {
        let (changed, printed) = project.checked("main", config);
        assert_eq!(printed, "h1 9 84 h2\n");
        assert_eq!(
            changed.stat("cache.hits"),
            0,
            "{config:?}: {}",
            changed.stats
        );
        assert_eq!(changed.stat("cache.miss.config"), 5, "{config:?}");
        assert!(
            changed
                .rows()
                .iter()
                .all(|row| row.ends_with("miss config: config changed")),
            "{:?}",
            changed.rows()
        );
    }
    // The first configuration's artifacts are still there: retention keeps
    // three per namespace.
    project
        .cached("main", &["--config", "app/n=1"])
        .assert_all_hits(5);
    // No configuration is a configuration too.
    let (unset, _) = project.checked("main", &[]);
    assert_eq!(unset.stat("cache.miss.config"), 5, "{}", unset.stats);
}

// -- Input read through a helper or a custom reader ---------------------------

const HELPER_INPUT: &[(&str, &str)] = &[
    (
        "lib/helpers.cljs",
        "(ns lib.helpers (:require [light.build :as b])) (def reads (atom 0)) (def table (when (exists? js/lightCljsBuild) (swap! reads inc) (b/resource \"lib/table.txt\")))",
    ),
    (
        "lib/macros.cljs",
        "(ns lib.macros (:require [lib.helpers :as h])) (defmacro table [] (str h/table \" #\" @h/reads))",
    ),
    ("app/other.cljs", "(ns app.other) (def value 1)"),
    (
        "app/a.cljs",
        "(ns app.a (:require [lib.macros :refer [table]])) (def value (table))",
    ),
    (
        "app/b.cljs",
        "(ns app.b (:require [lib.macros :refer [table]])) (def value (table))",
    ),
    (
        "app/main.cljs",
        "(ns app.main (:require [app.other] [app.a :as a] [app.b :as b])) (js/console.log (str a/value \",\" b/value))",
    ),
    ("lib/table.txt", "rows"),
];

/// A custom reader that inlines a resource.
const READER: &str = "(fn [path] (.resource js/lightCljsBuild path))";

const READER_INPUT: &[(&str, &str)] = &[
    ("app/plain.cljs", "(ns app.plain) (def value 1)"),
    (
        "app/core.cljs",
        "(ns app.core (:require [app.plain])) (def value (str #app/inline \"./data.txt\" \",\" app.plain/value))",
    ),
    (
        "app/main.cljs",
        "(ns app.main (:require [app.plain] [app.core])) (js/console.log app.core/value)",
    ),
    ("app/data.txt", "local"),
];

/// Every artifact that incorporated a helper's or a custom reader's read
/// misses, including `app.b`, whose macro reused the helper's value in
/// memory during population without reading it again.
#[test]
fn input_read_through_a_helper_or_a_custom_reader() {
    let project = Project::new(HELPER_INPUT);
    let (_, printed) = project.checked("app.main", &[]);
    // One initialization, during `app.a`'s compilation, reused by `app.b`.
    assert_eq!(printed, "rows #1,rows #1\n");
    project.cached("app.main", &[]).assert_all_hits(7);
    project.write("lib/table.txt", "cells");
    let (changed, printed) = project.checked("app.main", &[]);
    assert_eq!(printed, "cells #1,cells #1\n");
    assert_eq!(
        changed.rows(),
        [
            "app.a miss inputs: external input changed",
            "app.b miss inputs: external input changed",
            "app.main miss dependencies: dependency app.a changed",
        ]
    );
    assert_eq!(changed.stat("cache.hits"), 4);
    // With `app.a` hitting, `app.b`'s miss initializes the helper itself.
    project.write(
        "app/b.cljs",
        "(ns app.b (:require [lib.macros :refer [table]])) (def value (table)) ; edited",
    );
    let (edited, printed) = project.checked("app.main", &[]);
    assert_eq!(printed, "cells #1,cells #1\n");
    assert_eq!(
        edited.rows(),
        [
            "app.b miss source: source changed",
            "app.main miss dependencies: dependency app.b changed",
        ]
    );

    // A custom reader, in child processes of this binary.
    let project = Project::new(READER_INPUT);
    let oracle = project.child("app.main", false);
    assert_eq!(oracle.run(), "local,1\n");
    let populating = project.child("app.main", true);
    assert_eq!(populating.js, oracle.js);
    project.child("app.main", true).assert_all_hits(3);
    project.write("app/data.txt", "edited");
    let changed = project.child("app.main", true);
    assert_eq!(changed.js, project.child("app.main", false).js);
    assert_eq!(changed.run(), "edited,1\n");
    assert_eq!(
        changed.rows(),
        [
            "app.core miss inputs: external input changed",
            "app.main miss dependencies: dependency app.core changed",
        ]
    );
    assert_eq!(changed.stat("cache.hits"), 1);
}

impl Project {
    /// A build of `entry` by a child process of this binary with the custom
    /// readers `#app/inline`, `#app/uniq` and `#app/core-target` registered,
    /// with the cache when `cached`.
    fn child(&self, entry: &str, cached: bool) -> Build {
        self.child_with(entry, cached, &[])
    }

    /// [`Self::child`] with the extra environment `env`.
    fn child_with(&self, entry: &str, cached: bool, env: &[(&str, &str)]) -> Build {
        let out = self.dir.join("child.js");
        let report = self.dir.join("child.stats");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["child", "--exact", "--ignored", "--nocapture", "--quiet"])
            .env(CHILD, entry)
            .env("ROOTS", self.dir.join("src"))
            .env("OUT", &out)
            .env("REPORT", &report)
            .envs(env.iter().copied());
        if cached {
            command.env("CACHE", self.cache());
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "the child fails: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Build {
            js: fs::read_to_string(out).unwrap(),
            stats: fs::read_to_string(report).unwrap(),
        }
    }
}

/// What a child process does when `LIGHT_CACHE_MATRIX_CHILD` names an
/// entry: compile it from `ROOTS` with the readers `#app/inline`,
/// `#app/uniq` and `#app/core-target`, through the cache in `CACHE` when
/// set, on the Pants build's macro backend when `NATIVE` is set, writing
/// the script to `OUT` and the `--stats` report to `REPORT`.
#[test]
#[ignore = "a child process of the other tests"]
fn child() {
    use light_cljs::MacroHost;
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compile_stats::CompileStats;
    use light_cljs::compiler_data::Name;
    let Ok(entry) = std::env::var(CHILD) else {
        return;
    };
    let path = |name: &str| PathBuf::from(std::env::var_os(name).expect(name));
    let roots = [path("ROOTS")];
    let mut host = MacroHost::new();
    host.register_compiler_data_reader(Name::new(Some("app"), "inline"), READER)
        .unwrap();
    host.register_compiler_data_reader(Name::new(Some("app"), "uniq"), UNIQ_READER)
        .unwrap();
    host.register_compiler_data_reader(Name::new(Some("app"), "core-target"), CORE_TARGET_READER)
        .unwrap();
    #[cfg(feature = "bbp-macros")]
    if std::env::var_os("NATIVE").is_some() {
        host.use_native_macros();
    }
    let mut stats = CompileStats::default();
    let program = match std::env::var_os("CACHE") {
        Some(dir) => {
            light_cljs::compiler::analyze_namespace_entry_cached(
                &entry,
                &roots,
                &mut host,
                AnalysisOptions::default(),
                &mut Vec::new(),
                &light_cljs::compile_cache::CacheOptions {
                    dir: dir.into(),
                    blockers: Vec::new(),
                    retention: Default::default(),
                },
                Some(&mut stats),
                light_cljs::scheduler::Slots::ONE,
            )
            .unwrap()
            .0
        }
        None => light_cljs::compiler::analyze_namespace_entry(
            &entry,
            &roots,
            &mut host,
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .unwrap(),
    };
    fs::write(path("OUT"), light_cljs::emit_js(&program)).unwrap();
    fs::write(path("REPORT"), stats.report()).unwrap();
}

// -- Macro or helper implementation edit --------------------------------------

/// An edit to a helper's or a macro's implementation that keeps its
/// signature reaches every transitive compilation consumer; unrelated
/// namespaces hit.
#[test]
fn macro_or_helper_implementation_edit_with_an_unchanged_signature() {
    let project = Project::new(PLAIN_FIXTURE);
    let (_, printed) = project.checked("main", &[]);
    assert_eq!(printed, "h1 9 84 h2\n");
    project.cached("main", &[]).assert_all_hits(5);
    // The helper a macro runs.
    project.write("util.cljs", "(ns util) (defn helper [x] (str \"H\" x))");
    let (helper, printed) = project.checked("main", &[]);
    assert_eq!(printed, "H1 9 84 H2\n");
    assert_eq!(
        helper.rows(),
        [
            "util miss source: source changed",
            "macros miss dependencies: dependency util changed",
            // The first dependency that changed, in source order.
            "main miss dependencies: dependency util changed",
        ]
    );
    assert_eq!(helper.stat("cache.hits"), 2);
    // The macro itself.
    project.write(
        "macros.cljs",
        "(ns macros (:require [util])) (defmacro shout [x] (util/helper (* 10 x))) (defmacro twice [form] `(let [v# ~form] (+ v# v#)))",
    );
    let (macro_edit, printed) = project.checked("main", &[]);
    assert_eq!(printed, "H10 9 84 H2\n");
    assert_eq!(
        macro_edit.rows(),
        [
            "macros miss source: source changed",
            "main miss dependencies: dependency macros changed",
        ]
    );
    assert_eq!(macro_edit.stat("cache.hits"), 3);
}

// -- Resolution and views -----------------------------------------------------

/// A new source file that shadows a namespace, a changed alias, changed
/// reader metadata, and a `b/depend-on! :namespaces` consumer when a
/// namespace is added: the dependent artifacts miss.
#[test]
fn resolution_and_views() {
    // `lib` is `src/lib.cljc` until `src2/lib.cljs` shadows it, since a
    // `.cljs` file in any root comes before a `.cljc` one.
    let project = Project::new(&[
        ("lib.cljc", "(ns lib) (def value :cljc)"),
        ("plain.cljs", "(ns plain) (def value 1)"),
        (
            "user.cljs",
            "(ns user (:require [lib])) (def value lib/value)",
        ),
        (
            "main.cljs",
            "(ns main (:require [plain] [user])) (js/console.log plain/value (str user/value))",
        ),
    ]);
    let (_, printed) = project.checked("main", &[]);
    assert_eq!(printed, "1 :cljc\n");
    project.cached("main", &[]).assert_all_hits(4);
    project.write("src2/lib.cljs", "(ns lib) (def value :cljs)");
    let (shadowed, printed) = project.checked("main", &[]);
    assert_eq!(printed, "1 :cljs\n");
    assert_eq!(
        shadowed.rows(),
        [
            "lib miss source: source changed",
            "user miss dependencies: dependency lib changed",
            "main miss dependencies: dependency user changed",
        ]
    );
    assert_eq!(shadowed.stat("cache.hits"), 1);

    // A macro provider's alias names what its expansions call.
    let project = Project::new(&[
        ("impl/one.cljs", "(ns impl.one) (defn f [] :one)"),
        ("impl/two.cljs", "(ns impl.two) (defn f [] :two)"),
        (
            "calls.cljs",
            "(ns calls (:require [impl.one :as impl] [impl.two])) (defmacro call [] `(impl/f))",
        ),
        ("plain.cljs", "(ns plain) (def value 1)"),
        (
            "user.cljs",
            "(ns user (:require [calls :refer [call]])) (def value (call))",
        ),
        (
            "main.cljs",
            "(ns main (:require [plain] [user])) (js/console.log plain/value (str user/value))",
        ),
    ]);
    let (_, printed) = project.checked("main", &[]);
    assert_eq!(printed, "1 :one\n");
    project.cached("main", &[]).assert_all_hits(6);
    project.write(
        "calls.cljs",
        "(ns calls (:require [impl.one] [impl.two :as impl])) (defmacro call [] `(impl/f))",
    );
    let (realiased, printed) = project.checked("main", &[]);
    assert_eq!(printed, "1 :two\n");
    assert_eq!(
        realiased.rows(),
        [
            "calls miss source: source changed",
            "user miss dependencies: dependency calls changed",
            "main miss dependencies: dependency user changed",
        ]
    );
    assert_eq!(realiased.stat("cache.hits"), 3);

    // A macro reads the reader metadata of a Var its namespace does not
    // require; `doc` loads earlier.
    let project = Project::new(&[
        ("doc.cljs", "(ns doc) (def ^{:doc \"one\"} value 1)"),
        (
            "reflect.cljs",
            "(ns reflect) (defmacro doc-of [] (str (:doc (meta (resolve 'doc/value)))))",
        ),
        ("plain.cljs", "(ns plain) (def value 1)"),
        (
            "user.cljs",
            "(ns user (:require [reflect :refer [doc-of]])) (def value (doc-of))",
        ),
        (
            "main.cljs",
            "(ns main (:require [doc] [plain] [user])) (js/console.log plain/value user/value)",
        ),
    ]);
    let (_, printed) = project.checked("main", &[]);
    let pants = cfg!(all(
        feature = "bbp-macros",
        not(any(feature = "quickjs", feature = "v8-macros"))
    ));
    // The caller does not require doc: Pants resolve sees an unloaded Var.
    assert_eq!(printed, if pants { "1 \n" } else { "1 one\n" });
    project.cached("main", &[]).assert_all_hits(5);
    project.write("doc.cljs", "(ns doc) (def ^{:doc \"two\"} value 1)");
    let (metadata, printed) = project.checked("main", &[]);
    assert_eq!(printed, if pants { "1 \n" } else { "1 two\n" });
    assert_eq!(
        metadata.rows(),
        [
            "doc miss source: source changed",
            // `(resolve 'doc/value)` resolves as `reflect` is analyzed: an
            // observed edge.
            "reflect miss dependencies: dependency doc changed",
            "user miss dependencies: dependency doc changed",
            "main miss dependencies: dependency doc changed",
        ]
    );
    assert_eq!(metadata.stat("cache.hits"), 1);

    // The `:namespaces` view: the build's inventory is not known until it
    // ends, so the persistent cache bypasses its consumers
    // (light-cljs-322); they compile on every build and see an added
    // namespace, while the namespaces before them hit.
    let project = Project::new(&[
        (
            "inventory.cljs",
            "(ns inventory (:require [light.build :as b])) (defmacro names [] (b/depend-on! :namespaces) (count (all-ns)))",
        ),
        ("plain.cljs", "(ns plain) (def value 1)"),
        (
            "counted.cljs",
            "(ns counted (:require [inventory :refer [names]])) (def value (names))",
        ),
        (
            "main.cljs",
            "(ns main (:require [plain] [counted])) (js/console.log plain/value counted/value)",
        ),
    ]);
    let (_, before) = project.checked("main", &[]);
    let rows = [
        "counted bypass non-cacheable-input counted",
        "main bypass non-cacheable-input counted > main",
    ];
    assert_eq!(project.cached("main", &[]).rows(), rows);
    project.write("added.cljs", "(ns added) (def value 2)");
    project.write(
        "main.cljs",
        "(ns main (:require [plain] [added] [counted])) (js/console.log plain/value counted/value)",
    );
    let (added, after) = project.checked("main", &[]);
    if pants {
        // cljs.user, cljs.core, light.build, inventory and counted: the
        // unrelated added namespace stays outside counted's closure.
        assert_eq!(before, "1 5\n");
        assert_eq!(after, before);
    } else {
        assert_ne!(before, after, "the inventory grew");
    }
    assert_eq!(
        added.rows(),
        [
            "added miss no-artifact: no artifact was published",
            "counted bypass non-cacheable-input counted",
            "main bypass non-cacheable-input counted > main",
        ]
    );
    // `light.build`, `inventory` and `plain`.
    assert_eq!(added.stat("cache.hits"), 3);
}

// -- Mixed generated names and source-order generations -----------------------

/// `gen` redefines `inner` between two uses and has gensym-generating
/// macros; `a` and `b` expand them, on both sides of the redefinition, and
/// read their reader context's visible declarations; `c` runs no source
/// macro but generates names with destructuring and `#()`, so it can hit
/// after a miss.
const GENERATIONS: &[(&str, &str)] = &[
    (
        "gen.cljs",
        "(ns gen) (defmacro inner [] 1) (defmacro outer [] (inner)) (defn before [] (inner)) (defmacro inner [] 2) (defmacro nested [] '(inner)) (defmacro wrap [v] `(let [x# ~v] (fn [] [x# (inner)]))) (defmacro pair [a b] `(let [x# ~a y# ~b] (- x# y#))) (defmacro tag [] (list 'quote (gensym \"t\"))) (defmacro visible [] (let [c (:light-cljs/reader-context &env)] (list 'quote [(:visible c) (:names (:declared c))])))",
    ),
    (
        "a.cljs",
        "(ns a (:require [gen :refer [inner outer nested wrap pair tag visible]])) (def early (visible)) (def v [(gen/before) (outer) (inner) (nested) ((wrap 3)) (pair 10 (pair 3 1))]) (def t (tag)) (def late (visible))",
    ),
    (
        "b.cljs",
        "(ns b (:require [gen :refer [inner outer nested wrap pair tag visible]])) (def early (visible)) (def v [(gen/before) (outer) (inner) (nested) ((wrap 4)) (pair 20 (pair 5 1))]) (def t (tag)) (def late (visible))",
    ),
    (
        "c.cljs",
        "(ns c) (defn d [{:keys [x]} [y & more]] (#(+ %1 %2 x y (count more)) 1 2)) (def v (d {:x 1} [2 3 4])) (def t `local#)",
    ),
    (
        "main.cljs",
        "(ns main (:require [a] [b] [c])) (js/console.log (pr-str [a/early a/v a/late b/early b/v b/late c/v]) (count (set [a/t b/t c/t])))",
    ),
];

/// A script with each gensym spelling of `gen/tag` (a string such as `"t12"`,
/// or `"tD_K_1"` in a namespace's scope since light-cljs-332, the
/// only generated name that escapes into the fixture's output) renamed by
/// first occurrence, distinct spellings to distinct names: a hit keeps the
/// spelling it was captured with, so a mixed build may number them
/// differently from an uncached one, and must not otherwise differ.
/// Everything else, other generated names included, compares byte for byte.
fn normalized(js: &str) -> String {
    // The runtime key follows the text, gensyms included.
    let js = &light_cljs::runtime_key::unkeyed(js);
    let mut out = String::with_capacity(js.len());
    let mut seen: Vec<&str> = Vec::new();
    let mut rest = js.as_str();
    while let Some(at) = rest.find("\"t") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 2..]
            .find(|c: char| !c.is_ascii_digit() && c != '_')
            .map_or(rest.len(), |end| at + 2 + end);
        if digits > at + 2 && rest[digits..].starts_with('"') {
            let spelling = &rest[at + 1..digits];
            let index = match seen.iter().position(|seen| *seen == spelling) {
                Some(index) => index,
                None => {
                    seen.push(spelling);
                    seen.len() - 1
                }
            };
            out.push_str(&format!("\"gensym-{index}"));
            rest = &rest[digits..];
        } else {
            out.push_str("\"t");
            rest = &rest[at + 2..];
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn normalized_renames_only_tag_spellings_and_keeps_their_identity() {
    let script = |a: &str, b: &str| format!("f(\"{a}\", \"{b}\", \"t\", \"tx1\", l1, p1__2)");
    assert_eq!(
        normalized(&script("t3", "t4")),
        normalized(&script("t8", "t5"))
    );
    assert_ne!(
        normalized(&script("t3", "t4")),
        normalized(&script("t3", "t3"))
    );
    assert_eq!(
        normalized(&script("t17_2_1", "t17_3_1")),
        normalized(&script("t90_2_1", "t5_3_1"))
    );
    assert_ne!(
        normalized(&script("t17_2_1", "t17_3_1")),
        normalized(&script("t17_2_1", "t17_2_1"))
    );
    assert_ne!(normalized("[l1, l2]"), normalized("[l1, l1]"));
    assert_ne!(normalized("\"value1\""), normalized("\"value2\""));
    assert_ne!(normalized("p1__2"), normalized("p1__3"));
}

/// A build with hits and misses on both sides of a macro redefinition stays
/// hygienic and preserves declaration visibility.
#[test]
fn mixed_generated_names_and_source_order_generations() {
    let project = Project::new(GENERATIONS);
    let oracle = project.uncached("main", &[]);
    let printed = oracle.run();
    assert_eq!(
        printed,
        "[[1 [\"early\"]] [1 1 2 2 [3 2] 8] [4 [\"late\"]] [1 [\"early\"]] [1 1 2 2 [4 2] 16] [4 [\"late\"]] 8] 3\n"
    );
    assert!(
        printed.ends_with(" 3\n"),
        "three distinct gensyms: {printed}"
    );
    let populating = project.cached("main", &[]);
    assert_eq!(populating.js, oracle.js);
    project.cached("main", &[]).assert_all_hits(5);
    // `b` misses between the hits `a` and `c`; then `a` misses before the
    // hits `b` and `c`: `b` runs source macros after it, but `a` declares
    // nothing a source macro could see (light-cljs-347).
    let edits: [(&str, &str, &[&str]); 2] = [
        (
            "b.cljs",
            GENERATIONS[2].1,
            &[
                "b miss source: source changed",
                "main miss dependencies: dependency b changed",
            ],
        ),
        (
            "a.cljs",
            GENERATIONS[1].1,
            &[
                "a miss source: source changed",
                "main miss dependencies: dependency a changed",
            ],
        ),
    ];
    for round in 0..2 {
        for (path, text, expected) in edits {
            project.write(path, &format!("{text} ; edit {round}"));
            let mixed = project.cached("main", &[]);
            let rows = mixed.rows();
            assert_eq!(rows, expected);
            // `gen` and `c`, and `a` while it hits.
            let hits = 5 - expected.len() as u64;
            assert_eq!(mixed.stat("cache.hits"), hits, "{}", mixed.stats);
            assert_eq!(mixed.run(), printed, "{rows:?}");
            assert_eq!(normalized(&mixed.js), normalized(&oracle.js), "{rows:?}");
            let fresh = project.uncached("main", &[]);
            assert_eq!(normalized(&mixed.js), normalized(&fresh.js));
        }
    }
}

/// A custom reader that returns a quoted gensym.
const UNIQ_READER: &str = "(fn [_] (list 'quote (gensym \"r\")))";

const READER_GENSYMS: &[(&str, &str)] = &[
    ("app/a.cljs", "(ns app.a) (def s #app/uniq a)"),
    ("app/b.cljs", "(ns app.b) (def s #app/uniq b)"),
    (
        "app/main.cljs",
        "(ns app.main (:require [app.a :as a] [app.b :as b])) (js/console.log (pr-str [a/s b/s]) (= a/s b/s))",
    ),
];

/// A custom reader's gensyms, in child processes of this binary: a miss
/// after a hit, whose evaluator the hit never started, returns no symbol
/// the hit retains, and the build is the cache-off build's byte for byte
/// (light-cljs-350).
#[test]
fn custom_reader_gensyms_across_processes() {
    let project = Project::new(READER_GENSYMS);
    let oracle = project.child("app.main", false);
    let printed = oracle.run();
    assert!(printed.ends_with("] false\n"), "{printed}");
    assert_eq!(project.child("app.main", true).js, oracle.js);
    project.child("app.main", true).assert_all_hits(3);
    project.write("app/b.cljs", &format!("{} ; edited", READER_GENSYMS[1].1));
    let edited = project.child("app.main", true);
    assert_eq!(
        edited.rows(),
        [
            "app.b miss source: source changed",
            "app.main miss dependencies: dependency app.b changed",
        ]
    );
    assert_eq!(edited.stat("cache.hits"), 1);
    assert_eq!(edited.js, project.child("app.main", false).js);
    let mixed = edited.run();
    assert!(mixed.ends_with("] false\n"), "{mixed}");
    // `app.a`, a hit, keeps its symbol.
    assert_eq!(mixed.split(' ').next(), printed.split(' ').next());
}

/// A custom reader that names a core function.
const CORE_TARGET_READER: &str = "(fn [_] 'cljs.core/even?)";

/// A custom reader's result as a mutation place, on the Pants build's macro
/// backend (custom readers need QuickJS): the build's core-write scan cannot
/// tell what `#app/core-target nil` reads as, so once a consumer rebinds it,
/// the cached provider whose helper calls `even?` is analyzed again rather
/// than installed, nothing is published, and the output is the cache-off
/// build's (light-cljs-588).
#[cfg(all(feature = "bbp-macros", feature = "quickjs"))]
#[test]
fn tagged_core_write_places_reject_cached_providers() {
    let project = Project::new(&[
        ("util.cljs", "(ns util) (defn probe [] (even? 2))"),
        (
            "macros.cljs",
            "(ns macros (:require [util])) (defmacro check [] (util/probe))",
        ),
        (
            "main.cljs",
            "(ns main (:require [util] [macros :refer [check]])) (js/console.log (check))",
        ),
    ]);
    let native = [("NATIVE", "1")];
    project.child_with("main", true, &native);
    let warm = project.child_with("main", true, &native);
    warm.assert_all_hits(3);
    assert_eq!(warm.run(), "true\n");

    project.write(
        "main.cljs",
        "(ns main (:require [util] [macros :refer [check]])) \
         (defmacro redefined [] (with-redefs [#app/core-target nil (fn [_] false)] (util/probe))) \
         (js/console.log (check) (redefined))",
    );
    let oracle = project.child_with("main", false, &native);
    assert_eq!(oracle.run(), "true false\n");
    let edited = project.child_with("main", true, &native);
    assert_eq!(edited.stat("cache.hits"), 0, "{}", edited.stats);
    assert_eq!(edited.stat("cache.miss.core-write"), 3, "{}", edited.stats);
    assert_eq!(edited.stat("cache.published"), 0, "{}", edited.stats);
    assert_eq!(edited.js, oracle.js);
    assert_eq!(edited.run(), "true false\n");
}

// -- Declaration view of source-macro users ----------------------------------

/// `user` runs source macros that see what earlier namespaces it does not
/// require declare: a protocol call and `satisfies?` on a number, a
/// multimethod call, the `&env` metadata of `other/foo` and how many macros
/// `&env` lists. `e` and `other` come before it and only `main` requires
/// them.
fn declaration_project(e: &str, other: &str) -> Project {
    Project::new(&[
        (
            "p.cljs",
            "(ns p) (defprotocol P (m [x])) (defmulti mm identity) (defmethod mm :default [_] :none)",
        ),
        (
            "mac.cljs",
            "(ns mac (:require [p])) (defmacro seen [] (pr-str [(when (satisfies? p/P 1) (p/m 1)) (p/mm :k) (get (:light-cljs/macros &env) 'other/foo) (count (:light-cljs/macros &env))]))",
        ),
        ("e.cljs", e),
        ("other.cljs", other),
        (
            "user.cljs",
            "(ns user (:require [mac :refer [seen]])) (def value (seen))",
        ),
        (
            "main.cljs",
            "(ns main (:require [e] [other] [user])) (js/console.log user/value)",
        ),
    ])
}

/// A source-macro user's view covers what earlier namespaces declare, not
/// their whole keys (light-cljs-347): an edit after the last form that
/// registers a declaration bundle, or to a namespace that registers none,
/// keeps it a hit, while a first extension, a multimethod method, a body a
/// bundle calls, a redefinition after the bundles, changed macro metadata,
/// a new macro and a new namespace each make it miss for its view. Every
/// build matches the uncached one, at one job and at four.
#[test]
fn declaration_view_of_source_macro_users() {
    const VIEW: &str = "user miss view: earlier namespaces changed";
    const MAIN: &str = "main miss dependencies: dependency e changed";
    const OTHER: &str = "main miss dependencies: dependency other changed";
    const E: &str = "e miss source: source changed";
    const OTHER_SOURCE: &str = "other miss source: source changed";
    for jobs in ["1", "4"] {
        let options = ["--jobs", jobs];
        let project = declaration_project(
            "(ns e (:require [p])) (defn helper [x] (* x 10))",
            "(ns other) (defmacro foo \"one\" [] 1)",
        );
        let step = |path: &str, text: &str, rows: &[&str], printed: &str| {
            project.write(path, text);
            let (build, output) = project.checked("main", &options);
            assert_eq!(build.rows(), rows, "{jobs} jobs: {text}");
            assert!(output.contains(printed), "{jobs} jobs: {output}");
        };
        let (_, printed) = project.checked("main", &options);
        assert!(printed.starts_with("[nil :none {:doc \"one\""), "{printed}");
        project.cached("main", &options).assert_all_hits(6);
        // A namespace that registers nothing: a body edit, a new `defn`.
        step(
            "e.cljs",
            "(ns e (:require [p])) (defn helper [x] (* x 20)) (defn more [] 1)",
            &[E, MAIN],
            "[nil :none",
        );
        // Its first extension, which the protocol call and `satisfies?` see.
        let extended = "(ns e (:require [p])) (defn helper [x] (* x 20)) (extend-protocol p/P number (m [x] (helper x)))";
        step("e.cljs", extended, &[E, VIEW, MAIN], "[20 :none");
        // A `defn` and a comment after the last bundle: a hit.
        step(
            "e.cljs",
            &format!("{extended} (defn after [] 2) ; edited"),
            &[E, MAIN],
            "[20 :none",
        );
        // The body the extension calls, before it.
        let extended = extended.replace("(* x 20)", "(* x 30)");
        step("e.cljs", &extended, &[E, VIEW, MAIN], "[30 :none");
        // A declared Var the extension calls, defined after it.
        let declared = "(ns e (:require [p])) (declare helper) (extend-protocol p/P number (m [x] (helper x))) (defn helper [x] (* x 40))";
        step("e.cljs", declared, &[E, VIEW, MAIN], "[40 :none");
        step(
            "e.cljs",
            &declared.replace("(* x 40)", "(* x 50)"),
            &[E, VIEW, MAIN],
            "[50 :none",
        );
        // A multimethod method.
        step(
            "e.cljs",
            &format!(
                "{} (defmethod p/mm :k [_] :e)",
                declared.replace("(* x 40)", "(* x 50)")
            ),
            &[E, VIEW, MAIN],
            "[50 :e",
        );
        // Changed macro metadata, then a new macro.
        step(
            "other.cljs",
            "(ns other) (defmacro foo \"two\" [] 1)",
            &[OTHER_SOURCE, VIEW, OTHER],
            "{:doc \"two\"",
        );
        step(
            "other.cljs",
            "(ns other) (defmacro foo \"two\" [] 1) (defmacro bar [] 2)",
            &[OTHER_SOURCE, VIEW, OTHER],
            "{:doc \"two\"",
        );
        // A namespace added before `user`.
        project.write("added.cljs", "(ns added) (def x 1)");
        step(
            "main.cljs",
            "(ns main (:require [e] [other] [added] [user])) (js/console.log user/value)",
            &[
                "added miss no-artifact: no artifact was published",
                VIEW,
                "main miss source: source changed",
            ],
            "[50 :e",
        );
    }
}

/// Six namespaces, so that `e`'s units merge (its drain point) before the
/// macro provider after them is compiled.
const SPACERS: [(&str, &str); 6] = [
    ("s1.cljs", "(ns s1) (def x 1)"),
    ("s2.cljs", "(ns s2) (def x 2)"),
    ("s3.cljs", "(ns s3) (def x 3)"),
    ("s4.cljs", "(ns s4) (def x 4)"),
    ("s5.cljs", "(ns s5) (def x 5)"),
    ("s6.cljs", "(ns s6) (def x 6)"),
];

/// A Var that a function body defines (`(def map 1)` inside `f`) shadows a
/// core name for a code read with `*ns*` bound to its namespace, and the
/// program has it only once the body merges, after the namespace is keyed:
/// the declaration view reads it from the settled unit (light-cljs-347).
#[test]
fn deferred_globals_change_source_macro_reader_views() {
    for jobs in ["1", "4"] {
        let options = ["--jobs", jobs];
        let mut files = vec![("e.cljs", "(ns e) (defn f [] nil)")];
        files.extend(SPACERS);
        files.extend([
            (
                "mac.cljs",
                "(ns mac) (defmacro read-in-e [] (binding [*ns* (the-ns 'e)] (pr-str (read-string \"`map\"))))",
            ),
            (
                "user.cljs",
                "(ns user (:require [mac :refer [read-in-e]])) (def value (read-in-e))",
            ),
            (
                "main.cljs",
                "(ns main (:require [e] [s1] [s2] [s3] [s4] [s5] [s6] [user])) (js/console.log user/value)",
            ),
        ]);
        let project = Project::new(&files);
        let (_, before) = project.checked("main", &options);
        project.cached("main", &options).assert_all_hits(10);
        project.write("e.cljs", "(ns e) (defn f [] (def map 1))");
        let (edited, after) = project.checked("main", &options);
        assert_ne!(before, after, "{jobs} jobs: the read sees e/map");
        assert!(
            edited
                .rows()
                .contains(&"user miss view: earlier namespaces changed"),
            "{jobs} jobs: {:?}",
            edited.rows()
        );
    }
}

/// A source macro that lists `all-ns` and finds namespaces with `find-ns`
/// keeps the uncached build's answer when an earlier namespace that
/// declares nothing is edited, and so keeps its view. A miss of its own
/// with the namespaces before it hits lists what the uncached build lists:
/// `early`, which the uncached build registers when its macro call runs,
/// is registered when its artifact installs (light-cljs-1004). `late`,
/// which no unit or macro call names, is listed by neither.
#[test]
fn declaration_view_preserves_namespace_inventory_on_mixed_hits() {
    for jobs in ["1", "4"] {
        let options = ["--jobs", jobs];
        let mut files = vec![
            (
                "mac.cljs",
                "(ns mac) (defmacro listed [] (pr-str [(sort (map str (all-ns))) (some? (find-ns 'early)) (some? (find-ns 'late)) (map str (all-ns))]))",
            ),
            (
                "early.cljs",
                "(ns early (:require [mac :refer [listed]])) (def x (listed)) (defn g [] 1)",
            ),
        ];
        files.extend(SPACERS);
        files.extend([
            ("late.cljs", "(ns late) (def y 1)"),
            (
                "user.cljs",
                "(ns user (:require [mac :refer [listed]])) (def value (listed))",
            ),
            (
                "main.cljs",
                "(ns main (:require [early] [s1] [s2] [s3] [s4] [s5] [s6] [late] [user])) (js/console.log early/x user/value)",
            ),
        ]);
        let project = Project::new(&files);
        let (cold, listed) = project.checked("main", &options);
        let warm = project.cached("main", &options);
        warm.assert_all_hits(11);
        assert_eq!(warm.js, cold.js, "{jobs} jobs");
        // `user` misses with every namespace before it a hit, so none of
        // their macro calls runs: it lists what the uncached build lists,
        // in its order.
        project.write(
            "user.cljs",
            "(ns user (:require [mac :refer [listed]])) (def value (listed)) ; edited",
        );
        let (mixed, printed) = project.checked("main", &options);
        assert_eq!(
            mixed.rows(),
            [
                "user miss source: source changed",
                "main miss dependencies: dependency user changed"
            ],
            "{jobs} jobs"
        );
        assert_eq!(mixed.js, cold.js, "{jobs} jobs");
        assert_eq!(printed, listed);
        assert!(
            printed.contains("\"early\" \"mac\" \"user\") true false"),
            "{jobs} jobs: {printed}"
        );
        // `early` declares nothing: `user` keeps its view.
        project.write(
            "early.cljs",
            "(ns early (:require [mac :refer [listed]])) (def x (listed)) (defn g [] 2)",
        );
        let (edited, _) = project.checked("main", &options);
        assert!(
            !edited
                .rows()
                .contains(&"user miss view: earlier namespaces changed"),
            "{jobs} jobs: {:?}",
            edited.rows()
        );
    }
}

/// A registration adds every namespace of its program the registry lacks,
/// in program order. Where an earlier namespace no longer adds one of them
/// itself, a hit that registered the others adds it with them, in that
/// order, as the uncached build's registration does (light-cljs-1004).
#[test]
fn installed_registrations_keep_the_registry_order() {
    for jobs in ["1", "4"] {
        let options = ["--jobs", jobs];
        let project = Project::new(&[
            ("m0.cljs", "(ns m0) (defmacro one [] 1)"),
            (
                "a.cljs",
                "(ns a (:require [m0 :refer [one]])) (def x (one))",
            ),
            ("b.cljs", "(ns b) (def y 2)"),
            (
                "mac.cljs",
                "(ns mac) (defmacro listed [] (pr-str (map str (all-ns))))",
            ),
            (
                "user.cljs",
                "(ns user (:require [mac :refer [listed]])) (def value (listed))",
            ),
            (
                "main.cljs",
                "(ns main (:require [m0] [a] [b] [mac] [user])) (js/console.log user/value)",
            ),
        ]);
        let (_, before) = project.checked("main", &options);
        project.cached("main", &options).assert_all_hits(6);
        // `a` no longer expands a macro, so `mac`'s registration adds it.
        project.write("a.cljs", "(ns a (:require [m0 :refer [one]])) (def x 1)");
        let (mixed, after) = project.checked("main", &options);
        // `mac` is a hit; `a` and `user` are not.
        assert!(
            !mixed.rows().iter().any(|row| row.starts_with("mac "))
                && mixed.rows().iter().any(|row| row.starts_with("user ")),
            "{jobs} jobs: {:?}",
            mixed.rows()
        );
        assert_eq!(before, after, "{jobs} jobs");
        assert!(
            after.contains("\"m0\" \"a\" \"b\" \"mac\" \"user\""),
            "{after}"
        );
    }
}

/// A namespace whose only source macro call is in a function body is added
/// to the registry by that unit's analysis: on the coordinator with one job
/// or an engine-less pool, where installing its artifact adds it too, and
/// in a pool worker's own registry with worker engines, where installing
/// adds it to none (light-cljs-1004). A definition with `:test` metadata is
/// analyzed on the coordinator whatever the pool, so what its unit adds is
/// in the coordinator's registry with worker engines too, and installing
/// adds it there (light-cljs-1005).
#[test]
fn unit_registry_additions_install_where_the_unit_ran() {
    let isolates = ["--jobs", "4", "--macro-memory-limit", "268435456"];
    // One worker isolate: the coordinator keeps the declaration bundles.
    let one_isolate = ["--jobs", "2", "--macro-memory-limit", "134217728"];
    let user = "(ns user (:require [mac :refer [listed]])) (js/console.log (listed))";
    let configurations = [
        &["--jobs", "1"][..],
        &["--jobs", "4"],
        &isolates,
        &one_isolate,
    ];
    for options in configurations {
        let project = Project::new(&[
            (
                "mac.cljs",
                "(ns mac) (defmacro listed [] (pr-str (map str (all-ns)))) (defmacro mk [] (create-ns 'extra) `(fn [] 1)) (defmacro one [] 1)",
            ),
            (
                "early.cljs",
                "(ns early (:require [mac :refer [listed]])) (defn f [] (listed))",
            ),
            (
                "tested.cljs",
                "(ns tested (:require [mac :refer [mk]])) (def ^{:test (fn [] nil)} t (mk))",
            ),
            (
                "methods.cljs",
                "(ns methods (:require [mac :refer [one]])) (defmulti m identity) (defmethod m :a [_] (one))",
            ),
            ("user.cljs", user),
            (
                "main.cljs",
                "(ns main (:require [early] [tested] [methods] [user])) (js/console.log (early/f) (methods/m :a))",
            ),
        ]);
        let (cold, listed) = project.checked("main", options);
        let warm = project.cached("main", options);
        warm.assert_all_hits(6);
        assert_eq!(warm.js, cold.js, "{options:?}");
        project.write("user.cljs", &format!("{user} ; edited"));
        let (mixed, printed) = project.checked("main", options);
        assert_eq!(
            mixed.rows(),
            [
                "user miss source: source changed",
                "main miss dependencies: dependency user changed"
            ],
            "{options:?}"
        );
        assert_eq!(printed, listed, "{options:?}");
        // The `:test` definition's unit adds `tested` and `extra` to the
        // coordinator's registry with every pool.
        assert!(
            printed.contains("\"tested\" \"extra\""),
            "{options:?}: {printed}"
        );
        if options.len() == 2 {
            assert!(
                printed.starts_with("(\"cljs.user\" \"cljs.core\" \"mac\" \"early\" \"tested\""),
                "{printed}"
            );
        }
        // The bundle of `m`'s method adds `methods` where it is analyzed.
        assert_eq!(
            printed.contains("\"methods\""),
            options != isolates,
            "{options:?}: {printed}"
        );
    }
}

/// What a deferred unit adds to the registry is recorded with the rule
/// that places the unit, which its registration decides, so that a project
/// whose output does not depend on `--jobs` publishes the same artifacts
/// with one job and with worker engines: a definition's unit (`early`,
/// whose additions the lister leaves out), a `:test` definition's and the
/// namespace's own analysis. Each build then installs them and a one-file
/// edit matches the uncached build (light-cljs-1005).
#[test]
fn registry_additions_publish_the_same_artifacts_at_every_jobs() {
    let user = "(ns user (:require [mac :refer [listed]])) (js/console.log (listed))";
    let project = Project::new(&[
        (
            "mac.cljs",
            "(ns mac) (defmacro listed [] (pr-str (remove #{\"early\" \"other\"} (map str (all-ns))))) (defmacro mk [n] (create-ns n) `(fn [] 1))",
        ),
        (
            "early.cljs",
            "(ns early (:require [mac :refer [mk]])) (defn f [] ((mk other)))",
        ),
        (
            "tested.cljs",
            "(ns tested (:require [mac :refer [mk]])) (def ^{:test (fn [] nil)} t (mk extra)) (mk top)",
        ),
        ("user.cljs", user),
        (
            "main.cljs",
            "(ns main (:require [early] [tested] [user])) (js/console.log (early/f))",
        ),
    ]);
    let mut published: Option<Vec<(String, Vec<u8>)>> = None;
    for jobs in ["1", "4"] {
        let options = ["--jobs", jobs, "--macro-memory-limit", "268435456"];
        project.write("user.cljs", user);
        let _ = fs::remove_dir_all(project.cache());
        let (cold, listed) = project.checked("main", &options);
        assert!(
            listed.contains("\"tested\" \"top\" \"extra\" \"user\")"),
            "{jobs} jobs: {listed}"
        );
        let objects = project.objects();
        assert_eq!(objects.len(), 5, "{jobs} jobs");
        match &published {
            None => published = Some(objects),
            Some(serial) => assert!(&objects == serial, "the artifacts differ at {jobs} jobs"),
        }
        let warm = project.cached("main", &options);
        warm.assert_all_hits(5);
        assert_eq!(warm.js, cold.js, "{jobs} jobs");
        project.write("user.cljs", &format!("{user} ; edited"));
        let (_, printed) = project.checked("main", &options);
        assert_eq!(printed, listed, "{jobs} jobs");
    }
}

/// With worker engines, a namespace a pool worker's unit made with
/// `create-ns` is in that worker's registry only; a later top-level macro
/// call that makes it on the coordinator still adds it there, and its
/// namespace's artifact records it (light-cljs-1004).
#[test]
fn worker_registry_additions_do_not_hide_coordinator_additions() {
    let options = ["--jobs", "4", "--macro-memory-limit", "268435456"];
    let mut files = vec![
        (
            "mac.cljs",
            "(ns mac) (defmacro listed [] (pr-str (sort (map str (all-ns))))) (defmacro mk [] (create-ns 'extra) nil)",
        ),
        (
            "early.cljs",
            "(ns early (:require [mac :refer [mk]])) (defn f [] (mk))",
        ),
    ];
    files.extend(SPACERS);
    files.extend([
        ("later.cljs", "(ns later (:require [mac :refer [mk]])) (mk)"),
        (
            "user.cljs",
            "(ns user (:require [mac :refer [listed]])) (js/console.log (listed))",
        ),
        (
            "main.cljs",
            "(ns main (:require [early] [s1] [s2] [s3] [s4] [s5] [s6] [later] [user])) (early/f)",
        ),
    ]);
    let project = Project::new(&files);
    let (_, listed) = project.checked("main", &options);
    assert!(listed.contains("\"extra\""), "{listed}");
    project.cached("main", &options).assert_all_hits(11);
    project.write(
        "user.cljs",
        "(ns user (:require [mac :refer [listed]])) (js/console.log (listed)) ; edited",
    );
    let (_, printed) = project.checked("main", &options);
    assert_eq!(printed, listed);
}

/// What `later` adds to the registry after `early`, in
/// [`coordinator_repeats_a_worker_unit_registry_addition`].
enum Later {
    /// A definition with `:test` metadata, which the coordinator keeps.
    TestDefinition,
    /// A top-level macro call.
    TopLevel,
    /// A declaration bundle, which the coordinator keeps with one engine
    /// worker.
    Bundle,
}

/// A namespace a definition's unit in `early` made with `create-ns`, which
/// a placement in a later namespace that the coordinator analyzes makes
/// again ([`Later`]). With worker engines the first addition is in a
/// worker's registry only and the second in the coordinator's; with one
/// job the registry has it from the first. Each namespace's artifact
/// records the second as well, with its placement, so that the artifacts
/// are the same at every `--jobs` and installing them gives each
/// configuration's coordinator registry: cold, all-hit and one-edit builds
/// match the uncached build (light-cljs-1006).
fn coordinator_repeats_a_worker_unit_registry_addition(placement: Later) {
    let user = "(ns user (:require [mac :refer [listed]])) (js/console.log (listed))";
    let later = match placement {
        Later::TestDefinition => {
            "(ns later (:require [mac :refer [mk]])) (def ^{:test (fn [] nil)} t (mk))"
        }
        Later::TopLevel => "(ns later (:require [mac :refer [mk]])) (mk)",
        Later::Bundle => {
            "(ns later (:require [mac :refer [mk]])) (defmulti m identity) (defmethod m :a [_] ((mk)))"
        }
    };
    let bundle = matches!(placement, Later::Bundle);
    let mut files = vec![
        (
            "mac.cljs",
            "(ns mac) (defmacro listed [] (pr-str (sort (remove #{\"early\"} (map str (all-ns)))))) (defmacro mk [] (create-ns 'extra) `(fn [] 1))",
        ),
        (
            "early.cljs",
            "(ns early (:require [mac :refer [mk]])) (defn f [] ((mk)))",
        ),
    ];
    files.extend(SPACERS);
    files.extend([
        ("later.cljs", later),
        ("user.cljs", user),
        (
            "main.cljs",
            "(ns main (:require [early] [s1] [s2] [s3] [s4] [s5] [s6] [later] [user])) (js/console.log (early/f))",
        ),
    ]);
    let project = Project::new(&files);
    let serial = ["--jobs", "1", "--macro-memory-limit", "268435456"];
    let pool = ["--jobs", "4"];
    let engines = ["--jobs", "4", "--macro-memory-limit", "268435456"];
    // One worker isolate: the coordinator keeps the declaration bundles.
    let one_isolate = ["--jobs", "2", "--macro-memory-limit", "134217728"];
    let mut published: Option<Vec<(String, Vec<u8>)>> = None;
    for options in [&serial[..], &pool, &engines, &one_isolate] {
        project.write("user.cljs", user);
        let _ = fs::remove_dir_all(project.cache());
        let (cold, listed) = project.checked("main", options);
        // `later` adds `extra` to the coordinator's registry in every
        // configuration, a bundle where the coordinator keeps it. The
        // lister leaves out `early`, which the coordinator registers only
        // where its unit runs there, and sorts, so that the output, and
        // `user`'s artifact, are the same at every `--jobs`.
        assert_eq!(
            listed.contains("\"extra\" \"later\""),
            !bundle || options != &engines[..],
            "{options:?}: {listed}"
        );
        let objects = project.objects();
        if !bundle && (options == &serial[..] || options == &engines[..]) {
            match &published {
                None => published = Some(objects),
                Some(serial) => {
                    assert!(&objects == serial, "the artifacts differ at {options:?}")
                }
            }
        }
        let warm = project.cached("main", options);
        warm.assert_all_hits(11);
        assert_eq!(warm.js, cold.js, "{options:?}");
        project.write("user.cljs", &format!("{user} ; edited"));
        let (mixed, printed) = project.checked("main", options);
        assert_eq!(
            mixed.rows(),
            [
                "user miss source: source changed",
                "main miss dependencies: dependency user changed"
            ],
            "{options:?}"
        );
        assert_eq!(printed, listed, "{options:?}");
    }
}

#[test]
fn coordinator_unit_repeats_a_worker_unit_registry_addition() {
    coordinator_repeats_a_worker_unit_registry_addition(Later::TestDefinition);
}

#[test]
fn top_level_call_repeats_a_worker_unit_registry_addition() {
    coordinator_repeats_a_worker_unit_registry_addition(Later::TopLevel);
}

#[test]
fn bundle_repeats_a_worker_unit_registry_addition() {
    coordinator_repeats_a_worker_unit_registry_addition(Later::Bundle);
}

// -- Lowered limits -----------------------------------------------------------

/// `--macro-expansions` below a cached namespace's recorded count is
/// enforced: the limits are in every key, so the namespaces miss and their
/// compilation fails as the uncached one does, and the artifacts of the
/// original limit still hit.
#[test]
fn lowered_limits() {
    let project = Project::new(&[
        (
            "macros.cljs",
            "(ns macros) (defmacro one [] 1) (defmacro many [] `(+ (one) (one) (one) (one) (one) (one)))",
        ),
        (
            "heavy.cljs",
            "(ns heavy (:require [macros :refer [many]])) (def value (+ (many) (many) (many)))",
        ),
        (
            "main.cljs",
            "(ns main (:require [heavy])) (js/console.log heavy/value)",
        ),
    ]);
    let limit = ["--macro-expansions", "100"];
    let (_, printed) = project.checked("main", &limit);
    assert_eq!(printed, "18\n");
    project.cached("main", &limit).assert_all_hits(3);
    let lowered = ["--macro-expansions", "5"];
    let error = project.output("main", &lowered, true).err().expect("over");
    let oracle = project.output("main", &lowered, false).err().expect("over");
    assert!(error.contains("expansion"), "{error}");
    assert_eq!(error.lines().next(), oracle.lines().next());
    // A lower limit the build still fits is another key, too.
    let enough = ["--macro-expansions", "40"];
    let (refit, _) = project.checked("main", &enough);
    assert_eq!(refit.stat("cache.miss.options"), 3, "{}", refit.stats);
    project.cached("main", &limit).assert_all_hits(3);
}

// -- Repeated builds and teardown ---------------------------------------------

/// Ten builds in this process through `analyze_namespace_entry_cached`, the
/// first filling the cache and the others installing every namespace from
/// it, each leave nothing alive once their program, host and warnings are
/// dropped: no artifact, engine, engine handle or bridge payload
/// (`compile_stats::live_counts`), and every warm build makes the same
/// program. The counts are the process's, so this test compiles here while
/// every other test in this binary compiles in child processes. A captured
/// artifact counts while it is held, and `--compile --stats` reports the
/// counts at its end, all zero.
#[test]
fn repeated_builds_and_teardown() {
    use light_cljs::MacroHost;
    use light_cljs::analyzer::AnalysisOptions;
    use light_cljs::compile_stats::{CompileStats, LiveCounts, live_counts};
    let project = Project::new(PLAIN_FIXTURE);
    let roots = project.roots();
    let cache = light_cljs::compile_cache::CacheOptions {
        dir: project.cache(),
        blockers: Vec::new(),
        retention: Default::default(),
    };
    assert_eq!(live_counts(), LiveCounts::default());
    let mut tables = None;
    for build in 1..=10 {
        let mut host = MacroHost::new();
        let mut warnings = Vec::new();
        let mut stats = CompileStats::default();
        let (program, files) = light_cljs::compiler::analyze_namespace_entry_cached(
            "main",
            &roots,
            &mut host,
            AnalysisOptions::default(),
            &mut warnings,
            &cache,
            Some(&mut stats),
            light_cljs::scheduler::Slots::ONE,
        )
        .unwrap();
        let counts = stats.cache.take().unwrap();
        if build == 1 {
            assert_eq!((counts.hits, counts.published), (0, 5), "{counts:?}");
            // The macros ran in an engine the host still holds, whose
            // objects still own the provenance of forms they imported.
            let live = live_counts();
            assert!(live.engines > 0 && live.bridge_payloads > 0, "{live:?}");
        } else {
            assert_eq!((counts.hits, counts.misses.len()), (5, 0), "{counts:?}");
            assert_eq!(stats.macro_calls, 0);
            let sizes = (
                program.exprs.len(),
                program.globals.len(),
                program.strings.len(),
            );
            assert_eq!(*tables.get_or_insert(sizes), sizes, "build {build}");
        }
        drop((program, files, host, warnings, stats));
        assert_eq!(live_counts(), LiveCounts::default(), "after build {build}");
    }
    let mut artifacts = Vec::new();
    let mut host = MacroHost::new();
    light_cljs::compiler::analyze_namespace_entry_with_artifacts(
        "main",
        &roots,
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        None,
        &mut |capture| artifacts.push(capture.artifact.unwrap()),
    )
    .unwrap();
    drop(host);
    assert_eq!(live_counts().artifacts, 5);
    let copy = artifacts[0].clone();
    assert_eq!(live_counts().artifacts, 6);
    drop((copy, artifacts));
    assert_eq!(live_counts(), LiveCounts::default());
    let report = project.cached("main", &[]).stats;
    assert!(
        report.contains(
            "\nmemory.live.artifacts 0\n\
             memory.live.engines 0\n\
             memory.live.engine-handles 0\n\
             memory.live.bridge-payloads 0\n"
        ),
        "{report}"
    );
}

#[test]
fn requested_namespace_views_stay_guarded_in_cached_macro_helpers() {
    let a = "(ns a (:require [p] [m :refer [probe]])) (def value (probe))";
    let b = "(ns b (:require [x] [m :refer [probe]])) (def value (probe))";
    let project = Project::new(&[
        ("p.cljs", "(ns p) (defmacro v [] 9)"),
        (
            "x.cljs",
            "(ns x (:require [p :refer [v]]) (:import [Math max]))",
        ),
        (
            "m.cljs",
            "(ns m) (defmacro probe [] [(contains? (ns-refers 'x) 'v) (contains? (ns-imports 'x) 'max)])",
        ),
        ("a.cljs", a),
        ("b.cljs", b),
        (
            "main.cljs",
            "(ns main (:require [x] [a] [b])) (js/console.log (pr-str [a/value b/value]))",
        ),
    ]);
    let pants = cfg!(all(
        feature = "bbp-macros",
        not(any(feature = "quickjs", feature = "v8-macros"))
    ));
    let expected = if pants {
        "[[false false] [true true]]\n"
    } else {
        "[[true true] [true true]]\n"
    };
    assert_eq!(project.checked("main", &[]).1, expected);
    project.cached("main", &[]).assert_all_hits(6);
    // Reuse the provider artifact while both consumers invoke its helper
    // again under their distinct closures; cached IR must retain the guard.
    project.write("a.cljs", &format!("{a}\n; recompile consumer a"));
    project.write("b.cljs", &format!("{b}\n; recompile consumer b"));
    let (reused, printed) = project.checked("main", &[]);
    assert_eq!(printed, expected);
    assert_eq!(reused.stat("cache.hits"), 3);
}
