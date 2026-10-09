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
        "(let* [x {:a 1}] (assoc x :b [2 3]))",
        "(try (/ 1 0) (catch Exception e (ex-message e)) (finally (+ 1 2)))",
        "(mapv inc [1 2 3 4])",
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
