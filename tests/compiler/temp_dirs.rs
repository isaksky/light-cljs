//! The temporary-directory guard (`tests/support/temp_dir.rs`): a test that fails
//! still removes its directory, unless `LIGHT_CLJS_KEEP_TMP` is set, and never
//! removes a directory an environment variable names. Each check runs a
//! deliberately failing test in a child process with its own empty temporary
//! directory.
use crate::temp_dir::{KEEP, TempDir};
use std::{fs, path::Path, process::Command};

/// Set only in the child process, to the guard constructor [`failing_child`] uses
/// before it fails: `new`, `named_by` or `new_named_by`.
const CHILD: &str = "LIGHT_CLJS_TEMP_DIR_GUARD_CHILD";
/// The variable the child's `named_by` and `new_named_by` read.
const NAMED: &str = "LIGHT_CLJS_TEMP_DIR_GUARD_NAMED";

#[test]
#[ignore = "fails on purpose in the child process of the tests below; passes elsewhere"]
fn failing_child() {
    let dir = match std::env::var(CHILD).as_deref() {
        Ok("new") => TempDir::new("guard-child"),
        Ok("named_by") => TempDir::named_by(NAMED, "guard-child"),
        Ok("new_named_by") => TempDir::new_named_by(NAMED, "guard-child"),
        _ => return,
    };
    fs::write(dir.join("output.txt"), "partial output").unwrap();
    panic!("deliberate failure holding {}", dir.display());
}

/// Runs [`failing_child`] with the guard constructor `mode`, `LIGHT_CLJS_KEEP_TMP`
/// set to `keep` (or unset) and `named` as the directory its variable names.
/// Returns the child's temporary directory, the names left in it and the child's
/// stderr.
fn run_failing_child(
    mode: &str,
    keep: Option<&str>,
    named: Option<&Path>,
) -> (TempDir, Vec<String>, String) {
    let tmpdir = TempDir::new("guard-parent");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "temp_dirs::failing_child",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD, mode);
    // `std::env::temp_dir` reads TMPDIR on Unix and TMP or TEMP on Windows.
    for variable in ["TMPDIR", "TMP", "TEMP"] {
        command.env(variable, tmpdir.path());
    }
    match keep {
        Some(value) => command.env(KEEP, value),
        None => command.env_remove(KEEP),
    };
    match named {
        Some(path) => command.env(NAMED, path),
        None => command.env_remove(NAMED),
    };
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "the child test passed:\n{stdout}");
    assert!(stdout.contains("1 failed"), "{stdout}\n{stderr}");
    let mut names: Vec<_> = fs::read_dir(&tmpdir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    (tmpdir, names, stderr)
}

#[test]
fn a_failing_test_removes_its_directory() {
    for keep in [None, Some("0"), Some("")] {
        let (_, names, stderr) = run_failing_child("new", keep, None);
        assert!(stderr.contains("deliberate failure holding"), "{stderr}");
        assert_eq!(names, Vec::<String>::new(), "{keep:?}");
    }
}

#[test]
fn keep_tmp_keeps_the_directory_of_a_failing_test() {
    let (tmpdir, names, stderr) = run_failing_child("new", Some("1"), None);
    assert!(stderr.contains("deliberate failure holding"), "{stderr}");
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("light-guard-child-"), "{names:?}");
    let output = fs::read_to_string(tmpdir.join(&names[0]).join("output.txt")).unwrap();
    assert_eq!(output, "partial output");
}

#[test]
fn a_failing_test_keeps_a_directory_its_variable_names() {
    // Outside the child's temporary directory, holding a file of its own.
    let named = TempDir::new("guard-named");
    fs::write(named.join("sentinel.txt"), "kept").unwrap();
    let (_, names, stderr) = run_failing_child("named_by", None, Some(&named));
    assert!(stderr.contains("deliberate failure holding"), "{stderr}");
    assert_eq!(names, Vec::<String>::new());
    assert_eq!(
        fs::read_to_string(named.join("sentinel.txt")).unwrap(),
        "kept"
    );
    assert_eq!(
        fs::read_to_string(named.join("output.txt")).unwrap(),
        "partial output"
    );
}

#[test]
fn new_named_by_rejects_an_existing_directory_and_leaves_it_alone() {
    let named = TempDir::new("guard-named");
    fs::write(named.join("sentinel.txt"), "kept").unwrap();
    let (_, names, stderr) = run_failing_child("new_named_by", None, Some(&named));
    assert!(stderr.contains("names an existing directory"), "{stderr}");
    assert_eq!(names, Vec::<String>::new());
    let mut entries: Vec<_> = fs::read_dir(&named)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    entries.sort();
    assert_eq!(entries, ["sentinel.txt"]);
}
