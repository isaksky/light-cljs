//! The inventory/accounting contract is fast and independent of Java/research.
use std::process::Command;

#[test]
fn compiler_language_accounting_never_turns_missing_work_into_passes() {
    for test in ["test_inventory.py", "test_report.py", "test_full_ledger.py"] {
        let output = Command::new(std::env::var_os("PYTHON").unwrap_or_else(|| "python3".into()))
            .args(["-B", &format!("tests/reference/compiler-language/{test}")])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("Python 3 is required for the compiler language inventory checks");
        assert!(
            output.status.success(),
            "{test}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
