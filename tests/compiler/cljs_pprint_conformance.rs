//! Conformance of the bundled `cljs.pprint` with upstream ClojureScript:
//! upstream's own `cljs.pprint-test` suite (49 tests, 434 assertions) run
//! through `light-cljs test` and compiled at `none`, `prepare`, and `advanced`
//! under the default macro limits (light-cljs-1170).
use crate::temp_dir::TempDir;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn light_cljs(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn write_file(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn pprint_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cljs-pprint")
}

fn run_upstream_pprint_suite_at_level(level: &str) {
    let dir = TempDir::new(&format!("cljs-pprint-upstream-{level}"));
    let runner_root = dir.join("runner");
    write_file(
        &runner_root,
        "cljs/pprint_conformance_runner.cljs",
        "(ns cljs.pprint-conformance-runner\n\
           (:require [cljs.test :as t]\n\
                     [cljs.pprint-test]))\n\
         (enable-console-print!)\n\
         (defmethod t/report [::t/default :end-run-tests] [m]\n\
           (prn :summary (dissoc m :type)))\n\
         (t/run-tests 'cljs.pprint-test)\n",
    );
    let fixture_root = pprint_fixture_root();
    let output = light_cljs(
        dir.path(),
        &[
            "--warnings",
            "--compile",
            "--optimize",
            level,
            "--entry",
            "cljs.pprint-conformance-runner",
            "--source-root",
            fixture_root.to_str().unwrap(),
            "--source-root",
            runner_root.to_str().unwrap(),
            "--no-cache",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "--optimize {level} emitted unexpected stderr:\n{stderr}"
    );
    let js = stdout(output);
    let printed = crate::canonical_json::node_stdout(&js);
    assert_eq!(
        printed,
        "\nTesting cljs.pprint-test\n\nRan 49 tests containing 434 assertions.\n\
         0 failures, 0 errors.\n:summary {:test 49, :pass 434, :fail 0, :error 0}\n",
        "--optimize {level} test output mismatch"
    );
}

#[test]
fn upstream_pprint_suite_passes_under_light_cljs_test() {
    let dir = TempDir::new("cljs-pprint-upstream-test-cmd");
    let fixture_root = pprint_fixture_root();
    let output = light_cljs(
        dir.path(),
        &[
            "test",
            "--source-root",
            fixture_root.to_str().unwrap(),
            "--no-cache",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "light-cljs test emitted unexpected stderr:\n{stderr}"
    );
    let printed = stdout(output);
    assert_eq!(
        printed,
        "\nTesting cljs.pprint-test\n\nRan 49 tests containing 434 assertions.\n\
         0 failures, 0 errors.\n"
    );
}

#[test]
fn upstream_pprint_suite_passes_at_none() {
    run_upstream_pprint_suite_at_level("none");
}

#[test]
fn upstream_pprint_suite_passes_at_prepare() {
    run_upstream_pprint_suite_at_level("prepare");
}

#[test]
fn upstream_pprint_suite_passes_at_advanced() {
    run_upstream_pprint_suite_at_level("advanced");
}
