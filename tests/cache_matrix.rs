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
/// runtime consumer `main` miss. `after` runs a source macro later, so the
/// coarse fallback makes it miss too, for its view and not as a reader of
/// `consumer`'s input, which its manifest does not list.
const CONSUMER_MISSES: [&str; 3] = [
    "consumer miss inputs: external input changed",
    "after miss view: earlier namespaces changed",
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
    assert_eq!(changed.stat("cache.hits"), 4);

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
    assert_eq!(appeared.stat("cache.hits"), 4);

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
    assert_eq!(shadowed.stat("cache.hits"), 4);
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
    /// readers `#app/inline` and `#app/uniq` registered, with the cache
    /// when `cached`.
    fn child(&self, entry: &str, cached: bool) -> Build {
        let out = self.dir.join("child.js");
        let report = self.dir.join("child.stats");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["child", "--exact", "--ignored", "--nocapture", "--quiet"])
            .env(CHILD, entry)
            .env("ROOTS", self.dir.join("src"))
            .env("OUT", &out)
            .env("REPORT", &report);
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
/// entry: compile it from `ROOTS` with the readers `#app/inline` and
/// `#app/uniq`, through the cache in `CACHE` when set, writing the script
/// to `OUT` and the `--stats` report to `REPORT`.
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
    assert_eq!(printed, "1 one\n");
    project.cached("main", &[]).assert_all_hits(5);
    project.write("doc.cljs", "(ns doc) (def ^{:doc \"two\"} value 1)");
    let (metadata, printed) = project.checked("main", &[]);
    assert_eq!(printed, "1 two\n");
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
    assert_ne!(before, after, "the inventory grew");
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
    let mut out = String::with_capacity(js.len());
    let mut seen: Vec<&str> = Vec::new();
    let mut rest = js;
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
    // hit `c`, and `b`, which runs source macros after it, misses too.
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
                "b miss view: earlier namespaces changed",
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
