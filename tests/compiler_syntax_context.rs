//! Complete syntax semantics in applications and actual MacroHost bodies.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::io::Write;
use std::process::{Command, Stdio};

const FIXTURE: &str = include_str!("fixtures/compiler-syntax-context.tsv");
fn unhex(value: &str) -> String {
    assert_eq!(value.len() % 2, 0);
    String::from_utf8(
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|part| u8::from_str_radix(std::str::from_utf8(part).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn node(javascript: &str) -> String {
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
        .write_all(javascript.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "Unexpected syntax observation stderr"
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn row(identity: &str) -> Vec<&'static str> {
    let rows: Vec<_> = FIXTURE
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
        .map(|r| r.split('\t').collect::<Vec<_>>())
        .collect();
    assert_eq!(rows.len(), 7);
    let selected: Vec<_> = rows.into_iter().filter(|r| r[0] == identity).collect();
    assert_eq!(selected.len(), 1);
    let result = selected.into_iter().next().unwrap();
    assert_eq!(result.len(), 5);
    result
}
fn literal(identity: &str) {
    let fields = row(identity);
    let source = unhex(fields[2]);
    let expected = unhex(fields[3]);
    for macro_phase in [false, true] {
        // Serialize observed list data inside MacroHost before it becomes
        // expansion data; an unquoted returned list would be executable source.
        let observed_source = if macro_phase {
            format!("(defmacro syntax-gate [] (pr-str {source})) (syntax-gate)")
        } else {
            format!("(pr-str {source})")
        };
        let parsed = parse(&observed_source).unwrap();
        let program =
            analyze_with_macros(&observed_source, &parsed, &mut MacroHost::new()).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let actual = evaluator
            .string(evaluator.evaluate(&program).unwrap())
            .unwrap();
        println!("syntax-observation\t{identity}\tmacro={macro_phase}\tQuickJS\t{actual}");
        assert_eq!(actual, expected, "{identity} macro={macro_phase} QuickJS");
        let script = format!(
            "const rt = {RUNTIME_SOURCE}; console.log(({})(rt));",
            emit_js_unit(&program)
        );
        let actual = node(&script);
        println!("syntax-observation\t{identity}\tmacro={macro_phase}\tNode\t{actual}");
        assert_eq!(actual, expected, "{identity} macro={macro_phase} Node");
    }
}
#[test]
fn syntax_control_rest_and_recursion_execute_in_both_phases() {
    literal("control-rest-recursion");
}
#[test]
fn syntax_exception_interop_and_template_execute_in_both_phases() {
    literal("exception-interop-template");
}
#[test]
fn syntax_quote_and_case_star_execute_in_both_phases() {
    literal("quote-case-star");
}
#[test]
fn syntax_definition_and_var_state_execute_in_both_phases() {
    literal("definition-var-state");
}
#[test]
fn syntax_raw_type_constructor_executes_in_both_phases() {
    literal("raw-type-constructor");
}
#[test]
fn syntax_raw_record_constructor_executes_in_both_phases() {
    literal("raw-record-constructor");
}
#[test]
fn syntax_namespace_context_executes_with_actual_macro_environment() {
    let fields = row("namespace-header-context");
    let original = unhex(fields[2]);
    let root = std::env::temp_dir().join(format!("light-syntax-context-{}", std::process::id()));
    std::fs::create_dir_all(root.join("syntax")).unwrap();
    for extension in ["cljs", "clj"] {
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "tests/fixtures/compiler-syntax-context/local/syntax/observer.{extension}"
            )),
            root.join(format!("syntax/observer.{extension}")),
        )
        .unwrap();
    }
    for macro_phase in [false, true] {
        let source = if macro_phase {
            let (prefix, remainder) = original.split_once("(defn run [observe] ").unwrap();
            let body = remainder.strip_suffix(")\n(pr-str (run nil))\n").unwrap();
            format!("{prefix}(defmacro syntax-gate [] {body})\n(pr-str (syntax-gate))\n")
        } else {
            original.clone()
        };
        std::fs::write(root.join("syntax/header.cljs"), source).unwrap();
        let javascript = light_cljs::compile_namespace_with_macros(
            "syntax.header",
            &[root.clone()],
            &mut MacroHost::new(),
        )
        .unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let actual = evaluator
            .string(evaluator.evaluate_js(&javascript).unwrap())
            .unwrap();
        println!(
            "syntax-observation\tnamespace-header-context\tmacro={macro_phase}\tQuickJS\t{actual}"
        );
        assert_eq!(actual, "true");
        let actual = node(&format!("console.log({javascript});"));
        println!(
            "syntax-observation\tnamespace-header-context\tmacro={macro_phase}\tNode\t{actual}"
        );
        assert_eq!(actual, "true");
    }
    std::fs::remove_dir_all(root).unwrap();
}
