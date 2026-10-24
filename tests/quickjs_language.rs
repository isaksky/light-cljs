//! Adapted QuickJS text; synthetic fixture, not light-cljs output.
//! All selected upstream lines and assertions are unmodified; adapted rows: none.

use clj_quickjs::frontend::{ScriptOptions, compile_script_with};
use clj_quickjs::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView};

struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

#[test]
fn quickjs_operator_cases_run_unmodified() {
    let fixture = include_str!("fixtures/js/quickjs_test_language_operators.js");
    let (_, body) = fixture.split_once("*/\n").expect("license header");
    let cases = [
        "test_op1",
        "test_cvt",
        "test_eq",
        "test_inc_dec",
        "test_op2",
    ];
    let calls: String = cases.iter().map(|case| format!("{case}();\n")).collect();
    let definitions = body.strip_suffix(&calls).expect("five appended calls");
    // Pinned upstream lines 1-32 (helpers) and 39-220 (cases and F).
    // Their bytes were compared with research/quickjs at 7d1c13c5d8f8.
    assert_eq!(definitions.lines().count(), 32 + 182);
    let mut failures = Vec::new();
    for case in cases {
        let source = format!("{definitions}{case}();\n");
        // Preserve upstream's non-strict script mode. Compile once per case,
        // with the original helpers and complete function definitions.
        let executable = match compile_script_with(case, &source, ScriptOptions::SLOPPY) {
            Ok(executable) => executable,
            Err(error) => {
                failures.push(format!("{case}: compile: {error}"));
                continue;
            }
        };
        let runtime = Runtime::new();
        let task = runtime.spawn(&executable, &[]).unwrap();
        let mut completed = false;
        // Bound each case to one million instructions, collecting between
        // slices to exercise live objects and coercion continuations.
        for _ in 0..10_000 {
            match task
                .run_with(Budget::instructions(100), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Undefined, "{case}");
                    completed = true;
                    break;
                }
                RunOutcome::Yielded => {
                    runtime.collect().unwrap();
                }
                other => {
                    failures.push(format!("{case}: {other:?}"));
                    break;
                }
            }
        }
        if completed {
            eprintln!("{case}: complete (unmodified; adapted rows: none)");
        } else if !failures
            .last()
            .is_some_and(|failure| failure.starts_with(case))
        {
            failures.push(format!("{case}: exceeded instruction bound"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
