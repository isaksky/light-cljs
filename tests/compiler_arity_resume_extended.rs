//! Complete original helper workflows with strict JSON literals and actual phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};

const INPUT: &str = include_str!("fixtures/compiler-core-helpers.tsv");
const IDS: &[&str] = &[
    "trampoline-and-comparator",
    "update-map-metadata",
    "language-helper-partition-vectors",
    "language-helper-iteration-order",
    "language-helper-tagged-values",
    "language-helper-printing",
    "language-helper-js-conversions",
    "language-helper-munging",
    "language-helper-cat-early-stop",
    "language-helper-subvec-metadata-source",
];

fn node(source: &str) -> String {
    let mut child = Command::new("node")
        .arg("--max-old-space-size=96")
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
    assert!(output.stderr.is_empty(), "unexpected stderr");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn complete_core_helpers_execute_in_actual_phases_and_lifecycles() {
    let rows: Vec<_> = INPUT
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter(|line| IDS.contains(&line.split('\t').next().unwrap()))
        .collect();
    assert_eq!(rows.len(), IDS.len(), "missing complete original helper");
    let artifacts = std::env::var_os("LIGHT_ARITY_EXTENDED_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("light-arity-extended-{}", std::process::id()))
        });
    let mut reused = MacroHost::new();
    let warm = artifacts.join("warmup");
    std::fs::create_dir_all(&warm).unwrap();
    std::fs::write(
        warm.join("warm.cljs"),
        "(ns warm) (defmacro initialized [] :warm) (initialized)",
    )
    .unwrap();
    let warmed = compile_namespace_with_macros("warm", &[warm], &mut reused).unwrap();
    std::fs::write(artifacts.join("warmup.js"), warmed).unwrap();
    for row in rows {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let id = fields[0];
        let source = fields[1];
        let literal = fields[2];
        // The fixture retains compact JSON text on one tab-separated line.
        // Quote that exact text as a ClojureScript string without new deps.
        let literal_string = format!("\"{}\"", literal.replace('\\', "\\\\").replace('"', "\\\""));
        let checked = format!("(= {literal_string} (js/JSON.stringify (do {source})))");
        for lifecycle in ["fresh", "reused"] {
            for phase in ["application", "macro"] {
                let program = if phase == "macro" {
                    format!(
                        "(ns user)\n(defmacro complete-helper [] {checked})\n(pr-str (complete-helper))\n"
                    )
                } else {
                    format!("(ns user)\n(pr-str {checked})\n")
                };
                let root = artifacts.join(id).join(lifecycle).join(phase);
                std::fs::create_dir_all(&root).unwrap();
                std::fs::write(root.join("user.cljs"), program).unwrap();
                let mut fresh = MacroHost::new();
                let host = if lifecycle == "fresh" {
                    &mut fresh
                } else {
                    &mut reused
                };
                let javascript = compile_namespace_with_macros("user", &[root.clone()], host)
                    .unwrap_or_else(|error| panic!("{id} {lifecycle} {phase}: {error}"));
                std::fs::write(root.join("compiled.js"), &javascript).unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&javascript).unwrap())
                    .unwrap();
                assert_eq!(actual, "true", "{id} {lifecycle} {phase} QuickJS");
                println!(
                    "arity-extended-observation\t{id}\t{lifecycle}\t{phase}\tQuickJS\t{actual}"
                );
                let actual = node(&format!("console.log(String({javascript}));"));
                assert_eq!(actual, "true", "{id} {lifecycle} {phase} Node");
                println!("arity-extended-observation\t{id}\t{lifecycle}\t{phase}\tNode\t{actual}");
            }
        }
    }
}
