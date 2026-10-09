use light_cljs::interpreter::{
    Arity, Context, EvalErrorKind as E, EvalPhase, InvokeError, NativeDefinition, NativeEffects,
    NativeError, NativeMetadata, RootedValue, ValueView,
};
use std::cell::RefCell;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn wire(value: &RootedValue) -> protocol::Value {
    use protocol::Value as V;
    match value.inspect() {
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

// SCI supplies letfn as a macro backed by mutable local Vars, not a letfn*
// special form. These explicit primitive adaptations preserve named functions,
// binding scopes and calls while requiring no macro/Var implementation.
fn native_source<'a>(id: &str, source: &'a str) -> &'a str {
    include_str!("reference/sci/native-letfn.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .find(|(name, _)| *name == id)
        .map_or(source, |(_, native)| native)
}

#[test]
fn closures_execute_all_focused_pinned_sci_outcomes() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases
        .iter()
        .filter(|c| c.id.starts_with("sci-fn-") || c.id.starts_with("sci-letfn-"))
    {
        let mut context = Context::new();
        context.register_native(&OBSERVE);
        TRACE.with(|trace| trace.borrow_mut().clear());
        let outcome = match context.eval_string(native_source(case.id, case.source)) {
            Ok(value) => O::Returned(wire(&value)),
            Err(error) => {
                let (phase, category) = match error.kind {
                    E::InvalidForm(_) => (P::Analysis, C::InvalidForm),
                    E::UnresolvedSymbol(_) => (P::Analysis, C::UnresolvedSymbol),
                    E::Arity { .. } => (P::Execution, C::Arity),
                    _ => panic!("{}: {error}", case.id),
                };
                assert_eq!(format!("{:?}", error.phase), format!("{phase:?}"));
                O::Error { phase, category }
            }
        };
        assert_eq!(outcome, case.outcome, "{}: {}", case.id, case.source);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), case.effects, "{} effects", case.id));
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 42, "all new closure reference cases are executed");
}

fn integer(value: RootedValue) -> i64 {
    let ValueView::Integer(value) = value.inspect() else {
        panic!("not an integer: {value:?}")
    };
    value
}

#[test]
fn returned_closures_keep_owned_code_captures_and_sources_without_reanalysis() {
    let mut c = Context::new();
    let function = {
        let source = c.source("retained.clj", "(let* [x 40] (fn* [y] (+ x y)))");
        c.eval_source(&source).unwrap()
    };
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 1);
    assert_eq!(c.memory_stats().sources, 1);
    let before = c.memory_stats();
    for _ in 0..100 {
        let result = c.invoke(&function, &[c.integer(2)]).unwrap();
        assert_eq!(integer(result), 42);
        c.collect_garbage();
    }
    assert_eq!(c.memory_stats(), before);
    let function_id = function.heap_id().unwrap();
    drop(function);
    c.collect_garbage();
    assert!(c.heap_value(function_id).is_err());
    assert_eq!(c.memory_stats().executable_units, 0);
    assert_eq!(c.memory_stats().sources, 0);
}

#[test]
fn mutually_recursive_and_self_returning_closures_survive_frames_and_collect_cycles() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    let f = c.eval_string("(let* [x 37] (letfn* [f (fn* [n] (if (zero? n) g (g (dec n)))) g (fn* [n] (if (zero? n) x (f (dec n))))] f))").unwrap();
    c.collect_garbage();
    let g = c.invoke(&f, &[c.integer(0)]).unwrap();
    assert_eq!(integer(c.invoke(&g, &[c.integer(0)]).unwrap()), 37);
    drop(f);
    c.collect_garbage();
    assert_eq!(integer(c.invoke(&g, &[c.integer(2)]).unwrap()), 37);
    drop(g);
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, baseline_heap);
    assert_eq!(c.memory_stats().executable_units, 0);
    let f = c.eval_string("(fn* self [] self)").unwrap();
    let same = c.invoke(&f, &[]).unwrap();
    assert!(f.same_identity(&same));
    assert!(f.value_eq(&same));
    assert_eq!(f.value_hash(), same.value_hash());
    let other = c.eval_string("(fn* self [] self)").unwrap();
    assert!(!f.value_eq(&other));
    drop(c);
    assert_eq!(same.inspect(), ValueView::Function);
}

#[test]
fn simultaneous_factory_calls_capture_values_not_reused_local_slots() {
    let mut c = Context::new();
    let factory = c
        .eval_string("(fn* [x] (fn* named ([] (named 1)) ([y] (+ x y))))")
        .unwrap();
    let a = c.invoke(&factory, &[c.integer(10)]).unwrap();
    let b = c.invoke(&factory, &[c.integer(20)]).unwrap();
    drop(factory);
    c.collect_garbage();
    for _ in 0..10 {
        assert_eq!(integer(c.invoke(&a, &[]).unwrap()), 11);
        assert_eq!(integer(c.invoke(&b, &[]).unwrap()), 21);
    }
    let nested = c
        .eval_string("((fn* [x] (let* [x (fn* [] x)] (fn* [] (x)))) 73)")
        .unwrap();
    c.collect_garbage();
    assert_eq!(integer(c.invoke(&nested, &[]).unwrap()), 73);
}

#[test]
fn callable_abi_checks_owner_arity_and_preserves_body_error_source() {
    let mut c = Context::new();
    let f = c.eval_string("(fn* ([x] x) ([x y & xs] xs))").unwrap();
    assert!(matches!(
        c.invoke(&f, &[]),
        Err(InvokeError::Dispatch(E::Arity { actual: 0, .. }))
    ));
    let other = Context::new();
    assert!(matches!(
        c.invoke(&f, &[other.integer(1)]),
        Err(InvokeError::Dispatch(E::WrongOwner))
    ));
    assert!(matches!(
        c.invoke(&other.nil(), &[]),
        Err(InvokeError::Dispatch(E::WrongOwner))
    ));
    assert!(matches!(
        c.invoke(&c.integer(1), &[]),
        Err(InvokeError::Dispatch(E::NotCallable))
    ));
    let source = c.source("failure.clj", "(fn* [] (/ 1 0))");
    let f = c.eval_source(&source).unwrap();
    drop(source);
    c.collect_garbage();
    let InvokeError::Execution(error) = c.invoke(&f, &[]).unwrap_err() else {
        panic!("body error")
    };
    assert_eq!(error.span.source().name(), "failure.clj");
    assert_eq!(error.span.text(), "(/ 1 0)");
    assert_eq!(error.phase, EvalPhase::Execution);
    c.reset();
    assert!(matches!(
        c.invoke(&f, &[]),
        Err(InvokeError::Dispatch(E::WrongOwner))
    ));
    drop(c);
    assert_eq!(error.span.text(), "(/ 1 0)");
}

#[test]
fn callbacks_can_collect_and_invoke_interpreted_arguments_with_no_storage_borrow() {
    static CALL: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "user",
            name: "call",
            arity: Arity::Exact(2),
            effects: NativeEffects::Host,
            errors: &[],
        },
        callback: |context, args| {
            context.collect_garbage();
            context
                .invoke(&args[0], &args[1..])
                .map_err(|e| NativeError::Host(e.to_string()))
        },
    };
    let mut c = Context::new();
    c.register_native(&CALL);
    let result = c
        .eval_string("(let* [x 10 f (fn* [y] (+ x y))] [(call f 2) (f 3)])")
        .unwrap();
    assert_eq!(wire(&result).wire(), "(vector (int 12) (int 13))");
    let native = c.native_function(&CALL);
    let f = c.eval_string("(fn* [x] (* 2 x))").unwrap();
    assert_eq!(integer(c.invoke(&native, &[f, c.integer(8)]).unwrap()), 16);
}

#[test]
fn wide_calls_noncallables_and_failure_cleanup_are_explicit() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    let params = (0..300)
        .map(|i| format!("x{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let f = c.eval_string(&format!("(fn* [{params}] x299)")).unwrap();
    let args: Vec<_> = (0..300).map(|i| c.integer(i)).collect();
    assert_eq!(integer(c.invoke(&f, &args).unwrap()), 299);
    assert!(matches!(
        c.invoke(&f, &args[..299]),
        Err(InvokeError::Dispatch(E::Arity { actual: 299, .. }))
    ));
    drop(f);
    drop(args);
    c.register_native(&OBSERVE);
    TRACE.with(|trace| trace.borrow_mut().clear());
    let error = c
        .eval_string("((do (observe :callee) 42) (observe :a) (observe :b))")
        .unwrap_err();
    assert_eq!(error.kind, E::NotCallable);
    TRACE.with(|trace| assert_eq!(trace.borrow().len(), 3));
    TRACE.with(|trace| trace.borrow_mut().clear());
    c.eval_string("((fn* [a b] a) (/ 1 0) (observe :unreachable))")
        .unwrap_err();
    TRACE.with(|trace| assert!(trace.borrow().is_empty()));
    let error = c.eval_string("((fn* forever [] (forever)))").unwrap_err();
    assert_eq!(error.kind, E::CallDepthExceeded);
    drop(error);
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, baseline_heap);
    assert_eq!(c.memory_stats().executable_units, 0);
    assert_eq!(integer(c.eval_string("((fn* [x] x) 4)").unwrap()), 4);
}

#[test]
fn rest_is_a_counted_sequence_with_structural_equality_and_nil_at_minimum() {
    let mut c = Context::new();
    let value = c.eval_string("((fn* [x & more] more) 0 1 2)").unwrap();
    assert_eq!(value.inspect(), ValueView::Seq(2));
    assert_eq!(value.count(), Some(2));
    assert_eq!(integer(value.nth(1).unwrap().unwrap()), 2);
    let list = c.list(&[c.integer(1), c.integer(2)]).unwrap();
    assert!(value.value_eq(&list));
    assert_eq!(value.value_hash(), list.value_hash());
    assert_eq!(
        c.eval_string("((fn* [x & more] more) 0)")
            .unwrap()
            .inspect(),
        ValueView::Nil
    );
    let predicates = c.eval_string("((fn* [& xs] [(seq? xs) (list? xs) (sequential? xs) (counted? xs) (fn? (fn* [])) (ifn? (fn* []))]) 1)").unwrap();
    assert_eq!(
        wire(&predicates).wire(),
        "(vector (bool true) (bool false) (bool true) (bool true) (bool true) (bool true))"
    );
}

#[test]
fn deeply_nested_recursive_bodies_fail_before_exhausting_the_host_stack() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    let body = format!("{}(f){}", "(do ".repeat(30), ")".repeat(30));
    let error = c.eval_string(&format!("((fn* f [] {body}))")).unwrap_err();
    assert_eq!(error.kind, E::EvaluationDepthExceeded);
    drop(error);
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, baseline_heap);
    assert_eq!(c.memory_stats().executable_units, 0);
}

#[test]
fn malformed_letfn_and_destructuring_fail_during_analysis_without_effects() {
    let mut c = Context::new();
    for source in [
        "(letfn*)",
        "(letfn* ())",
        "(letfn* [f])",
        "(letfn* [1 (fn* [])])",
        "(letfn* [f 1])",
        "(letfn* [f (do (fn* []))])",
        "(fn* [[x]] x)",
    ] {
        let error = c.eval_string(source).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Analysis, "{source}");
        assert!(matches!(error.kind, E::InvalidForm(_)), "{source}: {error}");
    }
    assert_eq!(
        c.eval_string("(letfn* [])").unwrap().inspect(),
        ValueView::Nil
    );
}

#[test]
fn function_metadata_is_rejected_explicitly_instead_of_dropping_expressions() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    TRACE.with(|trace| trace.borrow_mut().clear());
    for source in [
        "^{:x (observe 1)} (fn* [] 2)",
        "(letfn* [f ^{:x (observe 1)} (fn* [] 2)] (f))",
    ] {
        let error = c.eval_string(source).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Analysis);
        assert!(matches!(error.kind, E::UnsupportedForm(_)));
    }
    TRACE.with(|trace| assert!(trace.borrow().is_empty()));
}

#[test]
fn callbacks_reentering_interpreted_code_share_the_context_depth_bounds() {
    static CALL: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "user",
            name: "call",
            arity: Arity::Exact(2),
            effects: NativeEffects::Host,
            errors: &[],
        },
        callback: |context, args| {
            context
                .invoke(&args[0], &args[1..])
                .map_err(|error| NativeError::Host(error.to_string()))
        },
    };
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    c.register_native(&CALL);
    let error = c
        .eval_string("((fn* self [n] (if (zero? n) n (call self (dec n)))) 40)")
        .unwrap_err();
    assert!(
        error.to_string().contains("CallDepthExceeded")
            || error.to_string().contains("EvaluationDepthExceeded")
    );
    assert!(
        error.to_string().len() < 10_000,
        "nested host errors grow linearly"
    );
    drop(error);
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, baseline_heap);
    assert_eq!(c.memory_stats().executable_units, 0);
    assert_eq!(integer(c.eval_string("((fn* [x] x) 7)").unwrap()), 7);
}

#[test]
fn sequence_data_can_reenter_the_owned_form_boundary_without_printing() {
    let mut c = Context::new();
    let value = c.eval_string("((fn* [& xs] xs) + 1 2)").unwrap();
    assert_eq!(value.inspect(), ValueView::Seq(3));
    let source = c.source("host-form", "");
    let form = c
        .form_from_data(&value, &source.span(0..0).unwrap())
        .unwrap();
    assert_eq!(integer(c.eval_form(&form).unwrap()), 3);
}
