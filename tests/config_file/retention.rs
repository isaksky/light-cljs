//! The top-level `:cache-retention N` and `--cache-retention N`
//! (docs/build-configuration.md, "As built (light-cljs-409)"): the
//! artifacts the shared store keeps per namespace, which the Phase 3
//! measurement found builds of one file exceed at the default three. Run as
//! processes in temporary copies of examples/features.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use crate::commands::copy;

const FEATURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");

/// Every namespace of examples/features, which every build of it stores.
const NAMESPACES: [&str; 6] = [
    "app.editor",
    "app.main",
    "app.reports",
    "app.shared",
    "clojure.string",
    "light.lazy",
];

/// `light-cljs ARGS` run in `dir`, without `LIGHT_CLJS`.
fn light(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(args)
        .current_dir(dir)
        .env_remove("LIGHT_CLJS")
        .output()
        .unwrap()
}

/// Run `args` in `dir` with `--stats`, which must succeed, and return its
/// cache hits and misses.
fn build(dir: &Path, args: &[&str]) -> (u64, u64) {
    let output = light(dir, &[args, &["--stats"]].concat());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{args:?}: {stderr}");
    let stat = |key: &str| {
        stderr
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(' ')?.parse().ok())
            .unwrap_or_else(|| panic!("{args:?}: no {key} in {stderr}"))
    };
    (stat("cache.hits"), stat("cache.misses"))
}

/// The candidates the project's store keeps for each namespace.
fn kept(project: &Path) -> Vec<usize> {
    let store = light_cljs::artifact_store::Store::open(project.join(".light-cljs/cache"));
    NAMESPACES
        .iter()
        .map(|namespace| store.candidates(namespace).len())
        .collect()
}

/// examples/features's file with `:cache-retention VALUE` after its
/// `:source-paths`, at line 8, column 19.
fn with_retention(project: &Path, value: &str) {
    let path = project.join("light-cljs.edn");
    let text = fs::read_to_string(&path).unwrap();
    let marker = "{:source-paths [\".\"]\n";
    assert_eq!(text.matches(marker).count(), 1);
    let text = text.replace(marker, &format!("{marker} :cache-retention {value}\n"));
    fs::write(&path, text).unwrap();
}

/// The dev and release builds of one project have two front-end keys per
/// namespace. With `:cache-retention 1` the store keeps one, so `compile`
/// after `release` misses every namespace; `--cache-retention 2` after the
/// command wins over the key and keeps both, so the next `compile` hits
/// every namespace; and without either the store keeps three.
#[test]
fn cache_retention_bounds_the_shared_store_and_the_flag_wins() {
    let (_dir, project) = copy("config-retention-features", FEATURES);
    with_retention(&project, "1");
    assert_eq!(build(&project, &["compile", "features"]), (0, 6));
    assert_eq!(build(&project, &["release", "features"]), (0, 6));
    assert_eq!(kept(&project), [1; 6]);
    assert_eq!(build(&project, &["compile", "features"]), (0, 6));
    assert_eq!(kept(&project), [1; 6]);

    let flag = ["--cache-retention", "2"];
    assert_eq!(
        build(&project, &[&["release", "features"][..], &flag].concat()),
        (0, 6)
    );
    assert_eq!(kept(&project), [2; 6]);
    assert_eq!(
        build(&project, &[&["compile", "features"][..], &flag].concat()),
        (6, 0)
    );
    assert_eq!(
        build(&project, &[&["release", "features"][..], &flag].concat()),
        (6, 0)
    );
    assert_eq!(kept(&project), [2; 6]);

    // The checkout's own file, without the key: three, as before.
    let (_dir, plain) = copy("config-retention-plain", FEATURES);
    assert_eq!(build(&plain, &["compile", "features"]), (0, 6));
    assert_eq!(build(&plain, &["release", "features"]), (0, 6));
    assert_eq!(
        build(&plain, &["compile", "features", "--config", "app/n=1"]),
        (0, 6)
    );
    assert_eq!(
        build(&plain, &["compile", "features", "--config", "app/n=2"]),
        (0, 6)
    );
    assert_eq!(kept(&plain), [3; 6]);
}

/// `--print-config` prints `:cache-retention` only when a layer or a flag
/// gives it, so a file without it prints as before; a flag says what it
/// replaced.
#[test]
fn print_config_shows_cache_retention_with_its_origin() {
    let (_dir, project) = copy("config-retention-print", FEATURES);
    let print = |args: &[&str]| {
        let output = light(&project, &[args, &["--print-config"]].concat());
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .replace(&project.display().to_string(), "PROJECT")
    };
    let top = |retention: &str| {
        format!(
            ";; build :features, dev mode, from PROJECT/light-cljs.edn
{{:source-paths [\".\"]                                 ; light-cljs.edn:7:16
 :cache-root \".light-cljs\"                           ; default
 :cache-blockers #{{}}                                 ; default
 :compile-threads 1                                  ; default
{retention} :builds
"
        )
    };
    let head = |text: &str| {
        let end = text.find(" :builds\n").unwrap() + " :builds\n".len();
        text[..end].to_owned()
    };
    assert_eq!(head(&print(&["compile", "features"])), top(""));
    assert_eq!(
        head(&print(&["compile", "features", "--cache-retention", "4"])),
        top(
            " :cache-retention 4                                  ; --cache-retention 4, over the default 3\n"
        )
    );
    with_retention(&project, "5");
    assert_eq!(
        head(&print(&["compile", "features"])),
        top(" :cache-retention 5                                  ; light-cljs.edn:8:19\n")
    );
    assert_eq!(
        head(&print(&["release", "features", "--cache-retention", "6"])),
        top(" :cache-retention 6                                  ; --cache-retention 6, over 5 at light-cljs.edn:8:19\n")
            .replace("dev mode", "release mode")
    );
}

/// `:cache-retention` is validated as the other top-level keys are: a
/// wrong type or zero is an error at its line and column, before anything
/// builds.
#[test]
fn a_bad_cache_retention_is_located_and_nothing_builds() {
    let (_dir, project) = copy("config-retention-errors", FEATURES);
    for (value, got) in [("0", "0"), ("\"3\"", "\"3\""), ("[3]", "[3]")] {
        with_retention(&project, value);
        let output = light(&project, &["compile", "features"]);
        assert!(!output.status.success(), "{value}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!(
                "light-cljs compile features: {}/light-cljs.edn:8:19: :cache-retention must be \
                 a positive integer, got {got}\n",
                project.display()
            ),
            "{value}"
        );
        assert!(!project.join("out").exists() && !project.join(".light-cljs").exists());
        // Back to the checkout's file for the next row.
        fs::copy(
            Path::new(FEATURES).join("light-cljs.edn"),
            project.join("light-cljs.edn"),
        )
        .unwrap();
    }
}
