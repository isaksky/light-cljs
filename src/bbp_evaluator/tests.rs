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
    match evaluator
        .cx()
        .unwrap()
        .task
        .global_value(name)
        .unwrap()
        .unwrap()
        .view()
    {
        ValueView::Number(value) => value,
        value => panic!("{name}: {value:?}"),
    }
}

/// Runs while both consumers are parked on the unavailable helper, on the
/// producer's thread, before the helper is compiled.
type WhileParked = Arc<dyn Fn(&BbpRuntime) + Send + Sync>;

#[test]
fn two_real_consumers_share_one_helper_and_input_across_a_parked_collection() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    share_one_helper_while_parked(
        shared,
        Arc::new(|shared: &BbpRuntime| {
            let before = shared.0.runtime.heap_stats().collections;
            shared.0.runtime.collect().unwrap();
            assert_eq!(shared.0.runtime.heap_stats().collections, before + 1);
        }),
        false,
    );
}

#[test]
fn a_parked_consumer_is_resumed_from_the_producers_wakeup() {
    // The producer compiles the helper only once the other consumer is
    // parked, so its completion's wakeups queue that consumer's next step.
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    share_one_helper_while_parked(
        shared.clone(),
        Arc::new(|shared: &BbpRuntime| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while shared.0.parked.lock().unwrap().is_empty() {
                assert!(std::time::Instant::now() < deadline, "no consumer parked");
                std::thread::yield_now();
            }
        }),
        true,
    );
    // The other consumer, at least; either may also park on the other's
    // initializer of the helper.
    assert!(shared.helper_counts().woken >= 1);
    assert!(shared.0.parked.lock().unwrap().is_empty());
}

#[test]
fn cancelling_a_consumer_whose_woken_step_is_queued_keeps_the_producer_and_peer() {
    // The first woken step is cancelled after the producer's completion
    // queued it and before any worker runs it.
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let produced = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicUsize::new(0));
    let consumer = native(
        "(declare helper) (fn [x] (try (helper x) (finally (js* \"__rt.effect(65534,0)\"))))",
    );
    let helper = native(include_str!(
        "../../tests/fixtures/native-helper-demand/helper.cljs"
    ));
    let results = std::thread::scope(|scope| {
        (0..2)
            .map(|_| {
                scope.spawn(|| {
                    let evaluator = Rc::new(shared.evaluator().unwrap());
                    evaluator.install_native_runner(Rc::new(|step: NativeStep| step.run()));
                    let first = cancelled.clone();
                    let submit: NativeSubmit = Arc::new(move |step: NativeStep, reply| {
                        if first.fetch_add(1, Ordering::SeqCst) == 0 {
                            let _cancel = step.task.cancel();
                        }
                        std::thread::spawn(move || {
                            let _ = reply.send(step.run());
                        });
                    });
                    evaluator.install_native_submitter(Rc::new(move || Some(submit.clone())));
                    let (in_runtime, produced, helper) =
                        (shared.clone(), produced.clone(), helper.clone());
                    evaluator
                        .clone()
                        .install_compiler_service(
                            "__lightCompileUnit",
                            Rc::new(move |_, _| {
                                produced.fetch_add(1, Ordering::SeqCst);
                                let deadline =
                                    std::time::Instant::now() + std::time::Duration::from_secs(5);
                                while in_runtime.0.parked.lock().unwrap().is_empty() {
                                    assert!(
                                        std::time::Instant::now() < deadline,
                                        "peer did not park"
                                    );
                                    std::thread::yield_now();
                                }
                                Ok(helper.clone())
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
    assert!(cancelled.load(Ordering::SeqCst) >= 1);
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "{results:?}"
    );
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
    // Each consumer's `finally` ran once, the cancelled one's included.
    assert_eq!(shared.0.counters.cleanups.load(Ordering::Relaxed), 2);
    assert!(shared.0.parked.lock().unwrap().is_empty());
}

#[test]
fn nested_producers_inherit_their_parents_registrations_and_catalog_cursor() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parent = shared.evaluator().unwrap();
    parent
        .register_compiler_root("cljs.user/helper", Some(7), false, "helper.cljs:1", None)
        .unwrap();
    // A worker's engine has no cursor: its view's count stands for it.
    let child = parent
        .spawn_producer(&parent.cx().unwrap(), None, |_| {})
        .unwrap();
    assert_eq!(child.catalog_cursor(), None);
    assert_eq!(child.registration_count(), parent.registration_count());
    child.set_catalog_cursor(3);
    let grandchild = child
        .spawn_producer(&child.cx().unwrap(), None, |_| {})
        .unwrap();
    assert_eq!(grandchild.catalog_cursor(), Some(3));
    assert_eq!(grandchild.registration_count(), child.registration_count());
}

#[test]
fn a_parked_context_is_borrowed_with_its_view_state_and_handles() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let (first, state) = shared.lent_evaluator().unwrap();
    assert!(state.is_none() && !first.prepared());
    first
        .install_string_service("__lightBuildService", Rc::new(|_| String::new()))
        .unwrap();
    let registrations = first.cx().unwrap().registrations.borrow().len();
    let value = first
        .evaluate_unit("rt => { globalThis.kept = 41; return 1; }")
        .unwrap();
    let heap = first.heap();
    assert!(first.park(LentState {
        installed: 5,
        ..Default::default()
    }));
    assert!(!first.has_context() && first.heap() == 0);
    // Another evaluator, as another slot's view would have, takes it.
    let (second, state) = shared.lent_evaluator().unwrap();
    assert_eq!(state.unwrap().installed, 5);
    assert!(second.prepared() && second.heap() == heap);
    second.release(value).unwrap();
    assert_eq!(numeric_global(&second, "kept"), 41.0);
    // A prepared context keeps its service stubs: none is evaluated again.
    second
        .install_string_service("__lightBuildService", Rc::new(|_| String::new()))
        .unwrap();
    assert_eq!(
        second.cx().unwrap().registrations.borrow().len(),
        registrations
    );
    // The first evaluator borrows nothing while the context is in use.
    assert!(first.adopt().is_none());
    let counts = shared.helper_counts();
    assert_eq!(
        (
            counts.contexts,
            counts.contexts_lent,
            counts.contexts_borrowed
        ),
        (1, 1, 1)
    );
    shared.clear_idle();
}

#[test]
fn macro_declarations_keep_task_state_across_collection_and_lending() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parent = shared.evaluator().unwrap();
    const DECLARATION: &str = "rt => { globalThis.declarations = (globalThis.declarations || 0) + 1; let calls = 0; return () => ++calls; }";
    let function = parent.source_macro_function(7, DECLARATION).unwrap();
    assert_eq!(
        parent.source_macro_function(7, DECLARATION).unwrap(),
        function
    );
    let call = |evaluator: &BbpEvaluator, function| {
        let value = evaluator.call(function, &[]).unwrap();
        let result = match evaluator.cx().unwrap().values.borrow()[&value.index].view() {
            ValueView::Number(n) => n,
            other => panic!("macro result: {other:?}"),
        };
        evaluator.release(value).unwrap();
        result
    };
    assert_eq!(call(&parent, function), 1.0);
    let child = parent
        .spawn_producer(&parent.cx().unwrap(), None, |_| {})
        .unwrap();
    let child_function = child.source_macro_function(7, DECLARATION).unwrap();
    assert_ne!(child_function, function);
    assert!(
        child
            .call(function, &[])
            .unwrap_err()
            .0
            .contains("invalid value handle")
    );
    assert_eq!(call(&child, child_function), 1.0);
    assert_eq!(call(&parent, function), 2.0);
    drop(child);
    shared.0.runtime.collect().unwrap();
    assert!(parent.park(LentState::default()));
    std::thread::spawn(move || {
        let (borrowed, state) = shared.lent_evaluator().unwrap();
        assert!(state.is_some());
        assert_eq!(
            borrowed.source_macro_function(7, DECLARATION).unwrap(),
            function
        );
        assert_eq!(numeric_global(&borrowed, "declarations"), 1.0);
        assert_eq!(call(&borrowed, function), 3.0);
    })
    .join()
    .unwrap();
}

fn churn(iterations: usize) -> String {
    format!(
        "rt => {{ let kept = null; for (let i = 0; i < {iterations}; i++) {{ kept = [i, {{v: i}}]; }} return kept[0]; }}"
    )
}

/// The accounted bytes a runtime holds after a collection with three booted
/// evaluators, and the bytes one iteration of [`churn`] admits, under the
/// default memory limit.
fn booted_and_churn_bytes() -> (usize, usize) {
    let booted = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluators = (0..3)
        .map(|_| booted.evaluator().unwrap())
        .collect::<Vec<_>>();
    booted.0.runtime.collect().unwrap();
    let stats = booted.0.runtime.heap_stats();
    let value = evaluators[0].evaluate_unit(&churn(1000)).unwrap();
    evaluators[0].release(value).unwrap();
    let admitted = booted.0.runtime.heap_stats().admitted_bytes_total - stats.admitted_bytes_total;
    (stats.last_live_bytes, admitted.div_ceil(1000))
}

/// Automatic collection under the compilation's aggregate byte budget
/// (`options.memory_limit`, mapped by `heap_policy`), with no forced
/// collection: while both consumers are parked, another task of the shared
/// runtime allocates several times the whole budget in garbage. Only
/// automatic collections can admit it, and they must not disturb the parked
/// consumers' saved frames, input identity or pre-wait effects.
#[test]
fn parked_consumers_survive_automatic_collection_under_the_memory_limit() {
    // Room for the realm, the two consumers, the churning task, the helper
    // and the garbage between two collections, and little else.
    let (three, per_iteration) = booted_and_churn_bytes();
    let options = EvaluatorOptions {
        memory_limit: three + three / 2,
        ..Default::default()
    };
    let cap = options.memory_limit;
    let shared = BbpRuntime::new(&options).unwrap();
    assert_eq!(shared.0.runtime.heap_stats().byte_limit, Some(cap));
    let iterations = 3 * cap / per_iteration;
    share_one_helper_while_parked(
        shared,
        Arc::new(move |shared: &BbpRuntime| {
            let before = shared.0.runtime.heap_stats();
            let churner = shared.evaluator().unwrap();
            let value = churner.evaluate_unit(&churn(iterations)).unwrap();
            churner.release(value).unwrap();
            drop(churner);
            let after = shared.0.runtime.heap_stats();
            assert!(
                after.admitted_bytes_total - before.admitted_bytes_total >= 2 * cap,
                "the churn must outgrow the budget: {after:?}"
            );
            assert!(
                after.collections >= before.collections + 2,
                "{} automatic collections while parked",
                after.collections - before.collections
            );
            assert!(after.peak_managed_bytes <= cap, "{after:?}");
        }),
        false,
    );
}

/// With `pooled`, each consumer runs its steps through a runner and parks
/// with a submitter that runs a woken step on a thread of its own, as a
/// pool worker would.
fn share_one_helper_while_parked(shared: BbpRuntime, while_parked: WhileParked, pooled: bool) {
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
                    if pooled {
                        evaluator.install_native_runner(Rc::new(|step: NativeStep| step.run()));
                        let submit: NativeSubmit = Arc::new(|step: NativeStep, reply| {
                            std::thread::spawn(move || {
                                let _ = reply.send(step.run());
                            });
                        });
                        evaluator.install_native_submitter(Rc::new(move || Some(submit.clone())));
                    }
                    let in_runtime = shared.clone();
                    let while_parked = while_parked.clone();
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
                                while_parked(&in_runtime);
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
                    let root = evaluator.cx().unwrap().values.borrow()[&argument.index].clone();
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
    // The helper builds its form with native core intrinsics only
    // (light-cljs-477): its function publishes once and both tasks share it.
    assert_eq!(
        results.iter().map(|r| r.1.helper_published).sum::<usize>(),
        1
    );
    assert_eq!(
        results.iter().map(|r| r.1.helper_per_task).sum::<usize>(),
        0
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
                    roots.push(evaluator.cx().unwrap().values.borrow()[&result.index].clone());
                }
                evaluator.release(result).unwrap();
            }
            if task == 0 || per_task != 0 {
                assert_eq!(numeric_global(&evaluator, "initialized"), 1.0);
            } else {
                assert!(
                    evaluator
                        .cx()
                        .unwrap()
                        .task
                        .global_value("initialized")
                        .unwrap()
                        .is_none()
                );
            }
            let log = evaluator.cx().unwrap().task.dependency_log();
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
                    .cx()
                    .unwrap()
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
                        .cx()
                        .unwrap()
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
    cancel_one_parked_consumer(EvaluatorOptions::default());
}

/// The same cancellation with the realm, three tasks and little else
/// fitting the memory limit.
#[test]
fn cancelling_one_parked_consumer_under_a_tight_memory_limit() {
    let (three, _) = booted_and_churn_bytes();
    cancel_one_parked_consumer(EvaluatorOptions {
        memory_limit: three + three / 2,
        ..Default::default()
    });
}

fn cancel_one_parked_consumer(options: EvaluatorOptions) {
    let shared = BbpRuntime::new(&options).unwrap();
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
    // Both workers and the cancellation ran under the memory limit.
    let stats = shared.0.runtime.heap_stats();
    assert_eq!(stats.byte_limit, Some(options.memory_limit));
    assert!(
        stats.peak_managed_bytes <= options.memory_limit,
        "{stats:?}"
    );
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
        evaluator.cx().unwrap().values.borrow()[&value.index].view(),
        ValueView::Number(42.0)
    );
    assert_eq!(shared.helper_counts().compiled, 1);
}

#[test]
fn abandoned_handoff_invalidates_its_queued_step_before_task_cleanup() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let step = NativeStep {
        task: evaluator.cx().unwrap().task.clone(),
        per_task: evaluator.cx().unwrap().per_task.clone(),
        counters: shared.0.counters.clone(),
        lanes: None,
        last_thread: evaluator.cx().unwrap().last_thread.clone(),
        admitted: Arc::new(Mutex::new(true)),
    };
    let before = shared.helper_counts().steps;
    drop(step.handoff());
    assert!(evaluator.cx().unwrap().task.cancel_requested());
    assert!(step.run().unwrap_err().contains("handoff cancelled"));
    assert_eq!(shared.helper_counts().steps, before);
    assert!(evaluator.run().unwrap_err().0.contains("cancelled"));
    assert!(shared.0.control.poll().is_ok());
}

/// A task that outgrows `options.memory_limit` observes the engine's
/// catchable denial; once it drops what it retained, the same task and a
/// fresh one of the runtime allocate again within the same budget.
#[test]
fn memory_limit_denial_is_catchable_and_the_runtime_recovers() {
    let options = EvaluatorOptions {
        memory_limit: 16 << 20,
        ..Default::default()
    };
    let shared = BbpRuntime::new(&options).unwrap();
    let evaluator = shared.evaluator().unwrap();
    // A list of small objects: the denial leaves less than one object free.
    let hog = "rt => { let head = null, n = 0; try { for (;;) { head = {next: head}; n++; } } catch (e) { head = null; return e === 'heap byte limit exceeded' ? n : -1; } }";
    // A source larger than what the denial left free, which fits only once
    // the released data is collected: its import collects first.
    let padded = format!("rt => {{ /*{}*/ return 7; }}", "x".repeat(16_000));
    for _ in 0..2 {
        let value = evaluator.evaluate_unit(hog).unwrap();
        let retained = evaluator.cx().unwrap().values.borrow()[&value.index].view();
        assert!(
            matches!(retained, ValueView::Number(n) if n > 1000.0),
            "{retained:?}"
        );
        let value = evaluator.evaluate_unit(&padded).unwrap();
        assert!(matches!(
            evaluator.cx().unwrap().values.borrow()[&value.index].view(),
            ValueView::Number(7.0)
        ));
    }
    let peer = shared.evaluator().unwrap();
    let value = peer.evaluate_unit(&churn(1000)).unwrap();
    assert!(matches!(
        peer.cx().unwrap().values.borrow()[&value.index].view(),
        ValueView::Number(999.0)
    ));
    let stats = shared.0.runtime.heap_stats();
    assert!(
        stats.peak_managed_bytes <= options.memory_limit,
        "{stats:?}"
    );
}

/// A large live result that leaves only a little room must still be
/// releasable: the release allocates no value-table entry for it.
#[test]
fn releasing_a_deferred_value_at_the_memory_limit_recovers() {
    let options = EvaluatorOptions {
        memory_limit: 4 << 20,
        ..Default::default()
    };
    // Each step leaves `freed` small objects' worth of room after the
    // denial, from too little for a value-table entry to plenty.
    for freed in [1, 2, 4, 8, 16, 32] {
        let shared = BbpRuntime::new(&options).unwrap();
        let evaluator = shared.evaluator().unwrap();
        let value = evaluator
            .evaluate_unit(&format!(
                "rt => {{ const kept = []; try {{ for (;;) kept.push({{}}); }} catch (e) {{}} kept.length -= {freed}; return kept; }}"
            ))
            .unwrap_or_else(|e| panic!("freed {freed}: {e}"));
        evaluator
            .release(value)
            .unwrap_or_else(|e| panic!("freed {freed}: {e}"));
        let value = evaluator.evaluate_unit(&churn(1000)).unwrap();
        assert!(matches!(
            evaluator.cx().unwrap().values.borrow()[&value.index].view(),
            ValueView::Number(999.0)
        ));
    }
}

/// At an exhausted heap a producer's or initializer's failure message is
/// still rooted (uncharged), so its waiters can be completed with it; the
/// runtime recovers once the data filling it is dropped. Delivery of that
/// root to parked consumers is the path the producer-failure tests cover.
#[test]
fn failure_roots_survive_an_exhausted_heap() {
    let options = EvaluatorOptions {
        memory_limit: 4 << 20,
        ..Default::default()
    };
    let shared = BbpRuntime::new(&options).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let runtime = &shared.0.runtime;
    let mut filler = vec![];
    for units in [4096, 16] {
        while let Some(root) = runtime.try_root(Primitive::string(&"x".repeat(units))) {
            filler.push(root);
        }
    }
    let message = "fixture helper failed";
    assert!(runtime.try_root(Primitive::string(message)).is_none());
    let failure = evaluator.failure_root(message);
    assert_eq!(
        failure.string().map(|s| s.to_string_lossy()).as_deref(),
        Some(message)
    );
    drop(filler);
    let value = evaluator.evaluate_unit(&churn(1000)).unwrap();
    assert!(matches!(
        evaluator.cx().unwrap().values.borrow()[&value.index].view(),
        ValueView::Number(999.0)
    ));
    assert!(runtime.heap_stats().peak_managed_bytes <= options.memory_limit);
}

/// A function built from lowered core operations and literals
/// (light-cljs-477) reads no task-owned runtime object, so it publishes;
/// one core value passed around (`map`) still closes over it.
#[test]
fn functions_over_lowered_core_operations_publish() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let lowered = "(fn [x] (if (seq x) (count x) (list 'let ['value x] {:a (:k x)} #{1} \
                   (get x 1) (second x) (keyword? x) `(f ~x ~@x) [] #{x 2} (contains? x 0) \
                   (conj (disj #{x} 1) 2) (assoc {} :k (dissoc {} :k)) (with-meta [] (meta x)))))";
    for (source, publishes) in [(lowered, true), ("(fn [x] (map first x))", false)] {
        let code = native(source);
        // Only `__rt` intrinsics, no read of the runtime object `rt`.
        assert_eq!(
            !code.replace("__rt.", "").contains("rt."),
            publishes,
            "{code}"
        );
        let f = evaluator
            .evaluate_unit(&format!("rt => ({code})(rt)"))
            .unwrap();
        let value = evaluator.cx().unwrap().values.borrow()[&f.index].clone();
        assert_eq!(
            shared.0.runtime.publish(&value).is_ok(),
            publishes,
            "{code}"
        );
        evaluator.release(f).unwrap();
    }
}

/// Literals past the counted-constructor limit (4,096 operands) emit the
/// runtime's array constructors, which check what the inline forms check:
/// dynamic set members are distinct, and reader-distinct literal members or
/// keys that lowered to equal values fail explicitly instead of collapsing.
#[test]
fn large_literals_use_runtime_constructors_that_keep_the_inline_checks() {
    let numbers = |n: usize| (0..n).map(|i| format!("{i} ")).collect::<String>();
    for (source, constructor) in [
        (
            format!("(fn [x] #{{x {}}})", numbers(4097)),
            "literalSetFromArray(",
        ),
        (
            format!("(fn [] '#{{{}}})", numbers(4097)),
            "constantSetFromArray(",
        ),
        (format!("(fn [x] #{{x {}}})", numbers(5)), "__rt.formSet("),
    ] {
        let code = native(&source);
        assert!(code.contains(constructor), "{constructor}");
    }
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    // Within the limit a large list stays one native constructor: the
    // function reads no runtime object and publishes.
    for source in [
        format!("(fn [] (list {}))", numbers(1025)),
        format!("(fn [x] [x {}])", numbers(1025)),
    ] {
        let code = native(&source);
        assert!(!code.replace("__rt.", "").contains("rt."), "{source}");
        let f = evaluator
            .evaluate_unit(&format!("rt => ({code})(rt)"))
            .unwrap();
        let value = evaluator.cx().unwrap().values.borrow()[&f.index].clone();
        assert!(shared.0.runtime.publish(&value).is_ok(), "{source}");
        evaluator.release(f).unwrap();
    }
    let many = "const a = []; for (let i = 0; i < 300; i++) a.push(i);";
    for (body, outcome) in [
        ("rt.literalSetFromArray([...a, 7])", Err("Duplicate key")),
        (
            "rt.constantSetFromArray(['a', ...a, 'a'])",
            Err("native literal set with reader-distinct equal members"),
        ),
        (
            "rt.constantMapFromArray(['a', 1, 'a', 2])",
            Err("native literal map with reader-distinct equal keys"),
        ),
        ("rt.literalSetFromArray(a)", Ok(300)),
        ("rt.constantSetFromArray(['a', ...a])", Ok(301)),
        ("rt.constantMapFromArray(['a', 1, 'b', 2])", Ok(2)),
    ] {
        let count = outcome.unwrap_or(-1);
        let result = evaluator.evaluate_unit(&format!(
            "rt => {{ {many} if (__rt.formCount({body}) !== {count}) throw new Error('count'); \
             return null; }}"
        ));
        match (result, outcome) {
            (Err(e), Err(expected)) => {
                assert!(e.to_string().contains(expected), "{body}: {e}")
            }
            (Ok(v), Ok(_)) => evaluator.release(v).unwrap(),
            (Ok(_), Err(expected)) => panic!("{body}: no {expected}"),
            (Err(e), Ok(_)) => panic!("{body}: {e}"),
        }
    }
}

#[test]
fn a_record_built_by_a_helper_initializer_is_published_and_read_by_two_tasks_by_identity() {
    published_by_two_tasks(
        "(defrecord R [a b])",
        "(->R 1 [2])",
        r#""cljs.user/R", "cljs.user/->R", "cljs.user/map->R""#,
        "if (__rt.cljGet(r, __rt.formKeyword(null, 'a'), null) !== 1) throw new Error('field');
         if (!__rt.formEquals(r, roots['cljs.user/->R'](1, __rt.formVector(2)))) throw new Error('equality');",
    );
}

#[test]
fn an_immutable_methodless_deftype_built_by_a_helper_initializer_is_published() {
    // A `deftype` without methods or mutable fields is a native struct
    // type all the same (light-cljs-478).
    published_by_two_tasks(
        "(deftype R [a b])",
        "(R. 1 [2])",
        r#""cljs.user/R", "cljs.user/->R""#,
        "if (rt.nativeInterop(r).b === undefined) throw new Error('field b');",
    );
}

/// Two tasks read the value `construct` that a helper initializer builds
/// with the type `declaration` makes (a bundle registering `names`): one
/// published struct of the task's type, read by identity, whose field `a`
/// is 1. `check` reads `r` and `roots` further.
fn published_by_two_tasks(declaration: &str, construct: &str, names: &str, check: &str) {
    // Unit 3 is the declaration bundle, unit 7 a helper whose initializer
    // builds the value (light-cljs-478).
    let bundle = native(declaration);
    let helper = native(&format!("{declaration} {construct}"));
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let mut records = Vec::new();
    let mut counts = BridgeStats::default();
    for _task in 0..2 {
        let evaluator = Rc::new(shared.evaluator().unwrap());
        let (bundle, helper) = (bundle.clone(), helper.clone());
        evaluator
            .clone()
            .install_compiler_service(
                "__lightCompileUnit",
                Rc::new(move |_, id| {
                    Ok(if id == 3 {
                        bundle.clone()
                    } else {
                        helper.clone()
                    })
                }),
            )
            .unwrap();
        evaluator
            .register_compiler_bundle(
                &format!("rt => globalThis.__lightRegisterBundle([{names}], null, [], \"fixture.cljs:1\", false, false, 3)"),
                3,
            )
            .unwrap();
        evaluator
            .register_compiler_root("cljs.user/helper", Some(7), false, "fixture.cljs:2", None)
            .unwrap();
        let result = evaluator
            .evaluate_unit("rt => globalThis.__lightCompilerRoots['cljs.user/helper']")
            .unwrap();
        records.push(evaluator.cx().unwrap().values.borrow()[&result.index].clone());
        evaluator.release(result).unwrap();
        // Each task reads a field, and the record's type is the task's `R`:
        // one struct type, made with the bundle's shared factory.
        let check = evaluator
            .evaluate_unit(&format!(
                "rt => {{ const roots = globalThis.__lightCompilerRoots, r = roots['cljs.user/helper'];
                 if (rt.nativeInterop(r).a !== 1) throw new Error('interop before the type is read');
                 {check}
                 if (__rt.structTypeOf(r) !== roots['cljs.user/R']) throw new Error('type identity');
                 return null; }}",
            ))
            .unwrap();
        evaluator.release(check).unwrap();
        let stats = evaluator.bridge_stats();
        counts.helper_compiled += stats.helper_compiled;
        counts.helper_published += stats.helper_published;
        counts.helper_per_task += stats.helper_per_task;
    }
    // The bundle's and the helper's factories, each compiled once.
    assert_eq!(counts.helper_compiled, 2);
    assert_eq!(counts.helper_published, 1);
    assert_eq!(counts.helper_per_task, 0);
    shared.0.runtime.scope(|s| {
        assert!(s.same_object(s.local(&records[0]).unwrap(), s.local(&records[1]).unwrap()));
        assert_eq!(
            s.form_kind(s.local(&records[0]).unwrap()),
            Some(big_boy_pants_js::FormKind::Struct)
        );
    });
}

/// A unit source long enough to take the shared factory path, whose
/// factory keeps per-task state in the calling task's globals.
fn shared_unit(tail: &str) -> String {
    let padding = "/* padding */ ".repeat(SHARED_UNIT_BYTES / 10);
    format!(
        "rt => {{ {padding} globalThis.unitCalls = (globalThis.unitCalls || 0) + 1; \
         return (rt === globalThis.__lightNativeRuntime ? 40 : 0) + globalThis.unitCalls{tail}; }}"
    )
}

#[test]
fn a_unit_source_compiles_once_for_every_task_and_runs_in_each() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let first = shared.evaluator().unwrap();
    let second = shared.evaluator().unwrap();
    let roots = second.value_roots().unwrap();
    let source = shared_unit("");
    for (evaluator, calls) in [(&first, 1.0), (&second, 1.0), (&first, 2.0)] {
        let value = evaluator.evaluate_unit(&source).unwrap();
        // Called with the evaluating task's own runtime, in its own globals.
        assert!(matches!(
            evaluator.cx().unwrap().values.borrow()[&value.index].view(),
            ValueView::Number(n) if n == 40.0 + calls
        ));
        evaluator.release(value).unwrap();
        assert_eq!(numeric_global(evaluator, "unitCalls"), calls);
    }
    // The task's own runtime handle is not counted as a held value.
    assert_eq!(second.value_roots().unwrap(), roots);
    let counts = shared.helper_counts();
    assert_eq!(counts.units_compiled, 1);
    assert_eq!(counts.units_shared, 3);
    assert_eq!(counts.contexts, 2);
}

#[test]
fn a_unit_expression_that_reads_its_task_is_evaluated_by_each_task() {
    // Not a function literal: making the factory reads the evaluating
    // task's globals, so no task may get another's.
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let padding = "/* padding */ ".repeat(SHARED_UNIT_BYTES / 10);
    let source =
        format!("(() => {{ {padding} const n = globalThis.unitSeed; return rt => n; }})()");
    for seed in [41.0, 42.0] {
        let evaluator = shared.evaluator().unwrap();
        let set = evaluator
            .evaluate_unit(&format!(
                "rt => {{ globalThis.unitSeed = {seed}; return null; }}"
            ))
            .unwrap();
        evaluator.release(set).unwrap();
        let value = evaluator.evaluate_unit(&source).unwrap();
        assert!(matches!(
            evaluator.cx().unwrap().values.borrow()[&value.index].view(),
            ValueView::Number(n) if n == seed
        ));
    }
    assert_eq!(shared.helper_counts().units_compiled, 0);
}

#[test]
fn an_unshareable_or_invalid_unit_is_evaluated_by_its_task_with_the_same_diagnostic() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let evaluated = shared.helper_counts().units_evaluated;
    // A syntax error reports what evaluating the source reports.
    let invalid = shared_unit(" +");
    let error = evaluator.evaluate_unit(&invalid).unwrap_err();
    let reference = shared.evaluator().unwrap();
    let expected = reference
        .evaluate(&format!("({invalid})(globalThis.__lightNativeRuntime)"))
        .unwrap_err();
    assert_eq!(error.0, expected.0);
    // A factory capturing mutable state does not publish; it still runs.
    let padding = "/* padding */ ".repeat(SHARED_UNIT_BYTES / 10);
    let captured =
        format!("(() => {{ let n = 0; return rt => {{ {padding} n += 1; return n; }}; }})()");
    let value = evaluator.evaluate_unit(&captured).unwrap();
    assert!(matches!(
        evaluator.cx().unwrap().values.borrow()[&value.index].view(),
        ValueView::Number(1.0)
    ));
    let counts = shared.helper_counts();
    assert_eq!(counts.units_compiled, 0);
    assert_eq!(counts.units_shared, 0);
    assert_eq!(counts.units_evaluated, evaluated + 2);
    // Both are remembered as unshareable and not compiled again.
    assert!(evaluator.evaluate_unit(&invalid).is_err());
    assert_eq!(shared.0.factories.lock().unwrap().len(), 2);
}

#[test]
fn replayed_registration_blocks_run_in_order_and_full_blocks_compile_once() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let registrations = (0..REPLAY_BLOCK + 3)
        .map(|i| {
            format!(
                "rt => {{ globalThis.order = (globalThis.order || '') + '{i},'; \
                 globalThis.own = (globalThis.own ?? true) && rt === globalThis.__lightNativeRuntime; \
                 return null; }}"
            )
        })
        .collect::<Vec<_>>();
    let expected = (0..REPLAY_BLOCK + 3)
        .map(|i| format!("{i},"))
        .collect::<String>();
    for _ in 0..2 {
        let evaluator = shared.evaluator().unwrap();
        for block in registrations.chunks(REPLAY_BLOCK) {
            let value = evaluator.evaluate_unit(&replay_source(block)).unwrap();
            evaluator.release(value).unwrap();
        }
        let global = |name| {
            evaluator
                .cx()
                .unwrap()
                .task
                .global_value(name)
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            global("order").string().map(|s| s.to_string_lossy()),
            Some(expected.clone())
        );
        assert_eq!(global("own").view(), ValueView::Bool(true));
    }
    // The full block and the three-registration tail, once each.
    let counts = shared.helper_counts();
    assert_eq!((counts.units_compiled, counts.units_shared), (2, 4));
}

#[path = "../../tests/support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[test]
fn weak_bridge_entries_preserve_live_occurrences_and_raw_metadata() {
    let rt = Runtime::new();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let form = host
        .read_compiler_form(&crate::compiler_data::Source::new(
            "weak-identity.cljs",
            "^{:m [9007199254740993]} [1 2]",
        ))
        .unwrap()
        .unwrap();
    let data = form.data();
    let meta = data.metadata().unwrap();
    let mut bridge = Bridge::default();
    let original = rt.scope(|s| {
        bridge.remember(&form, 128, &mut 1000).unwrap();
        let value = bridge.import(s, &data, 128, &mut 1000).unwrap();
        s.persist(value)
    });
    rt.collect().unwrap();
    bridge.prune(rt.heap_stats().collections);
    let raw_meta = rt.scope(|s| {
        let value = bridge.import(s, &data, 128, &mut 1000).unwrap();
        assert!(s.same_object(value, s.local(&original).unwrap()));
        let result = bridge.export(s, value, 128, &mut 1000).unwrap();
        assert_eq!(result.identity(), data.identity());
        let remembered = bridge.forms[&result.identity()].span();
        assert_eq!(remembered.source().name(), form.span().source().name());
        assert_eq!(remembered.bytes(), form.span().bytes());
        s.persist(s.form_meta(s.form_value(value)).unwrap())
    });
    let weak = original.downgrade();
    drop(original);
    rt.collect().unwrap();
    assert!(!weak.is_live());
    bridge.prune(rt.heap_stats().collections);
    rt.scope(|s| {
        // The raw metadata outlives its occurrence. Reimporting must wrap
        // that same map, preserving exact integer data and its old source.
        let value = bridge.import(s, &meta, 128, &mut 1000).unwrap();
        assert!(s.same_object(s.form_value(value), s.local(&raw_meta).unwrap()));
        let exported = bridge
            .export(s, s.local(&raw_meta).unwrap(), 128, &mut 1000)
            .unwrap();
        assert_eq!(exported.identity(), meta.identity());
        assert!(
            bridge.import(s, &meta, 128, &mut 1).is_err(),
            "memo reuse still charges the whole subtree"
        );
        assert!(
            bridge.import(s, &meta, 1, &mut 1000).is_err(),
            "memo reuse still enforces depth"
        );
    });
    drop(raw_meta);
    rt.collect().unwrap();
    bridge.prune(rt.heap_stats().collections);
    assert!(bridge.imports.is_empty());
    assert!(bridge.native_objects.is_empty());
    assert!(bridge.forms.is_empty());
    assert!(bridge.costs.is_empty());
    // Reusing the compiler Datum after collection imports a fresh object
    // with its original provenance; stale allocation slots cannot alias it.
    rt.scope(|s| {
        bridge.remember(&form, 128, &mut 1000).unwrap();
        let value = bridge.import(s, &data, 128, &mut 1000).unwrap();
        assert_eq!(
            bridge.export(s, value, 128, &mut 1000).unwrap().identity(),
            data.identity()
        );
    });
}

fn collapsed_metadata_after_collection(text: &str) -> (Runtime, Bridge, Form, Rooted) {
    let rt = Runtime::new();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let form = host
        .read_compiler_form(&crate::compiler_data::Source::new(
            "collapsed-metadata.cljs",
            text,
        ))
        .unwrap()
        .unwrap();
    let metadata = form.metadata().unwrap();
    let overwritten = metadata.children()[1].data();
    let mut bridge = Bridge::default();
    let raw = rt.scope(|s| {
        bridge.remember(&form, 128, &mut 1000).unwrap();
        let value = bridge.import(s, &form.data(), 128, &mut 1000).unwrap();
        s.persist(s.form_meta(s.form_value(value)).unwrap())
    });
    let discarded = bridge.imports[&overwritten.identity()].occurrence.clone();
    rt.collect().unwrap();
    assert!(!discarded.is_live(), "the native map overwrites the vector");
    rt.scope(|_| bridge.prune(rt.heap_stats().collections));
    assert!(!bridge.imports.contains_key(&overwritten.identity()));
    (rt, bridge, form, raw)
}

#[test]
fn weak_bridge_pruning_preserves_collapsed_descendant_provenance() {
    let (rt, mut bridge, form, raw) =
        collapsed_metadata_after_collection("^{1 ^{:tag [9]} [1 [2 [3]]] 1.0 nil} []");
    let metadata = form.metadata().unwrap();
    rt.scope(|s| {
        let exported = bridge
            .export(s, s.local(&raw).unwrap(), 128, &mut 1000)
            .unwrap();
        assert_eq!(exported.identity(), metadata.data().identity());
        let expanded = Form::from_bridge_expansion(
            exported,
            &form,
            ExpansionOrigin {
                macro_name: Name::new(None, "metadata"),
                call: form.span(),
                definition: None,
                helpers: vec![],
            },
            &bridge.forms,
        );
        fn check_spans(original: &Form, restored: &Form) {
            assert_eq!(restored.data().identity(), original.data().identity());
            assert_eq!(
                restored.span().source().name(),
                original.span().source().name()
            );
            assert_eq!(restored.span().bytes(), original.span().bytes());
            assert_eq!(restored.node().origin, original.node().origin);
            for (original, restored) in original.children().iter().zip(restored.children()) {
                check_spans(original, &restored);
            }
            if let Some(metadata) = original.metadata() {
                check_spans(&metadata, &restored.metadata().unwrap());
            }
        }
        check_spans(&metadata, &expanded);
    });
    drop(raw);
    rt.collect().unwrap();
    rt.scope(|_| bridge.prune(rt.heap_stats().collections));
    assert!(bridge.forms.is_empty());
    assert!(bridge.costs.is_empty());
}

#[test]
fn weak_bridge_pruning_charges_the_complete_restored_datum() {
    let (rt, mut bridge, form, raw) =
        collapsed_metadata_after_collection("^{1 [1 [2 [3]]] 1.0 nil} []");
    let metadata = form.metadata().unwrap().data();
    rt.scope(|s| {
        assert!(
            bridge.import(s, &metadata, 5, &mut 5).is_err(),
            "rewrapping must charge overwritten reader descendants"
        );
        assert!(
            bridge.import(s, &metadata, 2, &mut 10).is_err(),
            "rewrapping must retain the reader subtree's original depth"
        );
        let mut remaining = 10;
        let restored = bridge.import(s, &metadata, 5, &mut remaining).unwrap();
        assert_eq!(remaining, 0);
        assert!(s.same_object(s.form_value(restored), s.local(&raw).unwrap()));
        assert_eq!(
            bridge.export(s, restored, 5, &mut 10).unwrap().identity(),
            metadata.identity()
        );
        assert!(bridge.export(s, restored, 5, &mut 9).is_err());
        assert!(bridge.export(s, restored, 4, &mut 10).is_err());
    });
}

#[test]
fn retained_environment_snapshots_survive_collection_and_pruning() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let function = evaluator
        .evaluate_unit(&native(
            "(fn [form env]\n\
               (if (js* \"globalThis.savedEnvironment\")\n\
                 [(js* \"globalThis.savedEnvironment\") env]\n\
                 (do (js* \"(globalThis.savedEnvironment = ~{})\" env) env)))",
        ))
        .unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let read = |host: &mut MacroHost, text| {
        host.read_compiler_form(&crate::compiler_data::Source::new("environment.cljs", text))
            .unwrap()
            .unwrap()
    };
    let namespace = read(&mut host, "{:name saved.ns :bindings [1 2 3]}").data();
    let mut envelope = |text| {
        let data = read(&mut host, text).data();
        let D::Collection { kind, children } = data.kind() else {
            panic!("expected an environment map")
        };
        let mut children = children.clone();
        children[1] = namespace.clone();
        Datum::new(D::Collection {
            kind: *kind,
            children,
        })
    };
    let first = envelope("{:ns nil :line 1 :old-only true}");
    let second = envelope("{:ns nil :line 2 :new-only true}");
    let call = read(&mut host, "(snapshot)");
    let expand = |host: &mut MacroHost, environment| {
        host.expand_javascript(
            &evaluator,
            function,
            &call,
            environment,
            ExpansionOrigin {
                macro_name: Name::new(None, "snapshot"),
                call: call.span(),
                definition: None,
                helpers: vec![],
            },
        )
        .unwrap()
    };
    assert_eq!(
        expand(&mut host, &first).data().identity(),
        first.identity()
    );
    let saved = shared.0.bridge.lock().unwrap().imports[&first.identity()]
        .occurrence
        .clone();
    expand(&mut host, &second);
    // expand_javascript releases every input/result handle. Only macro task
    // state holds the first envelope; the bridge roots the second one.
    assert_eq!(
        shared
            .0
            .bridge
            .lock()
            .unwrap()
            .environment
            .as_ref()
            .unwrap()
            .0
            .identity(),
        second.identity()
    );
    shared.0.runtime.collect().unwrap();
    assert!(saved.is_live());
    let result = expand(&mut host, &second);
    assert_eq!(
        shared.0.bridge.lock().unwrap().collected,
        shared.0.runtime.heap_stats().collections
    );
    let snapshots = result.children();
    assert_eq!(snapshots[0].data().identity(), first.identity());
    assert_eq!(snapshots[1].data().identity(), second.identity());
    for (snapshot, line, present, absent) in [
        (&snapshots[0], 1, "old-only", "new-only"),
        (&snapshots[1], 2, "new-only", "old-only"),
    ] {
        let get = |key| {
            snapshot
                .data()
                .get(&Datum::new(D::Keyword(Name::new(None, key))))
                .unwrap()
        };
        assert_eq!(get("ns").unwrap().identity(), namespace.identity());
        assert!(matches!(get("line").unwrap().kind(), D::Integer(n) if *n == line));
        assert!(matches!(get(present).unwrap().kind(), D::Boolean(true)));
        assert!(get(absent).is_none());
    }
    shared.0.runtime.scope(|s| {
        let bridge = shared.0.bridge.lock().unwrap();
        let old = s.form_value(s.upgrade(&saved).unwrap());
        let new = s.form_value(s.local(&bridge.environment.as_ref().unwrap().1).unwrap());
        let key = s
            .new_keyword(
                None,
                string(s, &"ns".encode_utf16().collect::<Vec<_>>()).unwrap(),
            )
            .unwrap();
        assert!(s.same_object(
            s.form_get(old, key).unwrap().unwrap(),
            s.form_get(new, key).unwrap().unwrap()
        ));
    });
    evaluator.release(function).unwrap();
}

/// The original 400-call namespace chain exceeded 64 MiB after 224 calls.
/// Collect while the compiler/session are still alive, so teardown cannot
/// conceal command or bridge retention. All macro definitions remain usable.
#[test]
fn namespace_macro_chains_fit_the_default_heap_with_linear_live_bytes() {
    let mut previous = None;
    for nss in [4, 8] {
        let root = temp_dir::TempDir::new("native-macro-memory");
        std::fs::create_dir(root.join("scale")).unwrap();
        for n in 0..nss {
            let mut source = format!("(ns scale.n{n}");
            if n > 0 {
                source.push_str(&format!(" (:require [scale.n{} :as prev])", n - 1));
            }
            source.push_str(")\n");
            for i in 0..50 {
                source.push_str(&format!(
                    "(defn h{i} [x] (list '+ x {i}))\n\
                     (defmacro m{i} [x] (list (quote +) x {i}))\n\
                     (defn f{i} [] (m{i} {n}))\n"
                ));
            }
            source.push_str("(defn total [] (+");
            for i in 0..50 {
                source.push_str(&format!(" (f{i})"));
            }
            if n > 0 {
                source.push_str(" (prev/total)");
            }
            source.push_str("))\n");
            std::fs::write(root.join(format!("scale/n{n}.cljs")), source).unwrap();
        }
        std::fs::write(
            root.join("scale/app.cljs"),
            format!(
                "(ns scale.app (:require [scale.n{} :as last]))\n(js/console.log (last/total))\n",
                nss - 1,
            ),
        )
        .unwrap();
        let mut host = MacroHost::new();
        host.use_native_macros();
        assert_eq!(host.evaluator_options.memory_limit, 64 << 20);
        host.evaluator_options.timeout = std::time::Duration::from_secs(600);
        host.evaluator_options.poll_budget = 100_000_000_000;
        host.limits.expansions = 10_000_000;
        host.limits.generated_nodes = 1_000_000_000;
        let mut stats = crate::CompileStats::default();
        let (program, _) = crate::compiler::analyze_namespace_entry_with_stats(
            "scale.app",
            std::slice::from_ref(&root.to_path_buf()),
            &mut host,
            Default::default(),
            &mut Vec::new(),
            &mut stats,
        )
        .unwrap();
        assert_eq!(stats.native_macro_calls.len(), nss * 50);
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(crate::emit_js(&program))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            if nss == 4 { "5200\n" } else { "11200\n" }
        );
        let shared = host
            .session_handle()
            .unwrap()
            .shared()
            .native_runtime(&host.evaluator_options)
            .unwrap();
        let rt = &shared.0.runtime;
        rt.collect().unwrap();
        let heap = rt.heap_stats();
        shared.0.bridge.lock().unwrap().prune(heap.collections);
        eprintln!(
            "{} calls: {heap:?}; objects {:?}",
            nss * 50,
            rt.heap_usage().objects
        );
        assert!(heap.peak_managed_bytes <= 64 << 20, "{heap:?}");
        assert!(heap.last_live_bytes < 24 << 20, "{heap:?}");
        if let Some(smaller) = previous {
            // Four more namespaces add under 2 MiB each, rather than
            // preserving an increasingly large environment at every call.
            assert!(heap.last_live_bytes < smaller + (8 << 20), "{heap:?}");
        }
        previous = Some(heap.last_live_bytes);
    }
}
