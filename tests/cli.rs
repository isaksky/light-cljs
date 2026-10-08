use std::{
    io::Write,
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
        (
            "1\n  (let [x 1] x)",
            "2:4: unsupported construct: macro execution (let)",
        ),
        ("(λ\n ]", "2:2: expected ), found ] (bytes 5..6)"),
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
