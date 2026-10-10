//! Core helpers checked with literal host outcomes and independent runtime tests.
use light_cljs::compile;
use std::{path::PathBuf, process::Command};

#[test]
fn helper_arities_order_lazy_tails_and_collections_match_expected_values() {
    for line in include_str!("fixtures/compiler-core-helpers.tsv").lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let javascript =
            compile(fields[1]).unwrap_or_else(|error| panic!("{}: {error}", fields[0]));
        let output = Command::new("node")
            .args([
                "--max-old-space-size=96",
                "-e",
                &format!("console.log(JSON.stringify({javascript}));"),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            fields[0],
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            fields[2],
            "{}",
            fields[0]
        );
    }
}

#[test]
fn helper_callbacks_and_owned_runtime_values_follow_the_callable_abi() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/js_core_helpers.test.js");
    let output = Command::new("node").arg(path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
