//! Complete receiver dispatch contracts; no native-function -invoke controls.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::io::Write;
use std::process::{Command, Stdio};
const FIXTURE: &str = include_str!("fixtures/compiler-ifn-public-context.tsv");
fn unhex(text: &str) -> String {
    String::from_utf8(
        text.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|part| u8::from_str_radix(std::str::from_utf8(part).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn node(source: &str) -> String {
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn execute(identity: &str) {
    let rows: Vec<_> = FIXTURE
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect();
    assert_eq!(rows.len(), 5);
    let selected: Vec<_> = rows.iter().filter(|row| row[0] == identity).collect();
    assert_eq!(selected.len(), 1);
    let row = selected[0];
    assert_eq!(row.len(), 5);
    let source = unhex(row[2]);
    let expected = unhex(row[3]);
    // EDN equality preserves every map key/value while allowing whitespace and
    // optional comma separators in printer output. Observe the full actual data
    // once, assert its adjacent independent literal, and retain its raw print.
    let observation = format!(
        "(let [actual {source}] (if (= '{expected} actual) (pr-str actual) (throw (js/Error. \"complete IFn literal contract mismatch\"))))"
    );
    for reuse in [false, true] {
        let mut reused = MacroHost::new();
        for macro_phase in [false, true] {
            let mut fresh = MacroHost::new();
            let host = if reuse { &mut reused } else { &mut fresh };
            let observed_source = if macro_phase {
                format!("(defmacro ifn-receiver-gate [] {observation}) (ifn-receiver-gate)")
            } else {
                observation.clone()
            };
            let program =
                analyze_with_macros(&observed_source, &parse(&observed_source).unwrap(), host)
                    .unwrap_or_else(|error| {
                        panic!("{identity} macro={macro_phase} reuse={reuse}: {error}")
                    });
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let actual = evaluator
                .string(evaluator.evaluate(&program).unwrap())
                .unwrap();
            println!(
                "ifn-observation\t{identity}\tmacro={macro_phase}\treuse={reuse}\tQuickJS\t{actual}"
            );
            let actual_node = node(&format!(
                "const rt = {RUNTIME_SOURCE}; console.log(({})(rt));",
                emit_js_unit(&program)
            ));
            println!(
                "ifn-observation\t{identity}\tmacro={macro_phase}\treuse={reuse}\tNode\t{actual_node}"
            );
            assert_eq!(
                actual_node, actual,
                "{identity} macro={macro_phase} reuse={reuse}"
            );
        }
    }
}
#[test]
fn ifn_receiver_zero_through_six_execute_in_both_phases() {
    execute("callable-fixed-0-6");
}
#[test]
fn ifn_receiver_seven_through_fourteen_execute_in_both_phases() {
    execute("callable-fixed-7-14");
}
#[test]
fn ifn_receiver_fifteen_through_twenty_execute_in_both_phases() {
    execute("callable-fixed-15-20");
}
#[test]
fn ifn_receiver_terminal_rest_slot_executes_in_both_phases() {
    execute("callable-receiver-rest-22");
}
#[test]
fn ifn_receiver_metadata_failure_and_recovery_execute_in_both_phases() {
    execute("callable-receiver-copy-failure");
}
