//! Complete upstream namespace bodies and setup execute in both phases.
use light_cljs::MacroHost;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::path::Path;
use std::process::Command;

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for item in std::fs::read_dir(source).unwrap() {
        let item = item.unwrap();
        let output = destination.join(item.file_name());
        if item.file_type().unwrap().is_dir() {
            copy_tree(&item.path(), &output);
        } else {
            std::fs::copy(item.path(), output).unwrap();
        }
    }
}

#[test]
fn complete_residual_namespace_bodies_execute_in_both_phases() {
    let rows: Vec<_> = include_str!("fixtures/compiler-residual-source-context.tsv")
        .lines()
        .filter(|row| !row.starts_with('#') && !row.is_empty())
        .collect();
    assert_eq!(
        rows.len(),
        13,
        "empty or partial supported namespace selection"
    );
    let mut reused_host = MacroHost::new();
    for row in rows {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 5);
        for reused in [false, true] {
            for macro_phase in [false, true] {
                eprintln!(
                    "complete source context: {} macro={macro_phase} reused={reused}",
                    fields[0]
                );
                let root = std::env::temp_dir()
                    .join(format!("light-residual-source-{}", std::process::id()));
                copy_tree(
                    &Path::new(env!("CARGO_MANIFEST_DIR")).join(fields[2]),
                    &root,
                );
                let file = root.join(fields[3]);
                let source = std::fs::read_to_string(&file).unwrap();
                let source = if macro_phase {
                    let (prefix, body) = source.split_once("\n(defn run [] ").unwrap();
                    format!(
                        "{prefix}\n(defmacro upstream [] {body}\n(defn run [] (upstream))\n(pr-str (run))"
                    )
                } else {
                    format!("{source}\n(pr-str (run))")
                };
                std::fs::write(file, source).unwrap();
                let mut fresh_host = MacroHost::new();
                let javascript = light_cljs::compile_namespace_with_macros(
                    fields[1],
                    &[root.clone()],
                    if reused {
                        &mut reused_host
                    } else {
                        &mut fresh_host
                    },
                )
                .unwrap_or_else(|error| panic!("{} macro={macro_phase}: {error}", fields[0]));
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                assert_eq!(
                    evaluator
                        .string(evaluator.evaluate_js(&javascript).unwrap())
                        .unwrap(),
                    "true"
                );
                let output = Command::new("node")
                    .args(["-e", &format!("console.log({javascript});")])
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "true");
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }
}
