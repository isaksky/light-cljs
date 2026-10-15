//! Whole original SCI sources and independent retained outcome/effect literals.
//! Alias checks are additional observations, never substitutes for source runs.
use crate::temp_dir::TempDir;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};

const INPUT: &str = include_str!("../fixtures/compiler-local-api-source-context.tsv");

fn unhex(value: &str) -> String {
    assert_eq!(value.len() % 2, 0);
    String::from_utf8(
        value
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
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
    assert!(output.stderr.is_empty(), "unexpected stderr");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn original_local_api_sources_execute_in_both_phases_and_host_lifecycles(start: usize, end: usize) {
    let rows: Vec<_> = INPUT
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert_eq!(rows.len(), 121, "whole reviewed cohort drift");
    let mut reused = MacroHost::new();
    let artifact = TempDir::named_by("LIGHT_LOCAL_API_SOURCE_ARTIFACTS", "local-api-source");
    for row in &rows[start..end] {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 4);
        let id = fields[0];
        let source = unhex(fields[1]);
        let expected = unhex(fields[2]);
        for host_kind in ["fresh", "reused"] {
            for phase in ["application", "macro"] {
                let program_source = if phase == "macro" {
                    format!(
                        "(ns user)\n(defmacro api-source-check [] (pr-str (= '{expected} {source})))\n(api-source-check)\n"
                    )
                } else {
                    format!(
                        "(ns user)\n(defn run [observe] (= '{expected} {source}))\n(pr-str (run nil))\n"
                    )
                };
                let root = artifact.join(id).join(host_kind).join(phase);
                std::fs::create_dir_all(&root).unwrap();
                std::fs::write(root.join("user.cljs"), &program_source).unwrap();
                let mut fresh = MacroHost::new();
                let host = if host_kind == "fresh" {
                    &mut fresh
                } else {
                    &mut reused
                };
                let javascript =
                    compile_namespace_with_macros("user", std::slice::from_ref(&root), host)
                        .unwrap_or_else(|error| panic!("{id} {host_kind} {phase}: {error}"));
                std::fs::write(root.join("compiled.js"), &javascript).unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&javascript).unwrap())
                    .unwrap();
                println!(
                    "local-api-source-observation\t{id}\t{host_kind}\t{phase}\tQuickJS\t{actual}"
                );
                assert_eq!(actual, "true", "{id} {host_kind} {phase} QuickJS");
                let script = format!("console.log({javascript});");
                std::fs::write(root.join("node.js"), &script).unwrap();
                let actual = node(&script);
                println!(
                    "local-api-source-observation\t{id}\t{host_kind}\t{phase}\tNode\t{actual}"
                );
                assert_eq!(actual, "true", "{id} {host_kind} {phase} Node");
            }
        }
    }
}

// Each range reuses one host across its rows; ranges run as parallel tests and
// together cover all 121 rows.
row_tests!(original_local_api_sources_execute_in_both_phases_and_host_lifecycles;
    original_local_api_sources_001_through_010_execute_in_both_phases(0, 10),
    original_local_api_sources_011_through_020_execute_in_both_phases(10, 20),
    original_local_api_sources_021_through_030_execute_in_both_phases(20, 30),
    original_local_api_sources_031_through_040_execute_in_both_phases(30, 40),
    original_local_api_sources_041_through_050_execute_in_both_phases(40, 50),
    original_local_api_sources_051_through_060_execute_in_both_phases(50, 60),
    original_local_api_sources_061_through_070_execute_in_both_phases(60, 70),
    original_local_api_sources_071_through_080_execute_in_both_phases(70, 80),
    original_local_api_sources_081_through_090_execute_in_both_phases(80, 90),
    original_local_api_sources_091_through_100_execute_in_both_phases(90, 100),
    original_local_api_sources_101_through_110_execute_in_both_phases(100, 110),
    original_local_api_sources_111_through_121_execute_in_both_phases(110, 121),
);
