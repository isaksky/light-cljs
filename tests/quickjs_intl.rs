//! Adapted QuickJS text; synthetic fixture, not light-cljs output.
//! Every case body and expected string is unmodified; adapted rows: none.

use clj_quickjs::frontend::compile_script;
use clj_quickjs::host::{TimeZone, sim::SimTimeHost};
use clj_quickjs::{Budget, Host, RunOutcome, Runtime};

#[test]
fn quickjs_test_intl_cases() {
    let fixture = include_str!("fixtures/js/quickjs_test_intl.js");
    let (_, body) = fixture.split_once("*/\n").expect("license header");
    let lines: Vec<_> = body.split_inclusive('\n').collect();
    let definitions = lines[..470].concat();
    // One appended call per complete upstream case. No upstream row is
    // adapted: the policy chooses QuickJS strings for every expected value.
    let cases = [
        ("test_dateTimeFormat", "unmodified; adapted rows: none"),
        ("test_numberFormat", "unmodified; adapted rows: none"),
        ("test_collator", "unmodified; adapted rows: none"),
        ("test_listFormat", "unmodified; adapted rows: none"),
        ("test_relativeTimeFormat", "unmodified; adapted rows: none"),
        ("test_pluralRules", "unmodified; adapted rows: none"),
        ("test_locale", "unmodified; adapted rows: none"),
        ("test_segmenter", "unmodified; adapted rows: none"),
        ("test_displayNames", "unmodified; adapted rows: none"),
        ("test_durationFormat", "unmodified; adapted rows: none"),
    ];
    let calls: String = cases
        .iter()
        .map(|(name, _)| format!("{name}();\n"))
        .collect();
    assert_eq!(lines[470..].concat(), format!("\n{calls}"));
    let mut failures = Vec::new();
    for (case, adaptation) in cases {
        let source = format!("{definitions}\n{case}();\n");
        let executable = match compile_script(case, &source) {
            Ok(executable) => executable,
            Err(error) => {
                failures.push(format!("{case}: compile: {error}"));
                continue;
            }
        };
        for (zone_name, zone) in [
            ("UTC", TimeZone::UTC),
            ("UTC+05:30", TimeZone::fixed_offset_seconds(19_800).unwrap()),
        ] {
            let runtime = Runtime::new();
            let task = runtime.spawn(&executable, &[]).unwrap();
            let mut host = SimTimeHost::new(1_506_098_258_091.0, zone);
            assert_eq!(host.default_locale(), "en-US");
            let mut complete = false;
            // Bound each case to one million instructions. Collect between
            // slices to exercise live Intl objects and native continuations.
            for _ in 0..10_000 {
                match task.run_with(Budget::instructions(100), &mut host).unwrap() {
                    RunOutcome::Complete(_) => {
                        assert!(task.dependency_log().edges().is_empty(), "{case}");
                        complete = true;
                        break;
                    }
                    RunOutcome::Yielded => {
                        runtime.collect().unwrap();
                    }
                    other => {
                        failures.push(format!("{case} ({zone_name}): {other:?}"));
                        break;
                    }
                }
            }
            if complete {
                eprintln!("{case} ({zone_name}): passed {adaptation}");
            } else if !failures
                .last()
                .is_some_and(|failure| failure.starts_with(&format!("{case} ({zone_name}):")))
            {
                failures.push(format!("{case} ({zone_name}): exceeded instruction bound"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
