use light_cljs::interpreter::{
    Arity, Context, ErrorClass, EvalErrorKind as E, EvalPhase, ExceptionCategory, Name,
    NativeDefinition, NativeEffects, NativeError, NativeMetadata, Origin, PollPoint, RootedValue,
    ValueView,
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
fn exceptions_execute_all_focused_pinned_sci_outcomes() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases
        .iter()
        .filter(|c| c.id.starts_with("sci-exception-") || c.id == "sci-throw-error")
    {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let actual = match c.eval_string(case.source) {
            Ok(v) => O::Returned(wire(&v)),
            Err(e) => {
                let phase = match e.phase {
                    EvalPhase::Analysis => P::Analysis,
                    EvalPhase::Execution => P::Execution,
                    _ => panic!("{}: {e}", case.id),
                };
                let category = match e.kind {
                    E::UnresolvedSymbol(_) => C::UnresolvedSymbol,
                    E::InvalidForm(_) => C::InvalidForm,
                    E::Thrown(ExceptionCategory::ExceptionInfo) => C::ExceptionInfo,
                    E::Native {
                        error: NativeError::Type { .. },
                        ..
                    } => C::Type,
                    E::Native {
                        error: NativeError::Arity { .. },
                        ..
                    } => C::Arity,
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
    assert_eq!(count, 47);
}

#[test]
fn portable_categories_and_aliases_are_not_java_class_access() {
    let mut c = Context::new();
    for (source, expected) in [
        (
            "(try (throw (Throwable. \"root\")) (catch Exception e :wrong) (catch Throwable e (ex-message e)))",
            ValueView::String("root".encode_utf16().collect::<Vec<_>>().into()),
        ),
        (
            "(try (throw (ex-info \"x\" {})) (catch ExceptionInfo e 42))",
            ValueView::Integer(42),
        ),
        (
            "(try (throw (java.lang.Exception. \"x\")) (catch java.lang.Throwable e 42))",
            ValueView::Integer(42),
        ),
        (
            "(try (throw (java.lang.Throwable.)) (catch java.lang.Exception e :wrong) (catch Throwable e (ex-message e)))",
            ValueView::Nil,
        ),
    ] {
        assert_eq!(c.eval_string(source).unwrap().inspect(), expected);
    }
    for source in [
        "Exception",
        "Throwable",
        "(new Exception \"x\")",
        "(.getMessage (Exception. \"x\"))",
        "(try 1 (catch RuntimeException e 2))",
        "(try 1 (catch :default e 2))",
        "(try 1 (catch Exception :e 2))",
    ] {
        assert_eq!(
            c.eval_string(source).unwrap_err().class(),
            ErrorClass::Diagnostic,
            "{source}"
        );
    }
}

#[test]
fn errors_retain_sources_frames_and_generated_origins_without_user_data_overrides() {
    let error = {
        let mut c = Context::new();
        let source = c.source(
            "library.clj",
            "(def inner (fn* [x] (/ x 0)))\n(def outer (fn* [] (inner 1)))",
        );
        c.eval_source(&source).unwrap();
        let call = c.source("entry.clj", "\n(outer)");
        c.eval_source(&call).unwrap_err()
    };
    assert_eq!(error.class(), ErrorClass::Language);
    assert_eq!(error.span.text(), "(/ x 0)");
    assert_eq!(error.span.source().name(), "library.clj");
    assert_eq!(
        error
            .frames
            .iter()
            .map(|f| f.span.text())
            .collect::<Vec<_>>(),
        ["(/ x 0)", "(inner 1)", "(outer)"]
    );
    assert!(error.to_string().contains("entry.clj:2:1"));
    let mut c = Context::new();
    let source = c.source(
        "actual.clj",
        "\n(throw (ex-info \"bad\" {:line 999 :column 888 :file \"pretend\" :phase \"parse\"}))",
    );
    let error = c.eval_source(&source).unwrap_err();
    assert_eq!(error.phase, EvalPhase::Execution);
    assert_eq!(error.span.source().name(), "actual.clj");
    assert_eq!(
        error.span.source().line_column(error.span.bytes().start),
        Some((2, 1))
    );
    assert!(error.to_string().contains("bad"));
    let data = error.exception.unwrap().exception_data().unwrap();
    assert_eq!(
        data.get(&c.keyword(Name::new(None, "line")))
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Integer(999)
    );

    let source = c.source("macro-origin.clj", "(source-macro)");
    let quote = c
        .eval_string("'((fn* [] (throw (ex-info \"expanded\" {}))))")
        .unwrap();
    let form = c
        .form_from_data(&quote, &source.span(0..source.text().len()).unwrap())
        .unwrap();
    let error = c.eval_form(&form).unwrap_err();
    assert_eq!(error.origin, Origin::SuppliedData);
    assert_eq!(error.span.text(), "(source-macro)");
    assert_eq!(error.frames.len(), 1);
    assert_eq!(error.frames[0].origin, Origin::SuppliedData);
    assert_eq!(error.frames[0].span.source().name(), "macro-origin.clj");
}

#[test]
fn caught_values_causes_and_closures_remain_rooted_during_collection_and_after_reset() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let closure = c.eval_string("(try (throw (ex-info \"outer\" {:value [1 2]} (ex-info \"inner\" {:nested 3}))) (catch Exception e (observe :gc) (fn* [] e)))").unwrap();
    c.collect_garbage();
    let exception = c.invoke(&closure, &[]).unwrap();
    let data = exception.exception_data().unwrap();
    let cause = exception.exception_cause().unwrap();
    assert_eq!(
        exception.inspect(),
        ValueView::Exception(ExceptionCategory::ExceptionInfo)
    );
    assert!(exception.value_eq(&exception.clone()));
    assert_eq!(exception.value_hash(), exception.clone().value_hash());
    let other = c
        .eval_string("(ex-info \"outer\" {:value [1 2]} (ex-info \"inner\" {:nested 3}))")
        .unwrap();
    assert!(!exception.value_eq(&other));
    c.reset();
    drop(c);
    assert_eq!(data.count(), Some(1));
    assert_eq!(cause.exception_data().unwrap().count(), Some(1));
    assert_eq!(
        exception.exception_message().unwrap().inspect(),
        ValueView::String("outer".encode_utf16().collect::<Vec<_>>().into())
    );
}

static HOST_FAIL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "host-fail",
        arity: Arity::Exact(0),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |_, _| Err(NativeError::Host("host failed".into())),
};
fn interrupt(point: PollPoint) -> Result<(), String> {
    if point == PollPoint::Recur {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}
#[test]
fn host_failures_cancellation_and_depth_limits_cannot_be_caught_or_masked() {
    let mut c = Context::new();
    c.register_native(&HOST_FAIL);
    c.register_native(&OBSERVE);
    for (source, class) in [
        (
            "(try (host-fail) (catch Throwable e (observe :wrong)) (finally (observe :finally) (throw (ex-info \"mask\" {}))))",
            ErrorClass::Host,
        ),
        (
            "(try ((fn* f [] (f))) (catch Throwable e (observe :wrong)) (finally (observe :finally) (throw (ex-info \"mask\" {}))))",
            ErrorClass::Control,
        ),
        (
            "(try (loop* [x 0] (recur (inc x))) (catch Throwable e (observe :wrong)) (finally (observe :finally) (throw (ex-info \"mask\" {}))))",
            ErrorClass::Control,
        ),
    ] {
        c.set_execution_poll(Some(interrupt));
        TRACE.with(|t| t.borrow_mut().clear());
        let e = c.eval_string(source).unwrap_err();
        assert_eq!(e.class(), class, "{e}");
        TRACE.with(|t| {
            assert_eq!(
                t.borrow().as_slice(),
                [protocol::Value::Keyword(
                    None,
                    "finally".encode_utf16().collect()
                )]
            )
        });
        c.set_execution_poll(None);
        assert_eq!(
            c.eval_string("(+ 1 2)").unwrap().inspect(),
            ValueView::Integer(3)
        );
    }
}

#[test]
fn caught_failures_do_not_accumulate_temporary_arguments_or_partial_bindings() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    // Each iteration catches both a partially evaluated call and binding vector.
    // Collection during the handler exercises the restored roots immediately.
    let value = c.eval_string("(loop* [i 0] (if (= i 500) i (do (try (+ [i] (/ 1 0)) (catch Exception e (observe :gc))) (try (let* [x [i] y (/ 1 0)] x) (catch Exception e (observe :gc))) (recur (inc i)))))").unwrap();
    assert_eq!(value.inspect(), ValueView::Integer(500));
    drop(value);
    c.collect_garbage();
    let baseline = c.memory_stats();
    for _ in 0..10 {
        c.eval_string("(try (let* [a [1 2] b (/ 1 0)] a) (catch Exception e nil))")
            .unwrap();
        c.collect_garbage();
        assert_eq!(c.memory_stats(), baseline);
    }
    let foreign = Context::new();
    assert_eq!(
        c.exception(
            ExceptionCategory::Exception,
            &foreign.nil(),
            &c.nil(),
            &c.nil()
        )
        .unwrap_err(),
        NativeError::WrongOwner
    );
}

#[test]
fn terminal_cleanup_is_bounded_even_when_the_original_poll_is_sticky() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    c.register_native(&HOST_FAIL);
    c.set_execution_poll(Some(|_| Err("sticky cancellation".into())));
    TRACE.with(|t| t.borrow_mut().clear());
    let error = c
        .eval_string("(try (inc 0) (finally (observe :cleanup) (loop* [] (recur))))")
        .unwrap_err();
    assert_eq!(
        error.kind,
        E::ExecutionInterrupted("sticky cancellation".into())
    );
    TRACE.with(|t| assert_eq!(t.borrow().len(), 1));
    c.set_execution_poll(None);
    let error = c
        .eval_string("(try (host-fail) (finally (loop* [] (recur))))")
        .unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    // Nested finalizers share the allowance; exhaustion cannot be caught.
    let error = c.eval_string("(try (host-fail) (finally (try (loop* [] (recur)) (catch Throwable e (observe :wrong)) (finally (loop* [] (recur))))))").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    assert_eq!(
        c.eval_string("42").unwrap().inspect(),
        ValueView::Integer(42)
    );
}
