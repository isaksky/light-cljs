//! Audit the unchanged upstream emission fixture and its intentional runtime failure.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros, compile_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
const ORIGINAL: &str =
    include_str!("../research/clojurescript/src/test/cljs/cljs/invoke_test.cljs");
fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}
fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
#[test]
#[ignore = "exact unchanged optimizer source with genuine pin Closure dependency profile"]
fn unchanged_invoke_fixture_preserves_its_original_declared_call_failure() {
    let loader =
        std::fs::read_to_string(std::env::var("LIGHT_INVOKE_SCOPE_LOADER").unwrap()).unwrap();
    let root = std::path::PathBuf::from(std::env::var("LIGHT_INVOKE_SCOPE_ARTIFACTS").unwrap());
    let mut reused = MacroHost::new();
    let warm = compile_with_macros(
        "(defmacro invoke-scope-warmup [] true) (invoke-scope-warmup)",
        &mut reused,
    )
    .unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("warmup.js"), warm).unwrap();
    for lifecycle in ["fresh", "reused"] {
        let sources = root.join(lifecycle);
        std::fs::create_dir_all(sources.join("cljs")).unwrap();
        std::fs::write(sources.join("cljs/invoke_test.cljs"), ORIGINAL).unwrap();
        let mut fresh = MacroHost::new();
        let host = if lifecycle == "fresh" {
            &mut fresh
        } else {
            &mut reused
        };
        let javascript =
            compile_namespace_with_macros("cljs.invoke-test", std::slice::from_ref(&sources), host)
                .unwrap();
        std::fs::write(sources.join("compiled.js"), &javascript).unwrap();
        // Observe the whole original program. The final record forms remain present
        // and unexecuted after the original deliberately undefined declared call.
        let observed = format!(
            "(function(){{try {{{loader}; (0,eval)({}); return 'UNEXPECTED PROGRAM COMPLETION';}}catch(e){{return JSON.stringify({{category:e.name,message:e.message}});}}}})()",
            quote(&javascript)
        );
        std::fs::write(sources.join("observed.js"), &observed).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let quickjs = evaluator
            .string(evaluator.evaluate_js(&observed).unwrap())
            .unwrap();
        println!(
            "invoke-program-observation\t{lifecycle}\tQuickJS\t{}",
            hex(&quickjs)
        );
        assert!(quickjs.contains("\"category\":\"TypeError\""), "{quickjs}");
        let node = node(&format!("console.log({observed});"));
        println!(
            "invoke-program-observation\t{lifecycle}\tNode\t{}",
            hex(&node)
        );
        assert!(node.contains("\"category\":\"TypeError\""), "{node}");
    }
}
