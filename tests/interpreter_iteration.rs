use light_cljs::interpreter::{
    Arity, CollectionError, Context, EvalErrorKind as E, EvalPhase, ExceptionCategory,
    NativeDefinition, NativeEffects, NativeError, NativeMetadata, RootedValue, ValueView,
};
use std::cell::RefCell;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn wire(value: &RootedValue) -> protocol::Value {
    use protocol::Value as V;
    match value.inspect() {
        ValueView::CompilerLiteral(_) => panic!("compiler-only reader value in native fixture"),
        ValueView::LazySeq | ValueView::Delay => {
            panic!("deferred values must be projected by the evaluated fixture")
        }
        ValueView::NativeFunction(_)
        | ValueView::Function
        | ValueView::Var(_)
        | ValueView::Namespace(_)
        | ValueView::Unbound(_)
        | ValueView::Exception(_)
        | ValueView::Atom
        | ValueView::Input
        | ValueView::Output
        | ValueView::Volatile
        | ValueView::Protocol(_)
        | ValueView::Reify
        | ValueView::Record(_)
        | ValueView::RecordType(_)
        | ValueView::Reduced => {
            panic!("callables are outside fixture transport")
        }
        ValueView::Nil => V::Nil,
        ValueView::Boolean(v) => V::Bool(v),
        ValueView::Integer(v) => V::Int(v),
        ValueView::Double(v) => V::Float(if v.is_nan() {
            f64::NAN.to_bits()
        } else {
            v.to_bits()
        }),
        ValueView::Ratio {
            numerator,
            denominator,
        } => V::Ratio(numerator, denominator),
        ValueView::Character(v) => V::Char(v),
        ValueView::String(v) => V::String(v.to_vec()),
        ValueView::Symbol(v) => V::Symbol(v.namespace().map(<[u16]>::to_vec), v.name().to_vec()),
        ValueView::Keyword(v) => V::Keyword(v.namespace().map(<[u16]>::to_vec), v.name().to_vec()),
        ValueView::List(_) => V::List(value.elements().unwrap().iter().map(wire).collect()),
        ValueView::Seq(_) => V::Seq(value.elements().unwrap().iter().map(wire).collect()),
        ValueView::Vector(_) => V::Vector(value.elements().unwrap().iter().map(wire).collect()),
        ValueView::Map(_) => {
            let mut pairs: Vec<_> = value
                .entries()
                .unwrap()
                .iter()
                .map(|(k, v)| (wire(k), wire(v)))
                .collect();
            pairs.sort_by_key(|(k, v)| format!("(entry {} {})", k.wire(), v.wire()));
            V::Map(pairs)
        }
        ValueView::Set(_) => {
            let mut values: Vec<_> = value.elements().unwrap().iter().map(wire).collect();
            values.sort_by_key(V::wire);
            V::Set(values)
        }
    }
}

thread_local! { static TRACE: RefCell<Vec<protocol::Value>> = const { RefCell::new(Vec::new()) }; }
static OBSERVE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "observe",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |context, args| {
        TRACE.with(|trace| trace.borrow_mut().push(wire(&args[0])));
        context.collect_garbage();
        Ok(args[0].clone())
    },
};

#[test]
fn iteration_programs_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for case in cases.iter().filter(|c| c.id.starts_with("sci-iteration-")) {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let actual = match c.eval_string(case.source) {
            Ok(v) => O::Returned(wire(&v)),
            Err(e) => {
                let phase = match e.phase {
                    EvalPhase::Execution | EvalPhase::Macroexpansion => P::Execution,
                    EvalPhase::Analysis => P::Analysis,
                    EvalPhase::Read => P::Read,
                };
                let category = match e.kind {
                    E::InvalidForm(_) => C::InvalidForm,
                    E::Read(_) => C::Reader,
                    E::UnresolvedSymbol(_) => C::UnresolvedSymbol,
                    E::Arity { .. }
                    | E::Native {
                        error: NativeError::Arity { .. },
                        ..
                    } => C::Arity,
                    E::NoMatchingClause
                    | E::Collection(_)
                    | E::NotCallable
                    | E::Native {
                        error: NativeError::Type { .. },
                        ..
                    } => C::Type,
                    E::Native {
                        error: NativeError::Collection(CollectionError::InvalidIndex),
                        ..
                    } => C::Bounds,
                    E::Native {
                        error: NativeError::Collection(_),
                        ..
                    } => C::Type,
                    E::Native {
                        error: NativeError::Arithmetic(_),
                        ..
                    } => C::Arithmetic,
                    E::Thrown(ExceptionCategory::ExceptionInfo) => C::ExceptionInfo,
                    _ => panic!("{}: {e}", case.id),
                };
                O::Error { phase, category }
            }
        };
        if actual != case.outcome {
            failures.push(format!(
                "{}: {}\nactual {actual:?} expected {:?}",
                case.id, case.source, case.outcome
            ));
        }
        TRACE.with(|t| {
            if *t.borrow() != case.effects {
                failures.push(format!(
                    "{} effects {:?} expected {:?}",
                    case.id,
                    *t.borrow(),
                    case.effects
                ));
            }
        });
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        assert_eq!(
            c.eval_string("(+ 20 22)").unwrap().inspect(),
            ValueView::Integer(42)
        );
        count += 1;
    }
    assert_eq!(count, 90);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn registered_macros_expand_to_owned_reusable_code() {
    use light_cljs::interpreter::Name;
    for (name, program) in [
        ("for", "(for [x [1 2] :let [y (+ x 10)] :when (odd? x)] y)"),
        ("doseq", "(doseq [x [1 2] :while (< x 3)] x)"),
        ("dotimes", "(dotimes [i 3] i)"),
        ("while", "(while false 42)"),
    ] {
        let mut c = Context::new();
        c.set_macro_diagnostics(true);
        c.eval_string(program).unwrap();
        let quoted = c.eval_string(&format!("'{program}")).unwrap();
        let expanded = c.macroexpand(&quoted, None).unwrap();
        let source = c.source("owned-iteration.clj", "");
        let form = c
            .form_from_data(&expanded, &source.span(0..0).unwrap())
            .unwrap();
        let code = c.analyze_form(&form).unwrap();
        drop((source, form, expanded, quoted));
        c.collect_garbage();
        let expansions: u64 = c.macro_expansion_stats().iter().map(|s| s.expansions).sum();
        for _ in 0..3 {
            let result = c.eval_analyzed(&code).unwrap();
            if name == "for" {
                assert_eq!(
                    c.sequence_elements(&result).unwrap()[0].inspect(),
                    ValueView::Integer(11)
                );
            } else {
                assert_eq!(result.inspect(), ValueView::Nil);
            }
            c.collect_garbage();
        }
        assert_eq!(
            c.macro_expansion_stats()
                .iter()
                .map(|s| s.expansions)
                .sum::<u64>(),
            expansions
        );
        let stat = c
            .macro_expansion_stats()
            .iter()
            .find(|s| s.name == Name::new(Some("clojure.core"), name))
            .unwrap();
        assert!(stat.expansions > 0 && stat.generated_forms > 0);
        let var = c
            .resolve_var(&Name::new(Some("clojure.core"), name))
            .unwrap()
            .unwrap();
        assert_eq!(
            var.metadata()
                .unwrap()
                .get(&c.keyword(Name::new(None, "macro")))
                .unwrap()
                .unwrap()
                .inspect(),
            ValueView::Boolean(true)
        );
        assert_eq!(c.eval_string(name).unwrap_err().phase, EvalPhase::Analysis);
    }
}

#[test]
fn malformed_binding_and_modifier_forms_report_the_source_before_effects() {
    use light_cljs::interpreter::Origin;
    // SCI's permissive/malformed cases sometimes throw host NPE/AssertionError.
    // The native boundary deliberately validates all these as source errors.
    for program in [
        "(for)",
        "(for [x []])",
        "(for [x []] x x)",
        "(for 1 42)",
        "(for [x] x)",
        "(for [] 42)",
        "(for [:when true] 42)",
        "(for [x (observe [1]) :wat true] x)",
        "(for [x (observe [1]) :let 42] x)",
        "(for [x (observe [1]) :let [y]] x)",
        "(for [x (observe [1]) :other/when true] x)",
        "(doseq)",
        "(doseq (x [1]) x)",
        "(doseq [x] x)",
        "(doseq [x (observe [1]) :wat true] x)",
        "(doseq [x (observe [1]) :let 42] x)",
        "(doseq [x (observe [1]) :let [y]] x)",
        "(dotimes)",
        "(dotimes (x 3) x)",
        "(dotimes [] 42)",
        "(dotimes [x] 42)",
        "(dotimes [x (observe 1) y 2] x)",
        "(while)",
    ] {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let source = c.source("bad-iteration.clj", format!("\n {program}"));
        let error = c.eval_source(&source).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Macroexpansion, "{program}: {error}");
        assert_eq!(error.span.text(), program);
        assert_eq!(error.span.source().name(), "bad-iteration.clj");
        assert!(matches!(error.origin, Origin::MacroExpansion(_)));
        TRACE.with(|t| assert!(t.borrow().is_empty(), "{program}"));
    }
    for program in ["(for [1 [1]] 1)", "(doseq [1 [1]] 1)", "(dotimes [1 3] 1)"] {
        let mut c = Context::new();
        let error = c.eval_string(program).unwrap_err();
        assert!(matches!(
            error.phase,
            EvalPhase::Macroexpansion | EvalPhase::Analysis
        ));
        assert_eq!(error.span.source().text(), program);
    }
}

#[test]
fn retained_comprehensions_capture_values_and_keep_original_failure_spans() {
    use light_cljs::interpreter::{DeferredStatus, InvokeError};
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let retained = {
        let source = c.source("retained-for.clj", "(let [kept [\"kept\"] unused [1 2 3]] (for [x (list 1 2) :let [y (inc x)]] (observe [kept y])))");
        c.eval_source(&source).unwrap()
    };
    assert_eq!(retained.deferred_status(), Some(DeferredStatus::Unrealized));
    c.collect_garbage();
    TRACE.with(|t| t.borrow_mut().clear());
    let values = c.sequence_elements(&retained).unwrap();
    assert_eq!(values.len(), 2);
    c.collect_garbage();
    assert_eq!(c.sequence_elements(&retained).unwrap().len(), 2);
    TRACE.with(|t| assert_eq!(t.borrow().len(), 2));
    let failed = {
        let source = c.source(
            "for-body.clj",
            "(for [x (list 1)] (throw (ex-info \"failed\" {:x x})))",
        );
        c.eval_source(&source).unwrap()
    };
    c.collect_garbage();
    let NativeError::Invocation(error) = c.seq(&failed).unwrap_err() else {
        panic!("invocation error")
    };
    let InvokeError::Execution(error) = error.error() else {
        panic!("source-bearing error")
    };
    assert_eq!(error.span.source().name(), "for-body.clj");
    assert_eq!(error.span.text(), "(throw (ex-info \"failed\" {:x x}))");
    assert_eq!(c.seq(&failed).unwrap().inspect(), ValueView::Nil);
    let old = c.eval_string("(for [x [1]] x)").unwrap();
    c.reset();
    assert_eq!(c.seq(&old).unwrap_err(), NativeError::WrongOwner);
    drop(c);
    assert_eq!(retained.elements().unwrap().len(), 2);
    assert_eq!(
        values[1].nth(1).unwrap().unwrap().inspect(),
        ValueView::Integer(3)
    );
}

#[test]
fn iteration_and_expansion_share_fuel_allocation_and_depth_limits() {
    use light_cljs::interpreter::{ErrorClass, ResourceLimits};
    for program in [
        "(first (for [x (range) :when false] x))",
        "(first (for [x (range) y []] y))",
        "(doall (for [x (range)] x))",
        "(doseq [x (range)])",
        "(doseq [x [1] y (range)])",
        "(doseq [x (range) :when false] x)",
        "(dotimes [i 9223372036854775807])",
        "(while true)",
    ] {
        let mut c = Context::new();
        let source = c.source("infinite-iteration.clj", program);
        let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
        let code = c.analyze_form(&form).unwrap();
        c.set_resource_limits(ResourceLimits {
            fuel: Some(3000),
            ..Default::default()
        });
        let error = c.eval_analyzed(&code).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{program}: {error}");
        c.set_resource_limits(ResourceLimits::default());
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
    for program in [
        "(first (for [[x] [[42]] :let [y x] :when true :while true] y))",
        "(do (doseq [x [1 2] :when true :let [y x]] y) 42)",
        "(do (dotimes [i 3]) 42)",
        "(do (while false) 42)",
    ] {
        let mut baseline = Context::new();
        assert_eq!(
            baseline.eval_string(program).unwrap().inspect(),
            ValueView::Integer(42)
        );
        let usage = baseline.resource_usage();
        for allocation in [false, true] {
            let bound = if allocation {
                usage.allocation
            } else {
                usage.fuel
            };
            for budget in (0..48).chain([bound - 1, bound, bound + 1]) {
                let mut c = Context::new();
                c.set_resource_limits(ResourceLimits {
                    fuel: (!allocation).then_some(budget),
                    allocation: allocation.then_some(budget),
                    ..Default::default()
                });
                match c.eval_string(program) {
                    Ok(value) => assert_eq!(value.inspect(), ValueView::Integer(42)),
                    Err(error) => assert_eq!(
                        error.class(),
                        ErrorClass::Control,
                        "{program}/{allocation}/{budget}: {error}"
                    ),
                }
            }
        }
    }
    let mut c = Context::new();
    let value = c.eval_string("(for [x [1]] x)").unwrap();
    c.set_resource_limits(ResourceLimits {
        call_depth: 1,
        ..Default::default()
    });
    let NativeError::Invocation(error) = c.seq(&value).unwrap_err() else {
        panic!("depth failure")
    };
    assert_eq!(error.error().class(), ErrorClass::Control);
}

thread_local! {
    static SAMPLES: RefCell<Vec<(usize, light_cljs::interpreter::MemoryStats)>> = const { RefCell::new(Vec::new()) };
    static TOKEN: RefCell<Option<light_cljs::interpreter::CancellationToken>> = const { RefCell::new(None) };
}
static SAMPLE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "sample",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, args| {
        if let ValueView::Integer(n) = args[0].inspect()
            && n % 2000 == 0
        {
            let stack = 0u8;
            c.collect_garbage();
            SAMPLES.with(|s| {
                s.borrow_mut()
                    .push((&stack as *const u8 as usize, c.memory_stats()))
            });
        }
        Ok(c.nil())
    },
};
#[test]
fn discarded_prefixes_and_effectful_loops_keep_live_storage_and_stack_bounded() {
    for program in [
        "(loop [xs (for [x (range)] x) n 0] (if (= n 20000) (first xs) (do (sample n) (recur (next xs) (inc n)))))",
        "(loop [xs (for [x (range) y [0 1]] (+ (* x 2) y)) n 0] (if (= n 20000) (first xs) (do (sample n) (recur (next xs) (inc n)))))",
        "(do (doseq [x (range 20000)] (sample x)) 20000)",
        "(do (dotimes [i 20000] (sample i)) 20000)",
        "(def n 0) (while (< n 20000) (sample n) (def n (inc n))) n",
    ] {
        let mut c = Context::new();
        c.register_native(&SAMPLE);
        SAMPLES.with(|s| s.borrow_mut().clear());
        assert_eq!(
            c.eval_string(program).unwrap().inspect(),
            ValueView::Integer(20000)
        );
        SAMPLES.with(|s| {
            let s = s.borrow();
            assert_eq!(s.len(), 10);
            let stable = &s[2..];
            assert!(
                stable.iter().all(|(stack, m)| *stack == stable[0].0
                    && m.heap_values == stable[0].1.heap_values
                    && m.roots == stable[0].1.roots),
                "{program}: {s:?}"
            );
            assert!(stable[0].1.heap_values < 180, "{program}: {s:?}");
        });
    }
}
static CANCEL_AFTER: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "cancel-after",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, args| {
        if args[0].inspect() == ValueView::Integer(10) {
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
        }
        Ok(args[0].clone())
    },
};
#[test]
fn cancellation_is_terminal_in_every_form_and_runs_finally() {
    use light_cljs::interpreter::{CancellationToken, ErrorClass};
    for body in [
        "(doall (for [x (range)] (cancel-after x)))",
        "(first (for [x (range) :when (do (cancel-after x) false)] x))",
        "(doseq [x (range)] (cancel-after x))",
        "(dotimes [i 1000] (cancel-after i))",
        "(do (def n 0) (while true (cancel-after n) (def n (inc n))))",
    ] {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        c.register_native(&CANCEL_AFTER);
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        TRACE.with(|t| t.borrow_mut().clear());
        let error = c
            .eval_string(&format!(
                "(try {body} (catch Throwable e (observe :caught)) (finally (observe :cleanup)))"
            ))
            .unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{body}: {error}");
        TRACE.with(|t| {
            assert_eq!(
                *t.borrow(),
                vec![protocol::Value::Keyword(
                    None,
                    "cleanup".encode_utf16().collect()
                )]
            )
        });
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
}
