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
    crate::emitter::emit_native_macro_unit(&program, true, &[])
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

#[test]
fn initializer_namespace_retains_bindings_and_breaks_ancestry_ties_by_source_unit() {
    for _ in 0..32 {
        let mut bridge = Bridge::default();
        for (unit, position, scope, id, bound) in [
            (10, 30, 3, 100, false),
            (20, 20, 2, 200, true),
            (30, 20, 2, 300, false),
            (40, 10, 1, 400, false),
        ] {
            bridge
                .initializer_callers
                .entry(unit)
                .or_default()
                .insert(position, scope);
            bridge
                .initializer_namespaces
                .insert((unit, scope), (id, bound));
        }
        bridge
            .initializer_parents
            .insert(10, HashSet::from([20, 30]));
        bridge.initializer_parents.insert(20, HashSet::from([40]));
        bridge.initializer_parents.insert(30, HashSet::from([40]));
        assert_eq!(bridge.initializer_caller(10), Some(1));
        assert_eq!(bridge.initializer_namespace(10), Some(200));
        bridge.initializer_namespaces.insert((10, 3), (100, true));
        assert_eq!(bridge.initializer_namespace(10), Some(100));
        bridge
            .initializer_callers
            .get_mut(&10)
            .unwrap()
            .insert(10, 1);
        bridge.initializer_namespaces.insert((10, 1), (101, false));
        assert_eq!(bridge.initializer_namespace(10), Some(101));
    }
}

#[test]
fn explicit_namespace_cache_retains_only_observable_snapshots() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let result = evaluator
        .evaluate_unit(
            "rt => {
                const saved = rt.core.currentNs;
                try {
                    const ns = new rt.core.Namespace(null, rt.symbol(null, 'ephemeral'));
                    rt.core.currentNs = ns;
                    return __rt.vector(rt.compilerForms.namespaceContext(), ns);
                } finally { rt.core.currentNs = saved; }
            }",
        )
        .unwrap();
    let (snapshot, namespace) = shared.0.runtime.scope(|s| {
        let cx = evaluator.cx().unwrap();
        let values = cx.values.borrow();
        let result = s.local(&values[&result.index]).unwrap();
        (
            s.persist(s.nth(result, 0).unwrap()),
            s.downgrade(s.nth(result, 1).unwrap()),
        )
    });
    evaluator.release(result).unwrap();
    shared.0.runtime.collect().unwrap();
    assert!(
        namespace.is_live(),
        "an observable snapshot preserves identity"
    );
    let weak_snapshot = snapshot.downgrade();
    drop(snapshot);
    shared.0.runtime.collect().unwrap();
    assert!(
        !weak_snapshot.is_live(),
        "the cache must not root snapshots"
    );
    assert!(
        !namespace.is_live(),
        "an unobserved namespace is collectable"
    );
}

#[test]
fn temporary_multimethod_tables_are_reclaimed() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    for batch in 0..4 {
        let result = evaluator
            .evaluate_unit(
                "rt => {
                    let witnesses = __rt.vector();
                    for (let n = 0; n < 12; n++) {
                        let items = __rt.formVector();
                        for (let i = 0; i < 1024; i++) items = __rt.cljConj(items, i + n);
                        const payload = items, method = () => __rt.formCount(payload);
                        const methods = __rt.atomNew(__rt.formMap(__rt.cljKeyword('x'), method));
                        const prefs = __rt.atomNew(__rt.formMap());
                        const m = new rt.core.MultiFn('temporary', rt.core.identity, null,
                            __rt.atomNew(rt.core.makeHierarchy()), methods, prefs,
                            __rt.atomNew(__rt.formMap()), __rt.atomNew(null));
                        if (rt.core.invokeMulti(m, [__rt.cljKeyword('x')]) !== 1024)
                            throw new Error('temporary dispatch lost its capture');
                        for (const value of [methods, prefs, payload, method,
                            __rt.structField(m, '__method_state'), __rt.structField(m, '__prefer_state')])
                            witnesses = __rt.conj(witnesses, value);
                    }
                    return witnesses;
                }",
            )
            .unwrap();
        let weak = shared.0.runtime.scope(|s| {
            let cx = evaluator.cx().unwrap();
            let values = cx.values.borrow();
            let result = s.local(&values[&result.index]).unwrap();
            (0..72)
                .map(|i| s.downgrade(s.nth(result, i).unwrap()))
                .collect::<Vec<_>>()
        });
        evaluator.release(result).unwrap();
        shared.0.runtime.collect().unwrap();
        for (i, value) in weak.iter().enumerate() {
            assert!(
                !value.is_live(),
                "batch {batch}, discarded table/capture {i}"
            );
        }
    }

    // Keep only supplied atoms, with no MultiFn or task-local view left.
    // A fresh task must still find the same histories after collection.
    let result = evaluator
        .evaluate_unit(
            "rt => {
                const methods = __rt.atomNew(__rt.formMap()), prefs = __rt.atomNew(__rt.formMap());
                const h = __rt.atomNew(rt.core.makeHierarchy());
                const payload = __rt.formVector(10, 20, 30), method = () => __rt.formCount(payload);
                const m = new rt.core.MultiFn('retained', rt.core.identity, null, h, methods, prefs,
                    __rt.atomNew(__rt.formMap()), __rt.atomNew(null));
                rt.core.addMethod(m, __rt.cljKeyword('x'), method);
                rt.core.preferMethod(m, __rt.cljKeyword('p'), __rt.cljKeyword('q'));
                return __rt.vector(methods, prefs, h, payload, method,
                    __rt.structField(m, '__method_state'), __rt.structField(m, '__prefer_state'));
            }",
        )
        .unwrap();
    let (tables, weak) = shared.0.runtime.scope(|s| {
        let cx = evaluator.cx().unwrap();
        let values = cx.values.borrow();
        let result = s.local(&values[&result.index]).unwrap();
        let tables = (0..3)
            .map(|i| s.nth(result, i).unwrap())
            .collect::<Vec<_>>();
        (
            s.persist(s.new_vector(&tables).unwrap()),
            (0..7)
                .map(|i| s.downgrade(s.nth(result, i).unwrap()))
                .collect::<Vec<_>>(),
        )
    });
    evaluator.release(result).unwrap();
    drop(evaluator);
    shared.0.runtime.collect().unwrap();
    assert!(weak.iter().all(WeakHandle::is_live));

    let consumer = shared.evaluator().unwrap();
    let input = consumer.id().unwrap();
    consumer.request(1, input, tables.clone()).unwrap();
    let function = consumer
        .evaluate_unit(
            "rt => tables => {
                const m = new rt.core.MultiFn('consumer', rt.core.identity, null, __rt.nth(tables, 2),
                    __rt.nth(tables, 0), __rt.nth(tables, 1), __rt.atomNew(__rt.formMap()), __rt.atomNew(null));
                if (rt.core.invokeMulti(m, [__rt.cljKeyword('x')]) !== 3
                    || !__rt.formEquals(rt.core.prefers(m),
                        __rt.formMap(__rt.cljKeyword('p'), __rt.formSet(__rt.cljKeyword('q')))))
                    throw new Error('shared tables lost their histories');
                return __rt.vector(__rt.structField(m, '__method_state'), __rt.structField(m, '__prefer_state'));
            }",
        )
        .unwrap();
    let result = consumer.call(function, &[input]).unwrap();
    shared.0.runtime.scope(|s| {
        let cx = consumer.cx().unwrap();
        let values = cx.values.borrow();
        let result = s.local(&values[&result.index]).unwrap();
        for i in 0..2 {
            assert!(
                s.persist(s.nth(result, i).unwrap())
                    .same_object(&weak[5 + i].upgrade().unwrap())
            );
        }
    });
    for value in [result, function, input] {
        consumer.release(value).unwrap();
    }
    drop((consumer, tables));
    shared.0.runtime.collect().unwrap();
    assert!(weak.iter().all(|value| !value.is_live()));
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
fn inherited_helper_callers_select_their_own_binding_context() {
    use crate::effect_lanes::{Lanes, Target, identity};
    use std::{sync::mpsc, time::Duration};

    // Also lend the earlier position after both candidates have registered:
    // choosing once at reservation would still select the wrong bindings.
    for inherit_before_reservation in [true, false] {
        let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
        let lanes = Arc::new(Lanes::default());
        lanes.enable();
        lanes.schedule([0, 1, 2], false);
        shared.set_lanes(lanes.clone());
        let key = DependencyKey::new(shared.0.runtime.world(), DependencyId::new(7), INITIALIZER);
        let helper = DependencyKey::new(shared.0.runtime.world(), DependencyId::new(8), HELPER);
        let gates = Arc::new([
            (Mutex::new(false), Condvar::new()),
            (Mutex::new(false), Condvar::new()),
        ]);
        let (arrived, arrivals) = mpsc::channel();
        let observed = gates.clone();
        *shared.0.on_initializer.lock().unwrap() = Some(Arc::new(move |task, dependency| {
            assert_eq!(dependency, key);
            let ValueView::Number(caller) = task.global_value("caller").unwrap().unwrap().view()
            else {
                panic!("missing caller");
            };
            arrived.send(caller as usize).unwrap();
            let gate = &observed[caller as usize];
            let (_released, timeout) = gate
                .1
                .wait_timeout_while(gate.0.lock().unwrap(), Duration::from_secs(5), |released| {
                    !*released
                })
                .unwrap();
            assert!(!timeout.timed_out(), "candidate was not released");
        }));
        std::thread::scope(|scope| {
            let _root = lanes.enter_root();
            let _earliest = lanes.enter_unit(0);
            let mut waiting =
                inherit_before_reservation.then(|| lanes.wait_for(Target::Cell(identity(helper))));
            let run = |caller, unit, produces_helper: bool| {
                let evaluator = Rc::new(shared.evaluator().unwrap());
                evaluator.clone().install_compiler_service("__lightCompileUnit", Rc::new(|_, _| {
                    Ok("rt => { __rt.effect(65534, 0); return globalThis.__lightCompilerRoots['fixture/*x*']; }".into())
                })).unwrap();
                evaluator.preload("fixture/*x*", "rt => 'root'").unwrap();
                evaluator
                    .register_compiler_root("fixture/value", Some(7), false, "fixture.cljs:1", None)
                    .unwrap();
                // Install the overlay before entering the held activity, so
                // the late-inheritance case reaches reservation first.
                evaluator
                    .evaluate_unit(&format!(
                        "rt => {{
                        globalThis.fixtureSaved = __lightSaveCompilerBinding('fixture/*x*');
                        if ({produces_helper}) __lightBindCompilerRoot('fixture/*x*', 'helper');
                        return null;
                    }}"
                    ))
                    .unwrap();
                let _root = lanes.enter_root();
                let _unit = lanes.enter_unit(unit);
                let _helper = produces_helper.then(|| lanes.enter_cell(identity(helper)));
                let result = evaluator
                    .evaluate_unit(&format!(
                        "rt => {{
                        globalThis.caller = {caller};
                        try {{
                            return globalThis.__lightCompilerRoots['fixture/value'];
                        }} finally {{ __lightRestoreCompilerBinding('fixture/*x*', globalThis.fixtureSaved); }}
                    }}"
                    ))
                    .unwrap();
                let cx = evaluator.cx().unwrap();
                let values = cx.values.borrow();
                assert_eq!(
                    values[&result.index].string().unwrap().to_string_lossy(),
                    "helper"
                );
                assert!(cx.initializers.borrow().is_empty());
                assert!(cx.initializer_waits.borrow().is_empty());
                assert!(cx.initializer_lanes.borrow().is_empty());
            };
            let indirect = scope.spawn(move || run(0, 2, true));
            assert_eq!(arrivals.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
            let direct = scope.spawn(move || run(1, 1, false));
            assert_eq!(arrivals.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
            if waiting.is_none() {
                waiting = Some(lanes.wait_for(Target::Cell(identity(helper))));
            }
            for gate in gates.iter() {
                *gate.0.lock().unwrap() = true;
                gate.1.notify_all();
            }
            indirect.join().unwrap();
            drop(waiting);
            lanes.finish(0, false);
            direct.join().unwrap();
        });
        assert_eq!(shared.0.counters.cleanups.load(Ordering::Relaxed), 1);
        assert!(shared.0.pending_initializers.lock().unwrap().is_empty());
    }
}

#[test]
fn cancelling_a_superseded_initializer_reserver_keeps_its_owner_and_peer_alive() {
    cancel_transferred_initializer(false);
}

#[test]
fn cancelling_a_transferred_initializer_owner_fails_waiting_peers_before_effects() {
    cancel_transferred_initializer(true);
}

fn cancel_transferred_initializer(cancel_owner: bool) {
    use crate::effect_lanes::Lanes;
    use std::{sync::mpsc, time::Duration};

    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let lanes = Arc::new(Lanes::default());
    lanes.enable();
    lanes.schedule([0, 1, 2], false);
    shared.set_lanes(lanes.clone());
    let key = DependencyKey::new(shared.0.runtime.world(), DependencyId::new(7), INITIALIZER);
    let gates = Arc::new([
        (Mutex::new(false), Condvar::new()),
        (Mutex::new(false), Condvar::new()),
        (Mutex::new(false), Condvar::new()),
    ]);
    let (arrived, arrivals) = mpsc::channel();
    let observed = gates.clone();
    *shared.0.on_initializer.lock().unwrap() = Some(Arc::new(move |task, dependency| {
        assert_eq!(dependency, key);
        let ValueView::Number(caller) = task.global_value("caller").unwrap().unwrap().view() else {
            panic!("missing caller");
        };
        arrived.send((caller as usize, task.clone())).unwrap();
        let gate = &observed[caller as usize];
        let (_released, timeout) = gate
            .1
            .wait_timeout_while(gate.0.lock().unwrap(), Duration::from_secs(5), |released| {
                !*released
            })
            .unwrap();
        assert!(!timeout.timed_out(), "candidate was not released");
    }));
    let (parked, parking) = mpsc::channel();
    shared.on_wait(Arc::new(move |task, dependency| {
        if dependency == key {
            parked.send(task.id()).unwrap();
        }
    }));
    let release = |caller: usize| {
        *gates[caller].0.lock().unwrap() = true;
        gates[caller].1.notify_all();
    };
    std::thread::scope(|scope| {
        let run = |caller| {
            let evaluator = Rc::new(shared.evaluator().unwrap());
            evaluator
                .clone()
                .install_compiler_service(
                    "__lightCompileUnit",
                    Rc::new(|_, _| Ok("rt => { __rt.effect(65534, 0); return 42; }".into())),
                )
                .unwrap();
            evaluator
                .register_compiler_root("fixture/value", Some(7), false, "fixture.cljs:1", None)
                .unwrap();
            let _root = lanes.enter_root();
            let _unit = lanes.enter_unit(caller);
            let result = evaluator.evaluate_unit(&format!(
                "rt => {{ globalThis.caller = {caller}; return globalThis.__lightCompilerRoots['fixture/value']; }}"
            ));
            let cx = evaluator.cx().unwrap();
            assert!(cx.initializers.borrow().is_empty());
            assert!(cx.initializer_waits.borrow().is_empty());
            assert!(cx.initializer_lanes.borrow().is_empty());
            lanes.finish(caller, result.is_err());
            result
                .map(|value| {
                    assert_eq!(
                        cx.values.borrow()[&value.index].view(),
                        ValueView::Number(42.0)
                    );
                })
                .map_err(|e| e.to_string())
        };
        // The later task reserves first; the earlier caller takes ownership
        // while the original reserver is still held before factory execution.
        let original = scope.spawn(move || run(2));
        let (caller, original_task) = arrivals.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(caller, 2);
        let owner = scope.spawn(move || run(0));
        let (caller, owner_task) = arrivals.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(caller, 0);
        let peer = scope.spawn(move || run(1));
        let (caller, peer_task) = arrivals.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(caller, 1);
        assert_eq!(
            shared.0.pending_initializers.lock().unwrap()[&key].owner(Some(&lanes), key),
            owner_task.id()
        );

        release(1);
        assert_eq!(
            parking.recv_timeout(Duration::from_secs(5)).unwrap(),
            peer_task.id()
        );
        if cancel_owner {
            let _cancel = owner_task.cancel();
            release(0);
            assert!(owner.join().unwrap().unwrap_err().contains("cancelled"));
            release(2);
            let failure = original.join().unwrap().unwrap_err();
            assert!(failure.contains("cancelled"), "{failure}");
            let failure = peer.join().unwrap().unwrap_err();
            assert!(failure.contains("cancelled"), "{failure}");
        } else {
            let _cancel = original_task.cancel();
            release(2);
            assert!(original.join().unwrap().unwrap_err().contains("cancelled"));
            assert_eq!(
                shared.0.pending_initializers.lock().unwrap()[&key].owner(Some(&lanes), key),
                owner_task.id()
            );
            release(0);
            owner.join().unwrap().unwrap();
            peer.join().unwrap().unwrap();
        }
    });
    assert_eq!(
        shared.0.counters.cleanups.load(Ordering::Relaxed),
        usize::from(!cancel_owner)
    );
    assert!(shared.0.pending_initializers.lock().unwrap().is_empty());
    assert!(shared.0.control.poll().is_ok());
    match shared.0.runtime.cell_status(key) {
        big_boy_pants_js::CellStatus::Failed(_) if cancel_owner => {}
        big_boy_pants_js::CellStatus::Ready(_) if !cancel_owner => {}
        status => panic!("unexpected initializer outcome: {status:?}"),
    }
}

#[test]
fn cancelling_a_reserved_initializer_fails_its_claim_before_factory_effects() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    *shared.0.on_initializer.lock().unwrap() = Some(Arc::new(|task, _| {
        let _cancel = task.cancel();
    }));
    let evaluator = Rc::new(shared.evaluator().unwrap());
    evaluator
        .clone()
        .install_compiler_service(
            "__lightCompileUnit",
            Rc::new(|_, _| Ok("rt => { __rt.effect(65534, 0); return 42; }".into())),
        )
        .unwrap();
    evaluator
        .register_compiler_root("fixture/value", Some(7), false, "fixture.cljs:1", None)
        .unwrap();
    let failure = evaluator
        .evaluate_unit("rt => globalThis.__lightCompilerRoots['fixture/value']")
        .unwrap_err();
    assert!(failure.0.contains("cancelled"), "{failure}");
    assert_eq!(
        shared.0.counters.cleanups.load(Ordering::Relaxed),
        0,
        "a reserved initializer must not run before task selection"
    );
    let key = DependencyKey::new(shared.0.runtime.world(), DependencyId::new(7), INITIALIZER);
    assert!(matches!(
        shared.0.runtime.cell_status(key),
        big_boy_pants_js::CellStatus::Failed(_)
    ));
    assert!(shared.0.pending_initializers.lock().unwrap().is_empty());
    assert!(shared.0.control.poll().is_ok());
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
        gensyms: crate::generated_names::NativeGensyms::capture(),
        bridge: evaluator.bridge.clone(),
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

#[test]
fn native_gensym_reservations_count_against_the_shared_heap_limit() {
    let options = EvaluatorOptions {
        memory_limit: 4 << 20,
        ..Default::default()
    };
    let shared = BbpRuntime::new(&options).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let result = evaluator.evaluate_unit("rt => { const prefix='x'.repeat(4096); let n=0; try { for(;n<20000;n++) rt.core.gensym(prefix); } catch(e) { return n; } throw new Error('unaccounted gensym names'); }");
    match result {
        Ok(value) => assert!(
            matches!(evaluator.cx().unwrap().values.borrow()[&value.index].view(), ValueView::Number(n) if n > 0.0 && n < 20000.0)
        ),
        // A full reservation table may also deny the result table's entry.
        Err(error) => assert!(error.0.contains("heap byte limit exceeded"), "{error}"),
    }
    let bridge = shared.0.bridge.lock().unwrap();
    assert!(!bridge.generated_names.is_empty());
    assert!(bridge.generated_names.len() < 20000);
    let stats = shared.0.runtime.heap_stats();
    assert!(
        stats.peak_managed_bytes <= options.memory_limit,
        "{stats:?}"
    );
}

#[test]
fn identity_numeric_conversions_keep_imported_scalar_occurrences() {
    let evaluator = BbpEvaluator::new(Default::default()).unwrap();
    for operation in ["byte", "short", "float", "double"] {
        let function = evaluator
            .evaluate_unit(&format!("rt => x => rt.core.{operation}(x)"))
            .unwrap();
        for kind in [
            D::Integer(9_007_199_254_740_993),
            D::Character(b'a' as u16),
            D::String(vec![0xd800].into()),
        ] {
            let input = Datum::new(kind);
            let argument = evaluator.import_data(&input, Default::default()).unwrap();
            let result = evaluator.call(function, &[argument]).unwrap();
            let root = evaluator.cx().unwrap().values.borrow()[&result.index].clone();
            let output = evaluator
                .host_scope(|s| {
                    evaluator.with_bridge(|bridge| {
                        bridge.export(s, s.local(&root).map_err(error)?, 16, &mut 100)
                    })
                })
                .unwrap();
            assert_eq!(
                output.identity(),
                input.identity(),
                "{operation}: {input:?}"
            );
            evaluator.release(result).unwrap();
            evaluator.release(argument).unwrap();
        }
        evaluator.release(function).unwrap();
    }
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

/// Function roots publish with either lowered operations or consuming-task
/// runtime reads (light-cljs-521). Mutable user captures remain task-owned.
#[test]
fn native_function_roots_publish_without_capturing_the_runtime() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let lowered = "(fn [x] (if (seq x) (count x) (list 'let ['value x] {:a (:k x)} #{1} \
                   (get x 1) (second x) (keyword? x) `(f ~x ~@x) [] #{x 2} (contains? x 0) \
                   (conj (disj #{x} 1) 2) (assoc {} :k (dissoc {} :k)) (with-meta [] (meta x)))))";
    for (source, publishes) in [
        (lowered, true),
        ("(fn [x] (map first x))", true),
        ("(fn [x] *print-length*)", true),
        (
            "(let [state (js/Array.)] (fn [x] (.push state x) x))",
            false,
        ),
    ] {
        let code = native(source);
        // Intrinsics or task-global runtime reads never close over `rt`.
        assert!(!code.replace("__rt.", "").contains("rt."), "{code}");
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
/// runtime's array constructors: dynamic sets reject duplicates, while
/// constant literals preserve every position after lowering.
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
        ("rt.constantSetFromArray(['a', ...a, 'a'])", Ok(302)),
        ("rt.constantMapFromArray(['a', 1, 'a', 2])", Ok(2)),
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

fn positional_metadata_after_collection(text: &str) -> (Runtime, Bridge, Form, Rooted) {
    let rt = Runtime::new();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let form = host
        .read_compiler_form(&crate::compiler_data::Source::new(
            "positional-metadata.cljs",
            text,
        ))
        .unwrap()
        .unwrap();
    let metadata = form.metadata().unwrap();
    let retained = metadata.children()[1].data();
    let mut bridge = Bridge::default();
    let raw = rt.scope(|s| {
        bridge.remember(&form, 128, &mut 1000).unwrap();
        let value = bridge.import(s, &form.data(), 128, &mut 1000).unwrap();
        s.persist(s.form_meta(s.form_value(value)).unwrap())
    });
    let descendant = bridge.imports[&retained.identity()].occurrence.clone();
    rt.collect().unwrap();
    assert!(
        descendant.is_live(),
        "positional metadata traces the first equal key’s vector"
    );
    rt.scope(|_| bridge.prune(rt.heap_stats().collections));
    assert!(bridge.imports.contains_key(&retained.identity()));
    (rt, bridge, form, raw)
}

#[test]
fn weak_bridge_pruning_preserves_positional_descendant_provenance() {
    let (rt, mut bridge, form, raw) =
        positional_metadata_after_collection("^{1 ^{:tag [9]} [1 [2 [3]]] 1.0 nil} []");
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
        positional_metadata_after_collection("^{1 [1 [2 [3]]] 1.0 nil} []");
    let metadata = form.metadata().unwrap().data();
    rt.scope(|s| {
        assert!(
            bridge.import(s, &metadata, 5, &mut 5).is_err(),
            "rewrapping must charge every positional reader descendant"
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

#[test]
fn catalog_objects_match_plain_objects_before_and_after_demand() {
    use crate::compilation_session::Registration;
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parent = Rc::new(shared.evaluator().unwrap());
    parent
        .clone()
        .install_compiler_service(
            "__lightCompileUnit",
            Rc::new(|_, id| Ok(format!("rt => {id}"))),
        )
        .unwrap();
    let names = ["alpha", "10", "2", "01", "4294967295", "omega"];
    let registrations = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            Registration::Root(
                (*name).into(),
                Some(i + 1),
                None,
                false,
                "fixture.cljs:1".into(),
            )
        })
        .collect::<Vec<_>>();
    parent.install_catalog(0, &registrations).unwrap();
    // Each row gets a fresh object, also after demanding its initial getters.
    // Compare observable values, attributes and key order; getter identity is
    // tested within each object rather than between unrelated closures.
    let cases = [
        (
            "get/has",
            "return [o.alpha, 'alpha' in o, 'missing' in o, 'toString' in o];",
        ),
        (
            "assignment",
            "o.alpha = 80; o.extra = 81; return [o.alpha, o.extra];",
        ),
        ("delete", "return [delete o.alpha, 'alpha' in o, o.alpha];"),
        (
            "delete/recreate",
            "delete o.alpha; Object.defineProperty(o, 'alpha', {value: 80, configurable: true, enumerable: true, writable: true});",
        ),
        ("direct delete/recreate", "delete o.direct; o.direct = 80;"),
        (
            "full data descriptor",
            "Object.defineProperty(o, 'alpha', {value: 80, configurable: true, enumerable: true, writable: true}); return o.alpha;",
        ),
        (
            "full accessor descriptor",
            "Object.defineProperty(o, 'alpha', {get: () => 80, set: v => {o.extra = v;}, configurable: true, enumerable: true}); o.alpha = 81; return [o.alpha, o.extra];",
        ),
        (
            "enumerable only",
            "Object.defineProperty(o, 'alpha', {enumerable: true}); return o.alpha;",
        ),
        (
            "configurable only",
            "Object.defineProperty(o, 'alpha', {configurable: false}); return o.alpha;",
        ),
        (
            "empty descriptor",
            "Object.defineProperty(o, 'alpha', {}); return o.alpha;",
        ),
        (
            "writable conversion",
            "Object.defineProperty(o, 'alpha', {writable: true}); o.alpha = 80; return o.alpha;",
        ),
        (
            "descriptor identity",
            "const a = Object.getOwnPropertyDescriptor(o, 'alpha'); o.alpha; const b = Object.getOwnPropertyDescriptor(o, 'alpha'); return a.get === b.get;",
        ),
        (
            "saved getter after replacement",
            "const get = Object.getOwnPropertyDescriptor(o, 'alpha').get; Object.defineProperty(o, 'alpha', {value: 80}); return [o.alpha, get.call(o)];",
        ),
        (
            "copied getter after replacement",
            "const copy = Object.defineProperty({}, 'alpha', Object.getOwnPropertyDescriptor(o, 'alpha')); Object.defineProperty(o, 'alpha', {value: 80}); return [o.alpha, copy.alpha, copy.alpha];",
        ),
        (
            "setter only",
            "Object.defineProperty(o, 'alpha', {set: v => {o.extra = v;}}); o.alpha = 80; return [o.alpha, o.extra];",
        ),
        (
            "strict assignment",
            "try { (() => { 'use strict'; o.alpha = 80; })(); return 'accepted'; } catch (e) { return e.name; }",
        ),
        (
            "strict inherited assignment",
            "const child = Object.create(o); let error; try { (() => { 'use strict'; child.alpha = 80; })(); } catch (e) { error = e.name; } return [error, child.alpha, Object.getOwnPropertyDescriptor(child, 'alpha') !== undefined];",
        ),
        (
            "mixed ordering",
            "delete o['2']; o['2'] = 80; delete o['01']; o['01'] = 81; o[Symbol.for('last')] = 82; o['0'] = 83;",
        ),
        (
            "inherited assignment",
            "const child = Object.create(o); child.alpha = 80; child.direct = 81; return [child.alpha, child.direct, Object.getOwnPropertyDescriptor(child, 'alpha') !== undefined, Object.getOwnPropertyDescriptor(child, 'direct') !== undefined, o.direct];",
        ),
        (
            "preventExtensions",
            "Object.preventExtensions(o); o.extra = 80; return [Reflect.defineProperty(o, 'extra', {value: 81}), delete o.alpha, o.alpha];",
        ),
        (
            "seal",
            "Object.seal(o); o.direct = 80; return [delete o.alpha, o.alpha, Reflect.defineProperty(o, 'alpha', {enumerable: true})];",
        ),
        (
            "freeze",
            "Object.freeze(o); o.direct = 80; return [delete o.alpha, o.alpha, o.direct, Reflect.defineProperty(o, 'alpha', {value: 81})];",
        ),
    ];
    let mut failures = Vec::new();
    #[cfg(light_quickjs)]
    let legacy = crate::evaluator::Evaluator::new(EvaluatorOptions::default()).unwrap();
    for (demanded, overlaid) in [(false, false), (true, false), (false, true), (true, true)] {
        for (label, action) in cases {
            let child = parent
                .spawn_producer(
                    &parent.cx().unwrap(),
                    parent.service.borrow().clone(),
                    |_| {},
                )
                .unwrap();
            let source = format!(
                r#"rt => {{
                const names = ['alpha', '10', '2', '01', '4294967295', 'omega'];
                const makePlain = () => {{
                    const o = {{}};
                    names.forEach((name, i) => Object.defineProperty(o, name, {{get: () => i + 1, configurable: true}}));
                    return o;
                }};
                const observe = o => {{
                    if ({overlaid}) {{ o.direct = 70; o[Symbol.for('first')] = 71; o.tail = 72; }}
                    if ({demanded}) for (const name of names) o[name];
                    let result, failure = null;
                    try {{ result = (() => {{ {action} }})(); }} catch (e) {{ failure = String(e); }}
                    const descriptor = key => {{
                        const d = Object.getOwnPropertyDescriptor(o, key);
                        return 'value' in d ? ['data', d.value, d.writable, d.enumerable, d.configurable]
                            : ['accessor', typeof d.get, typeof d.set, d.enumerable, d.configurable];
                    }};
                    const keys = Reflect.ownKeys(o), enumerated = [];
                    for (const key in o) enumerated.push(key);
                    return JSON.stringify([result, failure, keys.map(String), Object.keys(o), enumerated,
                        keys.map(descriptor), Object.isExtensible(o), Object.isSealed(o), Object.isFrozen(o)]);
                }};
                const expected = observe(makePlain()), actual = observe(globalThis.__lightCompilerRoots);
                return actual === expected ? expected : 'difference: expected ' + expected + '\nactual   ' + actual;
            }}"#
            );
            let result = match child.evaluate_unit(&source) {
                Ok(result) => result,
                Err(error) => {
                    failures.push(format!(
                        "{label} (demanded={demanded}, overlaid={overlaid}): {error}"
                    ));
                    continue;
                }
            };
            let difference = child.cx().unwrap().values.borrow()[&result.index]
                .string()
                .unwrap()
                .to_string_lossy();
            child.release(result).unwrap();
            if difference.starts_with("difference:") {
                failures.push(format!(
                    "{label} (demanded={demanded}, overlaid={overlaid}): {difference}"
                ));
            }
            #[cfg(light_quickjs)]
            {
                let oracle = source.replace(
                    "observe(globalThis.__lightCompilerRoots)",
                    "observe(makePlain())",
                );
                let result = legacy.evaluate_unit(&oracle).unwrap();
                let expected = legacy.string(result).unwrap();
                legacy.release(result).unwrap();
                if difference != expected {
                    failures.push(format!("QuickJS {label} (demanded={demanded}, overlaid={overlaid}): expected {expected}; actual {difference}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn catalog_saved_getters_preserve_mutable_initializer_state() {
    use crate::compilation_session::Registration;
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parent = Rc::new(shared.evaluator().unwrap());
    parent.clone().install_compiler_service("__lightCompileUnit", Rc::new(|_, id| {
        Ok(format!("rt => {{ globalThis.initializations = (globalThis.initializations ?? 0) + 1; return {{unit: {id}, count: 0}}; }}"))
    })).unwrap();
    let registration = |unit| {
        Registration::Root(
            "alpha".into(),
            Some(unit),
            None,
            false,
            "fixture.cljs:1".into(),
        )
    };
    parent.install_catalog(0, &[registration(1)]).unwrap();
    // A second task exercises the completed unpublishable initializer fallback.
    let child = parent
        .spawn_producer(
            &parent.cx().unwrap(),
            parent.service.borrow().clone(),
            |_| {},
        )
        .unwrap();
    let before = r#"rt => {
        const o = globalThis.__lightCompilerRoots;
        globalThis.oldGetter = Object.getOwnPropertyDescriptor(o, 'alpha').get;
        globalThis.oldCopy = Object.defineProperty({}, 'alpha', {get: oldGetter});
        globalThis.oldValue = o.alpha;
        oldValue.count = 7;
        if (initializations !== 1 || oldCopy.alpha !== oldValue) throw 'first initialization';
    }"#;
    for task in [&*parent, &child] {
        let value = task.evaluate_unit(before).unwrap();
        task.release(value).unwrap();
    }
    parent.install_catalog(1, &[registration(2)]).unwrap();
    child.install_catalog(1, &[registration(2)]).unwrap();
    // Compare saved/copied getters across generations to ordinary lazy getters.
    let cases = [
        (
            "return [oldGetter.call(o) === oldValue, oldGetter.call(o).count];",
            "[true,7]",
        ),
        (
            "return [oldCopy.alpha === oldValue, oldCopy.alpha.count];",
            "[true,7]",
        ),
        (
            "oldCopy.alpha.count++; return [oldGetter.call({}) === oldValue, oldValue.count];",
            "[true,8]",
        ),
        (
            "return [newGetter.call({}) === newValue, o.alpha === newValue, o.alpha.count];",
            "[true,true,9]",
        ),
    ];
    for task in [&*parent, &child] {
        let value = task
            .evaluate_unit(
                r#"rt => {
            const o = globalThis.__lightCompilerRoots;
            globalThis.newGetter = Object.getOwnPropertyDescriptor(o, 'alpha').get;
            globalThis.newValue = o.alpha;
            newValue.count = 9;
        }"#,
            )
            .unwrap();
        task.release(value).unwrap();
        #[cfg(light_quickjs)]
        let legacy = crate::evaluator::Evaluator::new(EvaluatorOptions::default()).unwrap();
        #[cfg(light_quickjs)]
        {
            let value = legacy.evaluate_unit(r#"rt => {
                globalThis.initializations = 0;
                const getter = unit => { let value; return () => value ?? (initializations++, value = {unit, count: 0}); };
                globalThis.__lightCompilerRoots = Object.defineProperty({}, 'alpha', {get: getter(1), configurable: true});
                const o = globalThis.__lightCompilerRoots;
                globalThis.oldGetter = Object.getOwnPropertyDescriptor(o, 'alpha').get;
                globalThis.oldCopy = Object.defineProperty({}, 'alpha', {get: oldGetter});
                globalThis.oldValue = o.alpha; oldValue.count = 7;
                Object.defineProperty(o, 'alpha', {get: getter(2)});
                globalThis.newGetter = Object.getOwnPropertyDescriptor(o, 'alpha').get;
                globalThis.newValue = o.alpha; newValue.count = 9;
            }"#).unwrap();
            legacy.release(value).unwrap();
        }
        for (action, expected) in cases {
            let source = format!(
                "rt => {{ const o = globalThis.__lightCompilerRoots; const result = (() => {{ {action} }})(); if (initializations !== 2) throw 'initializer effect replayed: ' + initializations; return JSON.stringify(result); }}"
            );
            let value = task.evaluate_unit(&source).unwrap();
            let actual = task.cx().unwrap().values.borrow()[&value.index]
                .string()
                .unwrap()
                .to_string_lossy();
            task.release(value).unwrap();
            assert_eq!(actual, expected, "{action}");
            #[cfg(light_quickjs)]
            {
                let value = legacy.evaluate_unit(&source).unwrap();
                assert_eq!(actual, legacy.string(value).unwrap(), "QuickJS {action}");
                legacy.release(value).unwrap();
            }
        }
    }
}

#[test]
fn catalog_frontiers_preserve_mutations_and_unchanged_getter_identity() {
    use crate::compilation_session::Registration;
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = Rc::new(shared.evaluator().unwrap());
    evaluator
        .clone()
        .install_compiler_service(
            "__lightCompileUnit",
            Rc::new(|_, id| Ok(format!("rt => {id}"))),
        )
        .unwrap();
    let registration = |name: &str, unit| {
        Registration::Root(
            name.into(),
            Some(unit),
            None,
            false,
            "fixture.cljs:1".into(),
        )
    };
    let run = |source| {
        let value = evaluator.evaluate_unit(source).unwrap();
        evaluator.release(value).unwrap();
    };
    evaluator
        .install_catalog(0, &[registration("alpha", 1), registration("beta", 2)])
        .unwrap();
    run(r#"rt => {
        const o = globalThis.__lightCompilerRoots;
        globalThis.registry = o;
        globalThis.savedGetter = Object.getOwnPropertyDescriptor(o, 'alpha').get;
        Object.defineProperty(o, 'alpha', {enumerable: true});
        delete o.beta;
        o.first = 7; o.last = 8; delete o.first; o.first = 9;
    }"#);
    evaluator
        .install_catalog(2, &[registration("gamma", 3)])
        .unwrap();
    run(r#"rt => {
        const o = globalThis.__lightCompilerRoots;
        if (o !== globalThis.registry) throw 'registry identity';
        if (Object.getOwnPropertyDescriptor(o, 'alpha').get !== globalThis.savedGetter) throw 'unchanged getter replaced';
        if (Object.keys(o).join(',') !== 'alpha,last,first') throw 'partial descriptor lost';
        if ('beta' in o || Reflect.ownKeys(o).join(',') !== 'alpha,last,first,gamma') throw 'mutation lost';
    }"#);
    evaluator
        .install_catalog(3, &[registration("alpha", 4), registration("beta", 5)])
        .unwrap();
    run(r#"rt => {
        const o = globalThis.__lightCompilerRoots;
        if (Object.getOwnPropertyDescriptor(o, 'alpha').get === globalThis.savedGetter) throw 'redefinition lost';
        if (o.alpha !== 4 || o.beta !== 5 || globalThis.savedGetter.call(o) !== 1 || o.alpha !== 4) throw 'getter frontier';
        if (Object.keys(o).join(',') !== 'alpha,last,first') throw 'redefinition lost attributes';
        if (Reflect.ownKeys(o).join(',') !== 'alpha,last,first,gamma,beta') throw 'recreation order';
    }"#);
    run(r#"rt => {
        Object.defineProperty(globalThis.__lightCompilerRoots, 'alpha', {value: 80, configurable: true});
        globalThis.__lightCompilerRoots.preloaded = 81;
    }"#);
    evaluator
        .install_catalog(5, &[registration("alpha", 6), registration("preloaded", 7)])
        .unwrap();
    run(r#"rt => {
        const o = globalThis.__lightCompilerRoots;
        if (o.alpha !== 6 || o.preloaded !== 81) throw 'preload confused with override';
    }"#);
    evaluator
        .install_catalog(7, &[registration("preloaded", 8)])
        .unwrap();
    run(r#"rt => {
        if (globalThis.__lightCompilerRoots.preloaded !== 81) throw 'preload lost at redefinition';
    }"#);
}

#[test]
fn catalog_redefinitions_match_plain_data_and_preload_descriptors() {
    use crate::compilation_session::{BundleRecord, Registration};
    // Extend the object differential cases across catalog command boundaries.
    for (label, preload, bundle, expected) in [
        ("root override", false, false, 2),
        ("root preload", true, false, 80),
        ("bundle override", false, true, 80),
        ("bundle preload", true, true, 80),
    ] {
        let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
        let task = Rc::new(shared.evaluator().unwrap());
        task.clone().install_compiler_service("__lightCompileUnit", Rc::new(|_, id| {
            Ok(format!("rt => {{ if (globalThis.__lightCompilerBundleProducts !== undefined) globalThis.__lightCompilerBundleProducts.alpha = {id}; return {id}; }}"))
        })).unwrap();
        let registration = |unit| {
            if bundle {
                Registration::Bundle(Arc::new(BundleRecord {
                    products: vec!["alpha".into()],
                    unit,
                    extensions: vec![],
                    location: "fixture.cljs:1".into(),
                    once: false,
                    group: false,
                }))
            } else {
                Registration::Root(
                    "alpha".into(),
                    Some(unit),
                    None,
                    false,
                    "fixture.cljs:1".into(),
                )
            }
        };
        let run = |source: &str| {
            let value = task.evaluate_unit(source).unwrap();
            let result = task.cx().unwrap().values.borrow()[&value.index]
                .string()
                .map(|s| s.to_string_lossy());
            task.release(value).unwrap();
            result
        };
        if preload {
            run(
                "rt => Object.defineProperty(globalThis.__lightCompilerRoots, 'alpha', {value: 80, writable: true, configurable: true, enumerable: true})",
            );
        }
        task.install_catalog(0, &[registration(1)]).unwrap();
        run(
            "rt => Object.defineProperty(globalThis.__lightCompilerRoots, 'alpha', {value: 80, writable: true, configurable: true, enumerable: true})",
        );
        task.install_catalog(1, &[registration(2)]).unwrap();
        let source = format!(
            r#"rt => {{
            const o = globalThis.__lightCompilerRoots;
            const expected = {{}};
            Object.defineProperty(expected, 'alpha', {{value: 80, writable: true, configurable: true, enumerable: true}});
            if ({expected} === 2) Object.defineProperty(expected, 'alpha', {{get: () => 2, configurable: true}});
            const trace = object => {{ const d = Object.getOwnPropertyDescriptor(object, 'alpha'); return JSON.stringify([object.alpha, 'value' in d, d.writable, typeof d.get, d.enumerable, d.configurable, Reflect.ownKeys(object)]); }};
            const actual = trace(o), reference = trace(expected);
            return actual === reference ? reference : 'difference: ' + actual + ' != ' + reference;
        }}"#
        );
        let actual = run(&source).unwrap();
        assert!(!actual.starts_with("difference:"), "{label}: {actual}");
        #[cfg(light_quickjs)]
        {
            let legacy = crate::evaluator::Evaluator::new(EvaluatorOptions::default()).unwrap();
            let oracle =
                source.replace("const actual = trace(o)", "const actual = trace(expected)");
            let value = legacy.evaluate_unit(&oracle).unwrap();
            assert_eq!(actual, legacy.string(value).unwrap(), "QuickJS {label}");
            legacy.release(value).unwrap();
        }
    }
}

#[test]
fn catalog_mixed_redefinitions_and_inherited_preloads_follow_registration_order() {
    use crate::compilation_session::{BundleRecord, Registration};
    let root =
        |unit| Registration::Root("alpha".into(), Some(unit), None, false, "fixture:1".into());
    let bundle = |unit| {
        Registration::Bundle(Arc::new(BundleRecord {
            products: vec!["alpha".into()],
            unit,
            extensions: vec![],
            location: "fixture:1".into(),
            once: false,
            group: false,
        }))
    };
    for preload in [
        "",
        "Object.defineProperty(o, 'alpha', {value: 80, configurable: true});",
        "Object.defineProperty(o, 'alpha', {get: () => 81, configurable: true});",
        "Object.setPrototypeOf(o, {alpha: 82});",
        "Object.setPrototypeOf(o, Object.defineProperty({}, 'alpha', {get: () => 83}));",
    ] {
        for registrations in [
            vec![root(1), bundle(2), root(3)],
            vec![bundle(1), root(2), bundle(3)],
            vec![root(1), root(2), bundle(3), root(4)],
        ] {
            let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
            let run = |task: &BbpEvaluator, source: &str| {
                let value = task.evaluate_unit(source).unwrap();
                let result = task.cx().unwrap().values.borrow()[&value.index]
                    .string()
                    .map(|s| s.to_string_lossy());
                task.release(value).unwrap();
                result
            };
            let tasks = (0..2).map(|_| {
                let task = Rc::new(shared.evaluator().unwrap());
                task.clone().install_compiler_service("__lightCompileUnit", Rc::new(|_, unit| {
                    Ok(format!("rt => {{ if(globalThis.__lightCompilerBundleProducts)globalThis.__lightCompilerBundleProducts.alpha={unit}; return {unit}; }}"))
                })).unwrap();
                run(&task, &format!("rt => {{ const o=globalThis.__lightCompilerRoots; {preload} }}"));
                task
            }).collect::<Vec<_>>();
            tasks[0].install_catalog(0, &registrations).unwrap();
            for registration in &registrations {
                match registration {
                    Registration::Root(name, unit, test, once, location) => tasks[1]
                        .register_compiler_root(name, *unit, *once, location, *test)
                        .unwrap(),
                    Registration::Bundle(bundle) => tasks[1]
                        .register_compiler_bundle(&bundle.source(), bundle.unit)
                        .unwrap(),
                }
            }
            let observe = "rt => { const o=globalThis.__lightCompilerRoots, d=Object.getOwnPropertyDescriptor(o,'alpha'); return JSON.stringify([o.alpha,Reflect.ownKeys(o),d===undefined?null:[typeof d.get,d.value,d.enumerable,d.configurable]]); }";
            assert_eq!(
                run(&tasks[0], observe),
                run(&tasks[1], observe),
                "{preload}"
            );
        }
    }
}

#[test]
fn catalog_producers_share_data_without_evaluating_registration_sources() {
    use crate::compilation_session::Registration;
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parent = shared.evaluator().unwrap();
    let registrations = (0..128)
        .map(|unit| {
            Registration::Root(
                format!("helper{unit}"),
                Some(unit),
                None,
                false,
                "fixture:1".into(),
            )
        })
        .collect::<Vec<_>>();
    let roots = parent.value_roots().unwrap();
    parent.install_catalog(0, &registrations).unwrap();
    assert_eq!(parent.value_roots().unwrap(), roots);
    let evaluations = shared.helper_counts().units_evaluated;
    let child = parent
        .spawn_producer(&parent.cx().unwrap(), None, |_| {})
        .unwrap();
    assert_eq!(child.catalog_cursor(), Some(128));
    assert_eq!(child.registration_count(), 0);
    assert_eq!(shared.helper_counts().units_evaluated, evaluations);
    let parent_cx = parent.cx().unwrap();
    let child_cx = child.cx().unwrap();
    shared.0.runtime.scope(|s| {
        assert!(
            s.same_object(
                s.local(parent_cx.catalog_snapshot.borrow().as_ref().unwrap())
                    .unwrap(),
                s.local(child_cx.catalog_snapshot.borrow().as_ref().unwrap())
                    .unwrap(),
            )
        )
    });
    shared.0.runtime.collect().unwrap();
    let result = child
        .evaluate_unit("rt => Reflect.ownKeys(globalThis.__lightCompilerRoots).length")
        .unwrap();
    assert_eq!(
        child.cx().unwrap().values.borrow()[&result.index].view(),
        ValueView::Number(128.0)
    );
    child.release(result).unwrap();
}

#[test]
fn concurrent_catalog_task_memory() {
    use crate::compilation_session::Registration;
    for count in [600, 1200] {
        let shared = BbpRuntime::new(&EvaluatorOptions {
            memory_limit: 4 << 30,
            ..Default::default()
        })
        .unwrap();
        let rt = &shared.0.runtime;
        let report = |label: &str| {
            rt.collect().unwrap();
            eprintln!(
                "{count} {label}: {:?}; {:?}",
                rt.heap_stats(),
                rt.heap_usage()
            );
            rt.heap_stats().last_live_bytes
        };
        report("realm");
        // Reader tag functions are intentionally memoized on the shared
        // runtime by the first boot. Include that world-owned cache in the
        // no-task baseline while still checking exact task/catalog release.
        drop(shared.evaluator().unwrap());
        report("warm realm");
        let realm = rt.heap_usage().objects;
        let parent = shared.evaluator().unwrap();
        eprintln!("{count} boot task: {:?}", parent.cx().unwrap().task.stats());
        report("one boot");
        let boots = (0..7)
            .map(|_| shared.evaluator().unwrap())
            .collect::<Vec<_>>();
        report("eight boots");
        drop(boots);
        report("one boot again");
        let registrations = (0..count)
            .map(|unit| {
                Registration::Root(
                    format!("helper{unit}"),
                    Some(unit),
                    None,
                    false,
                    "fixture:1".into(),
                )
            })
            .collect::<Vec<_>>();
        parent.install_catalog(0, &registrations).unwrap();
        eprintln!(
            "{count} catalog task: {:?}",
            parent.cx().unwrap().task.stats()
        );
        report("one catalog");
        let one = rt.heap_usage().objects;
        let mut children = Vec::new();
        for _ in 0..7 {
            children.push(
                parent
                    .spawn_producer(&parent.cx().unwrap(), None, |_| {})
                    .unwrap(),
            );
        }
        report("eight catalogs");
        drop(children);
        report("drop children");
        assert_eq!(rt.heap_usage().objects, one);
        drop(parent);
        report("drop parent");
        assert_eq!(rt.heap_usage().objects, realm);
    }
}

#[test]
fn catalog_catchup_reuses_live_prefix_descriptors() {
    use crate::compilation_session::Registration;
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let registrations = (0..1004)
        .map(|unit| {
            Registration::Root(
                format!("helper{unit:04}"),
                None,
                None,
                false,
                "fixture:1".into(),
            )
        })
        .collect::<Vec<_>>();
    let lagging = shared.evaluator().unwrap();
    lagging.install_catalog(0, &registrations[..17]).unwrap();
    let prefix = shared.evaluator().unwrap();
    prefix.install_catalog(0, &registrations[..1000]).unwrap();
    // A later live frontier must not be used to satisfy an earlier one.
    let ahead = shared.evaluator().unwrap();
    ahead.install_catalog(0, &registrations).unwrap();
    let fresh = shared.evaluator().unwrap();
    fresh.install_catalog(0, &registrations[..1001]).unwrap();
    // Catch-up must prefer the greatest live prefix over this task's own
    // frontier, including the descriptor just added by the fresh task.
    lagging
        .install_catalog(17, &registrations[17..1003])
        .unwrap();
    shared.0.runtime.collect().unwrap();
    let snapshots = [&prefix, &fresh, &lagging, &ahead].map(|task| {
        task.cx()
            .unwrap()
            .catalog_snapshot
            .borrow()
            .clone()
            .unwrap()
    });
    drop((prefix, fresh, lagging, ahead));
    shared.0.runtime.collect().unwrap();
    let retained = shared.0.runtime.heap_stats().last_live_bytes;
    let duplicated = shared.0.runtime.scope(|s| {
        let snapshots = snapshots
            .each_ref()
            .map(|snapshot| s.local(snapshot).unwrap());
        for (snapshot, count) in snapshots.iter().zip([1000, 1001, 1003, 1004]) {
            assert_eq!(s.count(*snapshot).unwrap(), count);
        }
        for (earlier, later, count) in [(0, 1, 1000), (1, 2, 1001), (0, 3, 1000)] {
            for index in 0..count {
                if !s.same_object(
                    s.nth(snapshots[earlier], index).unwrap(),
                    s.nth(snapshots[later], index).unwrap(),
                ) {
                    return Some((earlier, later, index));
                }
            }
        }
        None
    });
    drop(snapshots);
    shared.0.runtime.collect().unwrap();
    println!(
        "retained catalog bytes at staggered 1000/1001/1003/1004 frontiers: {}",
        retained - shared.0.runtime.heap_stats().last_live_bytes
    );
    assert!(
        duplicated.is_none(),
        "duplicated descriptor: {duplicated:?}"
    );
}

#[test]
fn catalog_snapshots_live_only_as_long_as_the_tasks_using_them() {
    use crate::compilation_session::Registration;
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parent = shared.evaluator().unwrap();
    let registration = |unit| {
        Registration::Root(
            format!("helper{unit}"),
            Some(unit),
            None,
            false,
            "fixture:1".into(),
        )
    };
    parent.install_catalog(0, &[registration(0)]).unwrap();
    let first = parent
        .cx()
        .unwrap()
        .catalog_snapshot
        .borrow()
        .as_ref()
        .unwrap()
        .downgrade();
    let child = parent
        .spawn_producer(&parent.cx().unwrap(), None, |_| {})
        .unwrap();
    parent.install_catalog(1, &[registration(1)]).unwrap();
    shared.0.runtime.collect().unwrap();
    assert!(first.is_live(), "the lagging task owns its frontier");

    // Drop the only task at the newer frontier. After collection the child
    // must extend its own older snapshot rather than require a cached root.
    let second = parent
        .cx()
        .unwrap()
        .catalog_snapshot
        .borrow()
        .as_ref()
        .unwrap()
        .downgrade();
    drop(parent);
    shared.0.runtime.collect().unwrap();
    assert!(!second.is_live(), "unused frontiers must be collectable");
    child.install_catalog(1, &[registration(1)]).unwrap();
    shared.0.runtime.collect().unwrap();
    assert!(
        !first.is_live(),
        "advancing the last task releases its old frontier"
    );
    let value = child
        .evaluate_unit("rt => Reflect.ownKeys(globalThis.__lightCompilerRoots).length")
        .unwrap();
    assert_eq!(
        child.cx().unwrap().values.borrow()[&value.index].view(),
        ValueView::Number(2.0)
    );
    child.release(value).unwrap();
    let last = child
        .cx()
        .unwrap()
        .catalog_snapshot
        .borrow()
        .as_ref()
        .unwrap()
        .downgrade();
    drop(child);
    shared.0.runtime.collect().unwrap();
    assert!(
        !last.is_live(),
        "the registry must not own the final snapshot"
    );
}

#[test]
fn retained_catalog_bytes_grow_linearly_after_collecting_historical_frontiers() {
    use crate::compilation_session::Registration;
    let retained_bytes = |count| {
        let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
        let task = shared.evaluator().unwrap();
        for unit in 0..count {
            task.install_catalog(
                unit,
                &[Registration::Root(
                    format!("helper{unit:03}"),
                    None,
                    None,
                    false,
                    "fixture:1".into(),
                )],
            )
            .unwrap();
            if unit % 32 == 0 {
                shared.0.runtime.collect().unwrap();
            }
        }
        // Keep only catalog data, excluding the ordinary task-owned object,
        // initializer slots and VM frames from the retained-byte comparison.
        let snapshot = task
            .cx()
            .unwrap()
            .catalog_snapshot
            .borrow()
            .clone()
            .unwrap();
        drop(task);
        shared.0.runtime.collect().unwrap();
        let retained = shared.0.runtime.heap_stats().last_live_bytes;
        let last = snapshot.downgrade();
        drop(snapshot);
        shared.0.runtime.collect().unwrap();
        assert!(
            !last.is_live(),
            "the weak cache must release all catalog data after the last task"
        );
        retained - shared.0.runtime.heap_stats().last_live_bytes
    };
    let small = retained_bytes(200);
    let large = retained_bytes(400);
    println!("retained catalog bytes at 200/400 registrations: {small}/{large}");
    assert!(
        small > 0 && large > small && large <= small * 21 / 10,
        "retained catalog bytes at 200/400 registrations: {small}/{large}"
    );
}

#[test]
fn catalog_mutable_initializers_run_once_per_task_after_frontier_changes() {
    use crate::compilation_session::Registration;
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let parent = Rc::new(shared.evaluator().unwrap());
    parent.clone().install_compiler_service("__lightCompileUnit", Rc::new(|_, _| {
        Ok("rt => {globalThis.initialized=(globalThis.initialized??0)+1;return {count:0};}".into())
    })).unwrap();
    let registration = |name: &str, unit| {
        Registration::Root(name.into(), Some(unit), None, false, "fixture:1".into())
    };
    parent
        .install_catalog(0, &[registration("alpha", 1)])
        .unwrap();
    let child = parent
        .spawn_producer(
            &parent.cx().unwrap(),
            parent.service.borrow().clone(),
            |_| {},
        )
        .unwrap();
    for task in [&*parent, &child] {
        let value = task.evaluate_unit("rt => { globalThis.kept=globalThis.__lightCompilerRoots.alpha; kept.count=7; return kept===globalThis.__lightCompilerRoots.alpha; }").unwrap();
        assert_eq!(
            task.cx().unwrap().values.borrow()[&value.index].view(),
            ValueView::Bool(true)
        );
        task.release(value).unwrap();
        task.install_catalog(1, &[registration("beta", 2)]).unwrap();
        let value = task.evaluate_unit("rt => { if(initialized!==1 || kept!==globalThis.__lightCompilerRoots.alpha || kept.count!==7)throw 'initializer repeated'; }").unwrap();
        task.release(value).unwrap();
        assert_eq!(numeric_global(task, "initialized"), 1.0);
    }
    assert_eq!(shared.helper_counts().per_task, 2);
}

#[test]
fn invalid_compiler_literal_keys_cannot_enter_the_identity_cache() {
    let rt = Runtime::new();
    for key in [
        Datum::new(D::Integer(1)),
        Datum::new(D::Collection {
            kind: C::Vector,
            children: [].into(),
        }),
        Datum::new(D::Keyword(Name::new(Some("ns"), "key"))),
    ] {
        let data = Datum::new(D::CompilerLiteral {
            kind: L::JsObject,
            payload: Datum::new(D::Collection {
                kind: C::Map,
                children: vec![key, Datum::new(D::Integer(2))].into(),
            }),
        });
        let mut bridge = Bridge::default();
        rt.scope(|s| {
            for _ in 0..2 {
                let message = bridge
                    .import(s, &data, 64, &mut 100)
                    .unwrap_err()
                    .to_string();
                assert!(
                    message.contains("Invalid JS object literal key"),
                    "{message}"
                );
                assert!(!bridge.imports.contains_key(&data.identity()));
            }
        });
    }
    for payload in [
        Datum::new(D::Collection {
            kind: C::Map,
            children: vec![Datum::new(D::String("key".encode_utf16().collect()))].into(),
        }),
        Datum::new(D::Collection {
            kind: C::Vector,
            children: [].into(),
        }),
    ] {
        let data = Datum::new(D::CompilerLiteral {
            kind: L::JsObject,
            payload,
        });
        let mut bridge = Bridge::default();
        rt.scope(|s| {
            let message = bridge
                .import(s, &data, 64, &mut 100)
                .unwrap_err()
                .to_string();
            assert!(
                message.contains("invalid JS object literal payload"),
                "{message}"
            );
            assert!(!bridge.imports.contains_key(&data.identity()));
        });
    }
}

#[test]
fn compiler_literal_payloads_keep_identity_provenance_and_cached_limits_after_gc() {
    let rt = Runtime::new();
    let payload = Datum::new(D::Collection {
        kind: C::Vector,
        children: vec![
            Datum::new(D::String(vec![0xd800, 0, 0xdc00].into())),
            Datum::new(D::Integer(9_007_199_254_740_993)),
        ]
        .into(),
    });
    let metadata = Datum::new(D::Collection {
        kind: C::Map,
        children: vec![
            Datum::new(D::Keyword(Name::new(None, "tag"))),
            payload.clone(),
        ]
        .into(),
    });
    let data = Datum::new(D::CompilerLiteral {
        kind: L::JsArray,
        payload: payload.clone(),
    })
    .with_metadata(metadata.clone());
    let form = Form::from_data(
        data.clone(),
        crate::compiler_data::Source::new("literal.cljs", "#js [x]")
            .span(0..7)
            .unwrap(),
        crate::compiler_data::Origin::Source,
    );
    let mut bridge = Bridge::default();
    let root = rt.scope(|s| {
        bridge.remember(&form, 64, &mut 100).unwrap();
        let value = bridge.import(s, &data, 64, &mut 100).unwrap();
        s.persist(value)
    });
    rt.publish(&root).unwrap();
    rt.collect().unwrap();
    bridge.prune(rt.heap_stats().collections);
    rt.scope(|s| {
        let value = bridge.import(s, &data, 64, &mut 100).unwrap();
        assert!(s.same_object(value, s.local(&root).unwrap()));
        assert_eq!(
            bridge.export(s, value, 64, &mut 100).unwrap().identity(),
            data.identity()
        );
        let (_, imported_payload) = s.form_compiler_literal(s.form_value(value)).unwrap();
        let exported_payload = bridge.export(s, imported_payload, 64, &mut 100).unwrap();
        assert_eq!(exported_payload.identity(), payload.identity());
        assert_eq!(
            bridge.forms[&payload.identity()].span().source().name(),
            "literal.cljs"
        );
        let fresh = s
            .new_compiler_literal(JsString::from("js-array"), imported_payload)
            .unwrap();
        let fresh = s
            .form_with_meta(fresh, s.form_meta(s.form_value(value)))
            .unwrap();
        let exported = bridge.export(s, fresh, 64, &mut 100).unwrap();
        assert_eq!(
            exported.compiler_literal_data().unwrap().identity(),
            payload.identity()
        );
        assert_eq!(exported.metadata().unwrap().identity(), metadata.identity());
        for cached in [true, false] {
            if !cached {
                bridge.imports.clear();
            }
            assert!(bridge.import(s, &data, 64, &mut 3).is_err());
            assert!(bridge.import(s, &data, 2, &mut 100).is_err());
        }
    });
    drop(root);
    rt.collect().unwrap();
    bridge.prune(rt.heap_stats().collections);
    assert!(bridge.imports.is_empty());
    assert!(bridge.forms.is_empty());
    assert!(bridge.costs.is_empty());
}

#[test]
fn published_reader_js_tags_keep_builtin_identity_across_tasks_and_gc() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let owner = shared.evaluator().unwrap();
    let id = owner
        .evaluate_unit("rt => rt.core.readerInitialTagTable()")
        .unwrap();
    let tags = owner.cx().unwrap().values.borrow()[&id.index].clone();
    shared.0.runtime.publish(&tags).unwrap();
    let peer = shared.evaluator().unwrap();
    let argument = peer.id().unwrap();
    peer.request(1, argument, tags).unwrap();
    drop(owner);
    shared.0.runtime.collect().unwrap();
    let f = peer.evaluate_unit("rt => tags => rt.core.readerReadString(__rt.formMap(__rt.formKeyword(null,'readers'),tags),'#js [1]')").unwrap();
    let result = peer.call(f, &[argument]).unwrap();
    let value = peer.cx().unwrap().values.borrow()[&result.index].clone();
    shared.0.runtime.scope(|s| {
        let (tag, payload) = s.form_compiler_literal(s.local(&value).unwrap()).unwrap();
        assert_eq!(tag, "js-array");
        assert_eq!(
            s.view(s.form_nth(payload, 0).unwrap()),
            ValueView::Number(1.0)
        );
    });
}

#[test]
fn published_flatten_worklists_survive_owner_drop_gc_and_deep_trees() {
    let options = EvaluatorOptions {
        poll_budget: 100_000_000,
        timeout: std::time::Duration::from_secs(60),
        ..Default::default()
    };
    let shared = BbpRuntime::new(&options).unwrap();
    let owner = shared.evaluator().unwrap();
    let id = owner
        .evaluate_unit(
            "rt => {
        let tree = 7;
        for (let i = 0; i < 3000; i++) tree = __rt.formVector(tree);
        const queue = __rt.formCompilerLiteral('queue', __rt.formVector(1, tree, 9));
        return rt.core.flatten(__rt.lazySeq(() => queue));
    }",
        )
        .unwrap();
    let worklist = owner.cx().unwrap().values.borrow()[&id.index].clone();
    shared.0.runtime.publish(&worklist).unwrap();
    let peers = [shared.evaluator().unwrap(), shared.evaluator().unwrap()];
    let arguments = peers.each_ref().map(|peer| {
        let argument = peer.id().unwrap();
        peer.request(1, argument, worklist.clone()).unwrap();
        argument
    });
    drop(owner);
    shared.0.runtime.collect().unwrap();
    for (peer, argument) in peers.iter().zip(arguments) {
        let f = peer
            .evaluate_unit("rt => xs => __rt.cljNth(xs, 1)")
            .unwrap();
        let result = peer.call(f, &[argument]).unwrap();
        assert!(matches!(
            peer.cx().unwrap().values.borrow()[&result.index].view(),
            ValueView::Number(7.0)
        ));
        shared.0.runtime.collect().unwrap();
        let f = peer.evaluate_unit("rt => xs => __rt.cljCount(xs)").unwrap();
        let result = peer.call(f, &[argument]).unwrap();
        assert!(matches!(
            peer.cx().unwrap().values.borrow()[&result.index].view(),
            ValueView::Number(3.0)
        ));
    }
    assert!(shared.0.runtime.heap_stats().peak_managed_bytes <= options.memory_limit);
}

#[test]
fn flatten_of_infinite_empty_branches_obeys_the_aggregate_poll_budget() {
    let shared = BbpRuntime::new(&EvaluatorOptions {
        poll_budget: 100_000,
        timeout: std::time::Duration::from_secs(60),
        ..Default::default()
    })
    .unwrap();
    let evaluator = shared.evaluator().unwrap();
    let function = evaluator
        .evaluate_unit(
            "rt => () => {
        __rt.effect(65534, 0);
        const repeat = self => __rt.lazySeq(() => __rt.cljCons(__rt.formVector(), self(self)));
        return __rt.cljFirst(rt.core.flatten(repeat(repeat)));
    }",
        )
        .unwrap();
    // Leave enough VM slices to enter the walk, while making the poll budget
    // win deterministically over the independent heap and wall-clock limits.
    while shared.0.control.remaining_polls() > 16 {
        shared.0.control.poll().unwrap();
    }
    let error = evaluator.call(function, &[]).unwrap_err();
    assert!(error.0.contains("poll budget"), "{error}");
    assert_eq!(shared.0.counters.cleanups.load(Ordering::Relaxed), 1);
    assert_eq!(shared.0.control.remaining_polls(), 0);
    assert!(
        shared.evaluator().is_err(),
        "a peer shares the spent budget"
    );
}

#[test]
fn flatten_retains_leaf_occurrences_and_metadata_across_collection() {
    use crate::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env x] (first (flatten x)))";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&crate::emitter::emit_js_unit(&program))
        .unwrap();
    let call = host
        .read_compiler_form(&Source::new("flatten.cljs", "(same [[^{:m 1} leaf]])"))
        .unwrap()
        .unwrap();
    let original = call.children()[1].children()[0].children()[0].clone();
    let environment = Datum::new(DatumKind::Collection {
        kind: crate::CollectionKind::Map,
        children: Box::new([]),
    });
    for i in 0..2 {
        let result = host
            .expand_javascript(
                &evaluator,
                function,
                &call,
                &environment,
                ExpansionOrigin {
                    macro_name: Name::new(None, "same"),
                    call: call.span(),
                    definition: None,
                    helpers: vec![],
                },
            )
            .unwrap();
        assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
        assert_eq!(result.span().source().name(), "flatten.cljs");
        assert_eq!(result.span().bytes(), original.span().bytes());
        assert!(std::ptr::eq(
            result.metadata().unwrap().data().kind(),
            original.metadata().unwrap().data().kind(),
        ));
        assert_eq!(host.native_macro_calls()[i].exported, 0);
        assert_eq!(host.native_macro_calls()[i].reused, 1);
        evaluator.collect_garbage();
    }
    evaluator.release(function).unwrap();
}

#[test]
fn stale_lazy_wakeups_do_not_resume_a_task_parked_on_a_new_dependency() {
    let shared = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let evaluator = shared.evaluator().unwrap();
    let cx = evaluator.cx().unwrap();
    // The evaluator's command wait stands in for a newer dependency reached
    // after the owner polled and consumed an earlier lazy settlement itself.
    assert_eq!(cx.task.status(), big_boy_pants_js::TaskStatus::Waiting);
    let submitted = Arc::new(AtomicUsize::new(0));
    let count = submitted.clone();
    let (reply, _outcome) = std::sync::mpsc::sync_channel(1);
    shared.0.parked.lock().unwrap().insert(
        cx.task.id(),
        Parked {
            step: evaluator.step(&cx),
            reply,
            submit: Arc::new(move |_, _| {
                count.fetch_add(1, Ordering::Relaxed);
            }),
        },
    );
    shared.wake_tasks([cx.task.clone()]);
    assert_eq!(submitted.load(Ordering::Relaxed), 0);
    assert!(
        shared
            .0
            .parked
            .lock()
            .unwrap()
            .remove(&cx.task.id())
            .is_some()
    );
}
