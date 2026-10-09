use light_cljs::interpreter::{
    Arity, Context, ErrorClass, EvalErrorKind, EvalPhase, HandleError, Name, NativeDefinition,
    NativeEffects, NativeError, NativeMetadata, ResourceLimits, RootedValue, ValueView,
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
fn state_matches_pinned_sci_values_and_effects() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-state-")) {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|trace| trace.borrow_mut().clear());
        match c.eval_string(case.source) {
            Ok(value) => {
                let actual = protocol::Outcome::Returned(wire(&value));
                if actual != case.outcome {
                    failures.push(format!("{}: {:?} != {:?}", case.id, actual, case.outcome));
                }
            }
            Err(error) => {
                if !matches!(
                    error.kind,
                    EvalErrorKind::Native {
                        error: NativeError::Arity { .. },
                        ..
                    }
                ) || case.outcome
                    != (protocol::Outcome::Error {
                        phase: protocol::Phase::Execution,
                        category: protocol::Category::Arity,
                    })
                    || error.phase != EvalPhase::Execution
                {
                    failures.push(format!("{}: {error}", case.id));
                }
            }
        }
        TRACE.with(|t| {
            if *t.borrow() != case.effects {
                failures.push(format!(
                    "{} effects: {:?} != {:?}",
                    case.id,
                    t.borrow(),
                    case.effects
                ));
            }
        });
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 94);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn core(c: &Context, name: &str) -> RootedValue {
    c.resolve_var(&Name::new(Some("clojure.core"), name))
        .unwrap()
        .unwrap()
}
fn call(
    c: &mut Context,
    name: &str,
    args: &[RootedValue],
) -> Result<RootedValue, light_cljs::interpreter::InvokeError> {
    let function = core(c, name);
    c.invoke(&function, args)
}
fn int(value: &RootedValue) -> i64 {
    let ValueView::Integer(n) = value.inspect() else {
        panic!("expected integer: {value:?}")
    };
    n
}

#[test]
fn host_cells_retain_snapshots_and_reject_foreign_owners() {
    let mut c = Context::new();
    let initial = c.vector(&[c.integer(1)]).unwrap();
    let atom = c.atom(&initial).unwrap();
    let volatile = c.volatile(&initial).unwrap();
    let old = atom.state_value().unwrap();
    let replacement = c.vector(&[c.integer(2)]).unwrap();
    call(&mut c, "reset!", &[atom.clone(), replacement.clone()]).unwrap();
    call(&mut c, "vreset!", &[volatile.clone(), replacement.clone()]).unwrap();
    drop(initial);
    c.collect_garbage();
    assert_eq!(int(&old.elements().unwrap()[0]), 1);
    assert!(atom.state_value().unwrap().same_identity(&replacement));
    assert!(volatile.state_value().unwrap().same_identity(&replacement));
    let foreign = Context::new();
    assert!(matches!(
        c.atom(&foreign.integer(1)),
        Err(NativeError::WrongOwner)
    ));
    assert!(matches!(
        c.volatile(&foreign.nil()),
        Err(NativeError::WrongOwner)
    ));
    assert_eq!(
        call(&mut c, "reset!", &[atom.clone(), foreign.nil()])
            .unwrap_err()
            .class(),
        ErrorClass::Host
    );
    assert!(atom.state_value().unwrap().same_identity(&replacement));
    c.reset();
    assert_eq!(
        call(&mut c, "reset!", &[atom.clone(), replacement.clone()])
            .unwrap_err()
            .class(),
        ErrorClass::Host
    );
    assert_eq!(
        c.heap_value(atom.heap_id().unwrap()).unwrap_err(),
        HandleError::WrongOwner
    );
    drop(c);
    assert!(atom.state_value().unwrap().same_identity(&replacement));
    assert!(volatile.state_value().unwrap().same_identity(&replacement));
    assert_eq!(int(&old.elements().unwrap()[0]), 1);
}

#[test]
fn atom_volatile_closure_metadata_validator_and_watch_cycles_are_collected() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline = c.memory_stats();
    for _ in 0..30 {
        let cells = c.eval_string("(let [a (atom nil) v (volatile! nil)] (reset! a (fn [] [a v])) (vreset! v [a v]) (reset-meta! a {:self a :volatile v}) (set-validator! a (fn [_] (identical? a a))) (add-watch a a (fn [_ _ _ _] [a v])) [a v])").unwrap();
        let a = cells.elements().unwrap()[0].clone();
        let id = a.heap_id().unwrap();
        drop(cells);
        c.collect_garbage();
        let closure = a.state_value().unwrap();
        let returned = c.invoke(&closure, &[]).unwrap();
        assert!(returned.elements().unwrap()[0].same_identity(&a));
        assert!(
            a.metadata()
                .unwrap()
                .get(&c.keyword(Name::new(None, "self")))
                .unwrap()
                .unwrap()
                .same_identity(&a)
        );
        drop(returned);
        drop(closure);
        drop(a);
        c.collect_garbage();
        assert_eq!(c.heap_value(id).unwrap_err(), HandleError::StaleId);
        let after = c.memory_stats();
        assert_eq!(after.heap_values, baseline.heap_values);
        assert_eq!(after.executable_units, baseline.executable_units);
        assert_eq!(after.roots, baseline.roots);
    }
}

#[test]
fn context_reset_detaches_live_cycles_without_leaking_them_into_new_state() {
    let mut c = Context::new();
    let a = c
        .eval_string("(def a (atom nil)) (reset! a a) (reset-meta! a {:self a}) a")
        .unwrap();
    c.collect_garbage();
    c.reset();
    assert!(c.eval_string("a").is_err());
    assert!(a.state_value().unwrap().same_identity(&a));
    assert!(matches!(a.metadata().unwrap().inspect(), ValueView::Map(1)));
    let independent = Context::new();
    let b = independent.atom(&independent.nil()).unwrap();
    assert!(!a.value_eq(&b));
    assert!(!a.same_identity(&b));
    drop(c);
    assert!(a.state_value().unwrap().same_identity(&a));
}

static RESET_CONTEXT: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "reset-context",
        arity: Arity::AtLeast(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |c, args| {
        if args.len() == 4 || args[0].inspect() == ValueView::Integer(1) {
            c.reset();
        }
        Ok(c.boolean(true))
    },
};
static FOREIGN: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "foreign",
        arity: Arity::AtLeast(0),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |_, _| Ok(Context::new().integer(9)),
};
#[test]
fn callback_context_reset_and_foreign_results_fail_without_borrow_panics_or_outer_commit() {
    for operation in [
        "(reset! a 1)",
        "(reset-vals! a 1)",
        "(swap! a inc)",
        "(swap-vals! a inc)",
        "(compare-and-set! a 0 1)",
    ] {
        let mut c = Context::new();
        c.register_native(&RESET_CONTEXT);
        let a = c
            .eval_string("(def a (atom 0 :validator reset-context)) a")
            .unwrap();
        let error = c.eval_string(operation).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Host, "{operation}: {error}");
        assert_eq!(int(&a.state_value().unwrap()), 0);
        assert!(c.eval_string("a").is_err());
    }
    let mut c = Context::new();
    c.register_native(&RESET_CONTEXT);
    let a = c
        .eval_string("(def a (atom 0)) (add-watch a :reset reset-context) a")
        .unwrap();
    assert_eq!(
        c.eval_string("(reset! a 1)").unwrap_err().class(),
        ErrorClass::Host
    );
    assert_eq!(
        int(&a.state_value().unwrap()),
        1,
        "watch failure follows commit"
    );
    for operation in [
        "(swap! a foreign)",
        "(alter-meta! a foreign)",
        "(set-validator! a foreign)",
    ] {
        let mut c = Context::new();
        c.register_native(&FOREIGN);
        let a = c.eval_string("(def a (atom 0)) a").unwrap();
        assert_eq!(
            c.eval_string(operation).unwrap_err().class(),
            ErrorClass::Host
        );
        assert_eq!(int(&a.state_value().unwrap()), 0);
        assert!(a.metadata().is_none());
    }
}

#[test]
fn callbacks_reenter_evaluation_with_gc_and_preserve_exception_sources() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let source = c.source("state-error.clj", "(def a (atom 1 :validator (fn [n] (observe n) (if (= n 1) true (throw (ex-info \"validation\" {:n n}))))))\n(reset! a 2)");
    let error = c.eval_source(&source).unwrap_err();
    assert_eq!(error.class(), ErrorClass::Language);
    assert!(error.span.text().contains("throw"));
    assert_eq!(error.span.source().name(), "state-error.clj");
    assert!(
        error
            .frames
            .iter()
            .any(|frame| frame.span.text() == "(reset! a 2)")
    );
    let exception = error.exception.unwrap();
    assert_eq!(
        exception
            .exception_data()
            .unwrap()
            .get(&c.keyword(Name::new(None, "n")))
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Integer(2)
    );
    assert_eq!(
        c.eval_string("@a").unwrap().inspect(),
        ValueView::Integer(1)
    );
    c.reset();
    assert!(matches!(exception.inspect(), ValueView::Exception(_)));
}

#[test]
fn repeated_swaps_release_obsolete_values_and_keep_identity_hash_stable() {
    let mut c = Context::new();
    let a = c.atom(&c.nil()).unwrap();
    let reset = core(&c, "reset!");
    let hash = a.value_hash();
    c.collect_garbage();
    let baseline = c.memory_stats();
    for n in 0..2000 {
        let value = c.vector(&[c.integer(n)]).unwrap();
        c.invoke(&reset, &[a.clone(), value]).unwrap();
        if n % 100 == 0 {
            c.collect_garbage();
            assert!(c.memory_stats().heap_values <= baseline.heap_values + 1);
            assert!(c.memory_stats().roots <= baseline.roots + 1);
        }
        assert_eq!(a.value_hash(), hash);
    }
    assert_eq!(int(&a.state_value().unwrap().elements().unwrap()[0]), 1999);
}

#[test]
fn fuel_allocation_and_depth_limits_cover_state_callbacks_and_retry_loops() {
    for (setup, operation, committed) in [
        (
            "(def a (atom 1 :validator (fn [x] (if (= x 1) true (loop [] (recur))))))",
            "(reset! a 2)",
            1,
        ),
        (
            "(def a (atom 1))",
            "(swap! a (fn [x] (reset! a (inc x)) x))",
            -1,
        ),
        (
            "(def a (atom 1)) (add-watch a :w (fn [& _] (loop [] (recur))))",
            "(reset! a 2)",
            2,
        ),
    ] {
        let mut c = Context::new();
        c.eval_string(setup).unwrap();
        let a = c.eval_string("a").unwrap();
        c.set_resource_limits(ResourceLimits {
            fuel: Some(5000),
            cleanup_fuel: 0,
            cleanup_allocation: 0,
            ..ResourceLimits::default()
        });
        let error = c.eval_string(operation).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{operation}: {error}");
        if committed >= 0 {
            assert_eq!(int(&a.state_value().unwrap()), committed);
        } else {
            assert!(int(&a.state_value().unwrap()) > 1);
        }
    }
    for limits in [
        ResourceLimits {
            allocation: Some(1200),
            ..ResourceLimits::default()
        },
        ResourceLimits {
            call_depth: 6,
            ..ResourceLimits::default()
        },
    ] {
        let mut c = Context::new();
        c.eval_string("(def a (atom 1)) (add-watch a :w (fn [_ r _ n] (reset! r (inc n))))")
            .unwrap();
        let a = c.eval_string("a").unwrap();
        c.set_resource_limits(limits);
        let error = c.eval_string("(reset! a 2)").unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{error}");
        assert!(int(&a.state_value().unwrap()) >= 2);
    }
}

#[test]
fn terminal_callback_failures_are_not_swallowed_by_language_catch() {
    let mut c = Context::new();
    c.eval_string("(def a (atom 1 :validator (fn [x] (if (= x 1) true (vec (repeat 1))))))")
        .unwrap();
    let a = c.eval_string("a").unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(1200),
        ..ResourceLimits::default()
    });
    let error = c
        .eval_string("(try (reset! a 2) (catch Throwable _ :caught))")
        .unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert_eq!(int(&a.state_value().unwrap()), 1);
}

#[test]
fn scalar_cas_uses_documented_representation_identity() {
    let mut c = Context::new();
    // JVM boxed scalar caching is not portable. Native immediates deliberately
    // compare their exact representation; distinct heaps still compare IDs.
    assert_eq!(wire(&c.eval_string("(let [a (atom 1000) b (atom 1.0) n (atom ##NaN)] [(compare-and-set! a 1000 2) (compare-and-set! b 1.0 2) (compare-and-set! n ##NaN 2)])").unwrap()), protocol::Value::Vector(vec![protocol::Value::Bool(true); 3]));
}

thread_local! {
    static CANCEL_TOKEN: RefCell<Option<light_cljs::interpreter::CancellationToken>> = const { RefCell::new(None) };
}
static CANCEL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "cancel",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |c, _| {
        CANCEL_TOKEN.with(|token| token.borrow().as_ref().unwrap().cancel());
        c.set_cancellation_token(None);
        Ok(c.boolean(true))
    },
};
#[test]
fn cancellation_during_validation_preserves_state_and_runs_bounded_cleanup() {
    let mut c = Context::new();
    c.register_native(&CANCEL);
    c.register_native(&OBSERVE);
    c.eval_string("(def a (atom 1 :validator (fn [n] (if (= n 1) true (cancel n)))))")
        .unwrap();
    let a = c.eval_string("a").unwrap();
    let token = light_cljs::interpreter::CancellationToken::default();
    CANCEL_TOKEN.with(|slot| *slot.borrow_mut() = Some(token.clone()));
    c.set_cancellation_token(Some(token));
    TRACE.with(|trace| trace.borrow_mut().clear());
    let error = c
        .eval_string(
            "(try (reset! a 2) (catch Throwable _ (observe :caught)) (finally (observe :cleanup)))",
        )
        .unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert_eq!(int(&a.state_value().unwrap()), 1);
    TRACE.with(|trace| {
        assert_eq!(
            *trace.borrow(),
            vec![protocol::Value::Keyword(
                None,
                "cleanup".encode_utf16().collect()
            )]
        )
    });
}

static REENTER: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "reenter",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |c, args| {
        c.collect_garbage();
        let increment = if int(&args[0]) == 1 {
            "(reset! a 5)"
        } else {
            "@a"
        };
        c.eval_string(increment)
            .map_err(|e| light_cljs::interpreter::InvokeError::Execution(Box::new(e)))?;
        c.collect_garbage();
        Ok(c.integer(int(&args[0]) + 1))
    },
};
#[test]
fn nested_host_evaluation_and_collection_share_the_same_cell_and_retry_scope() {
    let mut c = Context::new();
    c.register_native(&REENTER);
    assert_eq!(
        wire(
            &c.eval_string("(def a (atom 1)) [(swap-vals! a reenter) @a]")
                .unwrap()
        ),
        protocol::Value::Vector(vec![
            protocol::Value::Vector(vec![protocol::Value::Int(5), protocol::Value::Int(6)]),
            protocol::Value::Int(6)
        ])
    );
    let mut independent = Context::new();
    assert!(independent.eval_string("a").is_err());
    assert_eq!(
        independent.eval_string("@(atom 9)").unwrap().inspect(),
        ValueView::Integer(9)
    );
    assert_eq!(
        c.eval_string("@a").unwrap().inspect(),
        ValueView::Integer(6)
    );
}
