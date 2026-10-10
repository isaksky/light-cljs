use light_cljs::interpreter::{
    ArithmeticError as A, Arity, Context, EvalErrorKind as E, EvalPhase, Name, NativeDefinition,
    NativeEffects, NativeError as N, NativeErrorClass, NativeMetadata, RootedValue, ValueView,
    builtin_definitions,
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
        // Collection while active tests code, locals and argument roots.
        context.collect_garbage();
        Ok(args[0].clone())
    },
};

#[test]
fn native_programs_execute_against_pinned_sci_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases
        .iter()
        .filter(|case| case.id.starts_with("sci-native-"))
    {
        let mut context = Context::new();
        context.register_native(&OBSERVE);
        TRACE.with(|trace| trace.borrow_mut().clear());
        let outcome = match context.eval_string(case.source) {
            Ok(value) => O::Returned(wire(&value)),
            Err(error) => {
                assert_eq!(error.phase, EvalPhase::Execution, "{}: {error}", case.id);
                let E::Native { error, .. } = error.kind else {
                    panic!("unexpected {}: {error}", case.id);
                };
                let category = match error {
                    N::Arity { .. } => C::Arity,
                    N::Arithmetic(A::ConversionRange) | N::Type { .. } => C::Type,
                    N::Arithmetic(_) => C::Arithmetic,
                    _ => panic!("unexpected {}: {error}", case.id),
                };
                O::Error {
                    phase: P::Execution,
                    category,
                }
            }
        };
        assert_eq!(outcome, case.outcome, "{}: {}", case.id, case.source);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), case.effects, "{} effects", case.id));
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 39, "keep the compared scope explicit");
}

fn eval(text: &str) -> ValueView {
    Context::new().eval_string(text).unwrap().inspect()
}
fn native_error(text: &str) -> N {
    let error = Context::new().eval_string(text).unwrap_err();
    assert_eq!(error.phase, EvalPhase::Execution);
    assert_eq!(error.span.text(), text);
    let E::Native { error, .. } = error.kind else {
        panic!("expected native failure: {error}");
    };
    error
}

#[test]
fn bounded_numbers_reject_unrepresentable_results_after_exact_reduction() {
    for text in [
        "(/ -9223372036854775808 -1)",
        "(quot -9223372036854775808 -1)",
        "(/ 1 -9223372036854775808)",
        "(/ 1/9223372036854775807 2)",
        "(+ 9223372036854775807 1/2)",
        "(* 9223372036854775807 9223372036854775807)",
        "(quot 9223372036854775807 1/9223372036854775807)",
    ] {
        assert_eq!(native_error(text), N::Arithmetic(A::Overflow), "{text}");
    }
    for text in [
        "(long 9223372036854775808.0)",
        "(long -9223372036854777856.0)",
        "(char -1)",
        "(char 65536)",
        "(float ##Inf)",
    ] {
        assert_eq!(
            native_error(text),
            N::Arithmetic(A::ConversionRange),
            "{text}"
        );
    }
    for text in ["(/ 0)", "(/ 1 0)", "(rem 1 0)", "(mod 0.0 -0.0)"] {
        assert_eq!(native_error(text), N::Arithmetic(A::DivideByZero), "{text}");
    }
    assert_eq!(
        eval("(/ -9223372036854775808 -9223372036854775808)"),
        ValueView::Integer(1)
    );
    assert_eq!(eval("(rem -9223372036854775808 -1)"), ValueView::Integer(0));
    assert_eq!(
        eval("(* 9223372036854775807/2 2/9223372036854775807)"),
        ValueView::Integer(1)
    );
    assert_eq!(
        eval("(long -9223372036854775808.0)"),
        ValueView::Integer(i64::MIN)
    );
    assert_eq!(eval("(byte ##NaN)"), ValueView::Integer(0));
    assert_eq!(eval("(char ##NaN)"), ValueView::Character(0));
    for name in ["bigint", "biginteger", "bigdec"] {
        assert_eq!(
            native_error(&format!("({name} 1)")),
            N::UnsupportedNumber(match name {
                "bigint" => "bigint",
                "biginteger" => "biginteger",
                _ => "bigdec",
            })
        );
    }
    for source in ["1N", "1M", "9223372036854775808"] {
        assert_eq!(
            Context::new().eval_string(source).unwrap_err().phase,
            EvalPhase::Read
        );
    }
}

#[test]
fn floating_bits_and_declared_representation_differences_are_explicit() {
    for (text, bits) in [
        ("(- 0.0)", (-0.0f64).to_bits()),
        ("(+ -0.0)", (-0.0f64).to_bits()),
        ("(* -0.0)", (-0.0f64).to_bits()),
        ("(quot -0.0 3)", 0),
        ("(rem -4.0 2)", 0),
        ("(rem -0.0 3)", (-0.0f64).to_bits()),
        ("(double 2/3)", 0x3fe5555555555556),
        (
            "(double 2469135780246915/2000000000000000)",
            1.234567890123458f64.to_bits(),
        ),
        ("(double (float 1/3))", 0x3fd5555560000000),
    ] {
        let ValueView::Double(value) = eval(text) else {
            panic!("expected double: {text}")
        };
        assert_eq!(value.to_bits(), bits, "{text}");
    }
    assert_eq!(eval("(double? (float 1))"), ValueView::Boolean(true));
    assert_eq!(eval("(identical? 1000 1000)"), ValueView::Boolean(true));
    assert_eq!(eval("(identical? 0.0 -0.0)"), ValueView::Boolean(false));
    assert_eq!(eval("(= ##NaN ##NaN)"), ValueView::Boolean(false));
    assert_eq!(eval("(= 0.0 -0.0)"), ValueView::Boolean(true));
    for text in ["(* 1e308 2.0)", "(/ ##Inf 2)"] {
        assert_eq!(eval(text), ValueView::Double(f64::INFINITY));
    }
}

#[test]
fn metadata_registry_is_explicit_and_arity_errors_keep_language_provenance() {
    let mut context = Context::new();
    let mut names = std::collections::HashSet::new();
    for definition in builtin_definitions() {
        let metadata = definition.metadata;
        assert!(names.insert(metadata.qualified_name()));
        assert_eq!(
            metadata.effects,
            if matches!(metadata.name, "=" | "not=") {
                NativeEffects::Context
            } else {
                NativeEffects::Pure
            }
        );
        assert!(metadata.errors.contains(&NativeErrorClass::Arity));
        if !metadata.arity.accepts(0) {
            assert_eq!(
                context.invoke_native(definition, &[]).unwrap_err(),
                N::Arity {
                    expected: metadata.arity,
                    actual: 0
                }
            );
        }
    }
    assert!(names.contains(&Name::new(Some("clojure.core"), "int?")));
    // Pinned SCI returns [true false] for these symbol category checks.
    assert_eq!(eval("(ifn? 'x)"), ValueView::Boolean(true));
    assert_eq!(eval("(fn? 'x)"), ValueView::Boolean(false));
    let error = context.eval_string("(clojure.core/inc 1 2)").unwrap_err();
    assert_eq!(error.span.text(), "(clojure.core/inc 1 2)");
    assert!(
        matches!(error.kind, E::Native { name, error: N::Arity { actual: 2, .. } } if *name == Name::new(Some("clojure.core"), "inc"))
    );
    for source in [
        "(std/process-exit 0)",
        "(System/exit 0)",
        "(js/Math.abs -1)",
        "(user/+ 1 2)",
        "(missing 1)",
    ] {
        assert!(
            matches!(
                context.eval_string(source).unwrap_err().kind,
                E::UnresolvedSymbol(_)
            ),
            "{source}"
        );
    }
    assert_eq!(
        context
            .eval_string("(let* [+ -] (+ 5 2))")
            .unwrap()
            .inspect(),
        ValueView::Integer(3)
    );
    assert_eq!(
        context
            .eval_string("(let* [+ -] (clojure.core/+ 5 2))")
            .unwrap()
            .inspect(),
        ValueView::Integer(7)
    );
}

static REENTER: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "reenter",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[NativeErrorClass::Host],
    },
    callback: |context, args| {
        let answer = context
            .eval_string("(+ 40 2)")
            .map_err(|error| N::Host(error.to_string()))?;
        context.collect_garbage();
        context
            .vector(&[args[0].clone(), answer])
            .map_err(|error| N::Host(error.to_string()))
    },
};
static FOREIGN: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "foreign",
        arity: Arity::Exact(0),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |_, _| Ok(Context::new().integer(1)),
};
static RESET: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "reset",
        arity: Arity::Exact(0),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |context, _| {
        context.reset();
        Ok(context.nil())
    },
};
static FAIL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "fail",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[NativeErrorClass::Host],
    },
    callback: |_, _| Err(N::Host("deliberate test failure".into())),
};

#[test]
fn host_callbacks_reenter_collect_reject_foreign_values_and_release_scratch() {
    let mut context = Context::new();
    context.register_native(&REENTER);
    let value = context
        .eval_string("(let* [x [\"retained\"]] (host/reenter x))")
        .unwrap();
    assert_eq!(
        value
            .nth(0)
            .unwrap()
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::String("retained".encode_utf16().collect::<Vec<_>>().into())
    );
    assert_eq!(
        value.nth(1).unwrap().unwrap().inspect(),
        ValueView::Integer(42)
    );
    let source = context.source("reuse.clj", "(host/reenter [\"again\"])");
    let form = context
        .reader(&source)
        .unwrap()
        .read_form()
        .unwrap()
        .unwrap();
    let analyzed = context.analyze_form(&form).unwrap();
    for _ in 0..3 {
        assert_eq!(context.eval_analyzed(&analyzed).unwrap().count(), Some(2));
    }
    static REPLACEMENT: NativeDefinition = NativeDefinition {
        metadata: REENTER.metadata,
        callback: |context, _| Ok(context.integer(99)),
    };
    context.register_native(&REPLACEMENT);
    assert_eq!(
        context.eval_string("(host/reenter 1)").unwrap().inspect(),
        ValueView::Integer(99)
    );
    assert_eq!(
        context.eval_analyzed(&analyzed).unwrap().inspect(),
        ValueView::Integer(99)
    );
    assert!(matches!(
        Context::new()
            .eval_string("(host/reenter 1)")
            .unwrap_err()
            .kind,
        E::UnresolvedSymbol(_)
    ));
    let callable = Context::new().native_function(&REENTER);
    assert_eq!(
        callable.inspect(),
        ValueView::NativeFunction(REENTER.metadata)
    );
    let functions = context
        .eval_string("(let* [f +] (= {f [f]} {+ [+]}))")
        .unwrap();
    assert_eq!(functions.inspect(), ValueView::Boolean(true));
    drop(functions);
    assert_eq!(
        context
            .invoke_native(&REENTER, &[Context::new().nil()])
            .unwrap_err(),
        N::WrongOwner
    );
    assert_eq!(
        context.invoke_native(&FOREIGN, &[]).unwrap_err(),
        N::WrongOwner
    );
    assert_eq!(
        context.invoke_native(&RESET, &[]).unwrap_err(),
        N::WrongOwner
    );
    assert_eq!(
        context.eval_analyzed(&analyzed).unwrap_err().kind,
        E::WrongOwner
    );
    context.register_native(&FAIL);
    context.eval_string("").unwrap();
    context.collect_garbage();
    let baseline = context.memory_stats();
    for _ in 0..30 {
        assert!(matches!(
            context
                .eval_string("(let* [x [\"temp\"]] (host/fail x))")
                .unwrap_err()
                .kind,
            E::Native {
                error: N::Host(_),
                ..
            }
        ));
        context.collect_garbage();
    }
    let stats = context.memory_stats();
    assert_eq!(
        (
            stats.heap_values,
            stats.names,
            stats.sources,
            stats.executable_units,
            stats.roots
        ),
        (
            baseline.heap_values,
            baseline.names,
            baseline.sources,
            baseline.executable_units,
            baseline.roots
        )
    );
    assert_eq!(
        (stats.namespaces, stats.vars),
        (baseline.namespaces, baseline.vars)
    );
    assert_eq!(
        context.eval_string("(+ 1 2)").unwrap().inspect(),
        ValueView::Integer(3)
    );
}

#[test]
fn failed_calls_still_evaluate_arguments_once_and_stop_at_first_argument_error() {
    let mut context = Context::new();
    context.register_native(&OBSERVE);
    TRACE.with(|trace| trace.borrow_mut().clear());
    let error = context
        .eval_string("(42 (observe 1) (observe 2))")
        .unwrap_err();
    assert_eq!(error.kind, E::NotCallable);
    TRACE.with(|trace| {
        assert_eq!(
            *trace.borrow(),
            [protocol::Value::Int(1), protocol::Value::Int(2)]
        )
    });
    TRACE.with(|trace| trace.borrow_mut().clear());
    context
        .eval_string("(+ (observe 1) (/ 1 0) (observe 3))")
        .unwrap_err();
    TRACE.with(|trace| assert_eq!(*trace.borrow(), [protocol::Value::Int(1)]));
    TRACE.with(|trace| trace.borrow_mut().clear());
    context.eval_string("(+ (observe 1) missing)").unwrap_err();
    TRACE.with(|trace| assert!(trace.borrow().is_empty()));
}
