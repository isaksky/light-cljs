//! `--print-config` (docs/build-configuration.md, "Commands", the
//! `--print-config` bullet and its example): each build's merged settings,
//! flags included, as EDN a `light-cljs.edn` could hold, with each value's
//! origin in a comment. The commands run as processes in temporary copies of
//! the document's illustration, examples/features and
//! tests/fixtures/build-configuration/commands, and write nothing.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use light_cljs::config_file::{
    BuildMode, BuildSettings, ConfigFile, ConfigMerge, FILE_NAME, Setting, Value, ValueKind,
};

use crate::commands::{copy, tree};
use crate::temp_dir::TempDir;

const DOCUMENT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/build-configuration.md");
const FEATURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/build-configuration/commands"
);
/// Where the document's example says its project is.
const EXAMPLE_FILE: &str = "/work/site/light-cljs.edn";

/// The code block of the document after the first line that starts with
/// `marker`, with the block's own indentation removed.
fn document_block(marker: &str) -> String {
    let document = fs::read_to_string(DOCUMENT).unwrap();
    let mut lines = document
        .lines()
        .skip_while(|line| !line.trim_start().starts_with(marker));
    let fence = lines
        .find(|line| line.trim_start().starts_with("```"))
        .unwrap_or_else(|| panic!("no code block after {marker:?}"));
    let indent = fence.len() - fence.trim_start().len();
    let mut block = String::new();
    for line in lines.take_while(|line| !line.trim_start().starts_with("```")) {
        block.push_str(line.get(indent..).unwrap_or(""));
        block.push('\n');
    }
    block
}

/// The document's illustration ("A full illustration").
fn illustration() -> String {
    document_block("### A full illustration")
}

/// A temporary project holding `files`, and its canonical path, which is how
/// the child process's current directory names it.
fn project(label: &str, files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = TempDir::new(label);
    let project = fs::canonicalize(&*dir).unwrap().join("project");
    fs::create_dir_all(&project).unwrap();
    for (name, text) in files {
        fs::write(project.join(name), text).unwrap();
    }
    (dir, project)
}

/// Every path under `dir`, relative to it, sorted.
fn paths(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            found.push(path.strip_prefix(dir).unwrap().to_path_buf());
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    found.sort();
    found
}

/// `light-cljs ARGS` run in `dir` with only `environment` of the variables
/// the files read.
fn light(dir: &Path, args: &[&str], environment: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
    command
        .args(args)
        .current_dir(dir)
        .env_remove("LIGHT_CLJS")
        .env_remove("APP_VERSION")
        .env_remove("HTTP_PORT");
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().unwrap()
}

/// What `args` prints in `project` (or `from`, a directory under it): it
/// succeeds, says nothing on stderr, and writes nothing, so no output or
/// cache directory exists afterwards.
fn prints(project: &Path, from: &Path, args: &[&str], environment: &[(&str, &str)]) -> String {
    let before = paths(project);
    let output = light(from, args, environment);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{args:?}: {stderr}");
    assert_eq!(stderr, "", "{args:?}");
    assert_eq!(paths(project), before, "{args:?} wrote into the project");
    assert!(!project.join("out").exists());
    assert!(!project.join(".light-cljs").exists());
    stdout
}

/// `expected`, whose blocks name the file `light-cljs.edn` of `PROJECT`, for
/// the file of `project`.
fn located(expected: &str, project: &Path) -> String {
    expected.replace(
        "PROJECT/light-cljs.edn",
        &project.join(FILE_NAME).display().to_string(),
    )
}

/// The acceptance's golden test: the document's illustration, with
/// `APP_VERSION=2.4.0`, and `light-cljs release app --optimize none
/// --print-config` print the document's example exactly.
#[test]
fn the_illustration_prints_the_documents_example() {
    let (_dir, project) = project("print-config-illustration", &[(FILE_NAME, &illustration())]);
    let stdout = prints(
        &project,
        &project,
        &["release", "app", "--optimize", "none", "--print-config"],
        &[("APP_VERSION", "2.4.0")],
    );
    let example = document_block("- **`--print-config`** prints");
    assert!(
        example.starts_with(";; build :app, release mode, from "),
        "{example}"
    );
    let expected = example.replace(EXAMPLE_FILE, &project.join(FILE_NAME).display().to_string());
    assert_eq!(stdout, expected);
}

const FEATURES_RELEASE: &str = "\
;; build :features, release mode, from PROJECT/light-cljs.edn
{:source-paths [\".\"]                                 ; light-cljs.edn:7:16
 :cache-root \".light-cljs\"                           ; default
 :cache-blockers #{}                                 ; default
 :compile-threads 1                                  ; default
 :builds
 {:features
  {:target :esm                                      ; light-cljs.edn:10:12
   :output-dir \"out/release\"                         ; light-cljs.edn:11:16
   :modules {:main {:entries [app.main]}}            ; light-cljs.edn:12:13
   :compiler-options {:optimizations :advanced}      ; default
   :build-options {:cache-level :all}}}}             ; default
";

const FEATURES_DEV: &str = "\
;; build :features, dev mode, from PROJECT/light-cljs.edn
{:source-paths [\".\"]                                 ; light-cljs.edn:7:16
 :cache-root \".light-cljs\"                           ; default
 :cache-blockers #{}                                 ; default
 :compile-threads 1                                  ; default
 :builds
 {:features
  {:target :esm                                      ; light-cljs.edn:10:12
   :output-dir \"out/dev\"                             ; light-cljs.edn:13:22 (:dev)
   :modules {:main {:entries [app.main]}}            ; light-cljs.edn:12:13
   :build-options {:cache-level :all}}}}             ; default
";

/// `release features` and `compile features` on a copy of
/// examples/features, from its directory and from below it.
#[test]
fn the_features_example_prints_its_release_and_dev_configuration() {
    let (_dir, project) = copy("print-config-features", FEATURES);
    for from in [project.clone(), project.join("app")] {
        assert_eq!(
            prints(
                &project,
                &from,
                &["release", "features", "--print-config"],
                &[]
            ),
            located(FEATURES_RELEASE, &project)
        );
        assert_eq!(
            prints(
                &project,
                &from,
                &["compile", ":features", "--print-config"],
                &[]
            ),
            located(FEATURES_DEV, &project)
        );
    }
}

const COMMANDS_APP_AND_OTHER: &str = "\
;; build :app, release mode, from PROJECT/light-cljs.edn
{:source-paths [\"src\"]                               ; light-cljs.edn:3:16
 :cache-root \".light-cljs\"                           ; default
 :cache-blockers #{app.macros}                       ; light-cljs.edn:4:18
 :compile-threads 1                                  ; default
 :builds
 {:app
  {:target :esm                                      ; light-cljs.edn:6:17
   :output-dir \"out/app\"                             ; light-cljs.edn:7:21
   :modules {:main {:entries [app.main]}}            ; light-cljs.edn:8:18
   :config {:app/flavor :file                        ; light-cljs.edn:9:30
            :app/size 1}                             ; light-cljs.edn:9:46
   :compiler-options {:optimizations :advanced}      ; default
   :build-options {:cache-level :all}}}}             ; default

;; build :other, release mode, from PROJECT/light-cljs.edn
{:source-paths [\"src\"]                               ; light-cljs.edn:3:16
 :cache-root \".light-cljs\"                           ; default
 :cache-blockers #{app.macros}                       ; light-cljs.edn:4:18
 :compile-threads 1                                  ; default
 :builds
 {:other
  {:target :esm                                      ; light-cljs.edn:13:19
   :output-dir \"out/other\"                           ; light-cljs.edn:14:23
   :modules {:main {:entries [app.other]}}           ; light-cljs.edn:15:20
   :compiler-options {:optimizations :advanced}      ; default
   :build-options {:cache-level :off}                ; light-cljs.edn:16:40
   :release {:closure-defines {app.other/label \"label-file\"}}}}} ; light-cljs.edn:17:55 (:release)
";

/// Two ids print a block each, in the order named, a repeated one once.
#[test]
fn two_ids_print_a_block_each() {
    let (_dir, project) = copy("print-config-two", FIXTURE);
    assert_eq!(
        prints(
            &project,
            &project,
            &["release", "app", "other", "app", "--print-config"],
            &[]
        ),
        located(COMMANDS_APP_AND_OTHER, &project)
    );
    let (app, other) = COMMANDS_APP_AND_OTHER.split_once("\n\n").unwrap();
    assert_eq!(
        prints(
            &project,
            &project,
            &["release", "other", "app", "--print-config"],
            &[]
        ),
        located(&format!("{other}\n{app}\n"), &project)
    );
}

const COMMANDS_FLAGS: &str = "\
;; build :app, release mode, from PROJECT/light-cljs.edn
{:source-paths [\"src\" \"alt\"]                         ; --source-root src --source-root alt, over [\"src\"] at light-cljs.edn:3:16
 :cache-root \"tmp\"                                   ; --cache-dir tmp/cache, over the default \".light-cljs\"
 :cache-blockers #{app.macros}                       ; light-cljs.edn:4:18
 :compile-threads 2                                  ; --jobs 2, over the default 1
 :builds
 {:app
  {:target :esm                                      ; light-cljs.edn:6:17
   :output-dir \"out/x\"                               ; --out-dir out/x, over \"out/app\" at light-cljs.edn:7:21
   :modules {:main {:entries [app.other]}}           ; --entry app.other, over {:main {:entries [app.main]}} at light-cljs.edn:8:18
   :config {:app/flavor :flag                        ; --config app/flavor=:flag, over :file at light-cljs.edn:9:30
            :app/new [1 \"two\"]                       ; --config app/new=[1 \"two\"]
            :app/size 1}                             ; light-cljs.edn:9:46
   :compiler-options {:optimizations :prepare        ; --optimize prepare, over the release default :advanced
                      :pseudo-names true             ; --pseudo-names, over the default false
                      :elide-asserts true            ; --elide-asserts, over the default false
                      :cljs-warnings true            ; --warnings, over the release default false
                      :macro-nodes 5                 ; --macro-nodes 5, over the default 100000
                      :macro-poll-budget 7}          ; --macro-poll-budget 7, over the default 1000000
   :build-options {:cache-level :all                 ; default
                   :cache-blockers #{app.util}}      ; --cache-blocker app.util, added to #{app.macros} at light-cljs.edn:4:18
   :release {:compiler-options {:checked-arrays :warn} ; --checked-arrays-warn
             :closure-defines {app.other/count 3     ; --define app.other/count=3
                               app.other/label \"x y\" ; --define app.other/label=x y
                               app.other/on true}}}}} ; --define app.other/on=true

;; build :other, release mode, from PROJECT/light-cljs.edn
{:source-paths [\"src\"]                               ; light-cljs.edn:3:16
 :cache-root \"store\"                                 ; --cache-dir store/cache, over the default \".light-cljs\"
 :cache-blockers #{app.macros}                       ; light-cljs.edn:4:18
 :compile-threads 1                                  ; default
 :builds
 {:other
  {:target :esm                                      ; light-cljs.edn:13:19
   :output-dir \"out/other\"                           ; light-cljs.edn:14:23
   :modules {:main {:entries [app.other]}}           ; light-cljs.edn:15:20
   :config {:app/flavor :merged}                     ; --config-merge 1:1:23
   :compiler-options {:optimizations :advanced}      ; default
   :build-options {:cache-level :all}                ; --cache-dir store/cache, over :off at light-cljs.edn:16:40
   :release {:closure-defines {app.other/label \"flagged\"}}}}} ; --define app.other/label=flagged, over \"label-file\" at light-cljs.edn:17:55 (:release)
";

/// A flag after the ids is the origin of the setting it gives, saying what
/// it replaced or added to; `--config` and `--define` give one entry each.
#[test]
fn flags_print_as_the_origin_of_their_settings() {
    let (_dir, project) = copy("print-config-flags", FIXTURE);
    let app = prints(
        &project,
        &project,
        &[
            "release",
            "app",
            "--config",
            "app/flavor=:flag",
            "--config",
            "app/new=[1 \"two\"]",
            "--jobs",
            "2",
            "--cache-blocker",
            "app.util",
            "--cache-blocker",
            "app.macros",
            "--out-dir",
            "out/x",
            "--entry",
            "app.other",
            "--source-root",
            "src",
            "--source-root",
            "alt",
            "--macro-nodes",
            "5",
            "--macro-poll-budget",
            "7",
            "--elide-asserts",
            "--warnings",
            "--checked-arrays-warn",
            "--pseudo-names",
            "--optimize",
            "prepare",
            "--define",
            "app.other/on=true",
            "--define",
            "app.other/label=x y",
            "--define",
            "app.other/count=3",
            "--cache-dir",
            "tmp/cache",
            "--print-config",
        ],
        &[],
    );
    let other = prints(
        &project,
        &project,
        &[
            "release",
            "other",
            "--config-merge",
            "{:config {:app/flavor :merged}}",
            "--define",
            "app.other/label=flagged",
            "--cache-dir",
            "store/cache",
            "--print-config",
        ],
        &[],
    );
    assert_eq!(format!("{app}\n{other}"), located(COMMANDS_FLAGS, &project));
}

/// The settings a command reads, which `--print-config` prints: release
/// reads neither the watch's settings nor `:closure-defines` under
/// `:compiler-options`, which is merged into the build's own; dev mode reads
/// no optimization, define, checked-arrays or warning setting either.
fn read_by(mode: BuildMode) -> Vec<Setting> {
    let watch = [
        Setting::HttpPort,
        Setting::HttpHost,
        Setting::HttpStrict,
        Setting::Nrepl,
        Setting::FsWatchImpl,
        Setting::FsWatchLoopWait,
        Setting::ReloadStrategy,
        Setting::CompilerClosureDefines,
    ];
    let release_only = [
        Setting::Optimizations,
        Setting::PseudoNames,
        Setting::CheckedArrays,
        Setting::CljsWarnings,
        Setting::ClosureDefines,
    ];
    Setting::ALL
        .into_iter()
        .filter(|setting| !watch.contains(setting))
        .filter(|setting| mode == BuildMode::Release || !release_only.contains(setting))
        .collect()
}

/// A value's EDN with the entries of maps and the elements of sets sorted,
/// which are the same value in any order.
fn normal(value: &Value) -> String {
    let sorted = |mut texts: Vec<String>| {
        texts.sort();
        texts.join(" ")
    };
    match &value.kind {
        ValueKind::Vector(items) => format!(
            "[{}]",
            items.iter().map(normal).collect::<Vec<_>>().join(" ")
        ),
        ValueKind::Set(items) => format!("#{{{}}}", sorted(items.iter().map(normal).collect())),
        ValueKind::Map(entries) => format!(
            "{{{}}}",
            sorted(
                entries
                    .iter()
                    .map(|(key, value)| format!("{} {}", normal(key), normal(value)))
                    .collect()
            )
        ),
        _ => value.to_edn(),
    }
}

/// Each setting `mode` reads, normalized.
fn values(settings: &BuildSettings) -> Vec<(Setting, Option<String>)> {
    read_by(settings.mode())
        .into_iter()
        .map(|setting| (setting, settings.get(setting).map(normal)))
        .collect()
}

/// The lines of a block without their comments.
fn code(block: &str) -> Vec<&str> {
    block
        .lines()
        .filter(|line| !line.starts_with(";;"))
        .map(|line| line.split(" ; ").next().unwrap().trim_end())
        .collect()
}

/// Read `block`, one build printed by `--print-config`, back as a
/// `light-cljs.edn`, comments and all, and return its build's settings for
/// `mode` and how they print.
fn read_back(block: &str, mode: BuildMode) -> (BuildSettings, String) {
    let header = block.lines().next().unwrap();
    let id = header
        .strip_prefix(";; build :")
        .and_then(|rest| rest.split_once(','))
        .unwrap()
        .0;
    let (_dir, project) = project("print-config-read-back", &[(FILE_NAME, block)]);
    let file = ConfigFile::read(&project.join(FILE_NAME), &|_| None)
        .unwrap_or_else(|error| panic!("{error}\n{block}"));
    let build = file.find_build(id).unwrap();
    let settings = file
        .settings(build, mode, &[])
        .unwrap_or_else(|error| panic!("{error}\n{block}"));
    let printed = file.print_config(&settings, &[], false);
    (settings, printed)
}

/// A project, a command with its flags, its environment, and whether it
/// has flags after the ids.
type Case<'a> = (&'a Path, Vec<&'a str>, &'a [(&'a str, &'a str)], bool);

/// The round trip of the acceptance: the printed EDN, stripped of nothing,
/// read back with `ConfigFile::read`, gives a build whose settings equal the
/// printed ones. Without flags, they equal the original build's settings;
/// with flags, the build read back prints the same EDN, from the file alone.
#[test]
fn the_printed_configuration_reads_back_as_the_same_settings() {
    let (_dir, features) = copy("print-config-round-features", FEATURES);
    let (_dir, fixture) = copy("print-config-round-commands", FIXTURE);
    let (_dir, illustrated) = project(
        "print-config-round-illustration",
        &[(FILE_NAME, &illustration())],
    );
    let version = [("APP_VERSION", "2.4.0")];
    let cases: Vec<Case> = vec![
        (&features, vec!["release", "features"], &[], false),
        (&features, vec!["compile", "features"], &[], false),
        (&fixture, vec!["release", "app", "other"], &[], false),
        (&fixture, vec!["compile", "app", "other"], &[], false),
        (
            &illustrated,
            vec!["release", "app", "worker", "test"],
            &version,
            false,
        ),
        (
            &illustrated,
            vec!["compile", "app", "worker", "test"],
            &version,
            false,
        ),
        (
            &illustrated,
            vec!["release", "app", "--optimize", "none"],
            &version,
            true,
        ),
        (
            &fixture,
            vec![
                "release",
                "app",
                "--config",
                "app/flavor=:flag",
                "--define",
                "app.other/label=x",
                "--cache-blocker",
                "app.util",
                "--elide-asserts",
                "--jobs",
                "3",
                "--source-root",
                "alt",
            ],
            &[],
            true,
        ),
        (
            &fixture,
            vec![
                "compile",
                "app",
                "--elide-asserts",
                "--macro-expansions",
                "9",
                "--no-cache",
            ],
            &[],
            true,
        ),
    ];
    for (project, args, environment, flagged) in cases {
        let mode = if args[0] == "release" {
            BuildMode::Release
        } else {
            BuildMode::Dev
        };
        let mut command = args.clone();
        command.push("--print-config");
        let stdout = prints(project, project, &command, environment);
        let original = ConfigFile::read(&project.join(FILE_NAME), &|name| {
            environment
                .iter()
                .find(|(variable, _)| *variable == name)
                .map(|(_, value)| value.to_string())
        })
        .unwrap();
        for block in stdout.split("\n\n") {
            let block = format!("{}\n", block.trim_end());
            let (settings, printed) = read_back(&block, mode);
            assert_eq!(code(&printed), code(&block), "{args:?}");
            if !flagged {
                let build = original.find_build(settings.name()).unwrap();
                let before = original.settings(build, mode, &[]).unwrap();
                assert_eq!(values(&settings), values(&before), "{args:?}");
            }
        }
    }
}

/// What the printed configuration says runs: the build read back from it
/// writes the same files as the command with its flags.
#[test]
fn the_printed_configuration_builds_what_its_flags_build() {
    let cases: [&[&str]; 2] = [
        &[
            "release",
            "other",
            "--optimize",
            "none",
            "--define",
            "app.other/label=flagged",
            "--elide-asserts",
        ],
        &[
            "release",
            "app",
            "--config",
            "app/flavor=:flag",
            "--config",
            "app/extra=2",
            "--cache-blocker",
            "app.util",
            "--pseudo-names",
        ],
    ];
    for args in cases {
        let (_dir, flagged) = copy("print-config-build-flags", FIXTURE);
        let mut command = args.to_vec();
        command.push("--print-config");
        let printed = prints(&flagged, &flagged, &command, &[]);
        let output = light(&flagged, args, &[]);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let (_dir, from_file) = copy("print-config-build-file", FIXTURE);
        fs::write(from_file.join(FILE_NAME), &printed).unwrap();
        let output = light(&from_file, &args[..2], &[]);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let out = Path::new("out").join(args[1]);
        let written = tree(&flagged.join(&out));
        assert!(!written.is_empty());
        assert_eq!(tree(&from_file.join(&out)), written, "{args:?}");
    }
}

/// The library prints a dev build for `watch` with the settings the watch
/// reads too (light-cljs-403 runs it): the dev server, nREPL, the poller and
/// reloading, with an unset variable's default named.
#[test]
fn a_watch_build_prints_the_dev_server_and_reloading_settings() {
    let file = ConfigFile::from_source(
        Path::new("/work/site/light-cljs.edn"),
        illustration(),
        &|_| None,
    )
    .unwrap();
    let build = file.find_build("app").unwrap();
    let settings = file.settings(build, BuildMode::Dev, &[]).unwrap();
    assert_eq!(
        file.print_config(&settings, &[], true),
        "\
;; build :app, dev mode, from /work/site/light-cljs.edn
{:source-paths [\"src\" \"test\"]                        ; light-cljs.edn:1:16
 :cache-root \".light-cljs\"                           ; default
 :cache-blockers #{app.build-info}                   ; light-cljs.edn:2:18
 :compile-threads 1                                  ; default
 :http {:port 9630                                   ; light-cljs.edn:3:15, HTTP_PORT unset
        :host \"127.0.0.1\"}                           ; default
 :nrepl {:port 0}                                    ; light-cljs.edn:4:9
 :fs-watch {:loop-wait 100}                          ; default
 :builds
 {:app
  {:target :esm                                      ; light-cljs.edn:7:12
   :output-dir \"out/app\"                             ; light-cljs.edn:8:16
   :modules {:main {:entries [app.main]}}            ; light-cljs.edn:9:13
   :config {:app/api-origin \"http://127.0.0.1:8080\"} ; light-cljs.edn:13:35 (:dev)
   :compiler-options {:macro-timeout-ms 30000}       ; light-cljs.edn:11:41
   :build-options {:cache-level :all}                ; default
   :devtools {:reload-strategy :full}}}}             ; light-cljs.edn:12:32
"
    );
}

/// A store `--cache-dir` names that is no `:cache-root`'s `cache` directory
/// cannot print, since the printed file would name another store; a line
/// break in a flag's value stays inside its comment, so the block reads back.
#[test]
fn a_store_no_cache_root_names_is_refused_and_line_breaks_stay_in_comments() {
    let (_dir, project) = copy("print-config-refused", FIXTURE);
    let before = paths(&project);
    let output = light(
        &project,
        &["release", "other", "--cache-dir", "store", "--print-config"],
        &[],
    );
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "light-cljs release other: --cache-dir store: a light-cljs.edn names its store as \
         :cache-root's cache directory, which store is not, so --print-config cannot print it; \
         give --cache-dir a directory named cache\n"
    );
    assert_eq!(paths(&project), before);

    let block = prints(
        &project,
        &project,
        &[
            "release",
            "other",
            "--define",
            "app.other/label=first\nsecond",
            "--print-config",
        ],
        &[],
    );
    assert_eq!(
        block.lines().last().unwrap(),
        "   :release {:closure-defines {app.other/label \"first\\nsecond\"}}}}} ; --define \
         app.other/label=first\\nsecond, over \"label-file\" at light-cljs.edn:17:55 (:release)"
    );
    let (settings, printed) = read_back(&block, BuildMode::Release);
    assert_eq!(code(&printed), code(&block));
    assert_eq!(
        settings.get(Setting::ClosureDefines).map(Value::to_edn),
        Some("{app.other/label \"first\\nsecond\"}".to_owned())
    );
}

/// Each entry kept from a different layer prints its own origin: the file,
/// a mode map with an environment value, `LIGHT_CLJS`, two `--config-merge`s
/// and a flag.
#[test]
fn each_layer_of_a_merged_map_prints_as_its_entries_origin() {
    let source = "\
{:source-paths [\"src\"]
 :builds
 {:app {:target :esm
        :output-dir \"out/app\"
        :modules {:main {:entries [app.main]}}
        :config {:app/file 1 :app/replaced 0}
        :release {:config {:app/mode #light-cljs/env [\"MODE\" \"none\"]}}}}}
";
    let environment = |name: &str| match name {
        "MODE" => Some("fast".to_owned()),
        "LIGHT_CLJS" => Some("{:builds {:app {:config {:app/light 2}}}}".to_owned()),
        _ => None,
    };
    let path = Path::new("/work/site/light-cljs.edn");
    let file = ConfigFile::from_source(path, source.to_owned(), &environment).unwrap();
    let merges = [
        ConfigMerge::parse(
            1,
            "{:config {:app/one 1}}",
            file.project_dir(),
            &environment,
        )
        .unwrap(),
        ConfigMerge::parse(
            2,
            "{:config {:app/two 2 :app/replaced 3}}",
            file.project_dir(),
            &environment,
        )
        .unwrap(),
    ];
    let build = file.find_build("app").unwrap();
    let mut settings = file.settings(build, BuildMode::Release, &merges).unwrap();
    settings.set_entry_by_flag(
        &file,
        &merges,
        Setting::Config,
        Value::parse(":app/two").unwrap(),
        Value::parse("9").unwrap(),
        "--config app/two=9",
    );
    assert_eq!(
        file.print_config(&settings, &merges, false),
        "\
;; build :app, release mode, from /work/site/light-cljs.edn
{:source-paths [\"src\"]                               ; light-cljs.edn:1:16
 :cache-root \".light-cljs\"                           ; default
 :cache-blockers #{}                                 ; default
 :compile-threads 1                                  ; default
 :builds
 {:app
  {:target :esm                                      ; light-cljs.edn:3:17
   :output-dir \"out/app\"                             ; light-cljs.edn:4:21
   :modules {:main {:entries [app.main]}}            ; light-cljs.edn:5:18
   :config {:app/file 1                              ; light-cljs.edn:6:28
            :app/light 2                             ; LIGHT_CLJS:1:37
            :app/mode \"fast\"                         ; light-cljs.edn:7:38 (:release), from MODE
            :app/one 1                               ; --config-merge 1:1:20
            :app/replaced 3                          ; --config-merge 2:1:36
            :app/two 9}                              ; --config app/two=9, over 2 at --config-merge 2:1:20
   :compiler-options {:optimizations :advanced}      ; default
   :build-options {:cache-level :all}}}}             ; default
"
    );
}
