//! Native resource controls, adapted from pinned SCI interrupt_fn_test.cljc.
//! See tests/reference/sci/README.md for exact assertion anchors and differences.
use light_cljs::interpreter::*;
use std::cell::RefCell;

thread_local! {
    static EFFECTS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    static TOKEN: RefCell<Option<CancellationToken>> = const { RefCell::new(None) };
}
fn host(name: &'static str, arity: Arity, callback: NativeCallback) -> NativeDefinition {
    NativeDefinition {
        metadata: NativeMetadata {
            namespace: "user",
            name,
            arity,
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback,
    }
}
fn install(c: &mut Context, name: &'static str, arity: Arity, callback: NativeCallback) {
    c.register_native(Box::leak(Box::new(host(name, arity, callback))));
}
fn setup() -> Context {
    EFFECTS.with(|e| e.borrow_mut().clear());
    let mut c = Context::new();
    install(&mut c, "mark", Arity::Exact(1), |c, args| {
        let ValueView::Integer(n) = args[0].inspect() else {
            panic!("integer effect")
        };
        EFFECTS.with(|e| e.borrow_mut().push(n));
        Ok(c.nil())
    });
    c
}
fn analyzed(c: &mut Context, text: &str) -> AnalyzedForm {
    let source = c.source("limits.clj", text);
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    c.analyze_form(&form).unwrap()
}
fn assert_resource(error: &EvalError, expected: ResourceError) {
    assert_eq!(error.class(), ErrorClass::Control, "{error}");
    let actual = match &error.kind {
        EvalErrorKind::Resource(e) | EvalErrorKind::Collection(CollectionError::Resource(e)) => *e,
        EvalErrorKind::CallDepthExceeded => ResourceError::CallDepthExceeded,
        EvalErrorKind::EvaluationDepthExceeded => ResourceError::EvaluationDepthExceeded,
        EvalErrorKind::Native {
            error: NativeError::Collection(CollectionError::Resource(e)),
            ..
        } => *e,
        other => panic!("unexpected terminal kind: {other:?}"),
    };
    assert_eq!(actual, expected, "{error}");
}
fn limit(resource: ResourceLimit) -> ResourceError {
    ResourceError::LimitExceeded(resource)
}

#[test]
fn transduce_cancellation_is_terminal_at_every_callback_boundary() {
    for stage in 0..=4 {
        let mut c = setup();
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        install(&mut c, "boundary", Arity::Exact(1), |c, args| {
            let ValueView::Integer(n) = args[0].inspect() else {
                panic!("stage")
            };
            EFFECTS.with(|e| e.borrow_mut().push(n));
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
            // Swallowing a nested failure and collecting cannot erase Control.
            assert!(c.eval_string("(+ 1 2)").is_err());
            c.collect_garbage();
            Ok(c.nil())
        });
        let event = |n| {
            if n == stage {
                format!("(boundary {n})")
            } else {
                format!("(mark {n})")
            }
        };
        let program = format!(
            "(let [rf (fn ([] {} 0) ([a] {} a) ([a x] {} (+ a x))) xf (fn [rf] {} rf)] (try (transduce xf rf (lazy-seq {} (list 1 2))) (catch Throwable e (mark 99)) (finally (mark 9))))",
            event(0),
            event(4),
            event(3),
            event(1),
            event(2)
        );
        let error = c.eval_string(&program).unwrap_err();
        assert_resource(&error, ResourceError::Cancelled);
        let mut expected = if stage == 4 {
            vec![0, 1, 2, 3, 3, 4]
        } else {
            (0..=stage).collect()
        };
        expected.push(9);
        EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
    }
}

#[test]
fn into_cancellation_is_terminal_at_transform_source_step_and_completion() {
    for stage in 1..=4 {
        let mut c = setup();
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        install(&mut c, "boundary", Arity::Exact(1), |c, args| {
            let ValueView::Integer(n) = args[0].inspect() else {
                panic!("stage")
            };
            EFFECTS.with(|e| e.borrow_mut().push(n));
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
            // Swallowing a nested failure and collecting cannot erase Control.
            assert!(c.eval_string("(+ 1 2)").is_err());
            c.collect_garbage();
            Ok(c.nil())
        });
        let event = |n| {
            if n == stage {
                format!("(boundary {n})")
            } else {
                format!("(mark {n})")
            }
        };
        let program = format!(
            "(let [xf (fn [rf] {} (fn ([a] {} (rf a)) ([a x] {} (rf a x))))] (try (into [] xf (lazy-seq {} (list 1 2))) (catch Throwable e (mark 99)) (finally (mark 9))))",
            event(1),
            event(4),
            event(3),
            event(2)
        );
        let error = c.eval_string(&program).unwrap_err();
        assert_resource(&error, ResourceError::Cancelled);
        let mut expected = if stage == 4 {
            vec![1, 2, 3, 3, 4]
        } else {
            (1..=stage).collect()
        };
        expected.push(9);
        EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
    }
}

#[test]
fn native_transform_callbacks_preserve_sticky_cancellation() {
    for stage in 0..=6 {
        let mut c = setup();
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        install(&mut c, "boundary", Arity::Exact(1), |c, args| {
            let ValueView::Integer(n) = args[0].inspect() else {
                panic!("stage")
            };
            EFFECTS.with(|e| e.borrow_mut().push(n));
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
            assert!(c.eval_string("(+ 1 2)").is_err());
            c.collect_garbage();
            Ok(c.nil())
        });
        let event = |n| {
            if n == stage {
                format!("(boundary {n})")
            } else {
                format!("(mark {n})")
            }
        };
        let program = format!(
            "(let [rf (fn ([] {} []) ([a] {} a) ([a x] {} (conj a x))) xf (comp (map (fn [x] {} (inc x))) (filter (fn [x] {} true)) (remove (fn [x] {} false)))] (try (transduce xf rf (lazy-seq {} (list 1))) (catch Throwable e (mark 99)) (finally (mark 9))))",
            event(0),
            event(6),
            event(5),
            event(2),
            event(3),
            event(4),
            event(1)
        );
        assert_resource(
            &c.eval_string(&program).unwrap_err(),
            ResourceError::Cancelled,
        );
        let mut expected: Vec<_> = (0..=stage).collect();
        expected.push(9);
        EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
    }
}

#[test]
fn native_no_output_filters_and_applied_reducers_obey_limits() {
    for program in [
        "(into [] (filter (constantly false)) (range))",
        "(into [] (remove (constantly true)) (range))",
        "(apply ((filter odd?) conj) (range))",
        "(apply ((map +) conj) [] (range))",
        "(into [] cat (repeat []))",
        "(into [] cat [(range)])",
        "(into [] (mapcat (constantly nil)) (range))",
        "(apply (cat conj) (range))",
    ] {
        for (limits, expected) in [
            (
                ResourceLimits {
                    fuel: Some(3000),
                    ..Default::default()
                },
                limit(ResourceLimit::Fuel),
            ),
            (
                ResourceLimits {
                    allocation: Some(2000),
                    ..Default::default()
                },
                limit(ResourceLimit::Allocation),
            ),
        ] {
            let mut c = Context::new();
            let code = analyzed(&mut c, program);
            c.set_resource_limits(limits);
            assert_resource(&c.eval_analyzed(&code).unwrap_err(), expected);
        }
    }
    let mut c = Context::new();
    let rf = c
        .eval_string("((comp (map inc) (filter odd?) (remove neg?)) conj)")
        .unwrap();
    let accumulator = c.vector(&[]).unwrap();
    c.set_resource_limits(ResourceLimits {
        call_depth: 2,
        ..Default::default()
    });
    let error = c.invoke(&rf, &[accumulator, c.integer(0)]).unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert!(error.to_string().contains("CallDepthExceeded"), "{error}");
}

#[test]
fn cat_cancellation_at_both_nesting_levels_and_callbacks_skips_completion() {
    for stage in 0..=5 {
        let mut c = setup();
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        install(&mut c, "boundary", Arity::Exact(1), |c, args| {
            let ValueView::Integer(n) = args[0].inspect() else {
                panic!("stage")
            };
            EFFECTS.with(|e| e.borrow_mut().push(n));
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
            assert!(c.eval_string("(+ 1 2)").is_err());
            c.collect_garbage();
            Ok(c.nil())
        });
        let event = |n| format!("({} {n})", if n == stage { "boundary" } else { "mark" });
        let program = format!(
            "(let [rf (fn ([] {} []) ([a] {} a) ([a x] {} (conj a x))) xf (comp (mapcat (fn [x] {} x)) cat)] (try (transduce xf rf (lazy-seq {} (list (list (lazy-seq {} (list 1)))))) (catch Throwable e (mark 99)) (finally (mark 9))))",
            event(0),
            event(5),
            event(4),
            event(2),
            event(1),
            event(3)
        );
        assert_resource(
            &c.eval_string(&program).unwrap_err(),
            ResourceError::Cancelled,
        );
        let mut expected: Vec<_> = (0..=stage).collect();
        expected.push(9);
        EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
    }
    // Empty inner collections must still poll the outer traversal.
    let mut c = setup();
    let code = analyzed(
        &mut c,
        "(try (transduce cat (fn ([a] (mark 8) a) ([a x] (mark 7) (conj a x))) [] (repeat [])) (catch Throwable e (mark 99)) (finally (mark 9)))",
    );
    c.set_resource_limits(ResourceLimits {
        fuel: Some(3000),
        ..Default::default()
    });
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        limit(ResourceLimit::Fuel),
    );
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
}

#[test]
fn cat_wrappers_markers_and_nesting_charge_allocation_and_call_depth() {
    for allocation in 0..5 {
        let mut c = Context::new();
        let cat = c.eval_string("cat").unwrap();
        let rf = c.eval_string("conj").unwrap();
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..Default::default()
        });
        let error = c.invoke(&cat, &[rf]).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control);
        assert!(error.to_string().contains("Allocation"), "{error}");
    }
    let mut c = Context::new();
    let rf = c.eval_string("((comp cat cat) conj)").unwrap();
    let source = c.eval_string("[[1]]").unwrap();
    c.set_resource_limits(ResourceLimits {
        call_depth: 3,
        ..Default::default()
    });
    let error = c
        .invoke(&rf, &[c.vector(&[]).unwrap(), source])
        .unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert!(error.to_string().contains("CallDepthExceeded"), "{error}");
}

#[test]
fn unbounded_counted_processes_and_empty_output_share_resource_limits() {
    for transform in ["(drop ##Inf)", "(take-while (constantly true))"] {
        let program = format!(
            "(letfn [(src [i] (lazy-seq (cons i (src (inc i)))))] (into [] {transform} (src 0)))"
        );
        for (limits, expected) in [
            (
                ResourceLimits {
                    fuel: Some(3000),
                    ..Default::default()
                },
                limit(ResourceLimit::Fuel),
            ),
            (
                ResourceLimits {
                    allocation: Some(2000),
                    ..Default::default()
                },
                limit(ResourceLimit::Allocation),
            ),
            (
                ResourceLimits {
                    call_depth: 2,
                    ..Default::default()
                },
                ResourceError::CallDepthExceeded,
            ),
        ] {
            let mut c = Context::new();
            let code = analyzed(&mut c, &program);
            c.set_resource_limits(limits);
            assert_resource(&c.eval_analyzed(&code).unwrap_err(), expected);
        }
    }
    for transform in ["(take 2)", "(drop 2)", "(take-while odd?)"] {
        for allocation in [0, 1] {
            let mut c = Context::new();
            let xf = c.eval_string(transform).unwrap();
            let rf = c.eval_string("conj").unwrap();
            c.set_resource_limits(ResourceLimits {
                allocation: Some(allocation),
                ..Default::default()
            });
            let error = c.invoke(&xf, &[rf]).unwrap_err();
            assert_eq!(error.class(), ErrorClass::Control);
            assert!(error.to_string().contains("Allocation"), "{error}");
        }
    }
}

#[test]
fn counted_process_cancellation_stops_without_completion_or_output() {
    for transform in ["(drop ##Inf)", "(take-while (fn [x] (boundary x) false))"] {
        let mut c = setup();
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        install(&mut c, "boundary", Arity::Exact(1), |c, args| {
            let ValueView::Integer(n) = args[0].inspect() else {
                panic!("source item")
            };
            EFFECTS.with(|e| e.borrow_mut().push(n));
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
            assert!(c.eval_string("(+ 1 2)").is_err());
            c.collect_garbage();
            Ok(c.nil())
        });
        let source_effect = if transform == "(drop ##Inf)" {
            "(if (= i 2) (boundary i) (mark i))"
        } else {
            "nil"
        };
        let program = format!(
            "(letfn [(src [i] (lazy-seq {source_effect} (cons i (src (inc i)))))] (try (transduce {transform} (fn ([a] (mark 8) a) ([a x] (mark 7) (conj a x))) [] (src 0)) (catch Throwable e (mark 99)) (finally (mark 9))))"
        );
        assert_resource(
            &c.eval_string(&program).unwrap_err(),
            ResourceError::Cancelled,
        );
        let expected = if transform == "(drop ##Inf)" {
            vec![0, 1, 2, 9]
        } else {
            vec![0, 9]
        };
        EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
    }
}

#[test]
fn partition_buffer_growth_is_accounted_without_reserving_count_slots() {
    for count in ["0", "2147483647"] {
        for resource in [ResourceLimit::Fuel, ResourceLimit::Allocation] {
            let mut c = setup();
            let code = analyzed(
                &mut c,
                &format!(
                    "(try (transduce (partition-all {count}) (fn ([a] (mark 8) a) ([a x] (mark 7) a)) [] (range)) (catch Throwable e (mark 99)) (finally (mark 9)))"
                ),
            );
            c.set_resource_limits(ResourceLimits {
                fuel: (resource == ResourceLimit::Fuel).then_some(3000),
                allocation: (resource == ResourceLimit::Allocation).then_some(3000),
                ..Default::default()
            });
            assert_resource(&c.eval_analyzed(&code).unwrap_err(), limit(resource));
            EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
        }
    }
    let mut c = Context::new();
    let xf = c.eval_string("(partition-all 2147483647)").unwrap();
    let rf = c.eval_string("conj").unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(32),
        ..Default::default()
    });
    let reducer = c.invoke(&xf, &[rf]).unwrap();
    let a = c
        .invoke(&reducer, &[c.vector(&[]).unwrap(), c.integer(1)])
        .unwrap();
    assert_eq!(a.count(), Some(0));
    assert!(c.resource_usage().allocation < 32);
}

#[test]
fn partition_cancellation_at_full_flush_and_completion_preserves_finally() {
    for (source, stage) in [("[1 2]", 3), ("[1]", 3), ("[1]", 4)] {
        let mut c = setup();
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        install(&mut c, "boundary", Arity::Exact(1), |c, args| {
            let ValueView::Integer(n) = args[0].inspect() else {
                panic!("stage")
            };
            EFFECTS.with(|e| e.borrow_mut().push(n));
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
            assert!(c.eval_string("(+ 1 2)").is_err());
            c.collect_garbage();
            Ok(c.nil())
        });
        let event = |n| format!("({} {n})", if n == stage { "boundary" } else { "mark" });
        let program = format!(
            "(try (transduce (partition-all 2) (fn ([a] {} a) ([a x] {} (conj a x))) [] {source}) (catch Throwable e (mark 99)) (finally (mark 9)))",
            event(4),
            event(3)
        );
        assert_resource(
            &c.eval_string(&program).unwrap_err(),
            ResourceError::Cancelled,
        );
        let expected = if stage == 3 {
            vec![3, 9]
        } else {
            vec![3, 4, 9]
        };
        EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
    }
}

#[test]
fn partition_emission_and_completion_obey_output_and_call_depth_limits() {
    for source in ["[1 2]", "[1]"] {
        for completing in [false, true] {
            let mut c = setup();
            let out = c.output_stream(OutputBuffer::default());
            let _guard = c.bind_io(None, Some(&out), None).unwrap();
            let effect = if completing { "(print :done)" } else { "nil" };
            let step = if completing { "nil" } else { "(print x)" };
            let code = analyzed(
                &mut c,
                &format!(
                    "(try (transduce (partition-all 2) (fn ([a] (mark 4) {effect} a) ([a x] (mark 3) {step} (conj a x))) [] {source}) (catch Throwable e (mark 99)) (finally (mark 9)))"
                ),
            );
            c.set_resource_limits(ResourceLimits {
                output: Some(0),
                ..Default::default()
            });
            assert_resource(
                &c.eval_analyzed(&code).unwrap_err(),
                limit(ResourceLimit::Output),
            );
            let expected = if completing {
                vec![3, 4, 9]
            } else {
                vec![3, 9]
            };
            EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
        }
    }
    for (source, completing) in [("[1 2]", false), ("[1]", false), ("[1]", true)] {
        let mut c = setup();
        let completion = if completing { "(depth)" } else { "a" };
        let step = if completing { "(conj a x)" } else { "(depth)" };
        let code = analyzed(
            &mut c,
            &format!(
                "(letfn [(depth [] (depth))] (try (transduce (partition-all 2) (fn ([a] (mark 4) {completion}) ([a x] (mark 3) {step})) [] {source}) (catch Throwable e (mark 99)) (finally (mark 9))))"
            ),
        );
        c.set_resource_limits(ResourceLimits {
            call_depth: 32,
            ..Default::default()
        });
        assert_resource(
            &c.eval_analyzed(&code).unwrap_err(),
            ResourceError::CallDepthExceeded,
        );
        let expected = if completing {
            vec![3, 4, 9]
        } else {
            vec![3, 9]
        };
        EFFECTS.with(|e| assert_eq!(*e.borrow(), expected));
    }
    for completing in [false, true] {
        let mut c = Context::new();
        let r = c
            .eval_string("((partition-all 2) (completing conj))")
            .unwrap();
        let a = c
            .invoke(&r, &[c.vector(&[]).unwrap(), c.integer(1)])
            .unwrap();
        c.set_resource_limits(ResourceLimits {
            call_depth: 2,
            ..Default::default()
        });
        let args = if completing {
            vec![a]
        } else {
            vec![a, c.integer(2)]
        };
        let error = c.invoke(&r, &args).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control);
        assert!(error.to_string().contains("CallDepthExceeded"), "{error}");
    }
}

#[test]
fn transduce_direct_and_evaluated_calls_share_all_resource_limits() {
    let profiles = [
        (
            ResourceLimits {
                fuel: Some(30),
                ..Default::default()
            },
            limit(ResourceLimit::Fuel),
        ),
        (
            ResourceLimits {
                allocation: Some(15),
                ..Default::default()
            },
            limit(ResourceLimit::Allocation),
        ),
        (
            ResourceLimits {
                output: Some(0),
                ..Default::default()
            },
            limit(ResourceLimit::Output),
        ),
        (
            ResourceLimits {
                call_depth: 2,
                ..Default::default()
            },
            ResourceError::CallDepthExceeded,
        ),
    ];
    for (limits, expected) in profiles {
        for direct in [false, true] {
            let mut c = Context::new();
            let out = c.output_stream(OutputBuffer::default());
            let _guard = c.bind_io(None, Some(&out), None).unwrap();
            let code = analyzed(
                &mut c,
                "(transduce identity (completing (fn [a x] (print x) (+ a x))) 0 (range 100))",
            );
            let transduce = c.eval_string("transduce").unwrap();
            let xf = c.eval_string("identity").unwrap();
            let rf = c
                .eval_string("(completing (fn [a x] (print x) (+ a x)))")
                .unwrap();
            let source = c.eval_string("(range 100)").unwrap();
            c.set_resource_limits(limits);
            if direct {
                let error = c
                    .invoke(&transduce, &[xf, rf, c.integer(0), source])
                    .unwrap_err();
                assert_eq!(error.class(), ErrorClass::Control);
                match error {
                    InvokeError::Execution(error) => assert_resource(&error, expected),
                    InvokeError::Dispatch(kind) => match kind {
                        EvalErrorKind::CallDepthExceeded => {
                            assert_eq!(expected, ResourceError::CallDepthExceeded)
                        }
                        kind => assert_eq!(kind, EvalErrorKind::Resource(expected)),
                    },
                }
            } else {
                assert_resource(&c.eval_analyzed(&code).unwrap_err(), expected);
            }
        }
    }
    // Fixed-arity applied adapters count an invalid argument list completely;
    // even this unbounded argument traversal remains under the same budget.
    let mut c = Context::new();
    c.set_resource_limits(ResourceLimits {
        fuel: Some(1000),
        ..Default::default()
    });
    assert_resource(
        &c.eval_string("(apply (completing +) (range))").unwrap_err(),
        limit(ResourceLimit::Fuel),
    );
}

#[test]
fn into_direct_and_evaluated_calls_share_all_resource_limits() {
    let profiles = [
        (
            ResourceLimits {
                fuel: Some(30),
                ..Default::default()
            },
            limit(ResourceLimit::Fuel),
        ),
        (
            ResourceLimits {
                allocation: Some(15),
                ..Default::default()
            },
            limit(ResourceLimit::Allocation),
        ),
        (
            ResourceLimits {
                output: Some(0),
                ..Default::default()
            },
            limit(ResourceLimit::Output),
        ),
        (
            ResourceLimits {
                call_depth: 2,
                ..Default::default()
            },
            ResourceError::CallDepthExceeded,
        ),
    ];
    for (limits, expected) in profiles {
        for direct in [false, true] {
            let mut c = Context::new();
            let out = c.output_stream(OutputBuffer::default());
            let _guard = c.bind_io(None, Some(&out), None).unwrap();
            let code = analyzed(
                &mut c,
                "(into [] (fn [rf] (fn ([a] (rf a)) ([a x] (print x) (rf a x)))) (range 100))",
            );
            let into = c.eval_string("into").unwrap();
            let xf = c
                .eval_string("(fn [rf] (fn ([a] (rf a)) ([a x] (print x) (rf a x))))")
                .unwrap();
            let target = c.vector(&[]).unwrap();
            let source = c.eval_string("(range 100)").unwrap();
            c.set_resource_limits(limits);
            if direct {
                let error = c.invoke(&into, &[target, xf, source]).unwrap_err();
                assert_eq!(error.class(), ErrorClass::Control);
                match error {
                    InvokeError::Execution(error) => assert_resource(&error, expected),
                    InvokeError::Dispatch(kind) => match kind {
                        EvalErrorKind::CallDepthExceeded => {
                            assert_eq!(expected, ResourceError::CallDepthExceeded)
                        }
                        kind => assert_eq!(kind, EvalErrorKind::Resource(expected)),
                    },
                }
            } else {
                assert_resource(&c.eval_analyzed(&code).unwrap_err(), expected);
            }
        }
    }
}

#[test]
fn ensure_reduced_respects_allocation_limits_on_both_paths() {
    let mut c = Context::new();
    let ensure = c.eval_string("ensure-reduced").unwrap();
    let marker = c.reduced(&c.nil()).unwrap();
    c.set_resource_limits(ResourceLimits {
        // One argument scratch slot and one root for marker inspection.
        allocation: Some(2),
        ..ResourceLimits::default()
    });
    assert!(
        c.invoke(&ensure, std::slice::from_ref(&marker))
            .unwrap()
            .same_identity(&marker)
    );
    assert_eq!(c.resource_usage().allocation, 2);
    match c.invoke(&ensure, &[c.nil()]).unwrap_err() {
        InvokeError::Execution(error) => assert_resource(&error, limit(ResourceLimit::Allocation)),
        InvokeError::Dispatch(kind) => assert_eq!(
            kind,
            EvalErrorKind::Resource(limit(ResourceLimit::Allocation))
        ),
    }
    c.set_resource_limits(ResourceLimits {
        // Wrapping instead charges the scratch slot, marker and result root.
        allocation: Some(3),
        ..ResourceLimits::default()
    });
    let wrapped = c.invoke(&ensure, &[c.nil()]).unwrap();
    assert_eq!(wrapped.reduced_value().unwrap().inspect(), ValueView::Nil);
    assert_eq!(c.resource_usage().allocation, 3);
}

#[test]
fn infinite_recur_and_ordinary_recursion_stop_at_source_with_configured_limits() {
    let mut c = setup();
    let code = analyzed(
        &mut c,
        "(try (loop* [] (recur)) (catch Throwable e (mark 99)) (finally (mark 1)))",
    );
    c.set_resource_limits(ResourceLimits {
        fuel: Some(80),
        ..ResourceLimits::default()
    });
    let error = c.eval_analyzed(&code).unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Fuel));
    assert_eq!(error.span.source().name(), "limits.clj");
    assert!(matches!(
        error.span.text(),
        "(recur)" | "(loop* [] (recur))"
    ));
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [1]));
    assert!(c.resource_usage().fuel <= 80);
    c.set_resource_limits(ResourceLimits {
        call_depth: 5,
        ..ResourceLimits::default()
    });
    let error = c
        .eval_string("((fn* recursive [] (recursive)))")
        .unwrap_err();
    assert_resource(&error, ResourceError::CallDepthExceeded);
    assert_eq!(error.span.text(), "(recursive)");
    assert_eq!(
        c.eval_string("(+ 20 22)").unwrap().inspect(),
        ValueView::Integer(42)
    );
}

#[test]
fn read_analysis_expression_and_future_expansion_work_share_the_api() {
    let mut c = Context::new();
    let code = analyzed(&mut c, "(if true (+ 1 2) (+ 3 4))");
    c.set_resource_limits(ResourceLimits {
        fuel: Some(1),
        ..ResourceLimits::default()
    });
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        limit(ResourceLimit::Fuel),
    );
    c.set_resource_limits(ResourceLimits {
        fuel: Some(2),
        ..ResourceLimits::default()
    });
    let source = c.source("analysis.clj", "(fn* [x] (+ x x))");
    c.set_resource_limits(ResourceLimits::default());
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    c.set_resource_limits(ResourceLimits {
        fuel: Some(2),
        ..ResourceLimits::default()
    });
    let error = c.analyze_form(&form).unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Fuel));
    assert_eq!(error.phase, EvalPhase::Analysis);
    c.set_resource_limits(ResourceLimits {
        allocation: Some(8),
        ..ResourceLimits::default()
    });
    let error = c.eval_string("[1 2 3 4 5 6 7 8 9]").unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Allocation));
    assert_eq!(error.phase, EvalPhase::Read);

    install(&mut c, "expansion-work", Arity::Exact(0), |c, _| {
        loop {
            c.checkpoint(PollPoint::Macroexpansion)?;
        }
    });
    c.set_resource_limits(ResourceLimits {
        fuel: Some(200),
        ..ResourceLimits::default()
    });
    let error = c.eval_string("(expansion-work)").unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Fuel));
    assert_eq!(error.span.text(), "(expansion-work)");
}

#[test]
fn cancellation_is_sticky_across_callback_success_and_bounded_finalizers() {
    let mut c = setup();
    let token = CancellationToken::default();
    TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
    c.set_cancellation_token(Some(token));
    install(&mut c, "cancel", Arity::Exact(0), |c, _| {
        TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
        // Replacing host configuration cannot erase the admitted scope's token.
        c.set_cancellation_token(None);
        Ok(c.integer(42))
    });
    let error = c.eval_string("(try (try (cancel) (finally (mark 1) (throw (ex-info \"replacement\" {})))) (catch Throwable e (mark 99)) (finally (mark 2)))").unwrap_err();
    assert_resource(&error, ResourceError::Cancelled);
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [1, 2]));
    assert_eq!(
        c.eval_string("42").unwrap().inspect(),
        ValueView::Integer(42)
    );

    let token = CancellationToken::default();
    token.cancel();
    c.set_cancellation_token(Some(token));
    let error = c.eval_string("(mark 99)").unwrap_err();
    assert_resource(&error, ResourceError::Cancelled);
    assert_eq!(error.phase, EvalPhase::Read);
}

#[test]
fn nested_eval_cannot_reset_fuel_or_turn_terminal_failure_into_success() {
    let mut c = setup();
    install(&mut c, "reenter", Arity::Exact(0), |c, _| {
        for _ in 0..1000 {
            if c.eval_string("(inc 1)").is_err() {
                break;
            }
        }
        c.set_resource_limits(ResourceLimits::default());
        c.set_execution_poll(None);
        c.collect_garbage();
        Ok(c.integer(999))
    });
    c.set_resource_limits(ResourceLimits {
        fuel: Some(150),
        ..ResourceLimits::default()
    });
    let error = c
        .eval_string("(try (reenter) (catch Throwable e 99) (finally (mark 1)))")
        .unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Fuel));
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [1]));
    assert!(c.resource_usage().fuel <= 150);
    assert_eq!(
        c.eval_string("42").unwrap().inspect(),
        ValueView::Integer(42)
    );

    // Top-level forms and nested top-level do share the same allowance too.
    c.set_resource_limits(ResourceLimits {
        fuel: Some(100),
        ..ResourceLimits::default()
    });
    let error = c.eval_string(&"(do (inc 1)) ".repeat(100)).unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Fuel));
}

#[test]
fn entire_unwind_has_one_cleanup_allowance_and_cannot_loop_or_allocate_forever() {
    for finally in ["(loop* [] (recur))", "(loop* [v []] (recur (conj v 1)))"] {
        let mut c = setup();
        let text = format!(
            "(try (try (loop* [] (recur)) (finally (mark 1) {finally})) (finally (mark 2)))"
        );
        let code = analyzed(&mut c, &text);
        c.set_resource_limits(ResourceLimits {
            fuel: Some(30),
            cleanup_fuel: 40,
            cleanup_allocation: 20,
            ..ResourceLimits::default()
        });
        let error = c.eval_analyzed(&code).unwrap_err();
        assert_resource(&error, limit(ResourceLimit::Fuel));
        // An outer finally cannot obtain another allowance after the inner loop.
        EFFECTS.with(|e| assert_eq!(*e.borrow(), [1]));
        c.set_resource_limits(ResourceLimits::default());
        assert!(c.eval_string("(+ 1 2)").is_ok());
    }
}

#[test]
fn allocation_reductions_and_collection_realization_are_bounded() {
    let mut c = setup();
    let code = analyzed(
        &mut c,
        "(try (loop* [v []] (recur (conj v 1))) (finally (mark 1)))",
    );
    c.set_resource_limits(ResourceLimits {
        allocation: Some(50),
        ..ResourceLimits::default()
    });
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        limit(ResourceLimit::Allocation),
    );
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [1]));
    c.set_resource_limits(ResourceLimits::default());
    let reduce = c.eval_string("(fn* [xs] (reduce + 0 xs))").unwrap();
    let values: Vec<_> = (0..1000).map(|n| c.integer(n)).collect();
    let xs = c.vector(&values).unwrap();
    c.set_resource_limits(ResourceLimits {
        fuel: Some(1200),
        ..ResourceLimits::default()
    });
    let error = c.invoke(&reduce, std::slice::from_ref(&xs)).unwrap_err();
    let InvokeError::Execution(error) = error else {
        panic!("reducer body source")
    };
    assert_resource(&error, limit(ResourceLimit::Fuel));
    assert_eq!(error.span.text(), "(reduce + 0 xs)");
    c.set_resource_limits(ResourceLimits {
        allocation: Some(5),
        ..ResourceLimits::default()
    });
    let vector = collection_definitions()
        .iter()
        .find(|d| d.metadata.name == "vector")
        .unwrap();
    let before = c.memory_stats().heap_values;
    assert!(c.invoke_native(vector, &values).is_err());
    assert_eq!(
        c.memory_stats().heap_values,
        before,
        "bulk allocation denied before construction"
    );
    let vec = collection_definitions()
        .iter()
        .find(|d| d.metadata.name == "vec")
        .unwrap();
    let list = c.list(&values).unwrap();
    assert!(c.invoke_native(vec, &[list]).is_err());
}

#[test]
fn output_is_reserved_before_effect_and_shared_by_nested_eval() {
    let mut c = setup();
    install(&mut c, "write", Arity::Exact(0), |c, _| {
        c.charge_output(4)?;
        EFFECTS.with(|e| e.borrow_mut().push(4));
        Ok(c.nil())
    });
    install(&mut c, "nested-write", Arity::Exact(0), |c, _| {
        // Deliberately ignore the nested failure: the outer ABI must retain it.
        let _ = c.eval_string("(write) (write)");
        Ok(c.nil())
    });
    c.set_resource_limits(ResourceLimits {
        output: Some(6),
        ..ResourceLimits::default()
    });
    let error = c.eval_string("(nested-write)").unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Output));
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [4]));
    assert_eq!(c.resource_usage().output, 4);
}

#[test]
fn small_programs_and_ordinary_exceptions_keep_results_effects_and_forge_resistance() {
    let mut c = setup();
    c.set_resource_limits(ResourceLimits {
        fuel: Some(20_000),
        allocation: Some(20_000),
        output: Some(100),
        ..ResourceLimits::default()
    });
    let result = c.eval_string("(mark 1) (let* [x (reduce + [1 2 3])] (mark x) (try (throw (ex-info \"x\" {:sci.impl/interrupt :sci.impl/interrupt :Cancelled true})) (catch Exception e (+ x 36)) (finally (mark 2))))").unwrap();
    assert_eq!(result.inspect(), ValueView::Integer(42));
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [1, 6, 2]));
    assert_eq!(
        c.eval_string("(try (throw (ex-info \"a\" {})) (finally (throw (ex-info \"b\" {}))))")
            .unwrap_err()
            .exception
            .unwrap()
            .exception_message()
            .unwrap()
            .inspect(),
        ValueView::String("b".encode_utf16().collect())
    );
}

#[test]
fn quota_exhaustion_at_each_small_boundary_never_panics_or_returns_partial_success() {
    let programs = [
        "(def ^{:a 1} x 2)",
        "(defprotocol P (f [x] [x y]))",
        "(let* [x {:a 1}] (assoc x :b [2 3]))",
        "(try (/ 1 0) (catch Exception e (ex-message e)) (finally (+ 1 2)))",
        "(mapv inc [1 2 3 4])",
        "(transduce identity (completing + inc) 0 [1 2 3 4])",
        "(apply (completing + inc) [3 4])",
        "(into [] (comp cat cat (take 2)) [[[1 2 3]] [[4]]])",
        "(into [] (mapcat (fn [x] [x x])) [1 2])",
        "(apply (partial + 1 2 3 4) [5 6])",
        "(meta #'inc)",
        "(select-keys {:a 1 :b 2} [:a :b])",
        "(find {:a 1} :a)",
        "(ns-map 'user)",
        "(letfn* [f (fn* [] (f))] 42)",
    ];
    for text in programs {
        for (fuel, allocation) in (0..500)
            .map(|n| (Some(n), None))
            .chain((0..500).map(|n| (None, Some(n))))
        {
            let mut c = Context::new();
            c.set_resource_limits(ResourceLimits {
                fuel,
                allocation,
                ..ResourceLimits::default()
            });
            match c.eval_string(text) {
                Ok(_) => {}
                Err(error) => assert_eq!(
                    error.class(),
                    ErrorClass::Control,
                    "fuel={fuel:?}, allocation={allocation:?}, {text}: {error}"
                ),
            }
        }
    }
}

#[test]
fn direct_lowering_entries_report_limits_without_panicking_or_partial_forms() {
    let mut c = Context::new();
    let source = c.source("supplied.clj", "\"abcdef\"");
    let tree = light_cljs::parse(source.text()).unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(2),
        ..ResourceLimits::default()
    });
    let error: EvalError = c
        .lower_form(&source, &tree, tree.roots[0])
        .unwrap_err()
        .into();
    assert_resource(&error, limit(ResourceLimit::Allocation));
    assert_eq!(error.span.text(), source.text());
    let values = [c.integer(1), c.integer(2)];
    for value in [
        c.vector(&values).unwrap(),
        c.map(&[(values[0].clone(), values[1].clone())]).unwrap(),
    ] {
        c.set_resource_limits(ResourceLimits {
            allocation: Some(1),
            ..ResourceLimits::default()
        });
        let error: EvalError = c
            .form_from_data(&value, &source.span(0..0).unwrap())
            .unwrap_err()
            .into();
        assert_resource(&error, limit(ResourceLimit::Allocation));
    }
    let token = CancellationToken::default();
    token.cancel();
    c.set_cancellation_token(Some(token));
    let mut reader = c.reader(&source).unwrap();
    assert!(reader.read_form().is_err());
    assert!(
        reader.read_form().unwrap().is_none(),
        "reader stays fused after control failure"
    );
}

#[test]
fn wide_native_arguments_and_namespace_records_are_accounted() {
    let mut c = Context::new();
    let create = c
        .eval_string("(fn* [] (create-ns 'budget-namespace))")
        .unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(0),
        ..ResourceLimits::default()
    });
    match c.invoke(&create, &[]).unwrap_err() {
        InvokeError::Execution(error) => assert_resource(&error, limit(ResourceLimit::Allocation)),
        InvokeError::Dispatch(kind) => assert_eq!(
            kind,
            EvalErrorKind::Resource(limit(ResourceLimit::Allocation))
        ),
    }
    let args: Vec<_> = (0..100_000).map(|_| c.integer(1)).collect();
    let plus = builtin_definitions()
        .iter()
        .find(|d| d.metadata.name == "+")
        .unwrap();
    c.set_resource_limits(ResourceLimits {
        fuel: Some(1),
        ..ResourceLimits::default()
    });
    let error = c.invoke_native(plus, &args).unwrap_err();
    assert!(matches!(error, NativeError::Invocation(_)));
    assert_eq!(c.resource_usage().fuel, 1);
}

#[test]
fn forwarded_terminal_failure_keeps_its_original_inner_source() {
    let mut c = Context::new();
    install(&mut c, "forward", Arity::Exact(0), |c, _| {
        let source = c.source("inner.clj", "(loop* [] (recur))");
        c.eval_source(&source)
            .map_err(|e| NativeError::from(InvokeError::Execution(Box::new(e))))
    });
    c.set_resource_limits(ResourceLimits {
        fuel: Some(200),
        ..ResourceLimits::default()
    });
    let error = c.eval_string("(forward)").unwrap_err();
    assert_resource(&error, limit(ResourceLimit::Fuel));
    assert_eq!(error.span.source().name(), "inner.clj");
    assert!(error.frames.iter().any(|f| f.span.text() == "(forward)"));
}

#[test]
fn cleanup_exhaustion_at_every_small_boundary_preserves_original_failure() {
    for amount in 0..200 {
        let mut c = setup();
        install(&mut c, "host-failure", Arity::Exact(0), |_, _| {
            Err(NativeError::Host("original".into()))
        });
        let code = analyzed(
            &mut c,
            "(try (host-failure) (finally (def ^{:a 1} x 2) (meta #'x) (mark 1)))",
        );
        c.set_resource_limits(ResourceLimits {
            cleanup_fuel: amount,
            cleanup_allocation: amount,
            ..ResourceLimits::default()
        });
        let error = c.eval_analyzed(&code).unwrap_err();
        assert_eq!(
            error.class(),
            ErrorClass::Host,
            "cleanup allowance {amount}: {error}"
        );
        assert_eq!(error.span.text(), "(host-failure)");
    }
}

#[test]
fn scalar_roots_are_charged_and_gc_does_not_refund_allocations() {
    let mut c = setup();
    install(&mut c, "gc", Arity::Exact(0), |c, _| {
        c.collect_garbage();
        Ok(c.nil())
    });
    for text in [
        "(loop* [n 0] (recur (inc n)))",
        "(loop* [n 0] (gc) (recur (inc n)))",
    ] {
        c.set_resource_limits(ResourceLimits::default());
        let code = analyzed(&mut c, text);
        c.set_resource_limits(ResourceLimits {
            allocation: Some(80),
            ..ResourceLimits::default()
        });
        let error = c.eval_analyzed(&code).unwrap_err();
        assert_resource(&error, limit(ResourceLimit::Allocation));
        assert!(c.resource_usage().allocation <= 80);
    }
}

#[test]
fn namespace_refer_snapshots_propagate_every_small_allocation_boundary() {
    for allocation in 0..180 {
        let mut c = Context::new();
        c.eval_string("(create-ns 'source) (in-ns 'source) (def x 1) (in-ns 'user)")
            .unwrap();
        let code = analyzed(&mut c, "(refer 'source)");
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..ResourceLimits::default()
        });
        if let Err(error) = c.eval_analyzed(&code) {
            assert_resource(&error, limit(ResourceLimit::Allocation));
        }
    }
}

#[test]
fn latched_exhaustion_wins_over_later_dispatch_and_analysis_diagnostics() {
    let mut c = Context::new();
    let var = c.eval_string("(def x 1)").unwrap();
    let source = c.source("diagnostic.clj", "missing");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(0),
        ..ResourceLimits::default()
    });
    assert_eq!(
        c.invoke(&var, &[]).unwrap_err().class(),
        ErrorClass::Control
    );
    for allocation in 0..12 {
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..ResourceLimits::default()
        });
        let error = c.analyze_form(&form).unwrap_err();
        assert!(matches!(
            error.class(),
            ErrorClass::Control | ErrorClass::Diagnostic
        ));
        if error.class() == ErrorClass::Diagnostic {
            assert!(c.resource_usage().allocation <= allocation);
        }
    }
}

#[test]
fn protocol_registration_and_dispatch_share_allocation_fuel_and_cancellation_controls() {
    let text = format!(
        "(defprotocol P {})",
        (0..80)
            .map(|i| format!("(method{i} [receiver])"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    for limits in [
        ResourceLimits {
            fuel: Some(200),
            ..Default::default()
        },
        ResourceLimits {
            allocation: Some(400),
            ..Default::default()
        },
    ] {
        let mut c = Context::new();
        c.eval_string("(defprotocol Good (good [x]))").unwrap();
        let method = c.eval_string("good").unwrap();
        c.set_resource_limits(limits);
        assert_eq!(
            c.eval_string(&text).unwrap_err().class(),
            ErrorClass::Control
        );
        c.set_resource_limits(ResourceLimits::default());
        assert!(method.same_identity(&c.eval_string("good").unwrap()));
    }
    let mut c = Context::new();
    c.eval_string("(defprotocol P (f [x]))").unwrap();
    let method = c.eval_string("f").unwrap();
    let token = CancellationToken::default();
    c.set_cancellation_token(Some(token.clone()));
    token.cancel();
    assert_eq!(
        c.invoke(&method, &[c.nil()]).unwrap_err().class(),
        ErrorClass::Control
    );
    assert_eq!(
        c.eval_string("(defprotocol Other)").unwrap_err().class(),
        ErrorClass::Control
    );
    c.set_cancellation_token(None);
    c.set_resource_limits(ResourceLimits {
        allocation: Some(1),
        ..Default::default()
    });
    assert_eq!(
        c.invoke(&method, &[c.nil()]).unwrap_err().class(),
        ErrorClass::Control
    );
}

#[test]
fn reify_methods_and_construction_share_resource_controls() {
    for (limits, program, expected) in [
        (
            ResourceLimits {
                allocation: Some(1),
                ..Default::default()
            },
            "(reify P (p [_ n] n))",
            limit(ResourceLimit::Allocation),
        ),
        (
            ResourceLimits {
                fuel: Some(100),
                ..Default::default()
            },
            "(p (reify P (p [this n] (recur this (inc n)))) 0)",
            limit(ResourceLimit::Fuel),
        ),
        (
            ResourceLimits {
                call_depth: 10,
                ..Default::default()
            },
            "(p (reify P (p [this n] (p this (inc n)))) 0)",
            ResourceError::CallDepthExceeded,
        ),
        (
            ResourceLimits {
                allocation: Some(120),
                ..Default::default()
            },
            "(p (reify P (p [_ n] (vec (range n)))) 1000)",
            limit(ResourceLimit::Allocation),
        ),
    ] {
        let mut c = setup();
        c.eval_string("(defprotocol P (p [x n]))").unwrap();
        let code = analyzed(&mut c, program);
        c.set_resource_limits(limits);
        assert_resource(&c.eval_analyzed(&code).unwrap_err(), expected);
    }
}

#[test]
fn reify_callback_cancellation_is_sticky_and_runs_bounded_finally() {
    let mut c = setup();
    let token = CancellationToken::default();
    TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
    install(&mut c, "cancel-method", Arity::Exact(1), |c, args| {
        assert_eq!(args[0].inspect(), ValueView::Reify);
        TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
        c.set_cancellation_token(None);
        c.collect_garbage();
        Ok(c.nil())
    });
    c.eval_string("(defprotocol P (p [x]))").unwrap();
    let code = analyzed(
        &mut c,
        "(let [o (reify P (p [this] (try (cancel-method this) (mark 2) (finally (mark 9)))))] (try (p o) (catch Throwable _ (mark 99))))",
    );
    c.set_cancellation_token(Some(token));
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        ResourceError::Cancelled,
    );
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
}

#[test]
fn record_construction_update_key_realization_and_walk_obey_small_budgets() {
    let mut c = setup();
    c.eval_string("(defrecord R [x]) (def r (->R 1))").unwrap();
    let code = analyzed(&mut c, "(assoc (map->R {:x 2 :y 3}) :x 4 :z 5)");
    let mut reached_success = false;
    for allocation in 0..220 {
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..Default::default()
        });
        match c.eval_analyzed(&code) {
            Ok(v) => {
                assert_eq!(v.inspect(), ValueView::Record(3));
                reached_success = true;
            }
            Err(e) => assert_resource(&e, limit(ResourceLimit::Allocation)),
        }
    }
    assert!(reached_success);
    for source in [
        "(map->R (into {} (map (fn [n] [n n])) (range 1000)))",
        "(get {(->R (range)) :never} r)",
        "(clojure.walk/postwalk identity (->R (range)))",
        "(pr-str (->R (range)))",
    ] {
        c.set_resource_limits(ResourceLimits::default());
        let code = analyzed(&mut c, source);
        c.set_resource_limits(ResourceLimits {
            fuel: Some(500),
            ..Default::default()
        });
        assert_resource(
            &c.eval_analyzed(&code).unwrap_err(),
            limit(ResourceLimit::Fuel),
        );
    }
    c.set_resource_limits(ResourceLimits::default());
    let code = analyzed(&mut c, "(pr-str (->R (range 1000)))");
    c.set_resource_limits(ResourceLimits {
        output: Some(10),
        ..Default::default()
    });
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        limit(ResourceLimit::Output),
    );
    c.set_resource_limits(ResourceLimits::default());
    let ctor = c.eval_string("->R").unwrap();
    let token = CancellationToken::default();
    c.set_cancellation_token(Some(token.clone()));
    token.cancel();
    assert_eq!(
        c.invoke(&ctor, &[c.nil()]).unwrap_err().class(),
        ErrorClass::Control
    );
}

#[test]
fn record_lazy_key_callback_cancellation_keeps_finally_and_gc_roots() {
    let mut c = setup();
    let token = CancellationToken::default();
    TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
    install(&mut c, "cancel-field", Arity::Exact(1), |c, args| {
        TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
        c.collect_garbage();
        Ok(args[0].clone())
    });
    c.eval_string("(defrecord R [x])").unwrap();
    let code = analyzed(
        &mut c,
        "(try {(->R (map cancel-field [1 2])) :value} (catch Throwable _ (mark 99)) (finally (mark 9)))",
    );
    c.set_cancellation_token(Some(token));
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        ResourceError::Cancelled,
    );
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
}

#[test]
fn record_methods_share_call_fuel_allocation_output_and_finally_controls() {
    for (limits, method, expected) in [
        (
            ResourceLimits {
                fuel: Some(150),
                ..Default::default()
            },
            "(p [this n] (recur (assoc this :x (inc x)) n))",
            limit(ResourceLimit::Fuel),
        ),
        (
            ResourceLimits {
                call_depth: 10,
                ..Default::default()
            },
            "(p [this n] (p this n))",
            ResourceError::CallDepthExceeded,
        ),
        (
            ResourceLimits {
                allocation: Some(150),
                ..Default::default()
            },
            "(p [_ n] (vec (range n)))",
            limit(ResourceLimit::Allocation),
        ),
        (
            ResourceLimits {
                output: Some(5),
                ..Default::default()
            },
            "(p [_ n] (pr-str (range n)))",
            limit(ResourceLimit::Output),
        ),
    ] {
        let mut c = setup();
        c.eval_string(&format!(
            "(defprotocol P (p [r n])) (defrecord R [x] P {method}) (def r (->R 1))"
        ))
        .unwrap();
        let code = analyzed(
            &mut c,
            "(try (p r 1000) (catch Throwable _ (mark 99)) (finally (mark 9)))",
        );
        c.set_resource_limits(limits);
        assert_resource(&c.eval_analyzed(&code).unwrap_err(), expected);
        EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
    }
}

#[test]
fn record_method_cancellation_keeps_receiver_and_runs_finally() {
    let mut c = setup();
    let token = CancellationToken::default();
    TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
    install(
        &mut c,
        "cancel-record-method",
        Arity::Exact(1),
        |c, args| {
            assert!(matches!(args[0].inspect(), ValueView::Record(_)));
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
            c.collect_garbage();
            Ok(c.nil())
        },
    );
    c.eval_string("(defprotocol P (p [x])) (defrecord R [x] P (p [this] (try (cancel-record-method this) (mark x) (finally (mark 9)))))").unwrap();
    let code = analyzed(&mut c, "(try (p (->R 2)) (catch Throwable _ (mark 99)))");
    c.set_cancellation_token(Some(token));
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        ResourceError::Cancelled,
    );
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
}

#[test]
fn external_extensions_validate_and_build_tables_before_atomic_publication() {
    let mut c = setup();
    c.eval_string("(defprotocol P (p [x]) (q [x])) (defrecord R [n] P (p [_] 1) (q [_] 2))")
        .unwrap();
    let update = analyzed(&mut c, "(extend-type R P (p [_] 9) (q [_] 10))");
    let mut success = false;
    for allocation in 0..200 {
        c.set_resource_limits(ResourceLimits::default());
        c.eval_string("(extend-type R P (p [_] 1) (q [_] 2))")
            .unwrap();
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..Default::default()
        });
        let result = c.eval_analyzed(&update);
        let expected = if let Err(e) = result {
            assert_resource(&e, limit(ResourceLimit::Allocation));
            [1, 2]
        } else {
            success = true;
            [9, 10]
        };
        c.set_resource_limits(ResourceLimits::default());
        let actual = c.eval_string("[(p (->R 0)) (q (->R 0))]").unwrap();
        assert_eq!(
            actual
                .elements()
                .unwrap()
                .iter()
                .map(|v| if let ValueView::Integer(n) = v.inspect() {
                    n
                } else {
                    panic!("integer")
                })
                .collect::<Vec<_>>(),
            expected,
            "allocation {allocation}"
        );
    }
    assert!(success);
    // Long declaration normalization and table preparation remain interruptible.
    let methods = (0..200)
        .map(|n| format!("(p{n} [x])"))
        .collect::<Vec<_>>()
        .join(" ");
    c.eval_string(&format!("(defprotocol Large {methods})"))
        .unwrap();
    let implementations = (0..200)
        .map(|n| format!("(p{n} [_] {n})"))
        .collect::<Vec<_>>()
        .join(" ");
    for text in [
        format!("(extend-type R Large {implementations})"),
        format!("(extend-protocol Large R {implementations})"),
    ] {
        let code = analyzed(&mut c, &text);
        for source_analysis in [false, true] {
            c.set_resource_limits(ResourceLimits {
                fuel: Some(100),
                ..Default::default()
            });
            let error = if source_analysis {
                c.eval_string(&text).unwrap_err()
            } else {
                c.eval_analyzed(&code).unwrap_err()
            };
            assert_resource(&error, limit(ResourceLimit::Fuel));
            c.set_resource_limits(ResourceLimits::default());
            assert_eq!(
                c.eval_string("(satisfies? Large (->R 0))")
                    .unwrap()
                    .inspect(),
                ValueView::Boolean(false)
            );
        }
    }
}

#[test]
fn extended_methods_share_limits_exceptions_cancellation_and_finally() {
    for (limits, method, expected) in [
        (
            ResourceLimits {
                fuel: Some(150),
                ..Default::default()
            },
            "(p [r n] (recur r n))",
            limit(ResourceLimit::Fuel),
        ),
        (
            ResourceLimits {
                call_depth: 10,
                ..Default::default()
            },
            "(p [r n] (p r n))",
            ResourceError::CallDepthExceeded,
        ),
        (
            ResourceLimits {
                allocation: Some(150),
                ..Default::default()
            },
            "(p [_ n] (vec (range n)))",
            limit(ResourceLimit::Allocation),
        ),
        (
            ResourceLimits {
                output: Some(5),
                ..Default::default()
            },
            "(p [_ n] (pr-str (range n)))",
            limit(ResourceLimit::Output),
        ),
    ] {
        let mut c = setup();
        c.eval_string(&format!(
            "(defprotocol P (p [x n])) (defrecord R [v]) (def r (->R 2)) (extend-type R P {method})"
        ))
        .unwrap();
        let code = analyzed(
            &mut c,
            "(try (p r 1000) (catch Throwable _ (mark 99)) (finally (mark 9)))",
        );
        c.set_resource_limits(limits);
        assert_resource(&c.eval_analyzed(&code).unwrap_err(), expected);
        EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
    }
    let mut c = setup();
    install(&mut c, "extension-throw", Arity::Exact(0), |c, _| {
        c.collect_garbage();
        c.eval_string("(throw (ex-info \"host\" {:n 7}))")
            .map_err(|e| NativeError::from(InvokeError::Execution(Box::new(e))))
    });
    c.eval_string(
        "(defprotocol P (p [x])) (defrecord R [n]) (extend-type R P (p [_] (extension-throw)))",
    )
    .unwrap();
    assert_eq!(
        c.eval_string("(try (p (->R 1)) (catch Exception e (:n (ex-data e))) (finally (mark 9)))")
            .unwrap()
            .inspect(),
        ValueView::Integer(7)
    );
    EFFECTS.with(|e| {
        assert_eq!(*e.borrow(), [9]);
        e.borrow_mut().clear();
    });
    let token = CancellationToken::default();
    TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
    install(&mut c, "extension-cancel", Arity::Exact(0), |c, _| {
        TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
        c.set_cancellation_token(None);
        c.collect_garbage();
        Ok(c.nil())
    });
    c.eval_string(
        "(extend-protocol P R (p [_] (try (extension-cancel) (mark 2) (finally (mark 9)))))",
    )
    .unwrap();
    let code = analyzed(&mut c, "(try (p (->R 1)) (catch Throwable _ (mark 99)))");
    c.set_cancellation_token(Some(token));
    assert_resource(
        &c.eval_analyzed(&code).unwrap_err(),
        ResourceError::Cancelled,
    );
    EFFECTS.with(|e| assert_eq!(*e.borrow(), [9]));
}
