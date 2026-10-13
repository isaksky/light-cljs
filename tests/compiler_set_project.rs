//! Canonical project metadata and complete SCI source in fresh/reused phases.
use light_cljs::{
    MacroHost,
    evaluator::{Evaluator, EvaluatorOptions},
};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn compile(
    host: &mut MacroHost,
    source: &str,
) -> Result<String, light_cljs::compiler::NamespaceCompileError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "light-set-project-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("set_project")).unwrap();
    fs::write(root.join("set_project/main.cljs"), source).unwrap();
    let result = light_cljs::compile_namespace_with_macros(
        "set-project.main",
        std::slice::from_ref(&root),
        host,
    );
    fs::remove_dir_all(root).unwrap();
    result
}

fn both(host: &mut MacroHost, source: &str, expected: &str) {
    let js = compile(host, source).unwrap_or_else(|e| panic!("{e}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .string(evaluator.evaluate_js(&js).unwrap())
            .unwrap(),
        expected
    );
    let output = Command::new("node")
        .args(["-e", &format!("console.log({js});")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{expected}\n")
    );
    eprintln!("Node/QuickJS literal: {expected}");
    assert!(output.stderr.is_empty());
}

#[test]
fn project_contract_executes_in_fresh_and_reused_application_and_macro_phases() {
    let contract = include_str!("fixtures/compiler-set-project.cljs");
    for (id, name) in [
        ("sci-data-set-project-meta", "original-relation-meta"),
        (
            "sci-data-set-project-key-representative",
            "original-key-representative",
        ),
    ] {
        let original = include_str!("fixtures/sci-outcomes.tsv")
            .lines()
            .find(|line| line.starts_with(&format!("{id}\t")))
            .unwrap()
            .split('\t')
            .nth(2)
            .unwrap();
        assert!(contract.contains(&format!("(defn {name} [] {original})")));
    }
    let mut reused = MacroHost::new();
    for reuse in [false, true] {
        for macros in [false, true] {
            for (body, expected) in [
                ("(project-contract)", "true"),
                ("(project-arity-domain-contract)", "true"),
                ("(original-relation-meta)", "nil"),
                ("(original-key-representative)", "{:new true}"),
            ] {
                let mut fresh = MacroHost::new();
                let expression = if macros {
                    format!("(defmacro witness [] (list 'quote {body})) (pr-str (witness))")
                } else {
                    format!("(pr-str {body})")
                };
                eprintln!("project phase: reuse={reuse}, macro={macros}, body={body}");
                both(
                    if reuse { &mut reused } else { &mut fresh },
                    &format!("{contract}\n{expression}"),
                    expected,
                );
            }
        }
    }
}
