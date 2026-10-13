use light_cljs::compiler::analyze_namespace_entry;
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, analyze, parse, prepare_application,
};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[test]
fn prints_example() {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--text", "(+ 1 2)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OpenParen\t0..1\t\"(\"\nAtom\t1..2\t\"+\"\nAtom\t3..4\t\"1\"\nAtom\t5..6\t\"2\"\nCloseParen\t6..7\t\")\"\n"
    );
}

#[test]
fn reads_stdin_and_escapes_text() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("café,\"λ\\\"\"".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "Atom\t0..5\t\"café\"\nString\t6..12\t",
            "\"\\\"λ\\\\\\\"\\\"\"\n"
        )
    );
}

#[test]
fn rejects_missing_text() {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .arg("--text")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("requires a source string")
    );
}

#[test]
fn reports_lexical_errors_with_spans_and_nonzero_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--text", "[\"λ"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OpenBracket\t0..1\t\"[\"\nError(UnterminatedString)\t1..4\t\"\\\"λ\"\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("lexical errors")
    );
}

#[test]
fn prints_parser_nodes_and_reports_locations() {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--parse", "--text", "(+ 1 2)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("roots: [NodeId(3)]\n"));
    assert!(text.contains("1\tNumber(Integer)\t3..4\t\"1\""));
    assert!(
        text.contains(
            "Collection { kind: List, children: [NodeId(0), NodeId(1), NodeId(2)] }\t0..7"
        )
    );
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--parse", "--text", "(λ\n ]"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.contains("2:2: expected ), found ] (bytes 5..6)"));
    assert!(text.contains("opened at 1:1 (bytes 0..1)"));
}

#[test]
fn compiles_files_and_stdin_to_executable_javascript() {
    let path = std::env::temp_dir().join(format!("light-cljs-cli-{}.cljs", std::process::id()));
    std::fs::write(&path, "(let* [x 10 f (fn* [] x)] (let* [x 20] (f)))").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .arg("--compile")
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let js = String::from_utf8(output.stdout).unwrap();
    let result = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--print", &js])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"10\n");

    let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"(if 0 1 2)")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let js = String::from_utf8(output.stdout).unwrap();
    let result = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--print", &js])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"1\n");
}

#[test]
fn analysis_cli_shows_ids_and_compile_errors_never_emit_partial_javascript() {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--analyze", "--text", "(let* [x 1] x)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("LocalId("));
    assert!(text.contains("LocalBinding"));
    for (source, expected) in [
        (
            "1\n  unknown",
            "2:3: unresolved symbol: unknown (bytes 4..11)",
        ),
        ("(λ\n ]", "2:2: expected ), found ] (bytes 5..6)"),
        (
            "1\n  (:x)",
            "2:3: literal keyword calls require one or two arguments (bytes 4..8)",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--compile", "--text", source])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
}

#[test]
fn vector_keyword_acceptance_compiles_and_executes_from_cli() {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--text", "(let [v [:demo/x 1]] (count v))"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--print", "eval(require('fs').readFileSync(0,'utf8'))"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&output.stdout)
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"2\n");
}

fn light_cljs(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .unwrap()
}

fn compiled(args: &[&str]) -> String {
    let output = light_cljs(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn optimize_selects_the_application_level_for_text_and_namespace_compiles() {
    let source = r#"(def unused "OPTIMIZE_CLI_SENTINEL") 42"#;
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let root = "examples/namespaces";
    let namespace = analyze_namespace_entry(
        "demo.main",
        &[Path::new(env!("CARGO_MANIFEST_DIR")).join(root)],
        &mut MacroHost::new(),
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap();
    for level in OptimizationLevel::ALL {
        let name = level.name();
        let expected = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            expected.contains("OPTIMIZE_CLI_SENTINEL"),
            level == OptimizationLevel::None,
            "{level}"
        );
        // The flag may come before or after the input.
        for args in [
            ["--compile", "--optimize", name, "--text", source],
            ["--compile", "--text", source, "--optimize", name],
        ] {
            assert_eq!(compiled(&args), expected, "{args:?}");
        }
        let result = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .args(["--print", &expected])
            .output()
            .unwrap();
        assert_eq!(result.stdout, b"42\n", "{level}");

        let expected = prepare_application(&namespace, ApplicationOptions::new(level)).emit_js();
        for args in [
            [
                "--compile",
                "--optimize",
                name,
                "--entry",
                "demo.main",
                "--source-root",
                root,
            ],
            [
                "--compile",
                "--entry",
                "demo.main",
                "--source-root",
                root,
                "--optimize",
                name,
            ],
        ] {
            assert_eq!(compiled(&args), expected, "{args:?}");
        }
    }
    assert_eq!(
        compiled(&["--compile", "--text", source]),
        prepare_application(&program, ApplicationOptions::default()).emit_js(),
        "prepare is the default"
    );
}

/// `--define NS/NAME=VALUE` replaces a `goog-define` default at every level,
/// for text and namespace compiles, and reads the value as the default's
/// kind; a malformed, unknown or mistyped define is an error.
#[test]
fn define_replaces_goog_define_defaults_at_every_level() {
    let source =
        r#"(goog-define DEBUG false) (goog-define NAME "app") (if DEBUG (str NAME "!") NAME)"#;
    let root = std::env::temp_dir().join(format!("light-cljs-cli-define-{}", std::process::id()));
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/main.cljs"),
        format!("(ns app.main) {source}"),
    )
    .unwrap();
    let root_arg = root.to_str().unwrap();
    let node = |js: &str| {
        let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .args(["--print", js])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    };
    for level in OptimizationLevel::ALL {
        let name = level.name();
        for (defines, expected) in [
            (&[][..], "app\n"),
            (
                &[
                    "--define",
                    "cljs.user/DEBUG=true",
                    "--define",
                    "cljs.user/NAME=x y",
                ][..],
                "x y!\n",
            ),
        ] {
            let mut args = vec!["--compile", "--optimize", name];
            args.extend_from_slice(defines);
            args.extend(["--text", source]);
            assert_eq!(node(&compiled(&args)), expected, "{args:?}");
        }
        let js = compiled(&[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root_arg,
            "--define",
            "app.main/DEBUG=true",
            "--optimize",
            name,
        ]);
        assert_eq!(node(&js), "app!\n", "{level}");
    }
    let _ = std::fs::remove_dir_all(&root);
    for (args, message) in [
        (
            &["--compile", "--define", "DEBUG=true", "--text", source][..],
            "--define requires NAMESPACE/NAME=VALUE",
        ),
        (
            &[
                "--compile",
                "--define",
                "cljs.user/MISSING=1",
                "--text",
                source,
            ][..],
            "no goog-define named cljs.user/MISSING in the program",
        ),
        (
            &[
                "--compile",
                "--define",
                "cljs.user/DEBUG=yes",
                "--text",
                source,
            ][..],
            "define cljs.user/DEBUG requires a boolean value, got \"yes\"",
        ),
        (
            &["--compile", "--text", source, "--define"][..],
            "--define requires NS/NAME=VALUE",
        ),
        (
            &[
                "--parse",
                "--define",
                "cljs.user/DEBUG=true",
                "--text",
                source,
            ][..],
            "--define requires --compile",
        ),
    ] {
        let output = light_cljs(args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

#[test]
fn optimize_rejects_unknown_levels_and_non_compile_modes() {
    for (args, message) in [
        (
            &["--compile", "--optimize", "fast", "--text", "1"][..],
            "unknown optimization level \"fast\": expected none, prepare or advanced",
        ),
        (
            &["--compile", "--text", "1", "--optimize"][..],
            "--optimize requires a level: none, prepare or advanced",
        ),
        (
            &[
                "--compile",
                "--entry",
                "demo.main",
                "--optimize",
                "ADVANCED",
            ][..],
            "unknown optimization level \"ADVANCED\"",
        ),
        (
            &["--parse", "--optimize", "none", "--text", "1"][..],
            "--optimize requires --compile",
        ),
        (
            &["--analyze", "--text", "1", "--optimize", "none"][..],
            "Usage:",
        ),
    ] {
        let output = light_cljs(args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

#[test]
fn size_report_prints_the_retention_table_on_stderr_without_changing_output() {
    // `+` is an operator at `Advanced`; `max` still reads the runtime, since
    // a string's length is no constant that folding computes (light-cljs-214).
    let source = "(js/console.log (+ 1 (max 2 (.-length \"abc\"))))";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let root = "examples/namespaces";
    let namespace = analyze_namespace_entry(
        "demo.main",
        &[Path::new(env!("CARGO_MANIFEST_DIR")).join(root)],
        &mut MacroHost::new(),
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap();
    for level in OptimizationLevel::ALL {
        let name = level.name();
        let text = prepare_application(&program, ApplicationOptions::new(level));
        let entry = prepare_application(&namespace, ApplicationOptions::new(level));
        // The flag may come before or after the input, with or without --entry.
        for (args, prepared) in [
            (
                &[
                    "--compile",
                    "--optimize",
                    name,
                    "--size-report",
                    "--text",
                    source,
                ][..],
                &text,
            ),
            (
                &[
                    "--compile",
                    "--text",
                    source,
                    "--size-report",
                    "--optimize",
                    name,
                ][..],
                &text,
            ),
            (
                &[
                    "--compile",
                    "--size-report",
                    "--entry",
                    "demo.main",
                    "--source-root",
                    root,
                    "--optimize",
                    name,
                ][..],
                &entry,
            ),
        ] {
            let output = light_cljs(args);
            assert!(output.status.success(), "{args:?}");
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                prepared.emit_js(),
                "{args:?}"
            );
            let report = String::from_utf8(output.stderr).unwrap();
            assert_eq!(report, prepared.retention().to_string(), "{args:?}");
            // The namespace demo reads no runtime binding and reaches no
            // reflection, so its advanced output registers no namespace and
            // embeds no runtime.
            if level == OptimizationLevel::Advanced && args.contains(&"--entry") {
                assert_eq!(report, "advanced: no runtime embedded\n", "{args:?}");
                continue;
            }
            let embedding = if level == OptimizationLevel::Advanced {
                "runtime slice"
            } else {
                "whole runtime"
            };
            assert!(
                report.starts_with(&format!("{name}: {embedding}, ")),
                "{args:?}: {report}"
            );
            assert!(report.contains("\nroot "), "{args:?}");
            assert!(report.contains("\nunit "), "{args:?}");
        }
    }
    // `+` is bytes 17..18 of the source and the first runtime read.
    let report = light_cljs(&["--compile", "--size-report", "--text", source]).stderr;
    let report = String::from_utf8(report).unwrap();
    let root = report.lines().nth(3).unwrap();
    assert!(root.starts_with("rt.core.add "), "{report}");
    assert!(root.ends_with(" source 0, bytes 17..18"), "{report}");
    let output = light_cljs(&["--parse", "--size-report", "--text", "1"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("--size-report requires --compile")
    );
}
