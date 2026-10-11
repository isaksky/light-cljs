//! Complete retained core adapters and literal contracts in both JS phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const FIXTURES: &str = include_str!("fixtures/compiler-core-gate.tsv");

fn node(script: &str) -> String {
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node is required for canonical application evidence");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("core gate Node execution exceeded 30 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn both(source: &str, expected: &str, identity: &str) {
    let parsed = parse(source).unwrap_or_else(|error| panic!("{identity}: {error}"));
    let program = analyze_with_macros(source, &parsed, &mut MacroHost::new())
        .unwrap_or_else(|error| panic!("{identity}: {error}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator
        .evaluate(&program)
        .unwrap_or_else(|error| panic!("QuickJS {identity}: {error}"));
    assert_eq!(
        evaluator.print(value).unwrap(),
        expected,
        "QuickJS {identity}"
    );
    let script = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&program)
    );
    assert_eq!(node(&script), expected, "Node {identity}");
}

fn cohort(group: &str, expected_workflows: usize) {
    let rows: Vec<_> = FIXTURES
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|columns| columns[0] == group)
        .collect();
    assert_eq!(
        rows.len(),
        expected_workflows,
        "missing/partial {group} cohort"
    );
    for columns in rows {
        assert_eq!(columns.len(), 5);
        both(columns[2], columns[3], columns[1]);
        // Returning finite literal expansion data keeps application and phase
        // observations independent; every original assertion runs during expansion.
        both(
            &format!("(defmacro core-gate [] {}) (core-gate)", columns[2]),
            columns[3],
            columns[1],
        );
    }
}

#[test]
fn core_numeric_applications_and_macros_execute() {
    cohort("numeric", 9);
}

#[test]
fn core_collections_applications_and_macros_execute() {
    cohort("collections", 28);
}

#[test]
fn core_state_applications_and_macros_execute() {
    cohort("state", 9);
}

#[test]
fn core_control_applications_and_macros_execute() {
    cohort("control", 14);
}

#[test]
fn core_exceptions_applications_and_macros_execute() {
    cohort("exceptions", 4);
}

#[test]
fn core_gate_preserves_complete_original_adapters_and_expectations() {
    let result = Command::new("python3")
        .args(["-B", "tests/reference/compiler-core-gate.py"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
