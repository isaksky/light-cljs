use light_cljs::interpreter::{
    Arity, CollectionError, Context, ErrorClass, EvalErrorKind as E, EvalPhase, ExceptionCategory,
    InvokeError, Name, NativeDefinition, NativeEffects, NativeError, NativeErrorClass,
    NativeMetadata, PollPoint, RootedValue, ValueView, sequence_definitions,
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

#[test]
fn sequence_programs_match_all_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-seq-")) {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let actual = match c.eval_string(case.source) {
            Ok(v) => O::Returned(wire(&v)),
            Err(e) => {
                let phase = match e.phase {
                    EvalPhase::Execution => P::Execution,
                    EvalPhase::Analysis => P::Analysis,
                    _ => panic!("{}: {e}", case.id),
                };
                let category = match e.kind {
                    E::InvalidForm(_) => C::InvalidForm,
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
    assert_eq!(count, 108);
}

#[test]
fn sequence_registry_checks_arity_owner_and_lazy_names() {
    let mut c = Context::new();
    let foreign = Context::new();
    let mut names = std::collections::HashSet::new();
    for definition in sequence_definitions() {
        let meta = definition.metadata;
        assert!(names.insert(meta.name));
        assert!(meta.errors.contains(&NativeErrorClass::Arity));
        for count in 0..6 {
            if !meta.arity.accepts(count) {
                let result = c.invoke_native(definition, &vec![c.nil(); count]);
                assert!(
                    matches!(result, Err(NativeError::Arity { actual, .. }) if actual == count)
                );
            }
        }
        assert_eq!(
            c.invoke_native(definition, &[foreign.nil()]).unwrap_err(),
            NativeError::WrongOwner
        );
        assert_eq!(
            c.eval_string(&format!("(ifn? clojure.core/{})", meta.name))
                .unwrap()
                .inspect(),
            ValueView::Boolean(true)
        );
    }
    assert_eq!(names.len(), 25);
    for name in [
        "map", "filter", "range", "iterate", "repeat", "take", "drop",
    ] {
        assert!(
            matches!(
                c.eval_string(name).unwrap().inspect(),
                ValueView::NativeFunction(_)
            ),
            "{name}"
        );
    }
}

#[test]
fn tails_markers_and_combinators_keep_values_and_code_alive() {
    let mut c = Context::new();
    let bundle = c.eval_string("(let* [x [\"kept\"]] [(reduced x) (constantly x) (partial vector x) (comp (fn* [v] [x v]) identity) (juxt (constantly x) identity) (complement (constantly x)) (rest (seq [0 x])) (rest (keys [9 (find {:a x} :a)]))])").unwrap();
    c.collect_garbage();
    let marker = bundle.nth(0).unwrap().unwrap();
    let retained = marker.reduced_value().unwrap();
    assert!(retained.same_identity(&c.invoke(&bundle.nth(1).unwrap().unwrap(), &[]).unwrap()));
    for index in 2..6 {
        c.invoke(&bundle.nth(index).unwrap().unwrap(), &[c.integer(7)])
            .unwrap();
        c.collect_garbage();
    }
    assert!(
        bundle
            .nth(6)
            .unwrap()
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .same_identity(&retained)
    );
    assert_eq!(
        bundle
            .nth(7)
            .unwrap()
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Keyword(Name::new(None, "a"))
    );
    let foreign = Context::new();
    assert_eq!(
        foreign.reduced(&marker).unwrap_err(),
        NativeError::WrongOwner
    );
    drop(bundle);
    c.reset();
    assert_eq!(
        marker
            .reduced_value()
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::String("kept".encode_utf16().collect())
    );
    assert!(matches!(
        c.invoke(&retained, &[]),
        Err(InvokeError::Dispatch(E::WrongOwner))
    ));
    drop(c);
    assert_eq!(marker.reduced_value().unwrap().count(), Some(1));
}

#[test]
fn unrooted_combinator_var_cycles_are_reclaimed() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    c.eval_string("(def f (comp identity (fn* [] f))) (ns-unmap 'user 'f)")
        .unwrap();
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, baseline_heap);
    assert_eq!(c.memory_stats().executable_units, 0);
}

thread_local! {
    static SAMPLES: RefCell<Vec<(usize, light_cljs::interpreter::MemoryStats)>> = const { RefCell::new(Vec::new()) };
    static ITERATIONS: RefCell<usize> = const { RefCell::new(0) };
}
static PROBE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "probe",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, args| {
        let ValueView::Integer(n) = args[0].inspect() else {
            unreachable!()
        };
        if n % 10000 == 0 {
            let stack_marker = 0u8;
            c.collect_garbage();
            SAMPLES.with(|s| {
                s.borrow_mut()
                    .push((&stack_marker as *const u8 as usize, c.memory_stats()))
            });
        }
        Ok(args[0].clone())
    },
};

#[test]
fn large_flat_reduction_reuses_stack_and_live_lexical_storage() {
    let mut c = Context::new();
    c.register_native(&PROBE);
    let input = c
        .vector(&(0..100000).map(|i| c.integer(i)).collect::<Vec<_>>())
        .unwrap();
    let reducer = c.eval_string("(fn* [a x] (probe x) (+ a x))").unwrap();
    let reduce = c.eval_string("reduce").unwrap();
    SAMPLES.with(|s| s.borrow_mut().clear());
    let result = c
        .invoke(&reduce, &[reducer, c.integer(0), input.clone()])
        .unwrap();
    assert_eq!(result.inspect(), ValueView::Integer(4_999_950_000));
    SAMPLES.with(|s| {
        let s = s.borrow();
        assert_eq!(s.len(), 10);
        let stable = &s[2..];
        assert!(stable.iter().all(|(stack, _)| *stack == stable[0].0));
        assert!(
            stable
                .iter()
                .all(|(_, stats)| stats.heap_values == stable[0].1.heap_values)
        );
        assert!(
            stable
                .iter()
                .all(|(_, stats)| stats.roots == stable[0].1.roots)
        );
        assert!(
            stable
                .iter()
                .all(|(_, stats)| stats.executable_units == stable[0].1.executable_units)
        );
    });
    let mapv = c.eval_string("mapv").unwrap();
    let identity = c.eval_string("identity").unwrap();
    assert_eq!(
        c.invoke(&mapv, &[identity, input.clone()]).unwrap().count(),
        Some(100000)
    );
    let traverse = c
        .eval_string(
            "(fn* [input] (loop* [xs (seq input) n 0] (if xs (recur (next xs) (inc n)) n)))",
        )
        .unwrap();
    assert_eq!(
        c.invoke(&traverse, &[input]).unwrap().inspect(),
        ValueView::Integer(100000)
    );
}

#[test]
fn iteration_polling_is_terminal_and_allows_finally_cleanup() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    for expression in [
        "(into [] [1 2 3 4])",
        "(reduce + [1 2 3 4])",
        "(reduce-kv + 0 [1 2 3 4])",
        "(mapv identity [1 2 3 4])",
        "(filterv identity [1 2 3 4])",
        "(some (constantly false) [1 2 3 4])",
        "(every? identity [1 2 3 4])",
        "(not-any? (constantly false) [1 2 3 4])",
        "(not-every? identity [1 2 3 4])",
        "(apply vector [1 2 3 4])",
        "((comp inc inc inc inc) 0)",
        "((juxt identity identity identity identity) 0)",
    ] {
        ITERATIONS.with(|n| *n.borrow_mut() = 0);
        TRACE.with(|t| t.borrow_mut().clear());
        c.set_execution_poll(Some(|point| {
            if point == PollPoint::Iteration {
                ITERATIONS.with(|n| {
                    *n.borrow_mut() += 1;
                    if *n.borrow() == 3 {
                        Err("iteration stop".into())
                    } else {
                        Ok(())
                    }
                })
            } else {
                Ok(())
            }
        }));
        let source =
            format!("(try {expression} (catch Exception e :caught) (finally (observe :cleanup)))");
        let error = c.eval_string(&source).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{expression}");
        assert_eq!(
            error.kind,
            E::ExecutionInterrupted("iteration stop".into()),
            "{expression}"
        );
        TRACE.with(|t| {
            assert_eq!(
                *t.borrow(),
                [protocol::Value::Keyword(
                    None,
                    "cleanup".encode_utf16().collect()
                )]
            )
        });
        c.set_execution_poll(None);
        c.collect_garbage();
    }
}

#[test]
fn applied_variadic_closure_keeps_deferred_tail_after_return_and_gc() {
    let mut c = Context::new();
    let f = c.eval_string("(apply (fn* [a & xs] (fn* [] [a (first (rest xs))])) 3 (keys [9 (find {:kept 7} :kept)]))").unwrap();
    c.collect_garbage();
    let value = c.invoke(&f, &[]).unwrap();
    assert!(value.value_eq(&c.eval_string("[3 :kept]").unwrap()));
}

#[test]
fn nested_native_apply_is_bounded_without_host_stack_growth() {
    let mut c = Context::new();
    let apply = c.eval_string("apply").unwrap();
    let plus = c.eval_string("+").unwrap();
    let mut arguments = c
        .list(&[plus, c.vector(&[c.integer(1), c.integer(2)]).unwrap()])
        .unwrap();
    for _ in 0..2000 {
        arguments = c.list(&[apply.clone(), arguments]).unwrap();
    }
    assert!(matches!(
        c.invoke(&apply, &[apply.clone(), arguments]),
        Err(InvokeError::Dispatch(E::CallDepthExceeded))
    ));
    assert_eq!(
        c.eval_string("(+ 1 2)").unwrap().inspect(),
        ValueView::Integer(3)
    );
    c.collect_garbage();
}

#[test]
fn deferred_capture_concatenations_survive_collection() {
    let mut c = Context::new();
    let f = c
        .eval_string("(apply partial (fn* [& xs] xs) 1 2 3 (keys [(find {:kept 9} :kept)]))")
        .unwrap();
    c.collect_garbage();
    let args = c
        .invoke(&f, &[c.string("tail".encode_utf16().collect::<Vec<_>>())])
        .unwrap();
    drop(f);
    c.collect_garbage();
    assert_eq!(args.count(), Some(5));
    assert_eq!(
        args.nth(3).unwrap().unwrap().inspect(),
        ValueView::Keyword(Name::new(None, "kept"))
    );
    assert_eq!(
        args.nth(4).unwrap().unwrap().inspect(),
        ValueView::String("tail".encode_utf16().collect())
    );
    c.reset();
    assert_eq!(
        args.nth(4).unwrap().unwrap().inspect(),
        ValueView::String("tail".encode_utf16().collect())
    );
}
