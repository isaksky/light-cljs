//! Compiler-backed evaluator subprocesses deliberately have no Node/JVM/SCI on PATH.
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
fn eval_text_stdin_empty_and_unicode_need_only_the_embedded_binary() {
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
            "(println \"hello\") (binding [*print-fn* *print-err-fn*] (print \"warning\")) 41 (+ 1 2)",
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
        "café.cljs",
        "(println (read-line)) (vector (= *file* (first (list *file*))) 42)",
    );
    ok(
        run(&["--eval", file.to_str().unwrap()], "héllo λ\n"),
        "héllo λ\n[true 42]\n",
    );
    let file = dir.write("file.cljs", "*file*");
    let output = command().args(["--eval"]).arg(&file).output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{:?}\n", file.to_str().unwrap())
    );
    ok(
        run(&["--eval", "--text", "(read-line)"], "compiled input\n"),
        "\"compiled input\"\n",
    );
    ok(run(&["--eval"], "(read-line)"), "nil\n");
}

#[test]
fn isolated_copy_loads_only_explicit_cljs_and_cljc_roots() {
    let dir = Directory::new();
    let binary = dir.0.join("light-cljs");
    fs::copy(env!("CARGO_BIN_EXE_light-cljs"), &binary).unwrap();
    dir.write("helper.cljc", "(ns helper) (def n #?(:clj 0 :cljs 40))");
    dir.write(
        "main.cljs",
        "(ns main (:require [helper :as h])) (println \"loaded\") (+ h/n 2)",
    );
    let output = Command::new(&binary)
        .env_clear()
        .env("PATH", "")
        .current_dir(&dir.0)
        .args(["--eval", "--source-root", ".", "main.cljs"])
        .output()
        .unwrap();
    ok(output, "loaded\n42\n");
    let output = Command::new(&binary)
        .env_clear()
        .current_dir(&dir.0)
        .args(["--eval", "main.cljs"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no source provider for helper"));
    dir.write("helper.cljs", "(ns helper) (def n 9)");
    ok(
        run(
            &[
                "--eval",
                "--source-root",
                dir.0.to_str().unwrap(),
                "--text",
                "(require 'helper) helper/n",
            ],
            "",
        ),
        "9\n",
    );
}

#[test]
fn script_failures_keep_output_and_never_print_a_success_result() {
    for (source, expected) in [
        ("42\n[", "unexpected end of input"),
        ("42\n(do \"λ\" missing)", "unresolved symbol: missing"),
        ("42\n(throw (ex-info \"oops\" {:n 1}))", "oops"),
    ] {
        let output = run(&["--eval", "--text", source], "");
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
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
}

#[test]
fn runtime_and_rendering_limits_fail_without_partial_results() {
    for (args, expected) in [
        (
            vec!["--poll-budget", "1000", "--text", "(loop [] (recur))"],
            "poll budget",
        ),
        (vec!["--memory-limit", "1", "--text", "[1 2 3]"], "memory"),
        (vec!["--output-limit", "2", "--text", "[1 2 3]"], "output"),
        (vec!["--timeout-ms", "0", "--text", "1"], "deadline"),
        (
            vec!["--source-limit", "1", "--text", "[1 2 3]"],
            "source byte limit",
        ),
    ] {
        let mut invocation = vec!["--eval"];
        invocation.extend(args);
        let output = run(&invocation, "");
        assert!(!output.status.success(), "{invocation:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
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
            "1\n2\n3\n[*1 *2 *3]\n(def n 40)\n(ns café)\n::x\n(clojure.core/+ cljs.user/n 2)\n[*1 *2]\n",
        ),
        "1\n2\n3\n[3 2 1]\n40\nnil\n:café/x\n42\n[42 :café/x]\n",
    );
    ok(
        run(&["--repl"], "(ns café) ::x (ns cljs.user) ::x\n"),
        "nil\n:café/x\nnil\n:cljs.user/x\n",
    );
}

#[test]
fn repl_multiline_uses_parser_rules_and_preserves_single_execution() {
    ok(
        run(
            &["--repl"],
            "(def n (atom 0))\n[(swap! n inc)\n  (str \"a)\n[b\")\n  @n]\n'\n#?(:cljs :kept)\n#_\n(println :discarded)\n(+ 20\n  22)",
        ),
        "#object[cljs.core.Atom {:val 0}]\n[1 \"a)\\n[b\" 1]\n:kept\n42\n",
    );
    ok(run(&["--repl"], " ; comment\n#_ :ignored\n"), "");
    ok(run(&["--repl"], ""), "");
    ok(run(&["--repl"], "42"), "42\n");
}

#[test]
fn repl_continues_after_reader_analysis_execution_and_print_errors() {
    let output = run(
        &["--repl"],
        "1\n2\n3\n] :discarded\nmissing\n(:phase (ex-data *e))\n(throw (ex-info \"original\" {:tag :kept}))\n(ex-data *e)\n[*1 *2 *3]\n(+ 20 22)\n(lazy-seq (throw (ex-info \"render\" {})))\n:recovered\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"1\n2\n3\n:analysis\n{:tag :kept}\n[{:tag :kept} :analysis 3]\n42\n:recovered\n"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("unexpected closing delimiter"));
    assert!(error.contains("unresolved symbol"));
    assert!(error.contains("original"));
    assert!(error.contains("render"));
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unsupported reader syntax"));
    ok(
        run(
            &["--repl"],
            "(require '[clojure.edn :as edn]) (try (edn/read) (catch :default e :caught)) ]\n:after\n",
        ),
        "nil\n:caught\n:after\n",
    );
}

#[test]
fn repl_eof_reports_incomplete_input_once_and_errors_do_not_shift_history() {
    let output = run(&["--repl"], "41\n[1\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"41\n");
    let error = String::from_utf8_lossy(&output.stderr);
    assert_eq!(error.matches("light-cljs:").count(), 1);
    assert!(error.contains("unexpected end of input"));
    assert!(error.contains("opened at <stdin>.cljc:2:1"));
    let output = run(
        &["--repl", "--poll-budget", "1000"],
        "(loop [] (recur))\n42\n",
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("poll budget"));
    ok(run(&["--eval", "--text", "42"], ""), "42\n");
}

#[test]
fn evaluation_options_are_validated_and_help_lists_both_families() {
    for args in [
        vec!["--eval", "--text"],
        vec!["--eval", "--fuel", "-1"],
        vec!["--eval", "--fuel", "18446744073709551616"],
        vec!["--eval", "--text", "1", "extra"],
        vec!["--repl", "file.cljs"],
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
        "--poll-budget",
        "--memory-limit",
        "--stack-limit",
        "--timeout-ms",
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
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .to_lowercase()
            .contains("utf-8")
    );
}

#[test]
fn repl_compacts_consumed_lines_and_preserves_original_locations() {
    ok(
        run(&["--repl", "--source-limit", "16"], &"42\n".repeat(300)),
        &"42\n".repeat(300),
    );
    let output = run(
        &["--repl"],
        &format!(
            "{}(throw (js/Error. \"boom\"))\n:after\n",
            "42\n".repeat(20)
        ),
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("<stdin>.cljc:21:1"));
    assert!(String::from_utf8_lossy(&output.stdout).ends_with(":after\n"));
}
#[test]
fn number_js_macros_and_clj_migration_are_explicit() {
    ok(run(&["--eval", "--text", "(/ 1 2)"], ""), "0.5\n");
    ok(
        run(
            &[
                "--eval",
                "--text",
                "(defmacro double-it [x] (* x 2)) [(double-it 21) (js/Math.sqrt 81)]",
            ],
            "",
        ),
        "[42 9]\n",
    );
    for flag in [
        "--fuel",
        "--allocation-limit",
        "--call-depth",
        "--evaluation-depth",
    ] {
        let output = run(&["--eval", flag, "1", "--text", "1"], "");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("is retired; use"));
    }
    let output = run(&["--eval", "old.clj"], "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("rename/adapt"));
}
#[test]
fn imported_macro_helpers_run_script_initializers_once() {
    let dir = Directory::new();
    dir.write("helper.cljc", "(ns helper) (def factor #?(:cljs 2 :clj missing)) (def hits (atom 0)) (swap! hits inc) (defn twice [x] (* factor x)) (defmacro doubled [x] (twice x))");
    ok(
        run(
            &[
                "--eval",
                "--source-root",
                dir.0.to_str().unwrap(),
                "--text",
                "(ns app (:require [helper :as h :refer [doubled]])) [(doubled 21) (h/twice 21) @h/hits]",
            ],
            "",
        ),
        "[42 42 1]\n",
    );
}

#[test]
fn definition_metadata_uses_text_file_and_incremental_repl_locations() {
    let probe = "(def ^{:file \"fake\" :line 0 :column 0} x 1) [(:file (meta #'x)) (:line (meta #'x)) (:column (meta #'x))]";
    ok(
        run(&["--eval", "--text", probe], ""),
        "[\"<text>.cljc\" 1 1]\n",
    );
    let directory = Directory::new();
    let path = directory.write("source.cljs", &format!("\n  {probe}"));
    ok(
        run(&["--eval", path.to_str().unwrap()], ""),
        &format!("[{:?} 2 3]\n", path.to_str().unwrap()),
    );
    for args in [
        vec!["--compile", "--text", probe],
        vec!["--compile", path.to_str().unwrap()],
    ] {
        let output = run(&args, "");
        assert!(output.status.success());
        let js = String::from_utf8(output.stdout).unwrap();
        let evaluator = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
        let expected = if args[1] == "--text" {
            "[\"<text>.cljc\" 1 1]".to_owned()
        } else {
            format!("[{:?} 2 3]", path.to_str().unwrap())
        };
        let result = evaluator
            .evaluate_js(&format!("({js}).toString()"))
            .unwrap();
        assert_eq!(evaluator.string(result).unwrap(), expected);
    }
    let out = run(
        &["--repl"],
        "\n(def ^{:file \"fake\"} x 1)\n\n[(:file (meta #'x)) (:line (meta #'x)) (:column (meta #'x))]\n(def ^{:file \"fake\"} y 2)\n[(:file (meta #'y)) (:line (meta #'y)) (:column (meta #'y))]\n",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("[\"<stdin>.cljc\" 2 1]"), "{text}");
    assert!(text.contains("[\"<stdin>.cljc\" 5 1]"), "{text}");
}
