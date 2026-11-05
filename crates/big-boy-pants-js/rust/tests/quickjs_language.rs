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

/// Runs each named case of a fixture that appends one `case();` call per
/// case after the unmodified upstream definitions, which must span
/// `upstream_lines` lines (their bytes were compared with research/quickjs
/// at 7d1c13c5d8f8). Every case must complete with undefined.
fn run_cases_unmodified(fixture: &str, cases: &[&str], upstream_lines: usize) {
    let (_, body) = fixture.split_once("*/\n").expect("license header");
    let calls: String = cases.iter().map(|case| format!("{case}();\n")).collect();
    let definitions = body.strip_suffix(&calls).expect("appended case calls");
    assert_eq!(definitions.lines().count(), upstream_lines);
    let mut failures = Vec::new();
    for &case in cases {
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

#[test]
fn quickjs_operator_cases_run_unmodified() {
    // Pinned upstream lines 1-32 (helpers) and 39-220 (cases and F).
    run_cases_unmodified(
        include_str!("fixtures/js/quickjs_test_language_operators.js"),
        &[
            "test_op1",
            "test_cvt",
            "test_eq",
            "test_inc_dec",
            "test_op2",
        ],
        32 + 182,
    );
}

/// The template, regexp-skip, parse (semicolon and arrow ASI), Unicode
/// identifier and number-literal cases of test_language.js (clj-quickjs-30).
/// `test_number_literals` needs the upstream file's sloppy mode for `01.a`.
#[test]
fn quickjs_literal_and_parse_cases_run_unmodified() {
    // Pinned upstream lines 1-32, 356-376, 389-397, 578-608, 634-639, 667-676.
    run_cases_unmodified(
        include_str!("fixtures/js/quickjs_test_language_parsing.js"),
        &[
            "test_template",
            "test_template_skip",
            "test_regexp_skip",
            "test_parse_semicolon",
            "test_parse_arrow_function",
            "test_unicode_ident",
            "test_number_literals",
        ],
        32 + 21 + 9 + 31 + 6 + 10,
    );
}

#[test]
fn quickjs_line_column_rows_run_unmodified() {
    // The four QuickJS-only function metadata rows 1131-1137 are omitted;
    // every selected row is unchanged, including the original strict directive.
    run_cases_unmodified(
        include_str!("fixtures/js/quickjs_test_builtin_positions.js"),
        &["test_line_column_numbers"],
        71 + 37 + 39 + 37,
    );
}
