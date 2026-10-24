//! Adapted QuickJS text; synthetic fixture, not light-cljs output.
//! The selected upstream lines and assertions are unmodified.

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime};

struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

#[test]
fn quickjs_builtin_cases_run_unmodified() {
    let fixture = include_str!("fixtures/js/quickjs_test_builtin_builtins.js");
    let (_, body) = fixture.split_once("*/\n").expect("license header");
    let calls =
        "test_array();\ntest_string();\ntest_math();\ntest_number();\ntest_json();\ntest_rope();\n";
    let body = body.strip_suffix(calls).expect("six appended calls");
    let lines: Vec<_> = body.split_inclusive('\n').collect();
    // Exact upstream range lengths, including blank lines within each range.
    assert_eq!(lines.len(), 71 + 43 + 108 + 18 + 39 + 84 + 25);
    let helpers = lines[..71].concat();
    let mut offset = 71;
    let mut failures = Vec::new();
    for (case, count) in [
        ("test_array", 43),
        ("test_string", 108),
        ("test_math", 18),
        ("test_number", 39),
        ("test_json", 84),
        ("test_rope", 25),
    ] {
        // Select complete, unchanged function ranges so an unsupported form in
        // one case cannot prevent the other cases from compiling and running.
        let source = format!(
            "{helpers}{}{case}();\n",
            lines[offset..offset + count].concat()
        );
        offset += count;
        let executable = match compile_script(case, &source) {
            Ok(executable) => executable,
            Err(error) => {
                failures.push(format!("{case}: compile: {error}"));
                continue;
            }
        };
        let runtime = Runtime::new();
        let task = runtime.spawn(&executable, &[]).unwrap();
        let mut yields = 0;
        let mut completed = false;
        // At most 100 million instructions per case, in 50,000-instruction
        // slices. Collect at every yield, including within both 100,000-unit
        // rope construction and validation loops.
        for _ in 0..2_000 {
            match task
                .run_with(Budget::instructions(50_000), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Complete(_) => {
                    completed = true;
                    break;
                }
                RunOutcome::Yielded => {
                    yields += 1;
                    runtime.collect().unwrap();
                }
                other => {
                    failures.push(format!("{case}: {other:?}"));
                    break;
                }
            }
        }
        if completed {
            if case == "test_rope" {
                assert!(yields > 0, "rope must exercise suspension and collection");
            }
            eprintln!("{case}: complete ({yields} yields/collections)");
        } else if !failures
            .last()
            .is_some_and(|failure| failure.starts_with(case))
        {
            failures.push(format!("{case}: exceeded 100 million instructions"));
        }
    }
    assert!(
        failures.is_empty(),
        "upstream cases failed:\n{}",
        failures.join("\n")
    );
}
