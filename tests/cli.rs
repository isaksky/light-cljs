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
