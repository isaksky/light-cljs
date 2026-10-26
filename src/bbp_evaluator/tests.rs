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
