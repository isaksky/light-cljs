//! Adapted QuickJS text; synthetic fixtures, not light-cljs output.
//! Both complete upstream files retain every byte after the license header.

use std::time::{Duration, Instant};

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime};

const BIGINT: &str = include_str!("fixtures/js/quickjs_test_bigint.js");
const PI: &str = include_str!("fixtures/js/quickjs_pi_bigint.js");
const CALLS: &str =
    "test_bigint1();\ntest_bigint2();\ntest_bigint3();\ntest_pi();\ntest_bigint_asintn();\n";
const PRINT_PRELUDE: &str = "\"use strict\"; function print(value) { __rt.effect(0, value); }\n";

#[derive(Default)]
struct PrintHost {
    lines: Vec<String>,
}

impl Host for PrintHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, call.arg) {
            (0, Primitive::String(line)) => {
                self.lines.push(line.to_string());
                Ok(Primitive::Undefined)
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

fn run(name: &str, source: &str, collect: bool) -> Result<PrintHost, String> {
    let executable = compile_script(name, source).map_err(|e| format!("{name}: {e}"))?;
    let runtime = Runtime::new();
    let task = runtime.spawn(&executable, &[]).unwrap();
    let mut host = PrintHost::default();
    let started = Instant::now();
    let mut yields = 0;
    let budget = if collect { 1_000 } else { 50_000 };
    // At most 20 million instructions and 30 seconds per run. The collected
    // test_pi run uses small slices with live BigInts in recursive VM frames.
    for _ in 0..(20_000_000 / budget) {
        if started.elapsed() > Duration::from_secs(30) {
            return Err(format!(
                "{name}: exceeded 30-second bound ({yields} yields)"
            ));
        }
        match task
            .run_with(Budget::instructions(budget), &mut host)
            .map_err(|e| format!("{name}: {e:?}"))?
        {
            RunOutcome::Complete(_) => {
                if collect {
                    assert!(yields > 0, "{name}: must yield with live BigInts");
                    assert_eq!(runtime.heap_stats().collections, yields);
                }
                assert!(task.dependency_log().edges().is_empty(), "{name}");
                eprintln!(
                    "{name}: complete ({yields} yields, {:?})",
                    started.elapsed()
                );
                return Ok(host);
            }
            RunOutcome::Yielded => {
                yields += 1;
                if collect {
                    runtime.collect().unwrap();
                }
            }
            other => return Err(format!("{name}: {other:?}")),
        }
    }
    Err(format!("{name}: exceeded 20 million instructions"))
}

#[test]
fn quickjs_test_bigint_runs_unmodified() {
    let (_, body) = BIGINT.split_once("*/\n").expect("license header");
    assert_eq!(body.lines().count(), 279);
    let definitions = BIGINT.strip_suffix(CALLS).expect("five upstream calls");
    let mut failures = Vec::new();
    // First execute the whole file unmodified, including its swallowed
    // ReferenceError from the deliberately absent __loadScript shell helper.
    match run("test_bigint.js (whole file)", BIGINT, false) {
        Ok(host) => assert!(host.lines.is_empty(), "whole file: unexpected print"),
        Err(error) => failures.push(error),
    }
    for case in [
        "test_bigint1",
        "test_bigint2",
        "test_bigint3",
        "test_pi",
        "test_bigint_asintn",
    ] {
        // Adaptation: replace exactly the five trailing calls with this one
        // call. All helpers, case bodies and assertions stay unchanged.
        let source = format!("{definitions}{case}();\n");
        match run(case, &source, case == "test_pi") {
            Ok(host) => assert!(host.lines.is_empty(), "{case}: unexpected print"),
            Err(error) => failures.push(error),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn quickjs_pi_bigint_example_prints_1000_digits() {
    let (_, body) = PI.split_once("*/\n").expect("license header");
    assert_eq!(body.lines().count(), 118);
    // Adaptation: prepend precisely this one line; its own strict directive
    // preserves strict execution. scriptArgs and top-level arguments stay
    // undefined, selecting the upstream default of 1,000 decimal places.
    let source = format!("{PRINT_PRELUDE}{PI}");
    let host = run("pi_bigint.js", &source, false).unwrap();
    let expected = BIGINT
        .split_once("assert(compute_pi(2000), \"")
        .unwrap()
        .1
        .split_once("\");")
        .unwrap()
        .0;
    assert_eq!(expected.len(), 2002);
    assert_eq!(host.lines, [expected[..1002].to_owned()]);
}
