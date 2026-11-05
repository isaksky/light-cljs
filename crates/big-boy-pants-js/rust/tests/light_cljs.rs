//! Execution of this checkout's real compiler runtime and checked output.
use big_boy_pants_js::frontend::compile_script;
use big_boy_pants_js::{RunOutcome, Runtime, ValueView};

fn evaluate(name: &str, source: &str, expected: f64) {
    let code = compile_script(name, source).unwrap();
    let runtime = Runtime::new();
    let task = runtime.spawn(&code, &[]).unwrap();
    let outcome = task.run().unwrap();
    assert!(
        matches!(outcome, RunOutcome::Complete(ref value) if value.view() == ValueView::Number(expected)),
        "{name}: {outcome:?}"
    );
}

#[test]
fn current_runtime_factory_initializes() {
    evaluate(
        "current-runtime.js",
        &format!(
            "const rt = {}; rt.core.add(1, 2);",
            include_str!("../../../../src/js_runtime.js")
        ),
        3.0,
    );
}

#[test]
fn checked_compiler_output_executes() {
    let runtime = include_str!("fixtures/light-cljs/runtime.js");
    let programs = [
        (
            "arithmetic",
            include_str!("fixtures/light-cljs/arithmetic.js"),
        ),
        ("closure", include_str!("fixtures/light-cljs/closure.js")),
        ("map", include_str!("fixtures/light-cljs/map.js")),
        ("vector", include_str!("fixtures/light-cljs/vector.js")),
        ("sequence", include_str!("fixtures/light-cljs/sequence.js")),
        ("macro", include_str!("fixtures/light-cljs/macro.js")),
        ("protocol", include_str!("fixtures/light-cljs/protocol.js")),
    ];
    let expected = include_str!("fixtures/light-cljs/expected.tsv")
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(programs.len(), expected.len());
    for ((name, program), expected) in programs.into_iter().zip(expected) {
        let (expected_name, expected) = expected.split_once('\t').unwrap();
        assert_eq!(name, expected_name);
        assert_eq!(program.matches("/* LIGHT_CLJS_RUNTIME */").count(), 1);
        evaluate(
            name,
            &program.replace("/* LIGHT_CLJS_RUNTIME */", runtime),
            expected.parse().unwrap(),
        );
    }
}
