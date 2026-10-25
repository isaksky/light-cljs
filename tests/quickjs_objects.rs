//! Adapted QuickJS text; synthetic fixture, not light-cljs output.
//! All selected upstream lines and assertions are unmodified; adapted rows: none.

use big_boy_pants_js::frontend::{ScriptOptions, compile_script_with};
use big_boy_pants_js::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView};

struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

#[test]
fn quickjs_object_cases_run_unmodified() {
    let fixtures: [(&str, &[&str], usize); 2] = [
        (
            include_str!("fixtures/js/quickjs_test_language_objects.js"),
            &[
                "test_delete",
                "test_constructor",
                "test_prototype",
                "test_class",
                "test_object_literal",
                "test_destructuring",
                "test_spread",
                "test_optional_chaining",
            ],
            32 + 28 + 8 + 10 + 74 + 10 + 6 + 9 + 24,
        ),
        (
            include_str!("fixtures/js/quickjs_test_builtin_objects.js"),
            &["test", "test_enum", "test_symbol"],
            71 + 49 + 16 + 31,
        ),
    ];
    let mut failures = Vec::new();
    for (fixture, cases, upstream_lines) in fixtures {
        let (_, body) = fixture.split_once("*/\n").expect("license header");
        let calls: String = cases.iter().map(|case| format!("{case}();\n")).collect();
        let definitions = body.strip_suffix(&calls).expect("appended case calls");
        // Pinned upstream range lengths. All selected bytes were compared
        // against research/quickjs at 7d1c13c5d8f8; adapted rows: none.
        assert_eq!(definitions.lines().count(), upstream_lines);
        for case in cases {
            let source = format!("{definitions}{case}();\n");
            // The language fixture is non-strict; the built-in fixture keeps
            // its original "use strict" directive. Compile once per case.
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
            // slices to exercise live objects and callback continuations.
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
                .is_some_and(|failure| failure.starts_with(&format!("{case}:")))
            {
                failures.push(format!("{case}: exceeded instruction bound"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
