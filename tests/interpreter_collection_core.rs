use light_cljs::interpreter::{
    Arity, CollectionError, Context, ErrorClass, EvalErrorKind as E, EvalPhase, ExceptionCategory,
    InvokeError, Name, NativeDefinition, NativeEffects, NativeError, NativeErrorClass,
    NativeMetadata, PollPoint, RootedValue, ValueView, collection_definitions,
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
fn collection_programs_match_all_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-coll-")) {
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
                    E::NotCallable
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
    assert_eq!(count, 159);
}

#[test]
fn every_registered_collection_name_has_checked_arity_and_owner_validation() {
    let mut c = Context::new();
    let foreign = Context::new();
    let mut names = std::collections::HashSet::new();
    for definition in collection_definitions() {
        let meta = definition.metadata;
        assert!(names.insert(meta.name));
        assert!(meta.errors.contains(&NativeErrorClass::Arity));
        let f = c.native_function(definition);
        for len in [0, 1, 2, 3, 4, 5] {
            if !meta.arity.accepts(len) {
                let args = vec![c.nil(); len];
                assert!(
                    matches!(c.invoke(&f, &args), Err(InvokeError::Dispatch(E::Native { error: NativeError::Arity { actual, .. }, .. })) if actual == len),
                    "{} {len}",
                    meta.name
                );
            }
        }
        assert!(matches!(
            c.invoke(&f, &[foreign.nil()]),
            Err(InvokeError::Dispatch(E::WrongOwner))
        ));
        assert_eq!(
            c.invoke_native(definition, &[foreign.nil()]).unwrap_err(),
            NativeError::WrongOwner
        );
        assert!(
            c.eval_string(&format!("(ifn? clojure.core/{})", meta.name))
                .unwrap()
                .value_eq(&c.boolean(true))
        );
    }
    assert_eq!(names.len(), 34);
    for name in [
        "list?",
        "vector?",
        "map?",
        "set?",
        "sequential?",
        "seq?",
        "coll?",
        "associative?",
        "counted?",
        "fn?",
        "ifn?",
    ] {
        for source in [format!("({name})"), format!("({name} nil nil)")] {
            assert!(matches!(
                c.eval_string(&source).unwrap_err().kind,
                E::Native {
                    error: NativeError::Arity { .. },
                    ..
                }
            ));
        }
    }
}

#[test]
fn collection_callable_abi_preserves_roots_ownership_and_vars() {
    let mut c = Context::new();
    for source in [":a", "'a", "{:a 1}", "#{:a}", "[1]"] {
        let f = c.eval_string(source).unwrap();
        assert!(matches!(
            c.invoke(&f, &[]),
            Err(InvokeError::Dispatch(E::Arity { .. }))
        ));
        let other = Context::new();
        assert!(matches!(
            c.invoke(&f, &[other.nil()]),
            Err(InvokeError::Dispatch(E::WrongOwner))
        ));
    }
    let result = c
        .eval_string("(def lookup {:a [\"retained\"]}) (#'lookup :a)")
        .unwrap();
    c.collect_garbage();
    c.reset();
    assert_eq!(
        wire(&result),
        protocol::Value::Vector(vec![protocol::Value::String(
            "retained".encode_utf16().collect()
        )])
    );
    for source in [
        "(nil (observe 1))",
        "(42 (observe 1))",
        "(\"x\" (observe 1))",
        "('() (observe 1))",
    ] {
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        assert_eq!(c.eval_string(source).unwrap_err().kind, E::NotCallable);
        TRACE.with(|t| assert_eq!(*t.borrow(), [protocol::Value::Int(1)]));
    }
}

static HOST_FAIL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "host-fail",
        arity: Arity::AtLeast(0),
        effects: NativeEffects::Host,
        errors: &[NativeErrorClass::Host],
    },
    callback: |_, _| Err(NativeError::Host("terminal host failure".into())),
};
static STOP: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "stop",
        arity: Arity::AtLeast(0),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        c.set_execution_poll(Some(|_: PollPoint| Err("stopped".into())));
        Ok(c.nil())
    },
};
#[test]
fn callbacks_forward_language_identity_source_frames_and_terminal_failures() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    c.register_native(&HOST_FAIL);
    c.register_native(&STOP);
    let good = c.eval_string("(let* [e (ex-info \"original\" {:a 1})] (try (update {} :a (fn* [_] (throw e))) (catch Exception x (identical? e x))))").unwrap();
    assert_eq!(good.inspect(), ValueView::Boolean(true));
    let error = c
        .eval_string("(update {} :a (fn* [x] (/ 1 0)))")
        .unwrap_err();
    assert_eq!(error.span.text(), "(/ 1 0)");
    assert!(
        error
            .frames
            .iter()
            .any(|frame| frame.span.text().starts_with("(update "))
    );
    assert_eq!(error.class(), ErrorClass::Language);
    for operation in [
        "(update {} :a host-fail)",
        "(update-in {} [:a :b] host-fail)",
        "(merge-with host-fail {:a 1} {:a 2})",
    ] {
        TRACE.with(|t| t.borrow_mut().clear());
        let error = c.eval_string(&format!("(try {operation} (catch Exception e (observe :caught)) (finally (observe :cleanup)))")).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Host);
        TRACE.with(|t| {
            assert_eq!(
                *t.borrow(),
                [protocol::Value::Keyword(
                    None,
                    "cleanup".encode_utf16().collect()
                )]
            )
        });
    }
    for operation in [
        "(update {} :a (fn* [x] (stop) (inc 1)))",
        "(merge-with (fn* [x y] (stop) (inc 1)) {:a 1} {:a 2})",
    ] {
        TRACE.with(|t| t.borrow_mut().clear());
        let error = c.eval_string(&format!("(try {operation} (catch Exception e (observe :caught)) (finally (observe :cleanup)))")).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control);
        c.set_execution_poll(None);
        TRACE.with(|t| {
            assert_eq!(
                *t.borrow(),
                [protocol::Value::Keyword(
                    None,
                    "cleanup".encode_utf16().collect()
                )]
            )
        });
    }
    assert_eq!(
        c.eval_string("(update {:a 1} :a inc)")
            .unwrap()
            .entries()
            .unwrap()[0]
            .1
            .inspect(),
        ValueView::Integer(2)
    );
}

#[test]
fn persistent_outputs_keep_metadata_and_callback_gc_roots_without_deep_copying() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let values = c.eval_string("(let* [v ^{:tag :kept} [\"shared\"] m ^{:tag :kept} {:a v} n (update m :a (fn* [old] (observe old) (conj old 2)))] [m n (meta n) (meta (empty n))])").unwrap();
    c.collect_garbage();
    let m = values.nth(0).unwrap().unwrap();
    let n = values.nth(1).unwrap().unwrap();
    let key = c.keyword(Name::new(None, "a"));
    let a = m.get(&key).unwrap().unwrap();
    let b = n.get(&key).unwrap().unwrap();
    assert_eq!(a.count(), Some(1));
    assert_eq!(b.count(), Some(2));
    assert!(
        a.nth(0)
            .unwrap()
            .unwrap()
            .same_identity(&b.nth(0).unwrap().unwrap())
    );
    assert!(
        values
            .nth(2)
            .unwrap()
            .unwrap()
            .value_eq(&values.nth(3).unwrap().unwrap())
    );
    let nested = c
        .eval_string("(loop* [i 0 v nil] (if (< i 1200) (recur (inc i) (assoc-in v [:x] v)) v))")
        .unwrap();
    let update = c.eval_string("update-in").unwrap();
    let path = c.vector(&vec![key; 1200]).unwrap();
    let identity = c.eval_string("identity").unwrap();
    // Deep paths are iterative and work even when all intermediate maps must be built.
    let value = c.invoke(&update, &[c.nil(), path, identity]).unwrap();
    assert_eq!(value.count(), Some(1));
    drop((nested, value));
    c.collect_garbage();
}

#[test]
fn deferred_projection_roots_and_checked_hashing_preserve_failures() {
    let mut c = Context::new();
    let keys = c.eval_string("(keys [(find {:a 1} :a) 42])").unwrap();
    c.collect_garbage();
    assert_eq!(keys.count(), Some(2));
    assert_eq!(
        keys.nth(0).unwrap().unwrap().inspect(),
        ValueView::Keyword(Name::new(None, "a"))
    );
    assert_eq!(keys.nth(1).unwrap_err(), CollectionError::ExpectedMapEntry);
    assert_eq!(
        keys.try_value_hash().unwrap_err(),
        CollectionError::ExpectedMapEntry
    );
    let expected = c
        .vector(&[c.keyword(Name::new(None, "a")), c.nil()])
        .unwrap();
    assert_eq!(
        keys.try_value_eq(&expected).unwrap_err(),
        CollectionError::ExpectedMapEntry
    );
    assert!(c.map(&[(keys.clone(), c.nil())]).is_err());
    assert!(c.set(std::slice::from_ref(&keys)).is_err());
    assert!(c.assoc(&c.map(&[]).unwrap(), &keys, &c.nil()).is_err());
    assert!(c.conj(&c.set(&[]).unwrap(), &keys).is_err());
    let map = c.map(&[(c.integer(1), c.integer(2))]).unwrap();
    assert!(map.get(&keys).is_err());
    // Total eager-value helpers retain their invariant even when failures are
    // nested in distinct containers sharing the same deferred sequence.
    let a = c.vector(std::slice::from_ref(&keys)).unwrap();
    let b = c.vector(std::slice::from_ref(&keys)).unwrap();
    assert!(a.value_eq(&b));
    assert_eq!(a.value_hash(), b.value_hash());
    c.reset();
    assert_eq!(keys.nth(1).unwrap_err(), CollectionError::ExpectedMapEntry);
    assert_eq!(
        keys.nth(0).unwrap().unwrap().inspect(),
        ValueView::Keyword(Name::new(None, "a"))
    );
}

#[test]
fn reset_during_a_failing_higher_order_callback_is_an_owner_failure() {
    static RESET_FAIL: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "user",
            name: "reset-fail",
            arity: Arity::AtLeast(0),
            effects: NativeEffects::Host,
            errors: &[NativeErrorClass::Host],
        },
        callback: |c, _| {
            c.reset();
            Err(NativeError::Host("failure after reset".into()))
        },
    };
    let mut c = Context::new();
    c.register_native(&RESET_FAIL);
    let error = c.eval_string("(update {} :a reset-fail)").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    assert!(matches!(
        error.kind,
        E::Native {
            error: NativeError::WrongOwner,
            ..
        }
    ));
    assert_eq!(
        c.eval_string("(+ 1 2)").unwrap().inspect(),
        ValueView::Integer(3)
    );
}

#[test]
fn sequence_metadata_and_entry_conversion_match_pinned_type_distinctions() {
    let mut c = Context::new();
    let metadata = c.eval_string("{:x 1}").unwrap();
    let empty = c.eval_string("empty").unwrap();
    let conj = c.eval_string("conj").unwrap();
    for source in [
        "(list* 1 [2])",
        "(list* [1])",
        "(keys {:a 1})",
        "(vals {:a 1})",
        "(list* \"a\")",
        "(list* #{1})",
    ] {
        let original = c.eval_string(source).unwrap();
        let marked = c.with_meta(&original, &metadata).unwrap();
        assert!(
            c.invoke(&empty, std::slice::from_ref(&marked))
                .unwrap()
                .metadata()
                .is_none(),
            "{source}"
        );
        assert!(
            c.invoke(&conj, &[marked, c.integer(3)])
                .unwrap()
                .metadata()
                .is_none(),
            "{source}"
        );
    }
    let list = c.eval_string("'(1)").unwrap();
    let marked = c.with_meta(&list, &metadata).unwrap();
    assert!(
        c.invoke(&empty, &[marked])
            .unwrap()
            .metadata()
            .unwrap()
            .value_eq(&metadata)
    );
    let entry = c.eval_string("(find {:a 1} :a)").unwrap();
    assert_eq!(
        c.with_meta(&entry, &metadata).unwrap_err(),
        CollectionError::UnsupportedMetadata
    );
}
