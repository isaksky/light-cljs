use super::*;
use crate::{MacroHost, analyze_with_macros, parse};
use std::sync::atomic::{AtomicUsize, Ordering};

fn native(source: &str) -> String {
    let mut host = MacroHost::new();
    host.use_native_macros();
    let mut program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    if let crate::ExprKind::Do(forms) = &program.exprs[program.root.0].kind {
        program.root = *forms.last().unwrap();
    }
    crate::emitter::emit_native_macro_unit(&program, true)
}

fn numeric_global(evaluator: &BbpEvaluator, name: &str) -> f64 {
    match evaluator.task.global_value(name).unwrap().unwrap().view() {
        ValueView::Number(value) => value,
        value => panic!("{name}: {value:?}"),
    }
}

#[test]
fn two_real_consumers_share_one_helper_and_input_across_a_parked_collection() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parked = Arc::new((Mutex::new(0usize), Condvar::new()));
    let seen = parked.clone();
    *shared.0.on_wait.lock().unwrap() = Some(Arc::new(move |_, key| {
        if key.kind() == HELPER {
            let mut count = seen.0.lock().unwrap();
            *count += 1;
            seen.1.notify_all();
        }
    }));
    let producers = AtomicUsize::new(0);
    let collected = AtomicUsize::new(0);
    let consumer = native(include_str!(
        "../../tests/fixtures/native-helper-demand/consumer.cljs"
    ));
    let helper = include_str!("../../tests/fixtures/native-helper-demand/helper.cljs");
    let mut host = MacroHost::new();
    let call = host
        .read_compiler_form(&crate::compiler_data::Source::new(
            "shared.cljs",
            "(call [1 2])",
        ))
        .unwrap()
        .unwrap();
    let input = call.children()[1].clone();
    let start = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let threads = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    let evaluator = Rc::new(shared.evaluator().unwrap());
                    let runtime = shared.0.runtime.clone();
                    let producers = &producers;
                    let collected = &collected;
                    let parked = parked.clone();
                    // The callback really invokes the compiler on the unavailable
                    // .cljs helper. Neither task preloads it or evaluates it early.
                    // Its owned captures are deliberately thread local.
                    let producer_count = Arc::new(AtomicUsize::new(0));
                    let count = producer_count.clone();
                    evaluator
                        .clone()
                        .install_compiler_service(
                            "__lightCompileUnit",
                            Rc::new(move |_, _| {
                                count.fetch_add(1, Ordering::SeqCst);
                                let mut arrivals = parked.0.lock().unwrap();
                                while *arrivals != 2 {
                                    let (next, timeout) = parked
                                        .1
                                        .wait_timeout(arrivals, std::time::Duration::from_secs(5))
                                        .unwrap();
                                    arrivals = next;
                                    assert!(
                                        !timeout.timed_out() || *arrivals == 2,
                                        "second consumer never parked"
                                    );
                                }
                                drop(arrivals);
                                runtime.collect().unwrap();
                                Ok(native(helper))
                            }),
                        )
                        .unwrap();
                    evaluator
                        .register_compiler_root(
                            "cljs.user/helper",
                            Some(7),
                            false,
                            "helper.cljs:1",
                            None,
                        )
                        .unwrap();
                    let function = evaluator.evaluate_unit(&consumer).unwrap();
                    let argument = evaluator
                        .import_form(&input, ConversionLimits::default())
                        .unwrap();
                    let root = evaluator.values.borrow()[&argument.index].clone();
                    start.wait();
                    let result = evaluator.call(function, &[argument]).unwrap();
                    let expanded = evaluator
                        .export_form(
                            result,
                            &call,
                            ExpansionOrigin {
                                macro_name: Name::new(None, "consumer"),
                                call: call.span(),
                                definition: None,
                                helpers: vec![],
                            },
                            ConversionLimits::default(),
                        )
                        .unwrap();
                    assert_eq!(
                        expanded.children()[1].data().identity(),
                        input.data().identity()
                    );
                    assert_eq!(numeric_global(&evaluator, "beforeDemand"), 1.0);
                    assert_eq!(numeric_global(&evaluator, "afterDemand"), 1.0);
                    let count = producer_count.load(Ordering::SeqCst);
                    producers.fetch_add(count, Ordering::SeqCst);
                    collected.fetch_add(count, Ordering::SeqCst);
                    (root, evaluator.bridge_stats(), std::thread::current().id())
                })
            })
            .collect::<Vec<_>>();
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(producers.load(Ordering::SeqCst), 1);
    assert_eq!(collected.load(Ordering::SeqCst), 1);
    assert_ne!(results[0].2, results[1].2);
    assert_eq!(*parked.0.lock().unwrap(), 2);
    shared.0.runtime.scope(|s| {
        assert!(s.same_object(
            s.local(&results[0].0).unwrap(),
            s.local(&results[1].0).unwrap()
        ))
    });
    assert_eq!(
        results.iter().map(|r| r.1.helper_compiled).sum::<usize>(),
        1
    );
    assert_eq!(
        results.iter().map(|r| r.1.helper_per_task).sum::<usize>(),
        2
    );
}

#[test]
fn immutable_initializers_publish_and_mutable_initializers_run_once_per_task() {
    for (source, published, per_task) in [
        (
            "(do (js* \"globalThis.initialized = (globalThis.initialized || 0) + 1\") [1 2])",
            1,
            0,
        ),
        (
            "(do (js* \"globalThis.initialized = (globalThis.initialized || 0) + 1\") (js* \"({n: 40})\"))",
            0,
            2,
        ),
    ] {
        let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
        let mut roots = Vec::new();
        let mut counts = BridgeStats::default();
        for task in 0..2 {
            let evaluator = Rc::new(shared.evaluator().unwrap());
            evaluator
                .clone()
                .install_compiler_service(
                    "__lightCompileUnit",
                    Rc::new(move |_, _| Ok(native(source))),
                )
                .unwrap();
            evaluator
                .register_compiler_root("cljs.user/helper", Some(7), false, "fixture.cljs:1", None)
                .unwrap();
            for read in 0..2 {
                let result = evaluator
                    .evaluate_unit("rt => globalThis.__lightCompilerRoots['cljs.user/helper']")
                    .unwrap();
                if read == 0 {
                    roots.push(evaluator.values.borrow()[&result.index].clone());
                }
                evaluator.release(result).unwrap();
            }
            if task == 0 || per_task != 0 {
                assert_eq!(numeric_global(&evaluator, "initialized"), 1.0);
            } else {
                assert!(
                    evaluator
                        .task
                        .global_value("initialized")
                        .unwrap()
                        .is_none()
                );
            }
            let log = evaluator.task.dependency_log();
            for kind in [BINDING, HELPER, INITIALIZER] {
                assert!(log.depends_on(DependencyKey::new(
                    shared.0.runtime.world(),
                    DependencyId::new(7),
                    kind
                )));
            }
            let stats = evaluator.bridge_stats();
            counts.helper_compiled += stats.helper_compiled;
            counts.helper_published += stats.helper_published;
            counts.helper_per_task += stats.helper_per_task;
        }
        assert_eq!(counts.helper_compiled, 1);
        assert_eq!(counts.helper_published, published);
        assert_eq!(counts.helper_per_task, per_task);
        shared.0.runtime.scope(|s| {
            assert_eq!(
                s.same_object(s.local(&roots[0]).unwrap(), s.local(&roots[1]).unwrap()),
                published == 1
            )
        });
    }
}

#[test]
fn producer_failure_is_shared_and_does_not_compile_again() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let evaluator = Rc::new(shared.evaluator().unwrap());
        let calls = calls.clone();
        evaluator
            .clone()
            .install_compiler_service(
                "__lightCompileUnit",
                Rc::new(move |_, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err("fixture producer failed".into())
                }),
            )
            .unwrap();
        evaluator
            .register_compiler_root("cljs.user/helper", Some(7), false, "fixture.cljs:1", None)
            .unwrap();
        let failure = evaluator
            .evaluate_unit("rt => globalThis.__lightCompilerRoots['cljs.user/helper']")
            .unwrap_err();
        assert!(failure.0.contains("fixture producer failed"), "{failure}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn initializer_failure_wakes_later_consumers_without_repeating_effects() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    for index in 0..2 {
        let evaluator = Rc::new(shared.evaluator().unwrap());
        evaluator.clone().install_compiler_service("__lightCompileUnit", Rc::new(|_, _| Ok(native(
            "(do (js* \"globalThis.initialized = 1\") (throw (js/Error. \"initializer failed\")))"
        )))).unwrap();
        evaluator
            .register_compiler_root("cljs.user/helper", Some(7), false, "fixture.cljs:1", None)
            .unwrap();
        let failure = evaluator
            .evaluate_unit("rt => globalThis.__lightCompilerRoots['cljs.user/helper']")
            .unwrap_err();
        assert!(failure.0.contains("initializer failed"), "{failure}");
        if index == 0 {
            assert_eq!(numeric_global(&evaluator, "initialized"), 1.0);
        } else {
            assert!(
                evaluator
                    .task
                    .global_value("initialized")
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[test]
fn falsy_initializer_failures_are_shared_without_repeating_effects() {
    for (thrown, expected) in [
        ("nil", "null"),
        ("(js* \"undefined\")", "undefined"),
        ("false", "false"),
        ("0", "0"),
        ("\"\"", ""),
    ] {
        let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
        for index in 0..2 {
            let evaluator = Rc::new(shared.evaluator().unwrap());
            evaluator
                .clone()
                .install_compiler_service(
                    "__lightCompileUnit",
                    Rc::new(move |_, _| {
                        Ok(native(&format!(
                            "(do (js* \"globalThis.initialized = (globalThis.initialized || 0) + 1\") (throw {thrown}))"
                        )))
                    }),
                )
                .unwrap();
            evaluator
                .register_compiler_root("cljs.user/helper", Some(7), false, "fixture.cljs:1", None)
                .unwrap();
            for _ in 0..2 {
                let failure = evaluator
                    .evaluate_unit("rt => globalThis.__lightCompilerRoots['cljs.user/helper']")
                    .unwrap_err();
                if index == 0 {
                    assert_eq!(failure.0, format!("big-boy-pants-js: {expected}"));
                } else {
                    assert!(
                        failure
                            .0
                            .contains(&format!("failed with String({expected:?})")),
                        "thrown {thrown}: {failure}"
                    );
                }
            }
            if index == 0 {
                assert_eq!(numeric_global(&evaluator, "initialized"), 1.0);
            } else {
                assert!(
                    evaluator
                        .task
                        .global_value("initialized")
                        .unwrap()
                        .is_none()
                );
            }
        }
        assert_eq!(shared.helper_counts().compiled, 1);
        assert_eq!(shared.helper_counts().published, 0);
        assert_eq!(shared.helper_counts().per_task, 0);
        assert!(matches!(
            shared.0.runtime.cell_status(DependencyKey::new(
                shared.0.runtime.world(),
                DependencyId::new(7),
                INITIALIZER
            )),
            big_boy_pants_js::CellStatus::Failed(_)
        ));
    }
}

#[test]
fn cancelling_one_parked_consumer_keeps_the_shared_producer_and_peer_alive() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let waiting = Arc::new((Mutex::new(Vec::<Task>::new()), Condvar::new()));
    let observed = waiting.clone();
    shared.on_wait(Arc::new(move |task, key| {
        if key.kind() == HELPER {
            assert_eq!(
                task.global_value("beforeDemand").unwrap().unwrap().view(),
                ValueView::Number(1.0)
            );
            observed.0.lock().unwrap().push(task.clone());
            observed.1.notify_all();
        }
    }));
    let produced = Arc::new(AtomicUsize::new(0));
    let consumer = native(
        "(declare helper) (fn [x] (js* \"globalThis.beforeDemand = (globalThis.beforeDemand || 0) + 1\") (try (helper x) (finally (js* \"__rt.effect(65534,0)\"))))",
    );
    let results = std::thread::scope(|scope| {
        (0..2)
            .map(|_| {
                scope.spawn(|| {
                    let evaluator = Rc::new(shared.evaluator().unwrap());
                    let waiting = waiting.clone();
                    let produced = produced.clone();
                    evaluator
                        .clone()
                        .install_compiler_service(
                            "__lightCompileUnit",
                            Rc::new(move |_, _| {
                                produced.fetch_add(1, Ordering::SeqCst);
                                let mut tasks = waiting.0.lock().unwrap();
                                while tasks.len() != 2 {
                                    let (next, timeout) = waiting
                                        .1
                                        .wait_timeout(tasks, std::time::Duration::from_secs(5))
                                        .unwrap();
                                    tasks = next;
                                    assert!(!timeout.timed_out(), "peer did not park");
                                }
                                let _cancel = tasks[0].cancel();
                                Ok(native(include_str!(
                                    "../../tests/fixtures/native-helper-demand/helper.cljs"
                                )))
                            }),
                        )
                        .unwrap();
                    evaluator
                        .register_compiler_root(
                            "cljs.user/helper",
                            Some(7),
                            false,
                            "helper.cljs:1",
                            None,
                        )
                        .unwrap();
                    let function = evaluator.evaluate_unit(&consumer).unwrap();
                    let value = evaluator.evaluate_unit("rt => 40").unwrap();
                    let result = evaluator.call(function, &[value]);
                    result.map(|_| ()).map_err(|error| error.to_string())
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(produced.load(Ordering::SeqCst), 1);
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| e.contains("cancelled")),
        "{results:?}"
    );
    assert!(
        shared.0.control.poll().is_ok(),
        "consumer cancellation is task-local"
    );
    assert_eq!(shared.helper_counts().compiled, 1);
    assert_eq!(shared.0.counters.cleanups.load(Ordering::Relaxed), 2);
}

#[test]
fn helper_waits_do_not_reset_aggregate_fuel_or_deadline() {
    for deadline in [false, true] {
        let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
        let evaluator = Rc::new(shared.evaluator().unwrap());
        let control = shared.0.control.clone();
        evaluator
            .clone()
            .install_compiler_service(
                "__lightCompileUnit",
                Rc::new(move |active, _| {
                    let remaining = control.remaining_polls();
                    active.control().poll().unwrap();
                    assert_eq!(control.remaining_polls(), remaining - 1);
                    // Spend this compilation's remaining polls inside real helper work.
                    // The next factory/resume must observe exhaustion, never a fresh budget.
                    if deadline {
                        control.expire_for_test();
                    } else {
                        for _ in 0..control.remaining_polls() {
                            control.poll().unwrap();
                        }
                    }
                    Ok(native(include_str!(
                        "../../tests/fixtures/native-helper-demand/helper.cljs"
                    )))
                }),
            )
            .unwrap();
        evaluator
            .register_compiler_root("cljs.user/helper", Some(7), false, "helper.cljs:1", None)
            .unwrap();
        let consumer = native(
            "(declare helper) (fn [x] (try (helper x) (finally (js* \"__rt.effect(65534,0)\"))))",
        );
        let function = evaluator.evaluate_unit(&consumer).unwrap();
        let value = evaluator.evaluate_unit("rt => 40").unwrap();
        let failure = evaluator.call(function, &[value]).unwrap_err();
        assert!(
            failure.0.contains(if deadline {
                "deadline exceeded"
            } else {
                "poll budget exhausted"
            }),
            "{failure}"
        );
        assert_eq!(shared.0.counters.cleanups.load(Ordering::Relaxed), 1);
        if !deadline {
            assert_eq!(shared.0.control.remaining_polls(), 0);
        }
        assert!(
            shared.evaluator().is_err(),
            "a sibling must share the exhausted budget"
        );
    }
}

#[test]
fn concurrent_initializer_cycle_is_diagnosed_and_wakes_both_consumers() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let initialized = Arc::new((Mutex::new(0usize), Condvar::new()));
    *shared.0.on_initializer.lock().unwrap() = Some(Arc::new(move |_, _| {
        let mut count = initialized.0.lock().unwrap();
        *count += 1;
        initialized.1.notify_all();
        while *count != 2 {
            let (next, timeout) = initialized
                .1
                .wait_timeout(count, std::time::Duration::from_secs(5))
                .unwrap();
            count = next;
            assert!(
                !timeout.timed_out(),
                "both initializer producers did not start"
            );
        }
    }));
    let outcomes = std::thread::scope(|scope| {
        ["a", "b"]
            .map(|name| {
                let shared = &shared;
                scope.spawn(move || {
                    let evaluator = Rc::new(shared.evaluator().unwrap());
                    evaluator
                        .clone()
                        .install_compiler_service(
                            "__lightCompileUnit",
                            Rc::new(|_, id| {
                                Ok(native(if id == 7 {
                                    "(declare b) b"
                                } else {
                                    "(declare a) a"
                                }))
                            }),
                        )
                        .unwrap();
                    for (name, id) in [("a", 7), ("b", 8)] {
                        evaluator
                            .register_compiler_root(
                                &format!("cljs.user/{name}"),
                                Some(id),
                                false,
                                "cycle.cljs:1",
                                None,
                            )
                            .unwrap();
                    }
                    evaluator
                        .evaluate_unit(&format!(
                            "rt => globalThis.__lightCompilerRoots['cljs.user/{name}']"
                        ))
                        .unwrap_err()
                        .to_string()
                })
            })
            .map(|t| t.join().unwrap())
    });
    for failure in outcomes {
        assert!(failure.contains("cycle"), "{failure}");
    }
    for id in [7, 8] {
        assert!(matches!(
            shared.0.runtime.cell_status(DependencyKey::new(
                shared.0.runtime.world(),
                DependencyId::new(id),
                INITIALIZER
            )),
            big_boy_pants_js::CellStatus::Failed(_)
        ));
    }
}

#[test]
fn recursive_helper_functions_do_not_form_initializer_wait_cycles() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = Rc::new(shared.evaluator().unwrap());
    evaluator.clone().install_compiler_service("__lightCompileUnit", Rc::new(|_, _| {
        Ok(native("(declare helper) (fn [x] (if (js* \"~{} === 0\" x) 42 (helper (js* \"~{} - 1\" x))))"))
    })).unwrap();
    evaluator
        .register_compiler_root("cljs.user/helper", Some(7), false, "recursive.cljs:1", None)
        .unwrap();
    let value = evaluator
        .evaluate_unit("rt => globalThis.__lightCompilerRoots['cljs.user/helper'](10)")
        .unwrap();
    assert_eq!(
        evaluator.values.borrow()[&value.index].view(),
        ValueView::Number(42.0)
    );
    assert_eq!(shared.helper_counts().compiled, 1);
}

#[test]
fn abandoned_handoff_invalidates_its_queued_step_before_task_cleanup() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let step = NativeStep {
        task: evaluator.task.clone(),
        per_task: evaluator.per_task.clone(),
        counters: shared.0.counters.clone(),
        last_thread: evaluator.last_thread.clone(),
        admitted: Arc::new(Mutex::new(true)),
    };
    let before = shared.helper_counts().steps;
    drop(step.handoff());
    assert!(evaluator.task.cancel_requested());
    assert!(step.run().unwrap_err().contains("handoff cancelled"));
    assert_eq!(shared.helper_counts().steps, before);
    assert!(evaluator.run().unwrap_err().0.contains("cancelled"));
    assert!(shared.0.control.poll().is_ok());
}
