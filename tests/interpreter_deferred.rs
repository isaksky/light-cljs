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
fn deferred_programs_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for case in cases.iter().filter(|c| c.id.starts_with("sci-deferred-")) {
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
    assert_eq!(count, 74);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn deferred_smoke() {
    let mut c = Context::new();
    for source in [
        "(= [(force (delay 3)) (realized? (delay 1))] [3 false])",
        "(let [s (lazy-seq [1 2])] (= [(realized? s) (first s) (realized? s) (count s) (vec s)] [false 1 true 2 [1 2]]))",
        "(letfn [(ints [n] (lazy-seq (cons n (ints (inc n)))))] (= (nth (ints 0) 1000) 1000))",
        "(letfn [(ints [n] (lazy-seq (cons n (ints (inc n)))))] (= (reduce (fn [s n] (if (= n 1000) (reduced s) (+ s n))) 0 (ints 0)) 499500))",
        "(let [s (lazy-seq (throw (ex-info \"bad\" {})))] (= [(try (first s) (catch Exception e :bad)) (realized? s) (seq s) (realized? s)] [:bad false nil true]))",
        "(let [i (lazy-seq [1]) o (lazy-seq i)] (= [(first o) (realized? o) (realized? i) (first i) (realized? i)] [1 true false 1 true]))",
        "(def s (lazy-seq s)) (= [(seq s) (realized? s)] [nil true])",
        "(def s (lazy-seq (first s) [1])) (= (first s) 1)",
        "(let [s (lazy-seq [1 2])] (= s [1 2]))",
        "(let [s (lazy-seq [1 2])] (= {[1 2] :yes} (hash-map s :yes)))",
        "(letfn [(ints [n] (lazy-seq (cons n (ints (inc n)))))] (= (first (concat [] (ints 3) [])) 3))",
    ] {
        assert_eq!(
            c.eval_string(source)
                .unwrap_or_else(|e| panic!("{source}: {e}"))
                .inspect(),
            ValueView::Boolean(true),
            "{source}"
        );
    }
}

#[test]
fn deferred_values_own_captures_code_and_cached_failures_without_root_cycles() {
    use light_cljs::interpreter::{DeferredStatus, ErrorClass, InvokeError, Name};
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let bundle = {
        let source = String::from(
            "(let [captured [\"kept\"]] [(delay (observe :delay) captured) (lazy-seq (observe :lazy) (list captured))])",
        );
        c.eval_string(&source).unwrap()
    };
    let delay = bundle.nth(0).unwrap().unwrap();
    let lazy = bundle.nth(1).unwrap().unwrap();
    drop(bundle);
    c.collect_garbage();
    assert_eq!(delay.deferred_status(), Some(DeferredStatus::Unrealized));
    assert_eq!(lazy.deferred_status(), Some(DeferredStatus::Unrealized));
    assert_eq!(
        lazy.elements().unwrap_err(),
        CollectionError::RequiresRealization
    );
    assert_eq!(
        lazy.try_value_eq(&c.list(&[]).unwrap()).unwrap_err(),
        CollectionError::RequiresRealization
    );
    let kept = c.force(&delay).unwrap();
    let values = c.sequence_elements(&lazy).unwrap();
    assert!(values[0].same_identity(&kept));
    assert!(c.force(&delay).unwrap().same_identity(&kept));
    c.collect_garbage();
    assert_eq!(lazy.deferred_status(), Some(DeferredStatus::Realized));
    assert_eq!(c.memory_stats().executable_units, 0);
    assert_eq!(lazy.elements().unwrap()[0].count(), Some(1));
    let foreign = Context::new().nil();
    assert_eq!(c.force(&foreign).unwrap_err(), NativeError::WrongOwner);
    assert_eq!(c.seq(&foreign).unwrap_err(), NativeError::WrongOwner);
    assert_eq!(
        c.sequence_elements(&foreign).unwrap_err(),
        NativeError::WrongOwner
    );
    c.reset();
    assert_eq!(c.force(&delay).unwrap_err(), NativeError::WrongOwner);
    assert_eq!(lazy.elements().unwrap()[0].count(), Some(1));
    drop(c);
    assert_eq!(
        kept.nth(0).unwrap().unwrap().inspect(),
        ValueView::String("kept".encode_utf16().collect())
    );

    let mut c = Context::new();
    c.eval_string("(def d (delay (throw (ex-info \"cached\" {:cycle d})))) (try (force d) (catch Exception e nil))").unwrap();
    c.collect_garbage();
    let d = c.eval_string("d").unwrap();
    let error = c.force(&d).unwrap_err();
    let NativeError::Invocation(error) = error else {
        panic!("forwarded error")
    };
    assert_eq!(error.error().class(), ErrorClass::Language);
    let InvokeError::Execution(error) = error.error() else {
        panic!("body provenance")
    };
    assert_eq!(error.span.text(), "(throw (ex-info \"cached\" {:cycle d}))");
    assert!(
        error
            .exception
            .as_ref()
            .unwrap()
            .exception_data()
            .unwrap()
            .get(&c.keyword(Name::new(None, "cycle")))
            .unwrap()
            .unwrap()
            .same_identity(&d)
    );
    // Remove both external handles and the Var; the cached error is raw traced data.
    drop(d);
    // The error reference above borrows the forwarded owning error; end its scope below.
}

#[test]
fn unreachable_deferred_success_failure_and_return_cycles_are_collected() {
    let mut c = Context::new();
    // Materialize builtin metadata before measuring reclaimed user cycles.
    c.eval_string("(ns-map 'clojure.core) (delay nil) (lazy-seq nil)")
        .unwrap();
    c.collect_garbage();
    let baseline = c.memory_stats();
    for source in [
        "(def d (delay d)) (force d) (ns-unmap 'user 'd)",
        "(def d (delay (throw (ex-info \"cached\" {:cycle d})))) (try (force d) (catch Exception e nil)) (ns-unmap 'user 'd)",
        "(def s (lazy-seq s)) (seq s) (ns-unmap 'user 's)",
        "(def s (lazy-seq (cons s nil))) (seq s) (ns-unmap 'user 's)",
        "(def s (lazy-seq (throw (ex-info \"bad\" {:cycle s})))) (try (seq s) (catch Exception e nil)) (ns-unmap 'user 's)",
    ] {
        c.eval_string(source).unwrap();
        c.collect_garbage();
        assert_eq!(
            c.memory_stats().heap_values,
            baseline.heap_values,
            "{source}"
        );
        assert_eq!(c.memory_stats().executable_units, 0, "{source}");
        assert_eq!(c.memory_stats().sources, 0, "{source}");
    }
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
        let ValueView::Integer(n) = args[0].inspect() else {
            unreachable!()
        };
        if n % 5000 == 0 {
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
fn walking_and_discarding_infinite_prefixes_has_bounded_live_storage_and_stack() {
    let mut c = Context::new();
    c.register_native(&SAMPLE);
    c.eval_string("(ns-map 'clojure.core) (letfn [(ints [n] (lazy-seq nil))] (loop [n 0] n))")
        .unwrap();
    c.collect_garbage();
    let baseline = c.memory_stats();
    SAMPLES.with(|s| s.borrow_mut().clear());
    let result = c.eval_string("(letfn [(ints [n] (lazy-seq (cons [n (vector n)] (ints (inc n)))))] (loop [xs (ints 0) n 0] (if (= n 50000) (first (first xs)) (do (sample n) (recur (next xs) (inc n))))))").unwrap();
    assert_eq!(result.inspect(), ValueView::Integer(50000));
    SAMPLES.with(|s| {
        let s = s.borrow();
        assert_eq!(s.len(), 10);
        let stable = &s[2..];
        assert!(stable.iter().all(|(stack, _)| *stack == stable[0].0));
        assert!(
            stable
                .iter()
                .all(|(_, m)| m.heap_values == stable[0].1.heap_values),
            "{s:?}"
        );
        assert!(
            stable.iter().all(|(_, m)| m.roots == stable[0].1.roots),
            "{s:?}"
        );
        assert!(
            stable
                .iter()
                .all(|(_, m)| m.executable_units == stable[0].1.executable_units)
        );
        assert!(stable[0].1.heap_values < baseline.heap_values + 100);
    });
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, baseline.heap_values);
}

#[test]
fn macroexpanded_lazy_bodies_retry_failures_and_release_successful_captures() {
    use light_cljs::interpreter::{DeferredStatus, Name};
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let quoted = c
        .eval_string("'(lazy-seq (observe :attempt) (throw (ex-info \"retry\" {})))")
        .unwrap();
    let expanded = c.macroexpand(&quoted, None).unwrap();
    let source = c.source("macroexpanded.clj", "(lazy-seq ...)");
    let form = c
        .form_from_data(&expanded, &source.span(0..14).unwrap())
        .unwrap();
    let lazy = c.eval_form(&form).unwrap();
    TRACE.with(|t| t.borrow_mut().clear());
    for _ in 0..2 {
        assert!(c.seq(&lazy).is_err());
        assert_eq!(lazy.deferred_status(), Some(DeferredStatus::Failed));
        c.collect_garbage();
    }
    TRACE.with(|t| {
        assert_eq!(
            *t.borrow(),
            vec![protocol::Value::Keyword(None, "attempt".encode_utf16().collect()); 2]
        )
    });
    c.eval_string("(def good false)").unwrap();
    let quoted = c
        .eval_string("'(lazy-seq (if good [42] (throw (ex-info \"retry\" {}))))")
        .unwrap();
    let expanded = c.macroexpand(&quoted, None).unwrap();
    let form = c
        .form_from_data(&expanded, &source.span(0..14).unwrap())
        .unwrap();
    let lazy = c.eval_form(&form).unwrap();
    assert!(c.seq(&lazy).is_err());
    let var = c.resolve_var(&Name::new(None, "good")).unwrap().unwrap();
    c.set_var_root(var.var_id().unwrap(), &c.boolean(true))
        .unwrap();
    assert_eq!(
        c.sequence_elements(&lazy).unwrap()[0].inspect(),
        ValueView::Integer(42)
    );
    assert_eq!(lazy.deferred_status(), Some(DeferredStatus::Realized));
}

thread_local! { static TOKEN: RefCell<Option<light_cljs::interpreter::CancellationToken>> = const { RefCell::new(None) }; }
static CANCEL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "cancel",
        arity: Arity::Exact(0),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        TOKEN.with(|t| t.borrow().as_ref().unwrap().cancel());
        c.set_cancellation_token(None);
        Ok(c.nil())
    },
};
#[test]
fn cancellation_unwinds_cleanup_and_preserves_the_reference_failure_state() {
    use light_cljs::interpreter::{CancellationToken, DeferredStatus, ErrorClass};
    for (name, demand) in [("delay", "force"), ("lazy-seq", "seq")] {
        let mut c = Context::new();
        c.register_native(&CANCEL);
        c.register_native(&OBSERVE);
        c.eval_string(&format!("(def pending ({name} (try (cancel) (catch Throwable e (observe :caught)) (finally (observe :cleanup))) [1]))")).unwrap();
        let pending = c.eval_string("pending").unwrap();
        let token = CancellationToken::default();
        TOKEN.with(|t| *t.borrow_mut() = Some(token.clone()));
        c.set_cancellation_token(Some(token));
        TRACE.with(|t| t.borrow_mut().clear());
        let error = c
            .eval_string(&format!(
                "(try ({demand} pending) (catch Throwable e (observe :outer-caught)))"
            ))
            .unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control);
        assert_eq!(pending.deferred_status(), Some(DeferredStatus::Failed));
        TRACE.with(|t| {
            assert_eq!(
                *t.borrow(),
                vec![protocol::Value::Keyword(
                    None,
                    "cleanup".encode_utf16().collect()
                )]
            )
        });
        c.collect_garbage();
        if name == "delay" {
            assert_eq!(
                c.eval_string("(realized? pending)").unwrap().inspect(),
                ValueView::Boolean(true)
            );
            assert_eq!(
                c.eval_string("(force pending)").unwrap_err().class(),
                ErrorClass::Control
            );
        } else {
            assert_eq!(
                c.eval_string("(realized? pending)").unwrap().inspect(),
                ValueView::Boolean(false)
            );
            assert_eq!(c.seq(&pending).unwrap().inspect(), ValueView::Nil);
            assert_eq!(pending.deferred_status(), Some(DeferredStatus::Realized));
        }
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
}

#[test]
fn infinite_realization_and_recursive_force_obey_shared_limits() {
    use light_cljs::interpreter::{DeferredStatus, ErrorClass, ResourceLimits};
    for source in [
        "(letfn [(ints [n] (lazy-seq (cons n (ints (inc n)))))] (count (ints 0)))",
        "(letfn [(wrappers [] (lazy-seq (wrappers)))] (first (wrappers)))",
        "(force (delay (loop [] (recur))))",
    ] {
        let mut c = Context::new();
        let text = c.source("deferred-budget.clj", source);
        let form = c.reader(&text).unwrap().read_form().unwrap().unwrap();
        let code = c.analyze_form(&form).unwrap();
        c.set_resource_limits(ResourceLimits {
            fuel: Some(4000),
            ..ResourceLimits::default()
        });
        assert_eq!(
            c.eval_analyzed(&code).unwrap_err().class(),
            ErrorClass::Control,
            "{source}"
        );
        c.set_resource_limits(ResourceLimits::default());
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
    let mut c = Context::new();
    c.eval_string("(def d (delay (force d)))").unwrap();
    c.set_resource_limits(ResourceLimits {
        call_depth: 8,
        ..ResourceLimits::default()
    });
    assert_eq!(
        c.eval_string("(force d)").unwrap_err().class(),
        ErrorClass::Control
    );
    c.set_resource_limits(ResourceLimits::default());
    let d = c.eval_string("d").unwrap();
    assert_eq!(d.deferred_status(), Some(DeferredStatus::Failed));
    assert!(c.force(&d).is_err());
}

#[test]
fn direct_embedding_force_reports_exhaustion_while_caching_failure_diagnostics() {
    use light_cljs::interpreter::{ErrorClass, ResourceLimits};
    let mut c = Context::new();
    let d = c.eval_string("(delay (/ 1 0))").unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(0),
        ..ResourceLimits::default()
    });
    let error = c.force(&d).unwrap_err();
    let NativeError::Invocation(error) = error else {
        panic!("terminal error")
    };
    assert_eq!(error.error().class(), ErrorClass::Control);
    c.set_resource_limits(ResourceLimits::default());
    assert!(c.force(&d).is_err());
}

#[test]
fn cached_embedding_results_and_diagnostics_check_root_allocation_limits() {
    use light_cljs::interpreter::{ErrorClass, ResourceLimits};
    let mut c = Context::new();
    let d = c.eval_string("(delay 42)").unwrap();
    let s = c.eval_string("(lazy-seq [42])").unwrap();
    let failed = c
        .eval_string("(delay (throw (ex-info \"cached\" {})))")
        .unwrap();
    c.force(&d).unwrap();
    c.seq(&s).unwrap();
    assert!(c.force(&failed).is_err());
    c.set_resource_limits(ResourceLimits {
        allocation: Some(0),
        ..ResourceLimits::default()
    });
    for result in [c.force(&d), c.seq(&s), c.force(&failed)] {
        let NativeError::Invocation(error) = result.unwrap_err() else {
            panic!("terminal allocation error")
        };
        assert_eq!(error.error().class(), ErrorClass::Control);
    }
    c.set_resource_limits(ResourceLimits::default());
    assert_eq!(c.force(&d).unwrap().inspect(), ValueView::Integer(42));
    assert_eq!(
        c.sequence_elements(&s).unwrap()[0].inspect(),
        ValueView::Integer(42)
    );
}
