//! Adapted QuickJS text; synthetic fixtures, not light-cljs output.
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
fn quickjs_function_scope_cases_run_unmodified() {
    let mut failures = Vec::new();
    for (fixture, line_count, cases, options) in [
        (
            include_str!("fixtures/js/quickjs_test_closure.js"),
            221,
            &[][..],
            ScriptOptions::SLOPPY,
        ),
        (
            include_str!("fixtures/js/quickjs_test_language_functions.js"),
            38 + 10 + 133 + 26,
            &[
                "test_arguments",
                "test_function_length",
                "test_argument_scope",
                "test_function_expr_name",
                "test_global_var_opt",
            ][..],
            ScriptOptions::SLOPPY,
        ),
        (
            include_str!("fixtures/js/quickjs_test_builtin_functions.js"),
            123 + 53,
            &["test_function", "test_eval2", "test_eval"][..],
            ScriptOptions::STRICT,
        ),
    ] {
        let (_, body) = fixture.split_once("*/\n").expect("license header");
        let calls: String = cases.iter().map(|case| format!("{case}();\n")).collect();
        let definitions = body.strip_suffix(&calls).expect("appended case calls");
        // Pinned upstream range lengths, including blank lines. Selected bytes
        // were compared against research/quickjs at 7d1c13c5d8f8.
        assert_eq!(definitions.lines().count(), line_count);
        let sources = if cases.is_empty() {
            // Run the entire closure file, including its original top-level
            // f/g1 assertion and all seven original trailing calls.
            vec![("test_closure.js", body.to_owned())]
        } else {
            cases
                .iter()
                .map(|&case| (case, format!("{definitions}{case}();\n")))
                .collect()
        };
        for (case, source) in sources {
            // Each language/builtin case gets the complete original helpers
            // and definitions, in a fresh task with the upstream script mode.
            let executable = match compile_script_with(case, &source, options) {
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
            // slices to exercise closures, arguments and eval continuations.
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
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
