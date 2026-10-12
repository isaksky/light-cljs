//! Exact retained library contexts, including their actual MacroHost observations.
//! The standalone provenance helper supplies complete, source-bound modules and
//! separate local contracts; historical SCI expectations are never overwritten.
use light_cljs::MacroHost;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::io::Write;
use std::process::{Command, Stdio};

const IDS: [&str; 22] = [
    "sci-execution-sci-data-edn-tag-discard",
    "sci-execution-sci-string-join-values",
    "sci-execution-sci-string-case-coercion",
    "sci-execution-sci-string-trim-unicode",
    "sci-execution-sci-string-index-empty",
    "sci-execution-sci-string-index-coerce",
    "sci-execution-sci-string-escape-map",
    "sci-execution-sci-string-escape-surrogates",
    "sci-execution-sci-string-index-character-wrap",
    "sci-execution-sci-string-index-empty-wrap",
    "sci-execution-sci-data-set-project-meta",
    "sci-execution-sci-data-edn-nil-bad-options",
    "sci-execution-sci-data-edn-eof-false",
    "sci-execution-sci-data-edn-eof-keyword",
    "sci-execution-sci-data-edn-eof-throw-name",
    "sci-execution-sci-data-edn-eof-error",
    "sci-execution-sci-data-edn-reader-failure-identity",
    "sci-execution-sci-data-edn-reject-tag-plus",
    "sci-execution-sci-data-edn-reject-tag-minus",
    "sci-execution-sci-data-edn-reject-tag-space",
    "sci-execution-sci-data-edn-reject-dispatch-comment",
    "sci-execution-sci-data-set-project-key-representative",
];

fn unhex(value: &str) -> String {
    assert_eq!(value.len() % 2, 0, "Odd-length source encoding");
    String::from_utf8(
        value
            .as_bytes()
            .as_chunks::<2>().0.iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonical(evaluator: &Evaluator, expression: &str) -> String {
    let source = format!(
        "JSON.stringify((function sort(v) {{ if (Array.isArray(v)) return v.map(sort); \
         if (v && typeof v === 'object') {{ const r = {{}}; \
         Object.keys(v).sort().forEach(k => r[k] = sort(v[k])); return r; }} \
         return v; }})({expression}))"
    );
    evaluator
        .string(evaluator.evaluate_js(&source).unwrap())
        .unwrap()
}

#[test]
#[ignore = "The standalone helper supplies exact 22 contexts and records fresh provenance"]
fn exact_library_contexts_execute_in_both_phases_and_host_lifecycles() {
    let input = std::env::var("LIGHT_LIBRARY_RECONCILIATION_INPUT").unwrap();
    let output = std::env::var("LIGHT_LIBRARY_RECONCILIATION_OUTPUT").unwrap();
    let text = std::fs::read_to_string(input).unwrap();
    let rows: Vec<_> = text.lines().collect();
    assert_eq!(rows.len(), IDS.len(), "Incomplete exact library cohort");
    let output_path = std::path::Path::new(&output);
    let artifact = output_path.parent().unwrap();
    let mut receipt = std::fs::File::create(output_path).unwrap();
    let mut reused_host = MacroHost::new();
    for (id, row) in IDS.into_iter().zip(rows) {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], id, "Changed or reordered original identity");
        let expected = unhex(fields[3]);
        for (host_kind, reused) in [("fresh", false), ("reused", true)] {
            for (phase, source_field) in [("application", 1), ("macro", 2)] {
                eprintln!("exact library: {id} phase={phase} host={host_kind}");
                let root = artifact
                    .join("modules")
                    .join(id)
                    .join(host_kind)
                    .join(phase);
                std::fs::create_dir_all(&root).unwrap();
                std::fs::write(root.join("user.cljs"), unhex(fields[source_field])).unwrap();
                let mut fresh_host = MacroHost::new();
                let javascript = light_cljs::compile_namespace_with_macros(
                    "user",
                    std::slice::from_ref(&root),
                    if reused {
                        &mut reused_host
                    } else {
                        &mut fresh_host
                    },
                )
                .unwrap_or_else(|error| panic!("{id} {host_kind} {phase}: {error}"));
                let compiled = root.join("compiled.js");
                std::fs::write(&compiled, &javascript).unwrap();
                let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
                let actual = evaluator
                    .string(evaluator.evaluate_js(&javascript).unwrap())
                    .unwrap();
                let actual_canonical = canonical(&evaluator, &format!("JSON.parse({actual:?})"));
                assert_eq!(
                    actual_canonical,
                    canonical(&evaluator, &expected),
                    "{id} {host_kind} {phase} QuickJS"
                );
                writeln!(
                    receipt,
                    "{id}\t{host_kind}\t{phase}\tQuickJS\t{}\t\t{}",
                    hex(format!("{actual}\n").as_bytes()),
                    compiled.display()
                )
                .unwrap();
                receipt.flush().unwrap();
                let script = format!("const actual = {javascript}; console.log(actual);");
                std::fs::write(root.join("node.js"), &script).unwrap();
                let mut node = Command::new("node")
                    .arg("-")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                node.stdin
                    .take()
                    .unwrap()
                    .write_all(script.as_bytes())
                    .unwrap();
                let node = node.wait_with_output().unwrap();
                assert!(
                    node.status.success(),
                    "{id}: {}",
                    String::from_utf8_lossy(&node.stderr)
                );
                assert!(node.stderr.is_empty(), "Unexpected original Node stderr");
                let raw = String::from_utf8(node.stdout).unwrap();
                let node_canonical = canonical(&evaluator, &format!("JSON.parse({raw:?})"));
                assert_eq!(
                    node_canonical, actual_canonical,
                    "{id} {host_kind} {phase} Node"
                );
                writeln!(
                    receipt,
                    "{id}\t{host_kind}\t{phase}\tNode\t{}\t{}\t{}",
                    hex(raw.as_bytes()),
                    hex(&node.stderr),
                    compiled.display()
                )
                .unwrap();
                receipt.flush().unwrap();
            }
        }
    }
}
