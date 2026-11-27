//! The `:node-test` and `:browser-test` targets (docs/build-configuration.md,
//! "Test builds"), run as `compile` and `release` processes in temporary
//! copies of examples/testing: the generated runner, Node's exit status with
//! `:autorun` and without it, `:ns-regexp`, and the errors of the keys a
//! target does not take.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use crate::commands::copy;

const TESTING: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/testing");

/// `light-cljs ARGS` run in `dir`, without `LIGHT_CLJS`.
fn light(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(args)
        .current_dir(dir)
        .env_remove("LIGHT_CLJS")
        .output()
        .unwrap()
}

/// `node FILE` in `dir`: its exit code and stdout.
fn node(dir: &Path, file: &str) -> (Option<i32>, String) {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(file)
        .current_dir(dir)
        .output()
        .expect("the test build tests require Node");
    (
        output.status.code(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

/// The stdout and stderr of `output`, as text.
fn texts(output: &Output) -> (String, String) {
    (
        String::from_utf8(output.stdout.clone()).unwrap(),
        String::from_utf8(output.stderr.clone()).unwrap(),
    )
}

const PASSED: &str =
    "\nTesting example.cart-test\n\nRan 3 tests containing 5 assertions.\n0 failures, 0 errors.\n";

/// A test namespace with a failing assertion, a failing assertion in a timer
/// callback and a test that throws.
const BROKEN: &str = "(ns example.broken-test
  (:require [cljs.test :refer [deftest is async]]))

(deftest wrong-sum (is (= 5 (+ 2 2))))

(deftest wrong-later
  (async done
    (js/setTimeout (fn [] (is (= :late :early)) (done)) 5)))

(deftest throws (throw (js/Error. \"boom\")))
";

#[cfg(light_v8)]
/// `compile node-test` and `release node-test` of examples/testing write
/// the generated runner's build and, with `:autorun true`, run it in Node,
/// whose exit status is the command's: zero while every test passes, its
/// asynchronous `js/setTimeout` test included, and nonzero once a test fails
/// or throws, which the output's own `node` run reports too.
#[test]
fn node_test_builds_run_in_node_and_fail_with_their_tests() {
    let (_dir, project) = copy("config-node-test", TESTING);
    for (command, file) in [
        ("compile", "out/node-test/index.js"),
        ("release", "out/node-test/light-cljs.test-runner.js"),
    ] {
        let output = light(&project, &[command, "node-test"]);
        let (stdout, stderr) = texts(&output);
        assert!(output.status.success(), "{command}: {stderr}");
        assert_eq!(stdout, PASSED, "{command}");
        let lines = stderr.lines().collect::<Vec<_>>();
        assert_eq!(lines[1], "[:node-test] Compiling ...", "{stderr}");
        assert_eq!(
            lines[3],
            format!("[:node-test] Running node {}", project.join(file).display()),
            "{stderr}"
        );
        assert_eq!(
            node(&project, file),
            (Some(0), PASSED.to_owned()),
            "{command}"
        );
    }
    let runner = fs::read_to_string(
        project.join(".light-cljs/test-runner/node-test/light_cljs/test_runner.cljs"),
    )
    .unwrap();
    assert!(
        runner.contains("(t/run-tests 'example.cart-test)"),
        "{runner}"
    );
    assert!(
        runner.contains("(set! (.-exitCode js/process) 1)"),
        "{runner}"
    );

    fs::write(project.join("test/example/broken_test.cljs"), BROKEN).unwrap();
    for (command, file) in [
        ("compile", "out/node-test/index.js"),
        ("release", "out/node-test/light-cljs.test-runner.js"),
    ] {
        let output = light(&project, &[command, "node-test"]);
        let (stdout, stderr) = texts(&output);
        assert_eq!(output.status.code(), Some(1), "{command}: {stderr}");
        assert!(
            stderr.ends_with(&format!("light-cljs {command} node-test: tests failed\n")),
            "{stderr}"
        );
        for expected in [
            "Testing example.broken-test",
            "FAIL in (wrong-sum)",
            "FAIL in (wrong-later)",
            "ERROR in (throws)",
            "Testing example.cart-test",
            "Ran 6 tests containing 8 assertions.\n2 failures, 1 errors.\n",
        ] {
            assert!(
                stdout.contains(expected),
                "{command}: {expected:?} in {stdout}"
            );
        }
        let (code, direct) = node(&project, file);
        assert_eq!(code, Some(1), "{command}: {direct}");
        assert_eq!(direct, stdout, "{command}");
    }

    // Without :autorun the command writes the build and runs nothing.
    let config = project.join("light-cljs.edn");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        text.replace("   :autorun true", "   :autorun false"),
    )
    .unwrap();
    let output = light(&project, &["compile", "node-test"]);
    let (stdout, stderr) = texts(&output);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(stdout, "");
    assert!(!stderr.contains("Running node"), "{stderr}");
    assert_eq!(node(&project, "out/node-test/index.js").0, Some(1));
}

#[cfg(light_v8)]
/// `:ns-regexp` selects the test namespaces, as a JavaScript regular
/// expression; a `:browser-test` build writes the runner that runs them in
/// the page, which Node can load too, since it touches the page only when
/// there is a `document`.
#[test]
fn ns_regexp_selects_the_namespaces_and_browser_test_builds_a_page_runner() {
    let (_dir, project) = copy("config-test-ns-regexp", TESTING);
    fs::write(project.join("test/example/broken_test.cljs"), BROKEN).unwrap();
    let config = project.join("light-cljs.edn");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        text.replace(
            "   :autorun true}",
            "   :autorun true\n   :ns-regexp \"^example[.]cart-\"}",
        ),
    )
    .unwrap();
    let output = light(&project, &["compile", "node-test"]);
    let (stdout, stderr) = texts(&output);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(stdout, PASSED);

    let output = light(&project, &["compile", "browser-test"]);
    let (stdout, stderr) = texts(&output);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(stdout, "");
    let runner = fs::read_to_string(
        project.join(".light-cljs/test-runner/browser-test/light_cljs/test_runner.cljs"),
    )
    .unwrap();
    assert!(
        runner.contains("(t/run-tests 'example.broken-test 'example.cart-test)"),
        "{runner}"
    );
    assert!(
        runner.contains("(defn ^:dev/after-load rerun []"),
        "{runner}"
    );
    let (code, stdout) = node(&project, "out/browser-test/index.js");
    // The page runner sets no exit status: a page has none.
    assert_eq!(code, Some(0));
    assert!(
        stdout.contains("Ran 6 tests containing 8 assertions.\n2 failures, 1 errors.\n"),
        "{stdout}"
    );
}

#[cfg(light_v8)]
/// The keys a test target does not take, or that only a test target takes,
/// are errors that say why, before anything compiles.
#[test]
fn test_build_keys_are_checked() {
    let (_dir, project) = copy("config-test-build-errors", TESTING);
    let config = project.join("light-cljs.edn");
    let text = fs::read_to_string(&config).unwrap();
    let cases = [
        (
            text.replace(
                "   :autorun true}",
                "   :autorun true\n   :modules {:main {:entries [example.cart-test]}}}",
            ),
            vec!["compile", "node-test"],
            "light-cljs compile node-test: build :node-test is a :node-test build, whose entry \
             is the cljs.test runner light-cljs generates; delete its :modules",
        ),
        (
            text.replace(
                "   :output-dir \"out/browser-test\"}",
                "   :output-dir \"out/browser-test\"\n   :autorun true}",
            ),
            vec!["compile", "browser-test"],
            "light-cljs compile browser-test: :autorun of build :browser-test runs a :node-test \
             build in Node; a :browser-test build runs its tests in the page, again after each \
             reload",
        ),
        (
            text.replace(
                "  {:target :browser-test\n   :output-dir \"out/browser-test\"}",
                "  {:target :esm\n   :output-dir \"out/app\"\n   :ns-regexp \"x\"\n   \
                 :modules {:main {:entries [example.cart]}}}",
            ),
            vec!["release", "browser-test"],
            "light-cljs release browser-test: :ns-regexp of build :browser-test applies to a \
             :node-test or :browser-test build, and its :target is :esm",
        ),
        (
            text.replace("   :autorun true}", "   :ns-regexp \"-spec$\"}"),
            vec!["compile", "node-test"],
            "light-cljs compile node-test: no test namespaces match :ns-regexp -spec$ under the \
             source roots",
        ),
        (
            text.replace("   :autorun true}", "   :ns-regexp \"(\"}"),
            vec!["compile", "node-test"],
            "light-cljs compile node-test: :ns-regexp: invalid regular expression (:",
        ),
        (
            text.clone(),
            vec!["compile", "node-test", "--entry", "example.cart-test"],
            "light-cljs compile node-test: --entry: build :node-test is a :node-test build, \
             whose entry is the cljs.test runner light-cljs generates; select its tests with \
             :ns-regexp",
        ),
    ];
    for (config_text, args, message) in cases {
        fs::write(&config, &config_text).unwrap();
        let output = light(&project, &args);
        let (_, stderr) = texts(&output);
        assert!(!output.status.success(), "{args:?}: {stderr}");
        assert!(
            stderr.lines().last().unwrap().starts_with(message),
            "{args:?}: expected {message:?}, got {stderr}"
        );
        assert!(!project.join("out").exists(), "{args:?}");
    }
}
