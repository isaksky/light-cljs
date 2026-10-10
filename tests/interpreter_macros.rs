use light_cljs::interpreter::{
    Arity, CollectionError, Context, ErrorClass, EvalErrorKind as E, EvalPhase, ExceptionCategory,
    Name, NativeDefinition, NativeEffects, NativeError, NativeMetadata, Origin, ResourceError,
    ResourceLimits, RootedValue, ValueView,
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
fn macro_programs_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-macro-")) {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let actual = match c.eval_string(case.source) {
            Ok(v) => O::Returned(wire(&v)),
            Err(e) => {
                let phase = match e.phase {
                    EvalPhase::Execution | EvalPhase::Macroexpansion => P::Execution,
                    EvalPhase::Analysis => P::Analysis,
                    _ => panic!("{}: {e}", case.id),
                };
                let category = match e.kind {
                    E::InvalidForm(_) => C::InvalidForm,
                    E::UnresolvedSymbol(_) => C::UnresolvedSymbol,
                    E::Arity { .. }
                    | E::Native {
                        error: NativeError::Arity { .. },
                        ..
                    } => C::Arity,
                    E::Collection(_)
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
        assert_eq!(actual, case.outcome, "{}: {}", case.id, case.source);
        TRACE.with(|t| assert_eq!(*t.borrow(), case.effects, "{} effects", case.id));
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        assert_eq!(
            c.eval_string("(+ 20 22)").unwrap().inspect(),
            ValueView::Integer(42)
        );
        count += 1;
    }
    assert_eq!(count, 50);
}

static RAW: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "raw",
        arity: Arity::Exact(3),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, args| {
        assert!(matches!(args[0].inspect(), ValueView::List(_)));
        assert!(matches!(args[1].inspect(), ValueView::Map(_)));
        c.collect_garbage();
        Ok(c.list(&[c.symbol(Name::new(None, "quote")), args[2].clone()])?)
    },
};

#[test]
fn host_and_interpreted_macros_share_checked_raw_abi() {
    let mut c = Context::new();
    let var = c.register_macro(&RAW);
    assert_eq!(
        c.eval_string("(host/raw unknown)").unwrap().inspect(),
        ValueView::Symbol(Name::new(None, "unknown"))
    );
    let error = c.eval_string("(host/raw)").unwrap_err();
    assert_eq!(error.phase, EvalPhase::Macroexpansion);
    assert!(matches!(
        error.kind,
        E::Native {
            error: NativeError::Arity { actual: 2, .. },
            ..
        }
    ));
    assert_eq!(error.span.text(), "(host/raw)");
    assert_eq!(
        error.origin,
        Origin::MacroExpansion(Name::new(Some("host"), "raw").into())
    );
    assert_eq!(
        c.eval_string("host/raw").unwrap_err().phase,
        EvalPhase::Analysis
    );
    c.set_var_macro(var.var_id().unwrap(), false).unwrap();
    assert!(matches!(
        c.eval_string("host/raw").unwrap().inspect(),
        ValueView::NativeFunction(_)
    ));
    let foreign = Context::new();
    assert!(c.macroexpand(&foreign.nil(), None).is_err());
    let own = c.nil();
    assert!(c.macroexpand(&own, Some(&foreign.nil())).is_err());
}

#[test]
fn generated_forms_keep_call_site_and_origin_after_owner_reset() {
    let mut c = Context::new();
    let source = c.source(
        "macro-source.clj",
        "(defmacro broken [] '(+ 1 missing))\n(broken)",
    );
    let error = c.eval_source(&source).unwrap_err();
    assert_eq!(error.phase, EvalPhase::Analysis);
    assert_eq!(error.span.text(), "(broken)");
    assert_eq!(
        error.origin,
        Origin::MacroExpansion(Name::new(Some("user"), "broken").into())
    );
    c.reset();
    drop(c);
    assert!(error.to_string().contains("macro-source.clj:2:1"));
    let mut c = Context::new();
    let error = c
        .eval_string("(defmacro fail [] (/ 1 0)) (fail)")
        .unwrap_err();
    assert_eq!(error.phase, EvalPhase::Macroexpansion);
    assert_eq!(error.span.text(), "(fail)");
    assert!(error.frames.iter().any(|f| f.span.text() == "(/ 1 0)"));
}

#[test]
fn effects_happen_per_analysis_and_diagnostics_attribute_generated_work() {
    let mut c = Context::new();
    c.set_macro_diagnostics(true);
    c.eval_string("(def n 0) (defmacro m [] (def n (inc n)) n)")
        .unwrap();
    let source = c.source("same.clj", "(m)");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let first = c.analyze_form(&form).unwrap();
    let second = c.analyze_form(&form).unwrap();
    for _ in 0..3 {
        assert_eq!(
            c.eval_analyzed(&first).unwrap().inspect(),
            ValueView::Integer(1)
        );
        assert_eq!(
            c.eval_analyzed(&second).unwrap().inspect(),
            ValueView::Integer(2)
        );
    }
    let stats = c
        .macro_expansion_stats()
        .iter()
        .find(|s| s.name == Name::new(Some("user"), "m"))
        .unwrap();
    assert_eq!(stats.expansions, 2);
    assert_eq!(stats.generated_forms, 2);
    assert!(!stats.elapsed.is_zero());
    c.set_macro_diagnostics(false);
    c.eval_form(&form).unwrap();
    assert_eq!(
        c.macro_expansion_stats()
            .iter()
            .find(|s| s.name == Name::new(Some("user"), "m"))
            .unwrap()
            .expansions,
        2
    );
}

#[test]
fn every_small_macro_budget_boundary_returns_terminal_failure_without_panics() {
    for allocation in [false, true] {
        let mut successes = 0;
        for budget in 0..1200 {
            let mut c = Context::new();
            c.set_resource_limits(ResourceLimits {
                fuel: (!allocation).then_some(budget),
                allocation: allocation.then_some(budget),
                ..ResourceLimits::default()
            });
            match c.eval_string("(defmacro m [x] (list '+ x 2)) (m 40)") {
                Ok(v) => {
                    assert_eq!(v.inspect(), ValueView::Integer(42));
                    successes += 1;
                }
                Err(e) => assert_eq!(
                    e.class(),
                    ErrorClass::Control,
                    "budget={budget}, allocation={allocation}: {e}"
                ),
            }
        }
        assert!(successes > 0, "allocation={allocation}");
    }
}

#[test]
fn recursive_expansions_and_macro_execution_obey_common_depth_and_fuel() {
    for definition in [
        "(defmacro m [] '(m))",
        "(defmacro m [] '(if true (m) 0))",
        "(defmacro m [] '(do (m)))",
    ] {
        let mut c = Context::new();
        c.eval_string(definition).unwrap();
        c.set_resource_limits(ResourceLimits {
            evaluation_depth: 12,
            ..ResourceLimits::default()
        });
        let error = c.eval_string("(m)").unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control);
        assert_eq!(error.kind, E::EvaluationDepthExceeded);
    }
    let mut c = Context::new();
    c.eval_string("(defmacro m [] (loop* [] (recur)))").unwrap();
    c.set_resource_limits(ResourceLimits {
        fuel: Some(200),
        ..ResourceLimits::default()
    });
    let error = c.eval_string("(m)").unwrap_err();
    assert_eq!(error.phase, EvalPhase::Macroexpansion);
    assert_eq!(error.class(), ErrorClass::Control);
    assert!(matches!(
        error.kind,
        E::Resource(ResourceError::LimitExceeded(_))
    ));
    assert_eq!(error.span.text(), "(m)");
}

#[test]
fn native_macro_reset_and_foreign_return_are_checked() {
    static RESET: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "reset",
            arity: Arity::Exact(2),
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback: |c, _| {
            c.reset();
            Ok(c.nil())
        },
    };
    static FOREIGN: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "foreign",
            arity: Arity::Exact(2),
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback: |_, _| Ok(Context::new().nil()),
    };
    for definition in [&RESET, &FOREIGN] {
        let mut c = Context::new();
        c.register_macro(definition);
        let error = c
            .eval_string(&format!("(host/{})", definition.metadata.name))
            .unwrap_err();
        assert_eq!(error.phase, EvalPhase::Macroexpansion);
        assert!(matches!(
            error.kind,
            E::Native {
                error: NativeError::WrongOwner,
                ..
            }
        ));
    }
}

#[test]
fn direct_macroexpand_admission_never_clears_latched_allocation_failures() {
    let input = |c: &Context| {
        c.list(&[c.symbol(Name::new(None, "+")), c.integer(1)])
            .unwrap()
    };
    let mut baseline = Context::new();
    baseline.macroexpand(&input(&baseline), None).unwrap();
    let required = baseline.resource_usage().allocation;
    for budget in 0..required + 3 {
        let mut c = Context::new();
        let input = input(&c);
        c.set_resource_limits(ResourceLimits {
            allocation: Some(budget),
            ..ResourceLimits::default()
        });
        let result = c.macroexpand(&input, None);
        if budget < required {
            assert_eq!(
                result.unwrap_err().class(),
                ErrorClass::Control,
                "budget={budget}"
            );
        } else {
            assert_eq!(result.unwrap().inspect(), ValueView::List(2));
        }
    }
    let mut c = Context::new();
    let form = c
        .form_from_data(
            &c.list(&[c.symbol(Name::new(None, "do"))]).unwrap(),
            &c.source("empty", "").span(0..0).unwrap(),
        )
        .unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(0),
        ..ResourceLimits::default()
    });
    assert_eq!(c.eval_form(&form).unwrap_err().class(), ErrorClass::Control);
}

#[test]
fn cancellation_during_macro_execution_is_terminal_and_runs_bounded_finally() {
    use light_cljs::interpreter::{CancellationToken, PollPoint};
    thread_local! { static TOKEN: RefCell<Option<CancellationToken>> = const { RefCell::new(None) }; }
    static CANCEL: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "user",
            name: "cancel",
            arity: Arity::Exact(0),
            effects: NativeEffects::Host,
            errors: &[],
        },
        callback: |c, _| {
            TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            let _ = c.checkpoint(PollPoint::Host);
            Ok(c.nil())
        },
    };
    let mut c = Context::new();
    c.register_native(&CANCEL);
    c.register_native(&OBSERVE);
    c.eval_string("(defmacro m [] (try (cancel) (catch Exception e (observe :caught)) (finally (observe :finally))))").unwrap();
    let token = CancellationToken::default();
    c.set_cancellation_token(Some(token.clone()));
    TOKEN.with(|t| *t.borrow_mut() = Some(token));
    TRACE.with(|t| t.borrow_mut().clear());
    let error = c.eval_string("(m)").unwrap_err();
    assert_eq!(error.phase, EvalPhase::Macroexpansion);
    assert_eq!(error.kind, E::Resource(ResourceError::Cancelled));
    assert_eq!(error.class(), ErrorClass::Control);
    TRACE.with(|t| {
        assert_eq!(
            *t.borrow(),
            [protocol::Value::Keyword(
                None,
                "finally".encode_utf16().collect()
            )]
        )
    });
    TOKEN.with(|t| *t.borrow_mut() = None);
}
