//! The `compile` and `release` commands (docs/build-configuration.md,
//! "Commands", "Precedence", "Several builds in one process" and "Errors"),
//! run as processes in temporary copies of examples/features and of
//! tests/fixtures/build-configuration/commands, each with its working
//! directory and environment set explicitly.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use light_cljs::BuildConfiguration;

use crate::temp_dir::TempDir;

const FEATURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/build-configuration/commands"
);

/// A temporary copy of `source`, and its canonical path, which is how the
/// child process's current directory names it.
pub(crate) fn copy(label: &str, source: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(label);
    let project = fs::canonicalize(&*dir).unwrap().join("project");
    copy_tree(Path::new(source), &project);
    (dir, project)
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            // A build of the checkout's own example is not part of it.
            if entry.file_name() != "out" && entry.file_name() != ".light-cljs" {
                copy_tree(&entry.path(), &target);
            }
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// `light-cljs ARGS` run in `dir`, without `LIGHT_CLJS`.
fn light(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(args)
        .current_dir(dir)
        .env_remove("LIGHT_CLJS")
        .output()
        .unwrap()
}

/// Run `args` in `dir`, which must succeed, and return its stderr.
fn succeeds(dir: &Path, args: &[&str]) -> String {
    let output = light(dir, args);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{args:?}: {stderr}");
    stderr
}

/// Run `args` in `dir`, which must fail, and return its stderr.
fn fails(dir: &Path, args: &[&str]) -> String {
    let output = light(dir, args);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!output.status.success(), "{args:?} succeeded: {stderr}");
    stderr
}

/// The names of the files in `dir`, sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Every file under `dir` with its bytes, by path relative to `dir`.
pub(crate) fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((
                    path.strip_prefix(dir).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
    files.sort();
    files
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The status lines a successful command prints for `builds`: the file
/// first, then each build's start and end, with the milliseconds left out.
fn status(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .map(|line| match line.split_once(" Build completed in ") {
            Some((build, rest)) => {
                let (millis, dir) = rest.split_once(" ms: ").unwrap();
                assert!(millis.parse::<u64>().is_ok(), "{line}");
                format!("{build} Build completed: {dir}")
            }
            None => line.to_owned(),
        })
        .collect()
}

/// The release and dev builds of examples/features, from the project
/// directory and from its `app/` subdirectory, which write to the same
/// place.
#[test]
fn the_features_example_builds_from_its_directory_and_below_it() {
    let (_dir, project) = copy("config-commands-features", FEATURES);
    let file = project.join("light-cljs.edn");
    for from in [project.clone(), project.join("app")] {
        let _ = fs::remove_dir_all(project.join("out"));
        let stderr = succeeds(&from, &["release", "features"]);
        assert_eq!(
            status(&stderr),
            [
                format!("light-cljs - config: {}", file.display()),
                "[:features] Compiling ...".to_owned(),
                format!(
                    "[:features] Build completed: {}",
                    project.join("out/release").display()
                ),
            ],
            "{}",
            from.display()
        );
        assert_eq!(
            listing(&project.join("out/release")),
            [
                "app.editor+app.reports.js",
                "app.editor.js",
                "app.main.js",
                "app.reports.js",
                "chunks.json"
            ]
        );
        let manifest = text(&project.join("out/release/chunks.json"));
        assert!(
            manifest.contains("app.editor.js") && manifest.contains("app.reports.js"),
            "{manifest}"
        );

        let stderr = succeeds(&from, &["compile", ":features"]);
        assert_eq!(
            status(&stderr)[2],
            format!(
                "[:features] Build completed: {}",
                project.join("out/dev").display()
            )
        );
        let dev = listing(&project.join("out/dev"));
        for name in ["manifest.json", "index.js", "runtime.js", "app.main.js"] {
            assert!(dev.contains(&name.to_owned()), "{dev:?}");
        }
        assert!(!from.join("app/out").exists() && !project.join("app/out").exists());
        // The cache is the project directory's, wherever the command runs.
        assert!(project.join(".light-cljs/cache").is_dir());
        assert!(!project.join("app/.light-cljs").exists());
    }
}

/// The release build of examples/features is what the equivalent flags
/// write, and `--optimize none` after the command replaces the release
/// default `:advanced`.
#[test]
fn optimize_after_release_replaces_the_advanced_default() {
    let (dir, project) = copy("config-commands-optimize", FEATURES);
    let root = project.to_str().unwrap();
    let flags = |level: &str, out: &str| {
        let out = dir.join(out);
        succeeds(
            &project,
            &[
                "--compile",
                "--optimize",
                level,
                "--entry",
                "app.main",
                "--source-root",
                root,
                "--out-dir",
                out.to_str().unwrap(),
                "--no-cache",
            ],
        );
        tree(&out)
    };
    let advanced = flags("advanced", "advanced");
    let none = flags("none", "none");
    assert_ne!(advanced, none);

    succeeds(&project, &["release", "features"]);
    assert_eq!(tree(&project.join("out/release")), advanced);
    fs::remove_dir_all(project.join("out")).unwrap();
    succeeds(&project, &["release", "features", "--optimize", "none"]);
    assert_eq!(tree(&project.join("out/release")), none);
}

/// `--config KEY=EDN` after the command replaces that key of the build's
/// `:config` and keeps the others.
#[test]
fn config_after_the_command_replaces_a_config_key() {
    let (_dir, project) = copy("config-commands-config", FIXTURE);
    succeeds(&project, &["compile", "app"]);
    let unit = text(&project.join("out/app/app.main.js"));
    assert!(
        unit.contains("\":file\"") && unit.contains("\"1\""),
        "{unit}"
    );
    succeeds(
        &project,
        &["compile", "app", "--config", "app/flavor=:flag"],
    );
    let unit = text(&project.join("out/app/app.main.js"));
    assert!(
        unit.contains("\":flag\"") && !unit.contains("\":file\"") && unit.contains("\"1\""),
        "{unit}"
    );
    // A flag naming one key twice is still an error.
    assert_eq!(
        fails(
            &project,
            &[
                "compile",
                "app",
                "--config",
                "app/flavor=:a",
                "--config",
                "app/flavor=:b"
            ]
        ),
        "light-cljs compile app: --config app/flavor=:b: the key app/flavor is given twice\n"
    );
}

/// `--source-root` replaces `:source-paths`, resolved against the project
/// directory, and `--out-dir` with one build id replaces `:output-dir`.
#[test]
fn source_root_and_out_dir_replace_the_files_values() {
    let (_dir, project) = copy("config-commands-roots", FIXTURE);
    succeeds(&project, &["release", "app", "--optimize", "none"]);
    let main = text(&project.join("out/app/app.main.js"));
    assert!(
        main.contains("root-src") && !main.contains("root-alt"),
        "{main}"
    );
    fs::remove_dir_all(project.join("out")).unwrap();

    succeeds(
        &project.join("src"),
        &[
            "release",
            "app",
            "--optimize",
            "none",
            "--source-root",
            "alt",
        ],
    );
    let main = text(&project.join("out/app/app.main.js"));
    assert!(
        main.contains("root-alt") && !main.contains("root-src"),
        "{main}"
    );

    let stderr = succeeds(
        &project.join("src"),
        &["release", "app", "--out-dir", "elsewhere"],
    );
    assert_eq!(
        status(&stderr)[2],
        format!(
            "[:app] Build completed: {}",
            project.join("elsewhere").display()
        )
    );
    assert_eq!(
        listing(&project.join("elsewhere")),
        ["app.main.js", "chunks.json"]
    );
    assert!(!project.join("src/elsewhere").exists());
}

/// `--cache-blocker` adds to the file's blockers, which the top level's
/// `:cache-blockers` give the build.
#[test]
fn cache_blocker_adds_to_the_files_blockers() {
    let (_dir, project) = copy("config-commands-blockers", FIXTURE);
    let bypasses = |stderr: &str| {
        stderr
            .lines()
            .filter(|line| line.contains(" bypass "))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let stderr = succeeds(&project, &["release", "app", "--stats"]);
    assert_eq!(
        bypasses(&stderr),
        [
            "app.macros bypass blocked app.macros",
            "app.main bypass blocked app.macros > app.main"
        ]
    );
    let stderr = succeeds(
        &project,
        &["release", "app", "--stats", "--cache-blocker", "app.util"],
    );
    assert_eq!(
        bypasses(&stderr),
        [
            "app.macros bypass blocked app.macros",
            "app.util bypass blocked app.util",
            "app.main bypass blocked app.macros > app.main"
        ]
    );
}

/// `--no-cache` turns the default cache off, and `--cache-dir` names the
/// store and turns it on over `:cache-level :off`.
#[test]
fn no_cache_and_cache_dir_override_the_cache_level() {
    let (_dir, project) = copy("config-commands-cache", FIXTURE);
    succeeds(&project, &["release", "app", "--no-cache"]);
    assert!(project.join("out/app/app.main.js").is_file());
    assert!(!project.join(".light-cljs").exists());
    succeeds(&project.join("src"), &["release", "app"]);
    assert!(project.join(".light-cljs/cache").is_dir());
    fs::remove_dir_all(project.join(".light-cljs")).unwrap();

    succeeds(&project, &["release", "other"]);
    assert!(project.join("out/other/app.other.js").is_file());
    assert!(!project.join(".light-cljs").exists());
    succeeds(
        &project.join("src"),
        &["release", "other", "--cache-dir", "store"],
    );
    assert!(project.join("store").is_dir() && !project.join("src/store").exists());
    assert!(!project.join(".light-cljs").exists());

    assert_eq!(
        fails(
            &project,
            &["release", "app", "--no-cache", "--cache-dir", "store"]
        ),
        "light-cljs release app: --no-cache and --cache-dir exclude each other\n"
    );
}

/// Several ids build one after another in the order named, and the first
/// failure stops the command before the next build writes anything.
#[test]
fn several_builds_run_in_the_order_named_and_stop_at_the_first_failure() {
    let (_dir, project) = copy("config-commands-several", FIXTURE);
    let stderr = succeeds(&project, &["release", "other", "app"]);
    assert_eq!(
        status(&stderr),
        [
            format!(
                "light-cljs - config: {}",
                project.join("light-cljs.edn").display()
            ),
            "[:other] Compiling ...".to_owned(),
            format!(
                "[:other] Build completed: {}",
                project.join("out/other").display()
            ),
            "[:app] Compiling ...".to_owned(),
            format!(
                "[:app] Build completed: {}",
                project.join("out/app").display()
            ),
        ]
    );
    fs::remove_dir_all(project.join("out")).unwrap();

    let stderr = fails(&project, &["release", "broken", "app"]);
    assert_eq!(
        stderr,
        format!(
            "light-cljs - config: {}\n[:broken] Compiling ...\nlight-cljs release broken: {}:3:2: \
             unresolved symbol: no-such-function (bytes 18..34)\n",
            project.join("light-cljs.edn").display(),
            project.join("src/app/broken.cljs").display()
        )
    );
    assert!(!project.join("out").exists());
}

/// The command rows of "Errors", and the flags the commands refuse, each
/// with its exact message. Nothing is written.
#[test]
fn command_errors_have_exact_messages() {
    let (dir, project) = copy("config-commands-errors", FIXTURE);
    let file = project.join("light-cljs.edn");
    let file = file.display();
    let writes_release = "release writes a chunked build into :output-dir, as --out-dir does: \
                          ES modules without source maps";
    let writes_dev = "compile writes the dev build into :output-dir, as --reloadable does, with \
                      a source map per namespace";
    let rows: Vec<(&[&str], String)> = vec![
        (
            &["release"],
            format!(
                "light-cljs release: requires one or more build ids; {file} defines :app, \
                 :broken, :other"
            ),
        ),
        (
            &["release", "--optimize", "none"],
            format!(
                "light-cljs release: requires one or more build ids; {file} defines :app, \
                 :broken, :other"
            ),
        ),
        (
            &["release", "ap"],
            format!(
                "light-cljs release ap: {file} defines no build :ap; did you mean :app? Its \
                 builds are :app, :broken, :other"
            ),
        ),
        (
            &["compile", "app", ":xyz"],
            format!(
                "light-cljs compile app :xyz: {file} defines no build :xyz; its builds are \
                 :app, :broken, :other"
            ),
        ),
        (
            &["release", "app", "other", "--entry", "x"],
            "light-cljs release app other --entry x: --entry and --out-dir name one build's \
             entry and directory; give one build id"
                .into(),
        ),
        (
            &["compile", "app", "other", "--out-dir", "x"],
            "light-cljs compile app other --out-dir x: --entry and --out-dir name one build's \
             entry and directory; give one build id"
                .into(),
        ),
        (
            &["release", "app", "--source-map", "app.js.map"],
            format!(
                "light-cljs release app --source-map app.js.map: {writes_release}; it takes no \
                 --source-map"
            ),
        ),
        (
            &["release", "app", "--source-map-inline"],
            format!(
                "light-cljs release app --source-map-inline: {writes_release}; it takes no \
                 --source-map-inline"
            ),
        ),
        (
            &["compile", "app", "--source-map-include-sources"],
            format!(
                "light-cljs compile app --source-map-include-sources: {writes_dev}; it takes \
                 no --source-map-include-sources"
            ),
        ),
        (
            &["release", "app", "--esm"],
            format!("light-cljs release app --esm: {writes_release}; it takes no --esm"),
        ),
        (
            &["compile", "app", "--esm"],
            format!("light-cljs compile app --esm: {writes_dev}; it takes no --esm"),
        ),
        (
            &["release", "app", "--serve", "0"],
            "light-cljs release app --serve 0: --serve is a flag of light-cljs watch, which \
             serves and watches the build; release builds once"
                .into(),
        ),
        (
            &["compile", "app", "--reload-dependents"],
            "light-cljs compile app --reload-dependents: --reload-dependents is a flag of \
             light-cljs watch, which serves and watches the build; compile builds once"
                .into(),
        ),
        (
            &["compile", "app", "--optimize", "advanced"],
            "light-cljs compile app --optimize advanced: --optimize applies to release builds; \
             compile writes the dev build at none semantics, as --compile --reloadable does"
                .into(),
        ),
        (
            &["compile", "app", "--checked-arrays-warn"],
            "light-cljs compile app --checked-arrays-warn: --checked-arrays-warn applies to \
             release builds; compile writes the dev build at none semantics, as --compile \
             --reloadable does"
                .into(),
        ),
        (
            &["release", "app", "--reloadable"],
            "light-cljs release app --reloadable: the command chooses the build: compile writes \
             the dev build, as --reloadable does, and release the optimized chunks; give no \
             --reloadable"
                .into(),
        ),
        (
            &["release", "app", "--frobnicate"],
            "light-cljs release app: --frobnicate is not a flag of release; see light-cljs \
             --help"
                .into(),
        ),
        (
            &["release", "app", "--jobs", "0"],
            "light-cljs release app: --jobs requires a positive integer, got 0".into(),
        ),
        (
            &["release", "app", "--config-merge", "{:dev {}}"],
            "light-cljs release app: --config-merge 1:1:2: :dev in --config-merge: the command \
             chooses the mode (compile and watch run :dev, release runs :release), so \
             --config-merge holds the build keys themselves"
                .into(),
        ),
    ];
    for (args, message) in rows {
        assert_eq!(fails(&project, args), format!("{message}\n"), "{args:?}");
    }
    assert!(!project.join("out").exists() && !project.join(".light-cljs").exists());

    // Not found: no light-cljs.edn in the directory or above it.
    let empty = fs::canonicalize(&*dir).unwrap().join("empty");
    fs::create_dir(&empty).unwrap();
    assert_eq!(
        fails(&empty, &["release", "app"]),
        format!(
            "light-cljs release app: no light-cljs.edn in {} or any parent directory; create \
             one (docs/build-configuration.md)\n",
            empty.display()
        )
    );
    assert_eq!(listing(&empty), Vec::<String>::new());

    // The file is validated before anything else, and its errors name it.
    fs::write(
        project.join("light-cljs.edn"),
        "{:source-paths [\"src\"]\n :builds {:app {:target :esm :output-path \"out\"}}}\n",
    )
    .unwrap();
    assert_eq!(
        fails(&project.join("src"), &["release", "app"]),
        format!(
            "light-cljs release app: {file}:2:30: unknown key :output-path in build :app; did \
             you mean :output-dir?\n"
        )
    );
}

/// `release app --serve 0` fails as `--compile --serve` does: refused,
/// before anything compiles.
#[test]
fn a_watch_flag_after_release_is_refused_as_after_compile() {
    let (_dir, project) = copy("config-commands-serve", FIXTURE);
    let flags = light(
        &project,
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            "src",
            "--out-dir",
            "out/app",
            "--serve",
            "0",
        ],
    );
    assert!(!flags.status.success());
    let command = fails(&project, &["release", "app", "--serve", "0"]);
    assert!(command.ends_with("--serve is a flag of light-cljs watch, which serves and watches the build; release builds once\n"), "{command}");
    assert!(!project.join("out").exists());
}

/// Each `--config-merge` reaches the build, in order, a file one read from
/// the project directory, and a `--config` flag still wins over them.
#[test]
fn config_merges_reach_command_builds() {
    let (_dir, project) = copy("config-commands-merges", FIXTURE);
    fs::write(project.join("merge.edn"), "{:config {:app/flavor :merged}}").unwrap();
    let merges = [
        "--config-merge",
        "merge.edn",
        "--config-merge",
        "{:config {:app/size 2}}",
        "--config-merge",
        "{:config {:app/size 3}}",
    ];
    let unit = || text(&project.join("out/app/app.main.js"));
    succeeds(
        &project.join("src"),
        &[&["compile", "app"][..], &merges].concat(),
    );
    let main = unit();
    assert!(
        main.contains("\":merged\"") && main.contains("\"3\"") && !main.contains("\":file\""),
        "{main}"
    );
    succeeds(
        &project.join("src"),
        &[
            &["compile", "app"][..],
            &merges,
            &["--config", "app/flavor=:flag"],
        ]
        .concat(),
    );
    let main = unit();
    assert!(
        main.contains("\":flag\"") && main.contains("\"3\"") && !main.contains("\":merged\""),
        "{main}"
    );
}

/// Every `release` output of build `other` under `args`, file by file.
fn release_other(project: &Path, args: &[&str]) -> Vec<(PathBuf, Vec<u8>)> {
    let _ = fs::remove_dir_all(project.join("out"));
    succeeds(project, &[&["release", "other"][..], args].concat());
    tree(&project.join("out/other"))
}

/// The compiler options of a build, given here by `--config-merge` (or
/// `LIGHT_CLJS` for the top-level `:compile-threads`), reach the build
/// exactly as their flags do, and a flag after the command wins.
#[test]
fn compiler_settings_reach_the_build_as_their_flags_do() {
    let (_dir, project) = copy("config-commands-options", FIXTURE);
    for (base, merge, flag) in [
        (
            &["--optimize", "none"][..],
            "{:elide-asserts true}",
            "--elide-asserts",
        ),
        (
            &["--optimize", "none"],
            "{:checked-arrays :warn}",
            "--checked-arrays-warn",
        ),
        (&[], "{:pseudo-names true}", "--pseudo-names"),
    ] {
        let merge = format!("{{:compiler-options {merge}}}");
        let plain = release_other(&project, base);
        let merged = release_other(&project, &[base, &["--config-merge", &merge]].concat());
        let flagged = release_other(&project, &[base, &[flag]].concat());
        assert_ne!(merged, plain, "{merge}");
        assert_eq!(merged, flagged, "{merge}");
    }

    // :cljs-warnings makes the undeclared Var a warning, as --warnings does.
    let warning = "WARNING: Use of undeclared Var app.broken/no-such-function [undeclared-var] \
                   (source 0, bytes 18..34)";
    fails(&project, &["release", "broken"]);
    for args in [
        &[
            "--config-merge",
            "{:compiler-options {:cljs-warnings true}}",
        ][..],
        &["--warnings"],
    ] {
        let stderr = succeeds(&project, &[&["release", "broken"][..], args].concat());
        assert_eq!(status(&stderr)[2], warning, "{args:?}");
    }

    // A macro limit: a merge's, then the flag over it.
    let limit = "{:compiler-options {:macro-expansions 1}}";
    let stderr = fails(&project, &["compile", "app", "--config-merge", limit]);
    assert!(
        stderr.contains("compiler macro expansion limit exceeded"),
        "{stderr}"
    );
    succeeds(
        &project,
        &[
            "compile",
            "app",
            "--config-merge",
            limit,
            "--macro-expansions",
            "100000",
        ],
    );

    // :compile-threads, and --jobs over it. QuickJS-only builds have one
    // slot whatever the setting says (`parse_slots`), so no pool.
    let pool = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["release", "app", "--stats", "--no-cache"])
            .args(args)
            .current_dir(&project)
            .env("LIGHT_CLJS", "{:compile-threads 3}")
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stderr}");
        stderr.lines().any(|line| line.starts_with("pool.workers "))
    };
    assert_eq!(pool(&[]), cfg!(feature = "v8-macros"));
    assert!(!pool(&["--jobs", "1"]));
}

/// A `--define` replaces the file's define of its name; naming one define
/// twice is an error, as naming one `--config` key twice is.
#[test]
fn a_define_flag_replaces_the_files_define_and_may_not_repeat() {
    let (_dir, project) = copy("config-commands-defines", FIXTURE);
    let labels = |args: &[&str]| {
        release_other(&project, &[&["--optimize", "none"][..], args].concat());
        let main = text(&project.join("out/other/app.other.js"));
        ["label-plain", "label-file", "label-flag"]
            .into_iter()
            .filter(|label| main.contains(label))
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&[]), ["label-file"]);
    assert_eq!(
        labels(&["--define", "app.other/label=label-flag"]),
        ["label-flag"]
    );
    assert_eq!(
        fails(
            &project,
            &[
                "release",
                "other",
                "--define",
                "app.other/label=a",
                "--define",
                "app.other/label=b"
            ]
        ),
        "light-cljs release other: --define app.other/label=b: the define app.other/label is \
         given twice\n"
    );
}

/// The flags that replace a build's output directory, its source roots or
/// the cache store are applied before the directories are compared: a flag
/// may move a build into a conflict, with both origins in the message, or
/// out of one a merge made.
#[test]
fn command_flags_are_applied_before_output_validation() {
    let (_dir, project) = copy("config-commands-outputs", FIXTURE);
    for (args, message) in [
        (
            &["release", "other", "--out-dir", "out/app"][..],
            "light-cljs release other: builds :app and :other both write out/app in release \
             mode (:other's from --out-dir, :app's from light-cljs.edn:7:21)",
        ),
        (
            &["release", "app", "--source-root", "out/app"],
            "light-cljs release app: build :app writes out/app in release mode \
             (light-cljs.edn:7:21), the source path \"out/app\" (--source-root); give it a \
             directory of its own",
        ),
        (
            &["release", "app", "--cache-dir", "out/app"],
            "light-cljs release app: build :app writes out/app in release mode \
             (light-cljs.edn:7:21), the cache store of out/app (--cache-dir); give it a \
             directory of its own",
        ),
        (
            &["release", "app", "--out-dir", ".light-cljs/cache"],
            "light-cljs release app: build :app writes .light-cljs/cache in release mode \
             (--out-dir), the cache store of :cache-root \".light-cljs\" (default); give it a \
             directory of its own",
        ),
        (
            &["release", "app", "--out-dir", "src", "--no-cache"],
            "light-cljs release app: build :app writes src in release mode (--out-dir), the \
             source path \"src\" (light-cljs.edn:3:17); give it a directory of its own",
        ),
        (
            &["release", "app", "--config-merge", "{:output-dir \"src\"}"],
            "light-cljs release app: --config-merge 1:1:14: build :app writes src in release \
             mode, the source path \"src\"; give it a directory of its own",
        ),
    ] {
        assert_eq!(fails(&project, args), format!("{message}\n"), "{args:?}");
    }
    assert!(!project.join("out").exists());
    succeeds(
        &project,
        &[
            "release",
            "app",
            "--config-merge",
            "{:output-dir \"src\"}",
            "--out-dir",
            "elsewhere",
        ],
    );
    assert!(project.join("elsewhere/app.main.js").is_file());
}

/// `BuildConfiguration::set` replaces a key's value, which `insert` refuses
/// to; a malformed value leaves the map as it was.
#[test]
fn build_configuration_set_replaces_a_value() {
    let mut configuration = BuildConfiguration::new();
    configuration.insert("app/flavor", ":file").unwrap();
    configuration.insert("app/size", "1").unwrap();
    assert_eq!(
        configuration.insert("app/flavor", ":flag"),
        Err("the key app/flavor is given twice".into())
    );
    configuration.set("app/flavor", "[:flag ,  2]").unwrap();
    configuration.set("app/new", "nil").unwrap();
    assert_eq!(
        configuration.text(),
        "{:app/flavor [:flag 2] :app/new nil :app/size 1}"
    );
    let before = configuration.clone();
    assert!(configuration.set("app/flavor", "[1").is_err());
    assert!(configuration.set(":app/flavor", "1").is_err());
    assert_eq!(configuration, before);
}

/// `watch`'s errors, each exact and before anything compiles: two ids refuse
/// `--entry` and `--out-dir`, and the flags `--watch` does not take, as
/// `compile` refuses them, while the watch's own flags are read as `--watch`
/// reads them; `compile` and `release` refuse `--no-nrepl` as a watch flag.
#[test]
fn watch_errors_have_exact_messages() {
    let (_dir, project) = copy("config-commands-watch-errors", FIXTURE);
    let file = project.join("light-cljs.edn");
    let file = file.display();
    let writes_watch = "watch writes the dev build into :output-dir, as --watch does, with a \
                        source map per namespace";
    let rows: Vec<(&[&str], String)> = vec![
        (
            &["watch"],
            format!(
                "light-cljs watch: requires one or more build ids; {file} defines :app, \
                 :broken, :other"
            ),
        ),
        (
            &["watch", "ap"],
            format!(
                "light-cljs watch ap: {file} defines no build :ap; did you mean :app? Its \
                 builds are :app, :broken, :other"
            ),
        ),
        (
            &["watch", "app", "other", "--entry", "x"],
            "light-cljs watch app other --entry x: --entry and --out-dir name one build's \
             entry and directory; give one build id"
                .into(),
        ),
        (
            &["watch", "app", ":other", "--out-dir", "x", "--print-config"],
            "light-cljs watch app :other --out-dir x: --entry and --out-dir name one build's \
             entry and directory; give one build id"
                .into(),
        ),
        (
            &["watch", "app", "--optimize", "advanced"],
            "light-cljs watch app --optimize advanced: --optimize applies to release builds; \
             watch writes the dev build at none semantics, as --watch does"
                .into(),
        ),
        (
            &["watch", "app", "--define", "app.other/label=x"],
            "light-cljs watch app --define app.other/label=x: --define applies to release \
             builds; watch writes the dev build at none semantics, as --watch does"
                .into(),
        ),
        (
            &["watch", "app", "--esm"],
            format!("light-cljs watch app --esm: {writes_watch}; it takes no --esm"),
        ),
        (
            &["watch", "app", "--reloadable"],
            "light-cljs watch app --reloadable: the command chooses the build: compile writes \
             the dev build, as --reloadable does, and release the optimized chunks; give no \
             --reloadable"
                .into(),
        ),
        (
            &["watch", "app", "--jobs", "2"],
            "light-cljs watch app: --jobs is not a flag of watch; see light-cljs --help".into(),
        ),
        (
            &["watch", "app", "--stats-file", "trace.tsv"],
            "light-cljs watch app: --stats-file is not a flag of watch; see light-cljs --help"
                .into(),
        ),
        (
            &["watch", "app", "--serve", "http"],
            "light-cljs watch app: --serve requires a port number, 0 to 65535".into(),
        ),
        (
            &["watch", "app", "--nrepl", "70000"],
            "light-cljs watch app: --nrepl requires a port number, 0 to 65535".into(),
        ),
        (
            &["watch", "app", "--poll-ms", "0"],
            "light-cljs watch app: --poll-ms requires a positive integer".into(),
        ),
        (
            &["watch", "app", "--nrepl", "0", "--no-nrepl"],
            "light-cljs watch app: --nrepl and --no-nrepl exclude each other".into(),
        ),
        (
            &["watch", "app", "--no-cache", "--cache-dir", "store"],
            "light-cljs watch app: --no-cache and --cache-dir exclude each other".into(),
        ),
        (
            &["compile", "app", "--no-nrepl"],
            "light-cljs compile app --no-nrepl: --no-nrepl is a flag of light-cljs watch, \
             which serves and watches the build; compile builds once"
                .into(),
        ),
        (
            &["release", "app", "--nrepl", "0"],
            "light-cljs release app --nrepl 0: --nrepl is a flag of light-cljs watch, which \
             serves and watches the build; release builds once"
                .into(),
        ),
    ];
    for (args, message) in rows {
        assert_eq!(fails(&project, args), format!("{message}\n"), "{args:?}");
    }
    assert!(!project.join("out").exists() && !project.join(".light-cljs").exists());
}
