use light_cljs::compiler::analyze_namespace_entry;
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, analyze, parse, prepare_application,
};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

// Temporary directories removed when their test ends, however it ends.
#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;
use temp_dir::TempDir;

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
    let dir = TempDir::new("cljs-cli");
    let path = dir.join("program.cljs");
    std::fs::write(&path, "(let* [x 10 f (fn* [] x)] (let* [x 20] (f)))").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .arg("--compile")
        .arg(&path)
        .output()
        .unwrap();
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
    let root = TempDir::new("cljs-cli-define");
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

/// The warning-mode builds, `--warnings --compile` and `--reloadable`, let a
/// macro call a helper its namespace declares first and defines after the
/// macro's use, as cuerdas.core's `unsafe-concat` calls its `repeat` in
/// Penpot.
#[test]
fn warning_mode_builds_let_macros_call_later_definitions_of_their_namespace() {
    let root = TempDir::new("cljs-cli-forward-helper");
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/text.cljc"),
        "(ns app.text (:refer-clojure :exclude [repeat]))\n\
         (declare repeat)\n\
         (defmacro stars [& xs] (repeat \"*\" (count xs)))\n\
         (defn three [] (stars 1 2 3))\n\
         (defn repeat [s n] (apply str (clojure.core/repeat n s)))\n\
         (three)",
    )
    .unwrap();
    let root_arg = root.to_str().unwrap();
    let namespace = ["--entry", "app.text", "--source-root", root_arg];
    for flags in [&["--compile"][..], &["--warnings", "--compile"]] {
        let output = light_cljs(&[flags, &namespace[..], &["--esm"]].concat());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stderr.is_empty(),
            "{flags:?}: {stderr}"
        );
        let js = String::from_utf8(output.stdout).unwrap();
        assert!(js.contains("\"***\""), "{flags:?}: {js}");
    }
    let out = root.join("dev");
    let output = light_cljs(
        &[
            &["--compile"][..],
            &namespace[..],
            &["--reloadable", "--out-dir", out.to_str().unwrap()],
        ]
        .concat(),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success() && stderr.is_empty(), "{stderr}");
    let unit = std::fs::read_to_string(out.join("app.text.js")).unwrap();
    assert!(unit.contains("\"***\""), "{unit}");
}

/// `--config KEY=EDN` gives `light.build/config` its values in a namespace
/// compile and a `--reloadable` build; a malformed flag, value or a key
/// given twice is an error, in `--watch` too.
#[test]
fn config_gives_macros_build_configuration_values() {
    let root = TempDir::new("cljs-cli-config");
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/macros.cljs"),
        "(ns app.macros (:require [light.build :as b]))\n\
         (defmacro setting [key] (pr-str (b/config key)))",
    )
    .unwrap();
    std::fs::write(
        root.join("app/main.cljs"),
        "(ns app.main (:require [app.macros :refer [setting]]))\n\
         (str (setting :app/flavor) \" \" (setting :app/ports) \" \" (setting :app/unset))",
    )
    .unwrap();
    let root_arg = root.to_str().unwrap();
    let namespace = [
        "--compile",
        "--entry",
        "app.main",
        "--source-root",
        root_arg,
    ];
    let flags = [
        "--config",
        "app/flavor=:dev",
        "--config",
        "app/ports=[80 443]",
    ];
    let js = compiled(&[&namespace[..], &flags[..]].concat());
    // The flags may come before --entry too.
    assert_eq!(
        compiled(&[&namespace[..1], &flags[..], &namespace[1..]].concat()),
        js
    );
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--print", &js])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        ":dev [80 443] nil\n"
    );
    let out = root.join("dev");
    let out_arg = out.to_str().unwrap();
    compiled(
        &[
            &namespace[..],
            &flags[..],
            &["--reloadable", "--out-dir", out_arg],
        ]
        .concat(),
    );
    let unit = std::fs::read_to_string(out.join("app.main.js")).unwrap();
    assert!(
        unit.contains("\":dev\"") && unit.contains("\"[80 443]\""),
        "{unit}"
    );

    let watch = [
        "--watch",
        "--entry",
        "app.main",
        "--source-root",
        root_arg,
        "--out-dir",
        out_arg,
    ];
    for (args, message) in [
        (&["--config"][..], "--config requires KEY=EDN"),
        (
            &["--config", "app/flavor"],
            "--config app/flavor: expected KEY=EDN",
        ),
        (
            &["--config", "app/n=1", "--config", "app/n=2"],
            "--config app/n=2: the key app/n is given twice",
        ),
        (
            &["--config", "app/n=[1"],
            "--config app/n=[1: cannot read the value",
        ),
        (
            &["--config", "app/mode=dev"],
            "\"dev\" is not a configuration value",
        ),
        (
            &["--config", ":app/n=1"],
            "the key :app/n is written without its colon, as app/n",
        ),
    ] {
        for command in [&namespace[..], &watch[..]] {
            let args = [command, args].concat();
            let output = light_cljs(&args);
            assert!(!output.status.success(), "{args:?}");
            assert!(output.stdout.is_empty(), "{args:?}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.contains(message), "{args:?}: {stderr}");
        }
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

/// `--elide-asserts` omits assertions while compiling, as upstream's
/// `:elide-asserts` does: at every level, for text and namespace compiles and
/// for `--eval`, an `assert` compiles to the same bytes as `nil` in its place
/// (at `advanced`, as the program without it), and a failing `:pre` condition
/// no longer throws.
#[test]
fn elide_asserts_omits_assertions_at_every_level() {
    let asserting = r#"(assert (= 1 1) "x") (js/console.log 1)"#;
    let plain = "(js/console.log 1)";
    let nil = "nil (js/console.log 1)";
    let pre = "(defn twice [x] {:pre [(pos? x)]} (* 2 x)) (twice -1)";
    let root = TempDir::new("cljs-cli-elide");
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(root.join("app/main.cljs"), format!("(ns app.main) {pre}")).unwrap();
    let root_arg = root.to_str().unwrap();
    let node = |js: &str| {
        Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .args(["--print", js])
            .output()
            .unwrap()
    };
    for level in OptimizationLevel::ALL {
        let name = level.name();
        // `none` and `prepare` emit the `nil` statement an elided assertion
        // leaves; `advanced` drops it.
        let expected = compiled(&["--compile", "--optimize", name, "--text", nil]);
        if level == OptimizationLevel::Advanced {
            assert_eq!(
                expected,
                compiled(&["--compile", "--optimize", name, "--text", plain])
            );
        }
        assert!(
            compiled(&["--compile", "--optimize", name, "--text", asserting]).len()
                > expected.len(),
            "{level}: assertions are kept by default"
        );
        // The flag may come before or after the input.
        for args in [
            [
                "--compile",
                "--elide-asserts",
                "--optimize",
                name,
                "--text",
                asserting,
            ],
            [
                "--compile",
                "--optimize",
                name,
                "--text",
                asserting,
                "--elide-asserts",
            ],
        ] {
            assert_eq!(compiled(&args), expected, "{args:?}");
        }

        let failing = node(&compiled(&["--compile", "--optimize", name, "--text", pre]));
        assert!(!failing.status.success(), "{level}");
        assert!(
            String::from_utf8_lossy(&failing.stderr).contains("Assert failed"),
            "{level}: {}",
            String::from_utf8_lossy(&failing.stderr)
        );
        let elided = compiled(&[
            "--compile",
            "--optimize",
            name,
            "--elide-asserts",
            "--text",
            pre,
        ]);
        assert_eq!(node(&elided).stdout, b"-2\n", "{level}");
        let entry = |elide: bool| {
            let mut args = vec!["--compile", "--optimize", name];
            if elide {
                args.push("--elide-asserts");
            }
            args.extend(["--entry", "app.main", "--source-root", root_arg]);
            node(&compiled(&args))
        };
        assert!(!entry(false).status.success(), "{level}");
        assert_eq!(entry(true).stdout, b"-2\n", "{level}");
    }

    let output = light_cljs(&["--eval", "--text", pre]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Assert failed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = light_cljs(&["--eval", "--elide-asserts", "--text", pre]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"-2\n");

    // The flag is the only switch: `*assert*` is no `goog-define`.
    for (args, message) in [
        (
            &["--parse", "--elide-asserts", "--text", pre][..],
            "--elide-asserts requires --compile",
        ),
        (
            &[
                "--compile",
                "--define",
                "cljs.core/*assert*=false",
                "--text",
                pre,
            ][..],
            "no goog-define named cljs.core/*assert* in the program",
        ),
    ] {
        let output = light_cljs(args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

/// `--elide-asserts` is also the `*assert*` that source macros read while
/// expanding, under each of its names, as upstream's `:elide-asserts` binds
/// `clojure.core/*assert*` while compiling: for text and namespace compiles
/// (a macro required from another namespace) and for `--eval`.
#[test]
fn elide_asserts_reaches_the_assert_source_macros_read() {
    let flags = "[*assert* clojure.core/*assert* cljs.core/*assert*]";
    let root = TempDir::new("cljs-cli-elide-macros");
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/macros.cljs"),
        format!("(ns app.macros) (defmacro m [] {flags})"),
    )
    .unwrap();
    std::fs::write(
        root.join("app/main.cljs"),
        "(ns app.main (:require [app.macros :refer [m]])) (pr-str (m))",
    )
    .unwrap();
    let root_arg = root.to_str().unwrap();
    let text = format!("(defmacro m [] {flags}) (pr-str (m))");
    let eval = format!("(defmacro m [] {flags}) (m)");
    let node = |js: &str| {
        let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .args(["--print", js])
            .output()
            .unwrap();
        assert!(output.status.success(), "{js}");
        String::from_utf8(output.stdout).unwrap()
    };
    for (elide, expected) in [
        (false, "[true true true]\n"),
        (true, "[false false false]\n"),
    ] {
        let flag = if elide { &["--elide-asserts"][..] } else { &[] };
        for input in [
            &["--text", &text][..],
            &["--entry", "app.main", "--source-root", root_arg],
        ] {
            let args = [&["--compile"][..], flag, input].concat();
            assert_eq!(node(&compiled(&args)), expected, "{args:?}");
        }
        let args = [&["--eval"][..], flag, &["--text", &eval]].concat();
        assert_eq!(compiled(&args), expected, "{args:?}");
    }
}

/// `--source-map PATH` writes a Source Map v3 of the output at every level
/// and ends the output with a `sourceMappingURL` comment naming it; the
/// JavaScript before that comment is the output without the flag. With
/// Node's `--enable-source-maps`, a throw from inside a function (renamed at
/// `advanced`) reports the `.cljs` file, line and column of the `js/Error.`
/// and the function's source name, and a throw from the runtime reports its
/// line of `src/js_runtime.js` under its own source name. `--esm` namespace
/// output maps each namespace to its file; `--source-map-inline` embeds the
/// map, and `--source-map-include-sources` adds the sources' text.
#[test]
fn source_maps_point_node_stack_traces_at_the_cljs_source_at_every_level() {
    let root = TempDir::new("cljs-cli-map");
    std::fs::create_dir_all(root.join("src/app")).unwrap();
    std::fs::create_dir_all(root.join("out")).unwrap();
    let program = "(defn explode [x]\n  (throw (js/Error. (str \"boom \" x))))\n\n(defn outer [n]\n  (let [doubled (* 2 n)]\n    (explode doubled)))\n\n(outer 21)\n";
    std::fs::write(root.join("app.cljs"), program).unwrap();
    std::fs::write(
        root.join("pick.cljs"),
        "(defn pick [v i]\n  (nth v i))\n\n(pick [1 2] 5)\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/app/util.cljs"),
        "(ns app.util)\n\n(defn check [x]\n  (when (neg? x)\n    (throw (js/Error. (str \"negative \" x))))\n  x)\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/app/main.cljs"),
        "(ns app.main\n  (:require [app.util :as u]))\n\n(u/check 1)\n(u/check -2)\n",
    )
    .unwrap();
    // A macro's throw in a local function, and raw JavaScript of several
    // lines.
    std::fs::write(
        root.join("local.cljs"),
        "(defmacro fail! [message]\n  `(throw (js/Error. ~message)))\n\n(defn run [xs]\n  (let [check (fn [n]\n                (when (neg? n)\n                  (fail! (str \"negative \" n)))\n                n)]\n    (mapv check xs)))\n\n(run [1 -2])\n",
    )
    .unwrap();
    std::fs::write(
        root.join("raw.cljs"),
        "(defn raw [x]\n  (js* \"(() => {\\n  const y = ~{};\\n  throw new Error(y);\\n})()\" x))\n\n(raw 1)\n",
    )
    .unwrap();
    // At `advanced`, a function whose devirtualized protocol call holds its
    // operands declares their locals ahead of its body (light-cljs-262).
    std::fs::write(
        root.join("protocol.cljs"),
        "(defprotocol Area\n  (area [s a b]))\n\n(defrecord Rect [w h]\n  Area\n  (area [_ a b] (* w h a b)))\n\n(defn measure [n]\n  (when (pos? n)\n    (throw (js/Error. (str \"area \" (area (->Rect n 2) (inc n) (dec n))))))\n  n)\n\n(measure 0)\n(measure 3)\n",
    )
    .unwrap();
    let path = |name: &str| root.join(name).to_str().unwrap().to_owned();
    // Node reports the real path of each source.
    let real = std::fs::canonicalize(&root).unwrap();
    let real = |name: &str| real.join(name).to_str().unwrap().to_owned();
    // The runtime line that throws for an index out of bounds.
    let runtime =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/js_runtime.js"))
            .unwrap();
    let bounds_line = 1 + runtime
        .lines()
        .position(|line| line.trim_start().starts_with("const outOfBounds = "))
        .expect("the runtime declares outOfBounds");
    // The first frame of the uncaught error that running `file` prints.
    let first_frame = |file: &str| {
        let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .args(["--enable-source-maps", file])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{file}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        stderr
            .lines()
            .find(|line| line.starts_with("    at "))
            .unwrap_or_else(|| panic!("{file}: no stack trace in\n{stderr}"))
            .to_owned()
    };
    let assert_frame = |frame: &str, name: &str, location: &str, context: &str| {
        assert!(
            frame.starts_with(&format!("    at {name} ("))
                && frame.ends_with(&format!("{location})")),
            "{context}: {frame}"
        );
    };
    for level in OptimizationLevel::ALL {
        let name = level.name();
        let map = path(&format!("out/app-{name}.js.map"));
        let plain = compiled(&["--compile", "--optimize", name, &path("app.cljs")]);
        let mapped = compiled(&[
            "--compile",
            "--optimize",
            name,
            &path("app.cljs"),
            "--source-map",
            &map,
        ]);
        assert_eq!(
            mapped,
            format!("{plain}//# sourceMappingURL=app-{name}.js.map\n"),
            "{level}: the JavaScript does not change"
        );
        let json = std::fs::read_to_string(&map).unwrap();
        assert!(
            json.starts_with(&format!(
                "{{\"version\":3,\"file\":\"app-{name}.js\",\"sources\":[\"../app.cljs\",\"light-cljs/js_runtime.js\"],\"names\":["
            )),
            "{level}: {json:.200}"
        );
        assert!(!json.contains("sourcesContent"), "{level}");
        let script = path(&format!("out/app-{name}.js"));
        std::fs::write(&script, &mapped).unwrap();
        assert_frame(
            &first_frame(&script),
            "explode",
            &format!("{}:2:10", real("app.cljs")),
            name,
        );

        // The macro's code maps to its call, and the local function has
        // the local's name; the map names the local and the parameter.
        let map = path(&format!("out/local-{name}.js.map"));
        let script = path(&format!("out/local-{name}.js"));
        let mapped = compiled(&[
            "--compile",
            "--optimize",
            name,
            "--source-map",
            &map,
            &path("local.cljs"),
        ]);
        std::fs::write(&script, &mapped).unwrap();
        assert_frame(
            &first_frame(&script),
            "check",
            &format!("{}:7:19", real("local.cljs")),
            name,
        );
        let json = std::fs::read_to_string(&map).unwrap();
        assert!(
            json.contains("\"check\"") && json.contains("\"n\""),
            "{level}: {json:.300}"
        );

        // Each line of the raw text maps to the `js*` form.
        let map = path(&format!("out/raw-{name}.js.map"));
        let script = path(&format!("out/raw-{name}.js"));
        let mapped = compiled(&[
            "--compile",
            "--optimize",
            name,
            "--source-map",
            &map,
            &path("raw.cljs"),
        ]);
        std::fs::write(&script, &mapped).unwrap();
        assert_frame(
            &first_frame(&script),
            "<anonymous>",
            &format!("{}:2:3", real("raw.cljs")),
            name,
        );

        // Readable, the declaration is a line of its own before the throw,
        // longer than the text from the `new` to the next expression.
        let source = path("protocol.cljs");
        for pseudo in [&[][..], &["--pseudo-names"]] {
            let map = path(&format!("out/protocol-{name}-{}.js.map", pseudo.len()));
            let script = path(&format!("out/protocol-{name}-{}.js", pseudo.len()));
            let args = [
                &["--compile", "--optimize", name][..],
                pseudo,
                &["--source-map", &map, &source],
            ]
            .concat();
            let mapped = compiled(&args);
            assert_eq!(
                mapped.contains("let operand0, operand1, operand2;")
                    || mapped.contains(".cljs$user$Area$area$arity$3("),
                level == OptimizationLevel::Advanced,
                "{args:?}: {mapped}"
            );
            std::fs::write(&script, &mapped).unwrap();
            assert_frame(
                &first_frame(&script),
                "measure",
                &format!("{}:10:12", real("protocol.cljs")),
                &format!("{args:?}"),
            );
        }

        let map = path(&format!("out/pick-{name}.js.map"));
        let script = path(&format!("out/pick-{name}.js"));
        let mapped = compiled(&[
            "--compile",
            "--optimize",
            name,
            "--source-map",
            &map,
            &path("pick.cljs"),
        ]);
        std::fs::write(&script, &mapped).unwrap();
        assert_frame(
            &first_frame(&script),
            "outOfBounds",
            &format!("{}:{bounds_line}:3", real("out/light-cljs/js_runtime.js")),
            name,
        );

        let map = path(&format!("out/main-{name}.mjs.map"));
        let script = path(&format!("out/main-{name}.mjs"));
        let source_root = path("src");
        let mut args = vec![
            "--compile",
            "--optimize",
            name,
            "--entry",
            "app.main",
            "--source-root",
            &source_root,
            "--esm",
        ];
        let plain = compiled(&args);
        args.extend(["--source-map", &map]);
        let mapped = compiled(&args);
        assert_eq!(
            mapped,
            format!("{plain}//# sourceMappingURL=main-{name}.mjs.map\n"),
            "{level}"
        );
        assert!(
            std::fs::read_to_string(&map)
                .unwrap()
                .contains("\"sources\":[\"../src/app/util.cljs\",\"../src/app/main.cljs\""),
            "{level}"
        );
        std::fs::write(&script, &mapped).unwrap();
        assert_frame(
            &first_frame(&script),
            "check",
            &format!("{}:5:12", real("src/app/util.cljs")),
            name,
        );
    }

    // A `:` in a file name is no URL scheme.
    let mapped = compiled(&[
        "--compile",
        &path("app.cljs"),
        "--source-map",
        &path("out/app:dev.js.map"),
    ]);
    assert!(mapped.ends_with("\n//# sourceMappingURL=app%3Adev.js.map\n"));
    std::fs::write(path("out/app:dev.js"), &mapped).unwrap();
    assert_frame(
        &first_frame(&path("out/app:dev.js")),
        "explode",
        &format!("{}:2:10", real("app.cljs")),
        "colon",
    );

    // Sources of an inline map are relative to the current directory,
    // where the JavaScript is expected.
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(&root)
        .args([
            "--compile",
            "--optimize",
            "advanced",
            "--source-map-inline",
            "--source-map-include-sources",
            "app.cljs",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let inline = String::from_utf8(output.stdout).unwrap();
    let (code, url) = inline.split_once("//# sourceMappingURL=").unwrap();
    assert_eq!(
        code,
        compiled(&["--compile", "--optimize", "advanced", &path("app.cljs")])
    );
    assert!(url.starts_with("data:application/json;charset=utf-8;base64,"));
    let script = path("inline.js");
    std::fs::write(&script, &inline).unwrap();
    assert_frame(
        &first_frame(&script),
        "explode",
        &format!("{}:2:10", real("app.cljs")),
        "inline",
    );
    // The text of every source, the runtime's too.
    let map = path("out/sources.js.map");
    compiled(&[
        "--compile",
        "--optimize",
        "advanced",
        &path("app.cljs"),
        "--source-map",
        &map,
        "--source-map-include-sources",
    ]);
    let json = std::fs::read_to_string(&map).unwrap();
    assert!(
        json.contains(&format!("\"sourcesContent\":[{program:?},\"")),
        "{json:.300}"
    );
    assert!(json.len() > runtime.len());

    for (args, message) in [
        (
            &["--compile", "--text", "1", "--source-map-include-sources"][..],
            "--source-map-include-sources requires --source-map PATH or --source-map-inline",
        ),
        (
            &[
                "--compile",
                "--source-map",
                "x.map",
                "--source-map-inline",
                "--text",
                "1",
            ][..],
            "--source-map and --source-map-inline exclude each other",
        ),
        (
            &["--compile", "--text", "1", "--source-map"][..],
            "--source-map requires a path",
        ),
        (
            &["--parse", "--source-map-inline", "--text", "1"][..],
            "--source-map-inline requires --compile",
        ),
    ] {
        let output = light_cljs(args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

/// A `key value` line of a `--stats` report.
type StatsCounter = (String, f64);

/// The counters and the per-namespace table of a `--stats` report, checking
/// its shape: a version line, `key value` counters, then
/// `[macro-calls-by-namespace]` and `NAMESPACE COUNT` rows.
fn parse_stats(report: &str) -> (Vec<StatsCounter>, Vec<(String, usize)>) {
    let mut lines = report.lines();
    assert_eq!(lines.next(), Some("light-cljs-stats 1"), "{report}");
    let mut counters = Vec::new();
    let mut table = Vec::new();
    let mut in_table = false;
    for line in lines {
        if line == "[macro-calls-by-namespace]" {
            in_table = true;
            continue;
        }
        let (key, value) = line.split_once(' ').expect(line);
        assert!(!value.contains(' '), "{line}");
        if in_table {
            table.push((key.to_owned(), value.parse().expect(line)));
        } else {
            counters.push((key.to_owned(), value.parse().expect(line)));
        }
    }
    assert!(in_table, "{report}");
    (counters, table)
}

#[test]
fn stats_reports_phase_times_and_macro_calls_on_stderr_without_changing_output() {
    let sources = TempDir::new("cli-stats");
    std::fs::create_dir(sources.join("app")).unwrap();
    let write =
        |name: &str, text: &str| std::fs::write(sources.join("app").join(name), text).unwrap();
    write(
        "macros.cljs",
        "(ns app.macros) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))",
    );
    write(
        "dep.cljs",
        "(ns app.dep (:require [app.macros :refer [next-value]])) (def value (next-value))",
    );
    write("plain.cljs", "(ns app.plain) (def answer 42)");
    write(
        "main.cljs",
        "(ns app.main (:require [app.dep :as d] [app.plain :as p] [app.macros :refer [next-value]])) (js/console.log d/value (next-value) p/answer)",
    );
    let root = sources.path().to_str().unwrap();
    let entry = "app.main";
    let plain = light_cljs(&["--compile", "--entry", entry, "--source-root", root]);
    assert!(plain.status.success());
    assert!(plain.stderr.is_empty());
    // Before or after the namespace options.
    for args in [
        &[
            "--compile",
            "--stats",
            "--entry",
            entry,
            "--source-root",
            root,
        ][..],
        &[
            "--compile",
            "--entry",
            entry,
            "--source-root",
            root,
            "--stats",
        ][..],
    ] {
        let output = light_cljs(args);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stderr}");
        assert_eq!(output.stdout, plain.stdout);
        let (counters, table) = parse_stats(&stderr);
        let value = |key: &str| {
            counters
                .iter()
                .find(|(name, _)| name == key)
                .unwrap_or_else(|| panic!("{key} in {stderr}"))
                .1
        };
        for phase in [
            "resolve-parse",
            "analysis",
            "deferred-units",
            "prepare-application",
            "emission",
            "write",
        ] {
            let time = value(&format!("time.{phase}-ms"));
            assert!(time >= 0.0 && time <= value("time.wall-ms"), "{phase}");
        }
        assert_eq!(value("namespaces"), 4.0);
        assert_eq!(value("macro-calls"), 3.0);
        assert!(value("macro-nodes.peak") > 0.0);
        assert!(value("deferred-units.registered") >= value("deferred-units.analyzed"));
        assert_eq!(
            value("deferred-units.analyzed"),
            value("deferred-units.prepared")
        );
        // Most calls first, ties in the order of analysis.
        assert_eq!(
            table,
            [
                ("app.macros", 1),
                ("app.dep", 1),
                ("app.main", 1),
                ("app.plain", 0)
            ]
            .map(|(name, count)| (name.to_owned(), count))
        );
    }
    // The startup build of --reloadable, with the same counts.
    let out_dir = sources.join("out");
    let output = light_cljs(&[
        "--compile",
        "--entry",
        entry,
        "--source-root",
        root,
        "--reloadable",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--stats",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    let (counters, _) = parse_stats(&stderr);
    assert!(
        counters.contains(&("macro-calls".to_owned(), 3.0)),
        "{stderr}"
    );
    assert!(out_dir.join("manifest.json").exists());
    // A single source is no namespace compilation.
    let output = light_cljs(&["--compile", "--stats", "--text", "1"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("--stats reports a namespace compilation"),
        "{stderr}"
    );
}

/// `--stats-file PATH` writes the exclusive-stage trace: stage totals that
/// add up to the traced wall time, each namespace with its requires, and each
/// unit with its demander. A macro the top level calls demands its helper
/// from the top-level pass; a macro a unit's body expands demands its helper
/// from that unit; the rest are drained. Output is unchanged.
#[test]
fn stats_file_traces_exclusive_stages_and_demand_edges() {
    let sources = TempDir::new("cli-stats-file");
    std::fs::create_dir(sources.join("app")).unwrap();
    let write =
        |name: &str, text: &str| std::fs::write(sources.join("app").join(name), text).unwrap();
    write(
        "macros.cljs",
        "(ns app.macros)
         (defn inner [x] (list 'inc x))
         (defmacro twice [x] (inner (inner x)))
         (defn outer [x] (twice x))
         (defmacro bump [x] (outer x))
         (js/console.log (bump 41))
         (def bumped (bump 41))
         (def native (let [a 1] a))
         (def plain (+ 1 2))",
    );
    write(
        "main.cljs",
        "(ns app.main (:require [app.macros :as m :refer [bump]])) (js/console.log (bump 1) m/bumped m/plain)",
    );
    let root = sources.path().to_str().unwrap();
    let args = ["--compile", "--entry", "app.main", "--source-root", root];
    let plain = light_cljs(&args);
    assert!(plain.status.success());
    let file = sources.join("trace.tsv");
    let mut traced = args.to_vec();
    traced.extend(["--stats-file", file.to_str().unwrap()]);
    let output = light_cljs(&traced);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    assert_eq!(output.stdout, plain.stdout);
    // The report keeps its shape, with the trace's counters.
    let (counters, _) = parse_stats(&stderr);
    let value = |key: &str| {
        counters
            .iter()
            .find(|(name, _)| name == key)
            .unwrap_or_else(|| panic!("{key} in {stderr}"))
            .1
    };
    let trace = std::fs::read_to_string(&file).unwrap();
    let mut lines = trace.lines();
    assert_eq!(lines.next(), Some("light-cljs-trace\t2"));
    let rows = |kind: &str| {
        trace
            .lines()
            .filter_map(|line| {
                let (row, rest) = line.split_once('\t')?;
                (row == kind).then(|| rest.split('\t').collect::<Vec<_>>())
            })
            .collect::<Vec<_>>()
    };
    // Exclusive stage times add up to the traced wall time.
    let stages = rows("stage");
    let stage = |name: &str| -> u128 {
        stages
            .iter()
            .find(|row| row[0] == name)
            .unwrap_or_else(|| panic!("stage {name} in {trace}"))[1]
            .parse()
            .unwrap()
    };
    let exclusive: u128 = [
        "other",
        "load",
        "prepare-source",
        "top-level",
        "drain",
        "append",
        "index",
        "release",
        "warnings",
        "unit",
        "finish",
    ]
    .into_iter()
    .map(stage)
    .sum();
    let wall = stage("trace-wall");
    assert!(exclusive <= wall && wall - exclusive <= 11, "{trace}");
    assert!(stage("wall") >= wall);
    assert_eq!(value("trace.units"), 5.0);
    // Namespaces by source index, with their requires.
    let namespaces = rows("namespace");
    let summary: Vec<_> = namespaces
        .iter()
        .map(|row| (row[0], row[1], row[13]))
        .collect();
    assert_eq!(
        summary,
        [("0", "app.macros", ""), ("1", "app.main", "app.macros")]
    );
    // Each unit's name, demander and depth.
    let units = rows("unit");
    let edges: Vec<_> = units
        .iter()
        .map(|row| (row[2], row[12], row[13], row[14]))
        .collect();
    assert_eq!(
        edges,
        [
            ("app.macros/inner", "unit", "1", "1"),
            ("app.macros/outer", "top-level", "0", "0"),
            ("app.macros/bumped", "drain", "0", "0"),
            ("app.macros/native", "drain", "0", "0"),
            ("app.macros/plain", "drain", "0", "0"),
        ],
        "{trace}"
    );
    let unit = |name: &str| units.iter().find(|row| row[2] == name).unwrap();
    // A native macro is a macro call, though not a host entry.
    assert_eq!(unit("app.macros/native")[9], "1");
    assert_eq!(unit("app.macros/plain")[9], "0");
    // The first evaluation of each root, with its demander: `bump` at the
    // top level evaluates `outer`, whose analysis expands `twice`, which
    // evaluates `inner`.
    let ids = |name: &str| unit(name)[0].to_owned();
    let executions: Vec<_> = rows("execute")
        .iter()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect::<Vec<_>>())
        .collect();
    assert_eq!(
        executions,
        [
            [
                ids("app.macros/outer"),
                "top-level".into(),
                "0".into(),
                "0".into()
            ],
            [
                ids("app.macros/inner"),
                "unit".into(),
                ids("app.macros/outer"),
                "0".into()
            ],
        ]
    );
    assert_eq!(value("trace.executions"), 2.0);
    // Every root a force touches, with the frame it was read in, once per
    // pair: the first evaluations again, and the reads of `outer` once it is
    // ready, by `bumped`'s expansion of `bump` and by `app.main`'s top level.
    // `inner`'s read is recorded first: its frame's entry ended first.
    let reads: Vec<_> = rows("read")
        .iter()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect::<Vec<_>>())
        .collect();
    let read =
        |unit: &str, kind: &str, demander: String| vec![ids(unit), kind.to_owned(), demander];
    assert_eq!(
        reads,
        [
            read("app.macros/inner", "unit", ids("app.macros/outer")),
            read("app.macros/outer", "top-level", "0".into()),
            read("app.macros/outer", "unit", ids("app.macros/bumped")),
            read("app.macros/outer", "top-level", "1".into()),
        ],
        "{trace}"
    );
    assert_eq!(value("trace.reads"), 4.0);
    assert_eq!(value("trace.reads.cross-namespace"), 1.0);
    // `outer` waited for `inner`, which it demanded.
    assert_eq!(unit("app.macros/outer")[8], unit("app.macros/inner")[5]);
    // Positions are the forms' lines.
    assert_eq!(unit("app.macros/plain")[3], "9");
    // Only the units that run a JavaScript macro enter the host.
    if cfg!(feature = "v8-macros") {
        let host: Vec<_> = units.iter().map(|row| (row[2], row[11])).collect();
        assert_eq!(
            host,
            [
                ("app.macros/inner", "0"),
                ("app.macros/outer", "1"),
                ("app.macros/bumped", "1"),
                ("app.macros/native", "0"),
                ("app.macros/plain", "0"),
            ]
        );
        assert_eq!(value("trace.units.host"), 2.0);
        assert_eq!(value("trace.units.without-macro-calls"), 1.0);
        assert!(value("trace.engine-ms") > 0.0);
    }
    // `--stats-file` needs a path, and a namespace compilation.
    let output = light_cljs(&["--compile", "--text", "1", "--stats-file"]);
    assert!(!output.status.success());
    let output = light_cljs(&["--compile", "--stats-file", "x.tsv", "--text", "1"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("--stats reports a namespace compilation")
    );
}

/// The read edges of `--stats-file` cover roots whose JavaScript was
/// prepared before their first evaluation and record bundles, such as a
/// record's, when a macro forces them; each (demander, unit) pair once.
#[test]
fn stats_file_traces_prepared_and_bundle_root_reads() {
    let sources = TempDir::new("cli-stats-file-reads");
    std::fs::create_dir(sources.join("app")).unwrap();
    let write =
        |name: &str, text: &str| std::fs::write(sources.join("app").join(name), text).unwrap();
    write(
        "shapes.cljs",
        "(ns app.shapes)
         (defprotocol Area (area [s]))
         (defrecord Square [n] Area (area [_] (* n n)))
         (defn square [n] (->Square n))
         (defmacro square-area [n] (area (square n)))",
    );
    write(
        "main.cljs",
        "(ns app.main (:require [app.shapes :refer [square-area]]))
         (js/console.log (square-area 3) (square-area 4))
         (def later (square-area 5))
         (js/console.log later)",
    );
    let root = sources.path().to_str().unwrap();
    let file = sources.join("trace.tsv");
    let output = light_cljs(&[
        "--compile",
        "--entry",
        "app.main",
        "--source-root",
        root,
        "--stats-file",
        file.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = std::fs::read_to_string(&file).unwrap();
    let rows = |kind: &str| {
        trace
            .lines()
            .filter_map(|line| {
                let (row, rest) = line.split_once('\t')?;
                (row == kind).then(|| rest.split('\t').map(str::to_owned).collect::<Vec<_>>())
            })
            .collect::<Vec<_>>()
    };
    // The protocol's and the record's declarations are bundles; `square` is
    // a plain root. All three were drained and prepared in app.shapes.
    let units = rows("unit");
    let id = |name: &str| {
        units
            .iter()
            .find(|row| row[2] == name)
            .unwrap_or_else(|| panic!("{name} in {trace}"))[0]
            .clone()
    };
    let (protocol, record, square, later) = (
        id("declaration#0"),
        id("declaration#1"),
        id("app.shapes/square"),
        id("app.main/later"),
    );
    // app.main's top level evaluates each for the first time, from
    // prepared JavaScript.
    let mut executions = rows("execute");
    executions.sort();
    let mut expected: Vec<_> = [&protocol, &record, &square]
        .into_iter()
        .map(|unit| vec![unit.clone(), "top-level".into(), "1".into(), "1".into()])
        .collect();
    expected.sort();
    assert_eq!(executions, expected, "{trace}");
    // Its two expansions read each once, and `later`'s expansion reads the
    // three ready ones again: `square`, the record's constructor it calls
    // and the protocol `area` dispatches on.
    let row = |unit: &String, kind: &str, demander: &str| {
        vec![unit.clone(), kind.to_owned(), demander.to_owned()]
    };
    assert_eq!(
        rows("read"),
        [
            row(&protocol, "top-level", "1"),
            row(&square, "top-level", "1"),
            row(&record, "top-level", "1"),
            row(&protocol, "unit", &later),
            row(&square, "unit", &later),
            row(&record, "unit", &later),
        ],
        "{trace}"
    );
}

/// The schedule models that read these traces pass their own Babashka tests
/// (benchmarks/concurrency/dag_test.bb): Option U's root edges on a
/// synthetic trace, and unchanged output on the retained probe run. Skipped
/// when `bb` is not on PATH.
#[test]
fn dag_model_tests_pass() {
    let Ok(probe) = Command::new("bb").arg("--version").output() else {
        eprintln!("bb unavailable; skipping benchmarks/concurrency/dag_test.bb");
        return;
    };
    if !probe.status.success() {
        eprintln!("bb unavailable; skipping benchmarks/concurrency/dag_test.bb");
        return;
    }
    let output = Command::new("bb")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/concurrency/dag_test.bb"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `LIGHT_CLJS_CAPTURE_ARTIFACTS=encode` encodes and decodes every captured
/// artifact and reports the sizes with `--stats`; the output is unchanged.
#[test]
fn codec_measurement_reports_sizes_without_changing_output() {
    let sources = TempDir::new("cli-codec");
    std::fs::create_dir(sources.join("app")).unwrap();
    let write =
        |name: &str, text: &str| std::fs::write(sources.join("app").join(name), text).unwrap();
    write(
        "macros.cljs",
        "(ns app.macros) (defn helper [x] (list 'inc x)) (defmacro bump [x] (helper x))",
    );
    write(
        "main.cljs",
        "(ns app.main (:require [app.macros :refer [bump]])) (js/console.log (bump 41))",
    );
    let root = sources.path().to_str().unwrap();
    let args = ["--compile", "--entry", "app.main", "--source-root", root];
    let plain = light_cljs(&args);
    assert!(plain.status.success());
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("LIGHT_CLJS_CAPTURE_ARTIFACTS", "encode")
        .args(args)
        .arg("--stats")
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    assert_eq!(output.stdout, plain.stdout);
    let (counters, _) = parse_stats(&stderr);
    let value = |key: &str| {
        counters
            .iter()
            .find(|(name, _)| name == key)
            .unwrap_or_else(|| panic!("{key} in {stderr}"))
            .1
    };
    assert_eq!(value("cache.capture.namespaces"), 2.0);
    assert_eq!(value("cache.codec.artifacts"), 2.0);
    assert_eq!(value("cache.codec.failed"), 0.0);
    let sections: f64 = [
        "strings",
        "declarations",
        "ir",
        "units",
        "diagnostics",
        "names",
    ]
    .iter()
    .map(|section| {
        let bytes = value(&format!("cache.codec.section.{section}"));
        assert!(bytes > 0.0, "{section}");
        bytes
    })
    .sum();
    let total = value("cache.codec.bytes");
    // The rest is the two headers and manifests.
    assert!(sections < total && total - sections < 2000.0, "{stderr}");
    assert!(value("cache.codec.largest") <= total);
    assert!(value("cache.codec.largest") * 2.0 >= total);
    let source = std::fs::metadata(sources.join("app/macros.cljs"))
        .unwrap()
        .len()
        + std::fs::metadata(sources.join("app/main.cljs"))
            .unwrap()
            .len();
    assert_eq!(value("cache.codec.source-bytes"), source as f64);
    assert!(value("cache.codec.ir-depth") > 1.0);
    assert!(value("cache.encode-ms") >= 0.0 && value("cache.decode-ms") >= 0.0);
    // Every namespace is keyed, and nothing changed after it was read.
    assert_eq!(value("cache.keys.namespaces"), 2.0);
    assert_eq!(value("cache.keys.changed"), 0.0);
    assert_eq!(value("cache.keys.source-bytes"), source as f64);
    assert!(value("cache.keys-ms") >= 0.0 && value("cache.revalidate-ms") >= 0.0);
}

#[test]
fn jobs_counts_work_slots_without_changing_output() {
    let root = "examples/namespaces";
    let entry = ["--compile", "--entry", "demo.main", "--source-root", root];
    let serial = compiled(&entry);
    for jobs in ["1", "4"] {
        // Before or after the namespace options, and with a cache.
        assert_eq!(
            compiled(&[&["--compile", "--jobs", jobs], &entry[1..]].concat()),
            serial
        );
        assert_eq!(compiled(&[&entry[..], &["--jobs", jobs]].concat()), serial);
        let cache = TempDir::new("cli-jobs-cache");
        let dir = cache.path().to_str().unwrap();
        let cached = [&entry[..], &["--jobs", jobs, "--cache-dir", dir]].concat();
        assert_eq!(compiled(&cached), serial);
    }
    // A single source has one namespace; the flag is accepted there too.
    assert_eq!(
        compiled(&["--compile", "--jobs", "8", "--text", "(+ 1 2)"]),
        compiled(&["--compile", "--text", "(+ 1 2)"])
    );
    for (args, message) in [
        (
            &["--compile", "--jobs", "0", "--text", "1"][..],
            "--jobs requires a positive integer, got 0",
        ),
        (
            &["--compile", "--text", "1", "--jobs", "many"][..],
            "--jobs requires a positive integer, got many",
        ),
        (
            &["--compile", "--text", "1", "--jobs"][..],
            "--jobs requires a number",
        ),
        (
            &["--parse", "--jobs", "2", "--text", "1"][..],
            "--jobs requires --compile",
        ),
    ] {
        let output = light_cljs(args);
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

/// With `--jobs N`, units are analyzed in persistent worker isolates, and
/// the program is the one a fresh isolate per unit gives, the correctness
/// reference (docs/compiler-concurrency.md, "As built (light-cljs-339)").
/// The counter fixture prints `1 1` with several jobs, the documented
/// undeclared behavior ("Semantics and determinism"), and `1 2`, the one-job
/// program, with `--cache-blocker macros`, whose consumers keep their units
/// on the main isolate (light-cljs-340).
#[test]
fn worker_isolates_match_a_fresh_isolate_per_unit() {
    let root = TempDir::new("cljs-cli-worker-isolates");
    std::fs::write(
        root.join("macros.cljs"),
        "(ns macros) (def counter (atom 0)) (defn helper [x] (list 'inc x)) \
         (defmacro next-value [] (swap! counter inc)) (defmacro twice [x] (helper (helper x)))",
    )
    .unwrap();
    std::fs::write(
        root.join("dep.cljs"),
        "(ns dep (:require [macros :refer [next-value twice]])) (def value (next-value)) \
         (defn f [n] (when (pos? n) (twice n))) (def g (vec (for [i (range 3)] (twice i))))",
    )
    .unwrap();
    std::fs::write(
        root.join("main.cljs"),
        "(ns main (:require [dep :as d] [macros :refer [next-value]])) \
         (js/console.log d/value (next-value) (d/f 1) (pr-str d/g))",
    )
    .unwrap();
    let root_arg = root.to_str().unwrap();
    let entry = ["--compile", "--entry", "main", "--source-root", root_arg];
    let run = |jobs: &[&str], env: &[(&str, &str)]| {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([&entry[..], &["--jobs"], jobs].concat())
            .envs(env.iter().copied())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{jobs:?} {env:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let node = |js: &str| {
        let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .args(["--eval", js])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    };
    let serial = run(&["1"], &[]);
    assert_eq!(node(&serial), "1 2 3 [2 3 4]\n");
    let several = if cfg!(feature = "v8-macros") {
        "1 1 3 [2 3 4]\n"
    } else {
        "1 2 3 [2 3 4]\n"
    };
    for jobs in ["2", "4", "8"] {
        let persistent = run(&[jobs], &[]);
        assert_eq!(node(&persistent), several, "{jobs} jobs");
        let fresh = run(&[jobs], &[("LIGHT_CLJS_FRESH_UNIT_ISOLATES", "1")]);
        assert_eq!(fresh, persistent, "{jobs} jobs");
        let blocked = run(&[jobs, "--cache-blocker", "macros"], &[]);
        assert_eq!(blocked, serial, "{jobs} jobs");
    }
}

#[test]
fn a_build_stopped_at_its_deadline_writes_no_output_at_any_job_count() {
    // Every unit's macro spins until the build's one deadline stops it, on
    // whichever isolate it runs: the build fails and writes nothing.
    let root = TempDir::new("cljs-cli-deadline");
    std::fs::write(
        root.join("macros.cljs"),
        "(ns macros) (defmacro spin [] (loop [] (recur)))",
    )
    .unwrap();
    let mut body = String::from("(ns main (:require [macros :refer [spin]]))\n");
    for k in 0..6 {
        body.push_str(&format!("(defn f{k} [] (spin))\n"));
    }
    std::fs::write(root.join("main.cljs"), body).unwrap();
    for jobs in ["1", "4"] {
        let out = root.join(format!("out-{jobs}"));
        let started = std::time::Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args([
                "--compile",
                "--entry",
                "main",
                "--source-root",
                root.to_str().unwrap(),
                "--out-dir",
                out.to_str().unwrap(),
                "--jobs",
                jobs,
                "--macro-timeout-ms",
                "400",
                "--macro-poll-budget",
                "1000000000",
                "--macro-memory-limit",
                "268435456",
            ])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{jobs} jobs");
        assert!(
            stderr.contains("deadline exceeded"),
            "{jobs} jobs: {stderr}"
        );
        assert!(output.stdout.is_empty(), "{jobs} jobs");
        assert!(!out.join("main.js").exists(), "{jobs} jobs");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{jobs} jobs: {:?}",
            started.elapsed()
        );
    }
}
