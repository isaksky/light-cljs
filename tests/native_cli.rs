//! Interpreter subprocesses deliberately have no Node/JVM/SCI on PATH.
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
    command
        .env("PATH", "")
        .env_remove("NODE")
        .env_remove("JAVA_HOME");
    command
}
fn run(args: &[&str], input: &str) -> Output {
    let mut child = command()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn ok(output: Output, expected: &str) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "light-cli-λ-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, source: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn eval_text_stdin_empty_and_unicode_need_only_the_native_binary() {
    ok(
        run(&["--eval", "--text", "(let [λ 40] (+ λ 2))"], ""),
        "42\n",
    );
    ok(
        run(&["--eval", "-"], "(ns café) ::résultat"),
        ":café/résultat\n",
    );
    ok(run(&["--eval"], "(+ 10 20)\n(+ 2 3)"), "5\n");
    ok(run(&["--eval"], ""), "nil\n");
    ok(run(&["--eval"], "; just a comment\n#_ :discarded"), "nil\n");
    ok(
        run(&["--eval", "--text", "\"\\uD800\""], ""),
        "\"\\ud800\"\n",
    );
}

#[test]
fn script_output_and_final_result_are_distinct() {
    let output = run(
        &[
            "--eval",
            "--text",
            "(println \"hello\") (binding [*out* *err*] (print \"warning\")) 41 (+ 1 2)",
        ],
        "",
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"hello\n3\n");
    assert_eq!(output.stderr, b"warning");
    // Adapt SCI io_test/with-out-str-test and with-in-str-test without wrappers.
    ok(
        run(
            &[
                "--eval",
                "--text",
                "[(with-out-str (print \"hello\")) (with-in-str \"foo\" (read-line))]",
            ],
            "",
        ),
        "[\"hello\" \"foo\"]\n",
    );
}

#[test]
fn file_execution_handles_utf8_paths_file_binding_and_runtime_stdin() {
    let dir = Directory::new();
    let file = dir.write(
        "café.clj",
        "(println (read-line)) (vector (= *file* (first (list *file*))) 42)",
    );
    ok(
        run(&["--eval", file.to_str().unwrap()], "héllo λ\n"),
        "héllo λ\n[true 42]\n",
    );
    let file = dir.write("file.clj", "*file*");
    let output = command().args(["--eval"]).arg(&file).output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{:?}\n", file.to_str().unwrap())
    );
    ok(
        run(&["--eval", "--text", "(read-line)"], "native input\n"),
        "\"native input\"\n",
    );
    ok(run(&["--eval"], "(read-line)"), "nil\n");
}

#[test]
fn isolated_copy_loads_only_explicit_clj_and_cljc_roots() {
    let dir = Directory::new();
    let binary = dir.0.join("native-light-cljs");
    fs::copy(env!("CARGO_BIN_EXE_light-cljs"), &binary).unwrap();
    dir.write("helper.cljc", "(ns helper) (def n #?(:clj 40 :cljs 0))");
    dir.write(
        "main.clj",
        "(ns main (:require [helper :as h])) (println \"loaded\") (+ h/n 2)",
    );
    let output = Command::new(&binary)
        .env_clear()
        .env("PATH", "")
        .current_dir(&dir.0)
        .args(["--eval", "--source-root", ".", "main.clj"])
        .output()
        .unwrap();
    ok(output, "loaded\n42\n");
    let output = Command::new(&binary)
        .env_clear()
        .current_dir(&dir.0)
        .args(["--eval", "main.clj"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("MissingNamespace"));
    dir.write("helper.clj", "(ns helper) (def n 9)");
    let output = command()
        .args(["--eval", "--source-root"])
        .arg(&dir.0)
        .args(["--text", "(require 'helper)"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("AmbiguousSources"));
}

#[test]
fn script_failures_report_phase_source_byte_column_and_no_success_result() {
    for (source, position, phase) in [
        ("42\n[", "<text>:2:2", "Read"),
        ("42\n(do \"λ\" missing)", "<text>:2:10", "Analysis"),
        ("42\n(/ 1 0)", "<text>:2:1", "Execution"),
        (
            "42\n(throw (ex-info \"oops\" {:n 1}))",
            "<text>:2:1",
            "Execution",
        ),
    ] {
        let output = run(&["--eval", "--text", source], "");
        assert!(!output.status.success(), "{source}");
        assert!(output.stdout.is_empty(), "{source}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(position), "{error}");
        assert!(error.contains(phase), "{error}");
        assert!(error.contains("bytes "), "{error}");
    }
    let output = run(
        &[
            "--eval",
            "--text",
            "(println \"before\") 99 unknown (println \"after\")",
        ],
        "",
    );
    assert!(!output.status.success());
    assert_eq!(output.stdout, b"before\n");
    let dir = Directory::new();
    dir.write("broken.clj", "(ns broken)\n  unknown");
    let output = command()
        .args(["--eval", "--source-root"])
        .arg(&dir.0)
        .args(["--text", "(require 'broken)"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("broken.clj:2:3"));
}

#[test]
fn runtime_and_rendering_limits_fail_without_partial_results() {
    for (args, expected) in [
        (vec!["--fuel", "100", "--text", "(loop [] (recur))"], "Fuel"),
        (
            vec!["--allocation-limit", "1", "--text", "[1 2 3]"],
            "Allocation",
        ),
        (vec!["--output-limit", "2", "--text", "[1 2 3]"], "Output"),
        (
            vec!["--call-depth", "1", "--text", "((fn f [] (f)))"],
            "CallDepthExceeded",
        ),
        (
            vec!["--evaluation-depth", "1", "--text", "(+ 1 (+ 2 3))"],
            "EvaluationDepthExceeded",
        ),
    ] {
        let mut invocation = vec!["--eval"];
        invocation.extend(args);
        let output = run(&invocation, "");
        assert!(!output.status.success(), "{invocation:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{:?}",
            output.stderr
        );
    }
    ok(
        run(&["--eval", "--print-length", "3", "--text", "(range)"], ""),
        "(0 1 2 ...)\n",
    );
    ok(
        run(&["--eval", "--print-level", "1", "--text", "[1 [2 3]]"], ""),
        "[1 #]\n",
    );
    let output = run(
        &[
            "--eval",
            "--text",
            "(lazy-seq (throw (ex-info \"render\" {})))",
        ],
        "",
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("render"));
}

#[test]
fn repl_retains_history_definitions_namespace_and_current_reader_context() {
    ok(
        run(
            &["--repl"],
            "1\n2\n3\n[*1 *2 *3]\n(def n 40)\n(ns café)\n::x\n(clojure.core/+ user/n 2)\n[*1 *2]\n",
        ),
        "1\n2\n3\n[3 2 1]\n#'user/n\nnil\n:café/x\n42\n[42 :café/x]\n",
    );
    ok(
        run(&["--repl"], "(ns café) ::x (ns user) ::x\n"),
        "nil\n:café/x\nnil\n:user/x\n",
    );
}

#[test]
fn repl_multiline_uses_parser_rules_and_preserves_single_execution() {
    ok(
        run(
            &["--repl"],
            "(def n (atom 0))\n[(swap! n inc)\n  (str \"a)\n[b\")\n  @n]\n'\n#?(:cljs :gone)\n:kept\n#_\n(println :discarded)\n(+ 20\n  22)",
        ),
        "#'user/n\n[1 \"a)\\n[b\" 1]\n:kept\n42\n",
    );
    ok(run(&["--repl"], " ; comment\n#_ :ignored\n"), "");
    ok(run(&["--repl"], ""), "");
    ok(run(&["--repl"], "42"), "42\n");
}

#[test]
fn repl_continues_after_reader_analysis_execution_and_print_errors() {
    let output = run(
        &["--repl"],
        "1\n2\n3\n] :discarded-rest-of-line\nmissing\n(/ 1 0)\n[*1 *2 *3]\n(:phase (ex-data *e))\n(throw (ex-info \"original\" {:tag :kept}))\n(ex-data *e)\n(+ 20 22)\n(print \"\\uD800\")\n:recovered\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "1\n2\n3\n[3 2 1]\n:execution\n{:tag :kept}\n42\n:recovered\n"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("<stdin>:4:1: Read"), "{error}");
    assert!(error.contains("<stdin>:5:1: Analysis"), "{error}");
    assert!(error.contains("<stdin>:6:1: Execution"), "{error}");
    assert!(error.contains("original"));
}

#[test]
fn repl_shares_the_language_input_cursor_and_recovers_runtime_reads() {
    ok(
        run(&["--repl"], "(read) 41\n(+ 1 *1)\n(read-line)tail\n:done\n"),
        "41\n42\n\"tail\"\n:done\n",
    );
    let output = run(&["--repl"], "(read) ]\n:after\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b":after\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("UnexpectedDelimiter"));
    ok(
        run(
            &["--repl"],
            "(try (clojure.edn/read) (catch Exception e :caught)) ]\n:after\n",
        ),
        ":caught\n:after\n",
    );
}

#[test]
fn repl_eof_reports_incomplete_input_once_and_errors_do_not_shift_history() {
    let output = run(&["--repl"], "41\n[1\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"41\n");
    let error = String::from_utf8_lossy(&output.stderr);
    assert_eq!(error.matches("light-cljs:").count(), 1);
    assert!(error.contains("UnexpectedEof"));
    assert!(error.contains("opened at <stdin>:2:1"));
    let output = run(&["--repl", "--fuel", "300"], "(loop [] (recur))\n42\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"42\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Fuel"));
    let output = run(&["--repl", "--fuel", "0"], "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Fuel"));
}

#[test]
fn native_options_are_validated_and_help_lists_both_families() {
    for args in [
        vec!["--eval", "--text"],
        vec!["--eval", "--fuel", "-1"],
        vec!["--eval", "--fuel", "18446744073709551616"],
        vec!["--eval", "--text", "1", "extra"],
        vec!["--repl", "file.clj"],
        vec!["--repl", "--text", "1"],
        vec!["--eval", "--source-root"],
        vec!["--eval", "--compile"],
    ] {
        let output = run(&args, "");
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let output = run(&["--help"], "");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for flag in [
        "--compile",
        "--parse",
        "--analyze",
        "--eval",
        "--repl",
        "--fuel",
    ] {
        assert!(help.contains(flag));
    }
}

#[test]
fn invalid_utf8_repl_input_is_a_fatal_host_read_error() {
    let mut child = command()
        .arg("--repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\xff\n42\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("utf-8"));
}

#[test]
fn repl_compacts_consumed_lines_without_spending_future_form_budgets() {
    for option in [["--fuel", "300"], ["--allocation-limit", "1000"]] {
        ok(
            run(&["--repl", option[0], option[1]], &"42\n".repeat(300)),
            &"42\n".repeat(300),
        );
    }
    let input = format!(
        "{}(defn boom []\n (/ 1 0))\n1\r\n2\r3\r\u{c}4\n(boom)\n(do \"λ\" missing)\n:after\n",
        "42\n".repeat(20)
    );
    let output = run(&["--repl"], &input);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}#'user/boom\n1\n2\n3\n4\n:after\n", "42\n".repeat(20))
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("<stdin>:22:2: Execution"), "{error}");
    assert!(error.contains("<stdin>:28:10: Analysis"), "{error}");
}
