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
fn lazy_core_programs_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for case in cases.iter().filter(|c| c.id.starts_with("sci-lazy-")) {
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
    assert_eq!(count, 268);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn partition_all_collection_arities_keep_lazy_groups_and_fractional_counts() {
    let mut c = Context::new();
    assert_eq!(c.eval_string("(let [groups (partition-all 2.5 [1 2 3 4 5]) stepped (partition-all 2 1 [1 2 3])] [(mapv vec groups) (mapv vector? groups) (mapv vec stepped)])").map(|v| wire(&v).wire()).unwrap(),
        "(vector (vector (vector (int 1) (int 2) (int 3)) (vector (int 4) (int 5))) (vector (bool false) (bool false)) (vector (vector (int 1) (int 2)) (vector (int 2) (int 3)) (vector (int 3))))");
}

#[test]
fn lazy_registry_arity_and_context_ownership_are_checked() {
    use light_cljs::interpreter::{NativeErrorClass, lazy_definitions};
    let mut c = Context::new();
    let foreign = Context::new();
    let mut names = std::collections::HashSet::new();
    for definition in lazy_definitions() {
        assert!(names.insert(definition.metadata.name));
        assert!(
            definition
                .metadata
                .errors
                .contains(&NativeErrorClass::Arity)
        );
        for count in 0..7 {
            if !definition.metadata.arity.accepts(count) {
                assert!(
                    matches!(c.invoke_native(definition, &vec![c.nil(); count]), Err(NativeError::Arity { actual, .. }) if actual == count)
                );
            }
        }
        assert_eq!(
            c.invoke_native(definition, &[foreign.nil()]).unwrap_err(),
            NativeError::WrongOwner
        );
        assert_eq!(
            c.eval_string(&format!("(ifn? clojure.core/{})", definition.metadata.name))
                .unwrap()
                .inspect(),
            ValueView::Boolean(true)
        );
    }
    assert_eq!(names.len(), 34);
}

#[test]
fn infinite_sources_and_all_consumers_observe_the_shared_budget() {
    use light_cljs::interpreter::{ErrorClass, ResourceLimits};
    // Adapt host-seq-producers-test and host-materializers-test at the SCI pin:
    // Context fuel replaces the supplied interrupt callback; remaining cases
    // independently cover native producer, predicate and traversal loops.
    for source in [
        "(doall (range))",
        "(doall (repeat :x))",
        "(doall (cycle [1 2 3]))",
        "(doall (iterate inc 0))",
        "(reduce + (range))",
        "(count (range))",
        "(into [] (range))",
        "(dorun (range))",
        "(vec (range))",
        "(doall (repeatedly (constantly :x)))",
        "(last (range))",
        "(butlast (range))",
        "(first (filter (constantly false) (range)))",
        "(first (keep (constantly nil) (range)))",
        "(first (drop-while (constantly true) (range)))",
        "(first (drop ##Inf (range)))",
        "(doall (map inc (range)))",
        "(doall (map-indexed vector (range)))",
        "(doall (take ##Inf (range)))",
        "(doall (remove odd? (range)))",
        "(doall (keep-indexed vector (range)))",
        "(doall (take-while (constantly true) (range)))",
        "(doall (concat (range)))",
        "(doall (interpose :x (range)))",
        "(doall (interleave (range) (range)))",
        "(doall (partition 2 (range)))",
        "(doall (partition-all 2 (range)))",
        "(first (partition-all ##Inf 1 (range)))",
        "(doall (take-nth 2 (range)))",
        "(first (mapcat (constantly nil) (range)))",
        "(apply + (range))",
        "(= (range) (range))",
    ] {
        let mut c = Context::new();
        let text = c.source("lazy-budget.clj", source);
        let form = c.reader(&text).unwrap().read_form().unwrap().unwrap();
        let code = c.analyze_form(&form).unwrap();
        c.set_resource_limits(ResourceLimits {
            fuel: Some(3000),
            ..ResourceLimits::default()
        });
        let error = c.eval_analyzed(&code).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{source}: {error}");
        c.set_resource_limits(ResourceLimits::default());
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
    let mut c = Context::new();
    let source = c.eval_string("(map inc (range))").unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(0),
        ..ResourceLimits::default()
    });
    let NativeError::Invocation(error) = c.seq(&source).unwrap_err() else {
        panic!("allocation control error")
    };
    assert_eq!(error.error().class(), ErrorClass::Control);
    c.set_resource_limits(ResourceLimits::default());
    let nested = c
        .eval_string("(iterate (fn [x] (first (map inc [x]))) 0)")
        .unwrap();
    c.set_resource_limits(ResourceLimits {
        call_depth: 1,
        ..ResourceLimits::default()
    });
    let NativeError::Invocation(error) = c.sequence_elements(&nested).unwrap_err() else {
        panic!("call-depth control error")
    };
    assert_eq!(error.error().class(), ErrorClass::Control);
}

#[test]
fn producer_roots_code_and_failure_provenance_survive_gc() {
    use light_cljs::interpreter::{DeferredStatus, InvokeError};
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let retained = c
        .eval_string("(let [x [\"kept\"]] (take 3 (map (fn [n] (observe n) [x n]) (range))))")
        .unwrap();
    c.collect_garbage();
    TRACE.with(|t| t.borrow_mut().clear());
    let values = c.sequence_elements(&retained).unwrap();
    c.collect_garbage();
    assert_eq!(values.len(), 3);
    assert_eq!(retained.deferred_status(), Some(DeferredStatus::Realized));
    assert_eq!(c.sequence_elements(&retained).unwrap().len(), 3);
    TRACE.with(|t| assert_eq!(t.borrow().len(), 3));
    let bad = {
        let text = c.source(
            "lazy-callback.clj",
            "(map (fn [x] (throw (ex-info \"callback\" {:x x}))) (list 9))",
        );
        c.eval_source(&text).unwrap()
    };
    c.collect_garbage();
    let NativeError::Invocation(error) = c.seq(&bad).unwrap_err() else {
        panic!("callback error")
    };
    let InvokeError::Execution(error) = error.error() else {
        panic!("source-bearing execution error")
    };
    assert_eq!(error.span.source().name(), "lazy-callback.clj");
    assert!(error.span.text().contains("throw"));
    assert_eq!(c.seq(&bad).unwrap().inspect(), ValueView::Nil);
    let foreign = Context::new();
    assert_eq!(c.seq(&foreign.nil()).unwrap_err(), NativeError::WrongOwner);
    c.reset();
    drop(c);
    assert_eq!(retained.elements().unwrap().len(), 3);
    assert_eq!(
        values[0]
            .nth(0)
            .unwrap()
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::String("kept".encode_utf16().collect())
    );
}

thread_local! {
    static SAMPLES: RefCell<Vec<(usize, light_cljs::interpreter::MemoryStats)>> = const { RefCell::new(Vec::new()) };
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
fn walking_discarded_core_prefixes_keeps_live_storage_bounded() {
    let mut c = Context::new();
    c.register_native(&SAMPLE);
    SAMPLES.with(|s| s.borrow_mut().clear());
    let value = c.eval_string("(loop [xs (map inc (range)) n 0] (if (= n 20000) (first xs) (do (sample n) (recur (next xs) (inc n)))))").unwrap();
    assert_eq!(value.inspect(), ValueView::Integer(20001));
    SAMPLES.with(|s| {
        let s = s.borrow();
        assert_eq!(s.len(), 10);
        let stable = &s[2..];
        assert!(
            stable.iter().all(|(stack, m)| *stack == stable[0].0
                && m.heap_values == stable[0].1.heap_values
                && m.roots == stable[0].1.roots),
            "{s:?}"
        );
        assert!(stable[0].1.heap_values < 100, "{s:?}");
    });
}

#[test]
fn native_sequences_are_unchunked_and_checked_at_numeric_boundaries() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    TRACE.with(|t| t.borrow_mut().clear());
    assert_eq!(
        c.eval_string("(first (map observe [1 2 3]))")
            .unwrap()
            .inspect(),
        ValueView::Integer(1)
    );
    TRACE.with(|t| assert_eq!(*t.borrow(), vec![protocol::Value::Int(1)]));
    assert_eq!(
        c.eval_string("(let [s (range 3)] [(seq? s) (counted? s) (realized? s)])")
            .unwrap()
            .elements()
            .unwrap()
            .iter()
            .map(RootedValue::inspect)
            .collect::<Vec<_>>(),
        vec![
            ValueView::Boolean(true),
            ValueView::Boolean(false),
            ValueView::Boolean(false)
        ]
    );
    assert_eq!(
        c.eval_string(
            "(= (vec (range 9223372036854775806 9223372036854775807 2)) [9223372036854775806])"
        )
        .unwrap()
        .inspect(),
        ValueView::Boolean(true)
    );
    assert_eq!(
        c.eval_string(
            "(= (vec (range -9223372036854775807 -9223372036854775808 -2)) [-9223372036854775807])"
        )
        .unwrap()
        .inspect(),
        ValueView::Boolean(true)
    );
    assert!(
        c.eval_string("(nth (range 9223372036854775807 ##Inf 1) 1)")
            .is_err()
    );
}

thread_local! {
    static CANCEL_TOKEN: RefCell<Option<light_cljs::interpreter::CancellationToken>> = const { RefCell::new(None) };
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
            CANCEL_TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
            c.set_cancellation_token(None);
        }
        Ok(args[0].clone())
    },
};
#[test]
fn cancellation_inside_core_callbacks_is_terminal_and_runs_finally() {
    use light_cljs::interpreter::{CancellationToken, ErrorClass};
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    c.register_native(&CANCEL_AFTER);
    let token = CancellationToken::default();
    CANCEL_TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
    c.set_cancellation_token(Some(token));
    TRACE.with(|t| t.borrow_mut().clear());
    let error = c.eval_string("(try (doall (map cancel-after (range))) (catch Throwable e (observe :caught)) (finally (observe :cleanup)))").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
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
