//! Conformance of the bundled `cljs.pprint` with upstream ClojureScript:
//! upstream's own `cljs.pprint-test` suite (49 tests, 434 assertions) run
//! through `light-cljs test` and compiled at `none`, `prepare`, and `advanced`
//! under the default macro limits, plus a 45-expression differential corpus
//! pinned against Maven ClojureScript 1.12.134 (light-cljs-1170).
use crate::{json::Json, temp_dir::TempDir};
use std::{
    collections::BTreeMap,
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

const DIFFERENTIAL_CORPUS_TSV: &str =
    include_str!("../fixtures/cljs-pprint/differential-corpus.tsv");
const DIFF_RESULT_PREFIX: &str = "__PPRINT_DIFF_RESULT__ ";

fn parse_differential_corpus() -> Vec<(&'static str, &'static str, String)> {
    DIFFERENTIAL_CORPUS_TSV
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|row| {
            let fields: [&str; 3] = row
                .split('\t')
                .collect::<Vec<_>>()
                .try_into()
                .unwrap_or_else(|_| panic!("expected 3 columns in differential corpus row: {row}"));
            let expected = match Json::parse(fields[2]) {
                Json::String(s) => s,
                other => panic!("expected JSON string for {}, got {other:?}", fields[0]),
            };
            (fields[0], fields[1], expected)
        })
        .collect()
}

fn run_differential_corpus_at_level(level: &str) {
    let cases = parse_differential_corpus();
    assert_eq!(cases.len(), 45);
    let mut source = String::from(
        "(ns app.pprint-diff-corpus\n\
           (:require [cljs.pprint :as pp]\n\
                     [clojure.string :as str]))\n\
         (defrecord SampleRec [x y])\n\
         (defn normalize-ref [s] (str/replace s #\"#<[^@]*@\\d+:\" \"#<REF@ID:\"))\n",
    );
    for (id, expr, _) in &cases {
        source.push_str(&format!(
            "(js/console.log \"{DIFF_RESULT_PREFIX}\" (js/JSON.stringify #js [\"{id}\" {expr}]))\n"
        ));
    }

    let dir = TempDir::new(&format!("cljs-pprint-diff-{level}"));
    let root = dir.join("src");
    write_file(&root, "app/pprint_diff_corpus.cljs", &source);
    let output = light_cljs(
        dir.path(),
        &[
            "--warnings",
            "--compile",
            "--optimize",
            level,
            "--entry",
            "app.pprint-diff-corpus",
            "--source-root",
            root.to_str().unwrap(),
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

    let mut actuals = BTreeMap::new();
    for line in printed.lines() {
        if let Some(payload) = line.strip_prefix(DIFF_RESULT_PREFIX) {
            match Json::parse(payload.trim()) {
                Json::Array(items) if items.len() == 2 => {
                    let Json::String(id) = &items[0] else {
                        panic!("expected string case id in {payload}");
                    };
                    let Json::String(val) = &items[1] else {
                        panic!("expected string output for {id}, got {:?}", items[1]);
                    };
                    actuals.insert(id.clone(), val.clone());
                }
                other => panic!("unexpected result payload {other:?}"),
            }
        }
    }
    assert_eq!(
        actuals.len(),
        cases.len(),
        "--optimize {level}: expected {} results, got {}",
        cases.len(),
        actuals.len()
    );
    for (id, _, expected) in &cases {
        let actual = actuals
            .get(*id)
            .unwrap_or_else(|| panic!("--optimize {level}: missing result for {id}"));
        assert_eq!(
            actual, expected,
            "--optimize {level}: differential mismatch for {id}"
        );
    }
}

#[test]
fn differential_corpus_matches_upstream_at_none() {
    run_differential_corpus_at_level("none");
}

#[test]
fn differential_corpus_matches_upstream_at_prepare() {
    run_differential_corpus_at_level("prepare");
}

#[test]
fn differential_corpus_matches_upstream_at_advanced() {
    run_differential_corpus_at_level("advanced");
}

/// Live differential check against Maven ClojureScript 1.12.134 via the
/// checked-in babashka script (`tests/fixtures/cljs-pprint/capture_reference.bb`).
#[ignore = "requires bb, Java, Clojure CLI, and Maven ClojureScript 1.12.134"]
#[test]
fn differential_corpus_matches_live_clojurescript_reference() {
    let script = pprint_fixture_root().join("capture_reference.bb");
    let output = Command::new("bb")
        .arg(&script)
        .arg("--check")
        .output()
        .expect("run capture_reference.bb");
    assert!(
        output.status.success(),
        "capture_reference.bb --check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    run_differential_corpus_at_level("none");
}
