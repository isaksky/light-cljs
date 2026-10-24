//! Adapted QuickJS text; synthetic fixture, not light-cljs output.
//! Only test_weak_ref is adapted: upstream lines 976-977 and 979-980 assert
//! live same-job targets, then a per-iteration timer collects and checks both
//! references are cleared. The fixture header records the exact changed lines.

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime};

struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

#[test]
fn quickjs_collection_cases_run() {
    let fixture = include_str!("fixtures/js/quickjs_test_builtin_collections.js");
    let (_, body) = fixture.split_once("*/\n").expect("license header");
    let calls = "test_typed_array();\ntest_map();\ntest_weak_map();\ntest_weak_map_cycles();\ntest_weak_ref();\ntest_finalization_registry();\n";
    let body = body.strip_suffix(calls).expect("six appended calls");
    let lines: Vec<_> = body.split_inclusive('\n').collect();
    // Pinned upstream range lengths; the WeakRef adaptation adds six lines.
    assert_eq!(lines.len(), 71 + 67 + 62 + 27 + 24 + 22 + 6 + 22);
    let helpers = lines[..71].concat();
    let mut offset = 71;
    let mut failures = Vec::new();
    for (case, count, weak_maps) in [
        ("test_typed_array", 67, 0),
        ("test_map", 62, 0),
        ("test_weak_map", 27, 1),
        ("test_weak_map_cycles", 24, 2),
        ("test_weak_ref", 28, 0),
        ("test_finalization_registry", 22, 0),
    ] {
        let definitions = lines[offset..offset + count].concat();
        offset += count;
        // Keep the upstream bodies intact. For WeakMap cases only, wrap the
        // task-local constructor to retain maps, never their keys or values.
        // Rust inspects still-live maps rather than merely their reclamation.
        let observation = if weak_maps > 0 {
            r#"
            const observed = [];
            const OriginalWeakMap = WeakMap;
            WeakMap = function() {
                const map = new OriginalWeakMap();
                observed.push(map);
                return map;
            };
            "#
        } else {
            ""
        };
        let result = match weak_maps {
            1 => "assert(observed.length, 1); __rt.vector(observed[0]);",
            2 => "assert(observed.length, 2); __rt.vector(observed[0], observed[1]);",
            _ => "",
        };
        let source = format!("{helpers}{definitions}{observation}{case}();\n{result}");
        let executable = match compile_script(case, &source) {
            Ok(executable) => executable,
            Err(error) => {
                failures.push(format!("{case}: compile: {error}"));
                continue;
            }
        };
        let runtime = Runtime::new();
        let task = runtime.spawn(&executable, &[]).unwrap();
        task.install_sim_host_bindings().unwrap();
        let mut completed = false;
        let mut yields = 0;
        // At most 10 million instructions per case. Map uses 10,000 per
        // slice; the smaller cases use 100. Collect at every yield, including
        // inside Map.forEach and typed-array methods. Yields do not end the
        // WeakRef job.
        let slice = if case == "test_map" { 10_000 } else { 100 };
        for _ in 0..10_000_000 / slice {
            match task
                .run_with(Budget::instructions(slice), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Complete(value) => {
                    if weak_maps > 0 {
                        runtime.scope(|scope| {
                            let maps = scope.local(&value).unwrap();
                            assert_eq!(scope.count(maps).unwrap(), weak_maps);
                            for i in 0..weak_maps {
                                let map = scope.persist(scope.nth(maps, i).unwrap());
                                assert_eq!(map.weak_map_live_entry_count(), Some(0), "{case}");
                            }
                        });
                    }
                    assert!(task.unhandled_rejections().entries.is_empty());
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
            if case == "test_map" || case == "test_typed_array" {
                assert!(yields > 0, "{case}: must yield and collect between slices");
            }
            eprintln!("{case}: complete ({yields} yields/collections)");
        } else if !failures
            .last()
            .is_some_and(|failure| failure.starts_with(case))
        {
            failures.push(format!("{case}: exceeded 10 million instructions"));
        }
    }
    assert!(
        failures.is_empty(),
        "upstream cases failed:\n{}",
        failures.join("\n")
    );
}
