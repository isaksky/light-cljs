//! A build from `light-cljs.edn` is the build of its equivalent flags
//! (docs/build-configuration.md, "Cache keys and fingerprints"): for every
//! row of "Flags and keys" that `compile` or `release` reads, the two write
//! the same bytes and share the same cache artifacts, so whichever runs second
//! reports every namespace a hit. Environment values enter the keys exactly as
//! the literals in their place would, and nothing else in the file does. The
//! last test is the Phase 1 exit criterion on examples/features.
//!
//! Each test runs the binary in a temporary copy of
//! tests/fixtures/build-configuration/commands (whose `light-cljs.edn` it
//! replaces) or of examples/features, with the working directory and the
//! environment set explicitly.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::commands::{copy, tree};

const FEATURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/build-configuration/commands"
);

/// Variables the tests set; each run removes them all first, and `LIGHT_CLJS`.
const VARIABLES: [&str; 4] = ["LIGHT_CLJS", "APP_FLAVOR", "APP_LABEL", "APP_UNREAD"];

/// What `--stats` says the persistent cache did.
#[derive(Debug, PartialEq)]
struct Cache {
    namespaces: usize,
    hits: usize,
    misses: usize,
    published: usize,
    /// The `[cache-misses-and-bypasses]` rows.
    rows: Vec<String>,
}

impl Cache {
    /// Every namespace was served from the cache: no miss and no bypass.
    fn all_hits(&self) -> bool {
        self.misses == 0 && self.rows.is_empty() && self.hits == self.namespaces && self.hits > 0
    }

    /// Every namespace but those `stored` bypassed was served from the
    /// cache, and those bypass again with the same rows.
    fn hits_all_but_bypasses_of(&self, stored: &Cache) -> bool {
        let bypasses: Vec<&String> = stored
            .rows
            .iter()
            .filter(|row| row.contains(" bypass "))
            .collect();
        self.misses == 0
            && self.rows.iter().collect::<Vec<_>>() == bypasses
            && self.hits + bypasses.len() == self.namespaces
            && self.hits > 0
    }
}

/// The cache lines of `--stats` in `stderr`, or `None` without a cache.
fn cache(stderr: &str) -> Option<Cache> {
    let value = |name: &str| {
        stderr.lines().find_map(|line| {
            line.strip_prefix(name)
                .and_then(|rest| rest.strip_prefix(' '))
                .map(|number| number.parse::<usize>().unwrap())
        })
    };
    let rows = stderr
        .lines()
        .skip_while(|line| *line != "[cache-misses-and-bypasses]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .map(str::to_owned)
        .collect();
    Some(Cache {
        namespaces: value("namespaces").unwrap(),
        hits: value("cache.hits")?,
        misses: value("cache.misses").unwrap(),
        published: value("cache.published").unwrap(),
        rows,
    })
}

/// Run `light-cljs ARGS` in `dir` with exactly `env` of [`VARIABLES`] set,
/// after removing `out/`, which must succeed. Returns stderr's cache lines.
fn run(project: &Path, dir: &Path, args: &[String], env: &[(&str, &str)]) -> Option<Cache> {
    let _ = fs::remove_dir_all(project.join("out"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
    command.args(args).current_dir(dir);
    for name in VARIABLES {
        command.env_remove(name);
    }
    command.envs(env.iter().copied());
    let output = command.output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{args:?} in {}: {stderr}",
        dir.display()
    );
    cache(&stderr)
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// A build of the fixture, written once as a file and once as its flags.
struct Case {
    command: &'static str,
    entry: &'static str,
    /// `:source-paths`, and as many `--source-root`s.
    paths: &'static [&'static str],
    /// More top-level keys, and more keys of build `:app`.
    top: &'static str,
    build: &'static str,
    /// The flags of `top` and `build`, `{project}` standing for the project
    /// directory. They name the store when they name a cache.
    flags: &'static [&'static str],
}

impl Case {
    const fn new(command: &'static str, entry: &'static str) -> Self {
        Self {
            command,
            entry,
            paths: &["src"],
            top: "",
            build: "",
            flags: &[],
        }
    }

    const fn paths(mut self, paths: &'static [&'static str]) -> Self {
        self.paths = paths;
        self
    }

    const fn keys(
        mut self,
        top: &'static str,
        build: &'static str,
        flags: &'static [&'static str],
    ) -> Self {
        self.top = top;
        self.build = build;
        self.flags = flags;
        self
    }

    fn file(&self) -> String {
        let paths = self
            .paths
            .iter()
            .map(|path| format!("{path:?}"))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            ";; Written by tests/config_file/equivalence.rs.\n\
             {{:source-paths [{paths}]\n \
             {top}\n \
             :builds\n \
             {{:app {{:target :esm\n        \
             :output-dir \"out/app\"\n        \
             :modules {{:main {{:entries [{entry}]}}}}\n        \
             {build}}}}}}}\n",
            top = self.top,
            entry = self.entry,
            build = self.build,
        )
    }

    /// The command line the file's build is: `compile` is `--compile
    /// --reloadable`, `release` is `--compile --optimize advanced`, and every
    /// path is the project directory joined with the file's.
    fn flag_args(&self, project: &Path) -> Vec<String> {
        let at = |path: &str| project.join(path).to_str().unwrap().to_owned();
        let given = |flag: &str| self.flags.contains(&flag);
        // `--warnings` and the checked-arrays flags come before `--compile`.
        let prefix = |flag: &&str| *flag == "--warnings" || flag.starts_with("--checked-arrays-");
        let mut args = strings(
            &self
                .flags
                .iter()
                .copied()
                .filter(prefix)
                .collect::<Vec<_>>(),
        );
        args.push("--compile".into());
        if self.command == "compile" {
            args.push("--reloadable".into());
        } else if !given("--optimize") {
            args.extend(strings(&["--optimize", "advanced"]));
        }
        args.extend(strings(&["--entry", self.entry]));
        for path in self.paths {
            args.extend(["--source-root".into(), at(path)]);
        }
        args.extend(["--out-dir".into(), at("out/app")]);
        if !given("--cache-dir") && !given("--no-cache") {
            args.extend(["--cache-dir".into(), at(".light-cljs/cache")]);
        }
        let project = project.to_str().unwrap();
        args.extend(
            self.flags
                .iter()
                .filter(|flag| !prefix(flag))
                .map(|flag| flag.replace("{project}", project)),
        );
        args.push("--stats".into());
        args
    }

    /// Run the file's build and the flags' in both orders, each order with an
    /// empty store: all four write the same bytes, and the second of each
    /// order hits every namespace the first stored.
    fn check(&self) {
        let (_dir, project) = copy("config-equivalence", FIXTURE);
        fs::write(project.join("light-cljs.edn"), self.file()).unwrap();
        let file = strings(&[self.command, "app", "--stats"]);
        let flags = self.flag_args(&project);
        let label = format!("{} {}: {:?}", self.command, self.build, self.flags);
        let mut outputs = Vec::new();
        for (first, second) in [(&file, &flags), (&flags, &file)] {
            for store in [".light-cljs", "state"] {
                let _ = fs::remove_dir_all(project.join(store));
            }
            let stored = run(&project, &project, first, &[]);
            outputs.push(tree(&project.join("out/app")));
            let served = run(&project, &project, second, &[]);
            outputs.push(tree(&project.join("out/app")));
            match (&stored, &served) {
                (Some(stored), Some(served)) => {
                    assert!(stored.published > 0, "{label}: {stored:?}");
                    assert!(
                        served.hits_all_but_bypasses_of(stored),
                        "{label}, {second:?} second: {served:?}"
                    );
                    // Only the blocker case bypasses.
                    assert_eq!(
                        stored.rows.iter().any(|row| row.contains(" bypass ")),
                        self.top.contains(":cache-blockers"),
                        "{label}: {stored:?}"
                    );
                    assert_eq!(served.hits, stored.published, "{label}");
                }
                // `:cache-level :off` and `--no-cache`: nothing is stored.
                (None, None) => {
                    assert!(!project.join(".light-cljs").exists(), "{label}");
                }
                _ => panic!("{label}: one of the two has a cache: {stored:?} {served:?}"),
            }
        }
        assert!(!outputs[0].is_empty(), "{label}");
        for output in &outputs[1..] {
            assert!(*output == outputs[0], "{label}: the outputs differ");
        }
    }
}

/// The rows `compile` and `release` both read.
const BOTH: &[Case] = &[
    // --entry, --out-dir and the default store are in every case.
    Case::new("", "app.main"),
    Case::new("", "app.main").paths(&["alt"]),
    // Two roots, of which app.other resolves in the second.
    Case::new("", "app.other").paths(&["alt", "src"]),
    Case::new("", "app.other").keys(
        "",
        ":compiler-options {:elide-asserts true}",
        &["--elide-asserts"],
    ),
    Case::new("", "app.main").keys(
        "",
        ":compiler-options {:macro-expansions 5000}",
        &["--macro-expansions", "5000"],
    ),
    Case::new("", "app.main").keys(
        "",
        ":compiler-options {:macro-nodes 200000}",
        &["--macro-nodes", "200000"],
    ),
    Case::new("", "app.main").keys(
        "",
        ":compiler-options {:macro-timeout-ms 20000}",
        &["--macro-timeout-ms", "20000"],
    ),
    Case::new("", "app.main").keys(
        "",
        ":compiler-options {:macro-memory-limit 134217728}",
        &["--macro-memory-limit", "134217728"],
    ),
    Case::new("", "app.main").keys(
        "",
        ":compiler-options {:macro-poll-budget 2000000}",
        &["--macro-poll-budget", "2000000"],
    ),
    // The map's canonical text does not depend on the order keys are given.
    Case::new("", "app.main").keys(
        "",
        ":config {:app/flavor :file :app/size [1 \"two\" {:three 3.5}]}",
        &[
            "--config",
            "app/size=[1 \"two\" {:three 3.5}]",
            "--config",
            "app/flavor=:file",
        ],
    ),
    Case::new("", "app.main").keys(":compile-threads 2", "", &["--jobs", "2"]),
    Case::new("", "app.main").keys(
        ":cache-root \"state\"",
        "",
        &["--cache-dir", "{project}/state/cache"],
    ),
    Case::new("", "app.main").keys("", ":build-options {:cache-level :off}", &["--no-cache"]),
    Case::new("", "app.main").keys(
        ":cache-blockers #{app.macros}",
        ":build-options {:cache-blockers #{app.util}}",
        &[
            "--cache-blocker",
            "app.macros",
            "--cache-blocker",
            "app.util",
        ],
    ),
];

/// The rows only `release` reads.
const RELEASE: &[Case] = &[
    Case::new("release", "app.other").keys(
        "",
        ":compiler-options {:optimizations :none}",
        &["--optimize", "none"],
    ),
    Case::new("release", "app.other").keys(
        "",
        ":compiler-options {:optimizations :prepare}",
        &["--optimize", "prepare"],
    ),
    Case::new("release", "app.other").keys(
        "",
        ":compiler-options {:optimizations :advanced}",
        &["--optimize", "advanced"],
    ),
    Case::new("release", "app.other").keys(
        "",
        ":release {:closure-defines {app.other/label \"label-file\"}}",
        &["--define", "app.other/label=label-file"],
    ),
    Case::new("release", "app.other").keys(
        "",
        ":release {:compiler-options {:closure-defines {app.other/label \"label-options\"}}}",
        &["--define", "app.other/label=label-options"],
    ),
    Case::new("release", "app.other").keys(
        "",
        ":compiler-options {:pseudo-names true}",
        &["--pseudo-names"],
    ),
    Case::new("release", "app.other").keys(
        "",
        ":release {:compiler-options {:checked-arrays :warn}}",
        &["--checked-arrays-warn"],
    ),
    Case::new("release", "app.other").keys(
        "",
        ":release {:compiler-options {:checked-arrays :error}}",
        &["--checked-arrays-error"],
    ),
    // Without warnings, app.broken's undeclared Var is an error.
    Case::new("release", "app.broken").keys(
        "",
        ":compiler-options {:cljs-warnings true}",
        &["--warnings"],
    ),
];

fn check_both(command: &'static str, cases: &[Case]) {
    for case in cases {
        Case { command, ..*case }.check();
    }
}

#[test]
fn compile_rows_equal_their_flags() {
    check_both("compile", BOTH);
}

#[test]
fn release_rows_of_both_modes_equal_their_flags() {
    check_both("release", BOTH);
}

#[test]
fn release_only_rows_equal_their_flags() {
    for case in RELEASE {
        case.check();
    }
}

/// The fixture with build `:app` of app.main and `config` as its `:config`,
/// and a second build, `:other`.
fn config_file(config: &str) -> String {
    format!(
        "{{:source-paths [\"src\"]\n \
         :builds\n \
         {{:app {{:target :esm\n        \
         :output-dir \"out/app\"\n        \
         :modules {{:main {{:entries [app.main]}}}}\n        \
         :config {{:app/flavor {config} :app/size 1}}}}\n  \
         :other {{:target :esm\n          \
         :output-dir \"out/other\"\n          \
         :modules {{:main {{:entries [app.other]}}}}}}}}}}\n"
    )
}

/// The `[cache-misses-and-bypasses]` rows when every namespace of
/// `release app` misses with `reason`.
fn misses(reason: &str) -> Vec<String> {
    ["light.build", "app.macros", "app.util", "app.main"]
        .map(|name| format!("{name} miss {reason}"))
        .to_vec()
}

/// A `:config` value from `#light-cljs/env` enters the key as the value it
/// resolves to, and the variable's name, and every variable the file does
/// not read, enter nothing.
#[test]
fn environment_values_enter_the_key_as_their_values() {
    let (_dir, project) = copy("config-equivalence-env", FIXTURE);
    let release = |config: &str, env: &[(&str, &str)]| {
        fs::write(project.join("light-cljs.edn"), config_file(config)).unwrap();
        let cache = run(
            &project,
            &project,
            &strings(&["release", "app", "--stats"]),
            env,
        )
        .unwrap();
        (cache, tree(&project.join("out/app")))
    };
    let keyword = "#light-cljs/env [\"APP_FLAVOR\" :as :keyword]";
    let dev = [("APP_FLAVOR", "dev")];

    let (stored, output) = release(keyword, &dev);
    assert_eq!(
        stored.rows,
        misses("no-artifact: no artifact was published")
    );
    // The literal it resolves to, and the flag, hit what it stored.
    let (literal, same) = release(":dev", &dev);
    assert!(literal.all_hits(), "{literal:?}");
    assert!(same == output);
    let flags = run(
        &project,
        &project,
        &strings(&[
            "--compile",
            "--optimize",
            "advanced",
            "--entry",
            "app.main",
            "--source-root",
            project.join("src").to_str().unwrap(),
            "--out-dir",
            project.join("out/app").to_str().unwrap(),
            "--cache-dir",
            project.join(".light-cljs/cache").to_str().unwrap(),
            "--config",
            "app/flavor=:dev",
            "--config",
            "app/size=1",
            "--stats",
        ]),
        &[],
    )
    .unwrap();
    assert!(flags.all_hits(), "{flags:?}");
    assert!(tree(&project.join("out/app")) == output);

    // The string form gives the string "dev", a different value.
    let (string, text) = release("#light-cljs/env \"APP_FLAVOR\"", &dev);
    assert_eq!(string.rows, misses("config: config changed"));
    assert!(text != output);

    // A changed variable misses; a variable the file does not read does not.
    let (changed, prod) = release(keyword, &[("APP_FLAVOR", "prod")]);
    assert_eq!(changed.rows, misses("config: config changed"));
    assert!(prod != output);
    let (unread, same) = release(keyword, &[("APP_FLAVOR", "prod"), ("APP_UNREAD", "x")]);
    assert!(unread.all_hits(), "{unread:?}");
    assert!(same == prod);
    let (back, same) = release(keyword, &[("APP_FLAVOR", "dev"), ("APP_UNREAD", "y")]);
    assert!(back.all_hits(), "{back:?}");
    assert!(same == output);
}

/// A `:closure-defines` value from the environment changes the output and
/// no front-end key.
#[test]
fn an_environment_define_changes_no_front_end_key() {
    let (_dir, project) = copy("config-equivalence-define", FIXTURE);
    fs::write(
        project.join("light-cljs.edn"),
        "{:source-paths [\"src\"]\n \
         :builds\n \
         {:other {:target :esm\n          \
         :output-dir \"out/other\"\n          \
         :modules {:main {:entries [app.other]}}\n          \
         :release {:closure-defines {app.other/label #light-cljs/env \"APP_LABEL\"}}}}}\n",
    )
    .unwrap();
    let release = |label: &str| {
        let args = strings(&["release", "other", "--stats"]);
        let cache = run(&project, &project, &args, &[("APP_LABEL", label)]).unwrap();
        let output = tree(&project.join("out/other"));
        let text = String::from_utf8(output.iter().flat_map(|(_, bytes)| bytes.clone()).collect())
            .unwrap();
        assert!(text.contains(label), "{label}: {text}");
        (cache, output)
    };
    let (stored, first) = release("label-first");
    assert!(stored.published > 0 && stored.hits == 0, "{stored:?}");
    let (served, second) = release("label-second");
    assert!(served.all_hits(), "{served:?}");
    assert_eq!(served.hits, stored.published);
    assert!(first != second);
}

/// `release` from a subdirectory of the project hits every artifact the
/// same `release` from the project directory stored, and writes the same
/// bytes.
#[test]
fn a_release_from_a_subdirectory_hits_the_project_directorys_artifacts() {
    let (_dir, project) = copy("config-equivalence-subdirectory", FIXTURE);
    fs::write(project.join("light-cljs.edn"), config_file(":file")).unwrap();
    let args = strings(&["release", "app", "--stats"]);
    let stored = run(&project, &project, &args, &[]).unwrap();
    let output = tree(&project.join("out/app"));
    assert_eq!(
        stored.rows,
        misses("no-artifact: no artifact was published")
    );
    for dir in ["src", "src/app"] {
        let served = run(&project, &project.join(dir), &args, &[]).unwrap();
        assert!(served.all_hits(), "{dir}: {served:?}");
        assert_eq!(served.hits, stored.published, "{dir}");
        assert!(tree(&project.join("out/app")) == output, "{dir}");
    }
    assert!(!project.join("src/.light-cljs").exists());
    assert!(!project.join("src/out").exists());
}

/// Editing a comment, another build's section, settings `release` does not
/// read, or the build's `:output-dir` changes no front-end key.
#[test]
fn edits_that_change_no_compiler_input_hit_every_namespace() {
    let (_dir, project) = copy("config-equivalence-edits", FIXTURE);
    let original = config_file(":file");
    let release = |text: &str, out: &str| {
        fs::write(project.join("light-cljs.edn"), text).unwrap();
        let args = strings(&["release", "app", "--stats"]);
        let cache = run(&project, &project, &args, &[]).unwrap();
        (cache, tree(&project.join(out)))
    };
    let (stored, output) = release(&original, "out/app");
    assert!(stored.published > 0 && stored.hits == 0, "{stored:?}");
    let edit = |from: &str, to: &str| {
        assert!(original.contains(from), "{from}");
        original.replacen(from, to, 1)
    };
    for (label, text, out) in [
        (
            "a comment",
            format!(";; A comment, then another.\n{}\n;; The end.\n", original),
            "out/app",
        ),
        (
            "another build's section",
            edit(
                "[app.other]}}",
                "[app.other]}}\n          :config {:app/flavor :other}\n          \
                 :compiler-options {:elide-asserts true}",
            ),
            "out/app",
        ),
        (
            ":http, :nrepl, :fs-watch and :devtools",
            edit(
                " :builds\n",
                " :http {:port 9631}\n :nrepl {:port 0}\n :fs-watch {:loop-wait 50}\n :builds\n",
            )
            .replacen(
                ":app/size 1}",
                ":app/size 1}\n        :devtools {:reload-strategy :full}",
                1,
            ),
            "out/app",
        ),
        (
            ":output-dir",
            edit("\"out/app\"", "\"out/moved/app\""),
            "out/moved/app",
        ),
    ] {
        let (served, same) = release(&text, out);
        assert!(served.all_hits(), "{label}: {served:?}");
        assert_eq!(served.hits, stored.published, "{label}");
        assert!(same == output, "{label}: the outputs differ");
    }
}

/// The Phase 1 exit criterion: in a copy of examples/features,
/// `light-cljs release features` writes the same files, byte for byte, as
/// the equivalent flags run in the same directory with the same (default)
/// cache, and the second of the two runs is all hits, in either order.
#[test]
fn release_features_equals_its_flags() {
    let (_dir, project) = copy("config-equivalence-features", FEATURES);
    let release = strings(&["release", "features", "--stats"]);
    let flags = strings(&[
        "--compile",
        "--optimize",
        "advanced",
        "--entry",
        "app.main",
        "--source-root",
        ".",
        "--out-dir",
        "out/release",
        "--stats",
    ]);
    let mut outputs: Vec<Vec<(PathBuf, Vec<u8>)>> = Vec::new();
    for (first, second) in [(&release, &flags), (&flags, &release)] {
        let _ = fs::remove_dir_all(project.join(".light-cljs"));
        let stored = run(&project, &project, first, &[]).unwrap();
        outputs.push(tree(&project.join("out/release")));
        assert_eq!(stored.hits, 0, "{first:?}: {stored:?}");
        assert_eq!(stored.published, stored.namespaces, "{first:?}: {stored:?}");
        let served = run(&project, &project, second, &[]).unwrap();
        outputs.push(tree(&project.join("out/release")));
        assert!(served.all_hits(), "{second:?} second: {served:?}");
        assert_eq!(served.hits, stored.published, "{second:?}");
    }
    assert_eq!(
        outputs[0]
            .iter()
            .map(|(path, _)| path.to_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "app.editor+app.reports.js",
            "app.editor.js",
            "app.main.js",
            "app.reports.js",
            "chunks.json"
        ]
    );
    for output in &outputs[1..] {
        assert!(*output == outputs[0], "the outputs differ");
    }
}
