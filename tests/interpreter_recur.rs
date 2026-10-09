use light_cljs::interpreter::{
    Arity, Context, EvalErrorKind as E, EvalPhase, InvokeError, NativeDefinition, NativeEffects,
    NativeError, NativeMetadata, PollPoint, RootedValue, ValueView,
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
fn recur_and_loops_execute_all_focused_pinned_sci_outcomes() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases
        .iter()
        .filter(|c| c.id.starts_with("sci-recur-") || c.id.starts_with("sci-loop-"))
    {
        let mut context = Context::new();
        context.register_native(&OBSERVE);
        TRACE.with(|trace| trace.borrow_mut().clear());
        if matches!(
            case.id,
            "sci-recur-if-test" | "sci-recur-callee" | "sci-recur-argument"
        ) {
            // The pinned SCI raw analyzer accepts recur in an if test/callee/argument. Our
            // required tail-position validation deliberately rejects it.
            assert_eq!(case.outcome, O::Returned(protocol::Value::Bool(true)));
            let error = context.eval_string(case.source).unwrap_err();
            assert_eq!(error.phase, EvalPhase::Analysis);
            assert_eq!(error.kind, E::InvalidForm("recur requires tail position"));
            count += 1;
            continue;
        }
        let outcome = match context.eval_string(case.source) {
            Ok(value) => O::Returned(wire(&value)),
            Err(error) => {
                let (phase, category) = match error.kind {
                    E::InvalidForm(_) => (P::Analysis, C::InvalidForm),
                    E::Native {
                        error: NativeError::Arithmetic(_),
                        ..
                    } => (P::Execution, C::Arithmetic),
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
    assert_eq!(count, 40);
}

fn integer(value: RootedValue) -> i64 {
    let ValueView::Integer(value) = value.inspect() else {
        panic!("not an integer")
    };
    value
}

thread_local! {
    static SAMPLES: RefCell<Vec<(usize, light_cljs::interpreter::MemoryStats)>> = const { RefCell::new(Vec::new()) };
    static POLLS: RefCell<(usize, usize)> = const { RefCell::new((0, 0)) };
}
static SAMPLE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "sample",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |context, args| {
        if integer(args[0].clone()) % 1000 == 0 {
            let marker = 0u8;
            context.collect_garbage();
            SAMPLES.with(|samples| {
                samples
                    .borrow_mut()
                    .push((&marker as *const u8 as usize, context.memory_stats()))
            });
        }
        Ok(context.nil())
    },
};

#[test]
fn large_loops_and_function_recur_reuse_stack_and_live_frame_storage() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    c.register_native(&SAMPLE);
    for source in [
        "(loop* [n 0] (sample n) (let* [temporary [n] discarded (fn* [] temporary)] (if (= n 100000) n (recur (inc n)))))",
        "((fn* [n] (sample n) (let* [temporary [n] discarded (fn* [] temporary)] (if (= n 100000) n (recur (inc n))))) 0)",
    ] {
        SAMPLES.with(|s| s.borrow_mut().clear());
        assert_eq!(integer(c.eval_string(source).unwrap()), 100000);
        SAMPLES.with(|s| {
            let samples = s.borrow();
            assert_eq!(samples.len(), 101);
            let addresses: Vec<_> = samples.iter().map(|(address, _)| *address).collect();
            assert!(
                addresses.iter().max().unwrap() - addresses.iter().min().unwrap() < 4096,
                "host stack must not grow with iterations"
            );
            let stable = &samples[2..];
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
                    .all(|(_, stats)| stats.slots == stable[0].1.slots)
            );
        });
        c.collect_garbage();
        assert_eq!(c.memory_stats().heap_values, baseline_heap);
        assert_eq!(c.memory_stats().executable_units, 0);
    }
}

#[test]
fn returned_function_recur_keeps_captures_and_analyzed_code() {
    let mut c = Context::new();
    let function = c
        .eval_string("(let* [step 3] (fn* [n] (if (> n 30) n (recur (+ n step)))))")
        .unwrap();
    c.collect_garbage();
    let before = c.memory_stats();
    for _ in 0..20 {
        assert_eq!(integer(c.invoke(&function, &[c.integer(0)]).unwrap()), 33);
        c.collect_garbage();
        assert_eq!(c.memory_stats(), before);
    }
}

#[test]
fn invalid_recur_preserves_precise_source_and_generated_form_origins() {
    let mut c = Context::new();
    for (source, span) in [
        ("(recur 1)", "(recur 1)"),
        ("(loop* [n 0] (identity (recur n)))", "(recur n)"),
        ("(fn* [x y] (recur x))", "(recur x)"),
        ("(loop* [x 1 y 2] (fn* [] (recur x y)))", "(recur x y)"),
        ("#(do (recur %) 1)", "(recur %)"),
    ] {
        let source = c.source("recur.clj", source);
        let error = c.eval_source(&source).unwrap_err();
        drop(source);
        c.collect_garbage();
        assert_eq!(error.phase, EvalPhase::Analysis);
        assert!(matches!(error.kind, E::InvalidForm(_)));
        assert_eq!(error.span.source().name(), "recur.clj");
        assert_eq!(error.span.text(), span);
    }
    let data = c.eval_string("'(fn* [x] (recur))").unwrap();
    let origin = c.source("macro-call.clj", "(generate-invalid)");
    let form = c
        .form_from_data(&data, &origin.span(0..origin.text().len()).unwrap())
        .unwrap();
    let error = c.eval_form(&form).unwrap_err();
    drop(form);
    drop(data);
    drop(origin);
    c.reset();
    assert_eq!(error.span.source().name(), "macro-call.clj");
    assert_eq!(error.span.text(), "(generate-invalid)");
    assert_eq!(error.phase, EvalPhase::Analysis);
}

#[test]
fn zero_argument_recur_polls_and_interrupts_with_original_backedge_location() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    c.set_execution_poll(Some(|point| {
        POLLS.with(|counts| {
            let mut counts = counts.borrow_mut();
            match point {
                PollPoint::Function => counts.0 += 1,
                PollPoint::Recur => counts.1 += 1,
                _ => {}
            }
            if counts.1 == 10000 {
                Err("requested stop".into())
            } else {
                Ok(())
            }
        })
    }));
    for (source, functions) in [("(loop* [] (recur))", 0), ("((fn* [] (recur)))", 1)] {
        POLLS.with(|counts| *counts.borrow_mut() = (0, 0));
        let error = c.eval_string(source).unwrap_err();
        assert_eq!(error.kind, E::ExecutionInterrupted("requested stop".into()));
        assert_eq!(error.phase, EvalPhase::Execution);
        assert_eq!(error.span.text(), "(recur)");
        POLLS.with(|counts| assert_eq!(*counts.borrow(), (functions, 10000)));
        drop(error);
        c.collect_garbage();
        assert_eq!(c.memory_stats().heap_values, baseline_heap);
        assert_eq!(c.memory_stats().executable_units, 0);
    }
    c.set_execution_poll(None);
    assert_eq!(integer(c.eval_string("((fn* [x] x) 8)").unwrap()), 8);
    c.set_execution_poll(Some(|_| Err("function stop".into())));
    assert!(matches!(
        c.invoke(&c.native_function(&OBSERVE), &[c.integer(1)]),
        Err(InvokeError::Dispatch(E::ExecutionInterrupted(_)))
    ));
    let error = c.eval_string("((fn* [] 7))").unwrap_err();
    assert_eq!(error.span.text(), "((fn* [] 7))");
    c.reset();
    assert_eq!(integer(c.eval_string("(inc 7)").unwrap()), 8);
}

#[test]
fn wide_recur_has_no_small_argument_limit_and_rest_slot_does_not_redispatch() {
    let mut c = Context::new();
    let params = (0..100)
        .map(|i| format!("x{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let args = (1..100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let replacements = (1..100)
        .map(|i| format!("x{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!(
        "((fn* [{params}] (if (zero? x0) x99 (recur (dec x0) {replacements}))) 500 {args})"
    );
    assert_eq!(integer(c.eval_string(&source).unwrap()), 99);
    assert_eq!(
        integer(
            c.eval_string("((fn* ([x y] 99) ([x & xs] (if (zero? x) xs (recur 0 42)))) 1)")
                .unwrap()
        ),
        42
    );
}

#[test]
fn malformed_loop_bindings_and_cross_try_recur_do_not_execute() {
    let mut c = Context::new();
    for source in [
        "(loop*)",
        "(loop* ())",
        "(loop* [x])",
        "(loop* [1 2])",
        "(loop* [[x] [1]] x)",
    ] {
        let error = c.eval_string(source).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Analysis);
        assert!(matches!(error.kind, E::InvalidForm(_)));
    }
    let error = c.eval_string("(fn* [] (try (recur)))").unwrap_err();
    assert_eq!(error.phase, EvalPhase::Analysis);
    assert_eq!(error.kind, E::InvalidForm("cannot recur across try"));
}

#[test]
fn replacement_operands_and_old_captures_stay_rooted_across_nested_evaluation() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline_heap = c.memory_stats().heap_values;
    c.register_native(&OBSERVE);
    let result = c.eval_string("(loop* [n 0 a nil b nil] (if (= n 2) [(a) (b)] (recur (inc n) (fn* [] n) (do (observe :collect) a))))").unwrap();
    assert_eq!(wire(&result).wire(), "(vector (int 1) (int 0))");
    drop(result);
    let result = c.eval_string("(loop* [n 0 a nil] (if (= n 2) (a) (recur (inc n) (let* [x (loop* [m 2] (observe :collect) (if (zero? m) n (recur (dec m))))] (fn* [] x)))))").unwrap();
    assert_eq!(integer(result), 1);
    let error = c
        .eval_string("(loop* [x 0 y 1] (recur (fn* [] x) (/ 1 0)))")
        .unwrap_err();
    assert_eq!(error.span.text(), "(/ 1 0)");
    drop(error);
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, baseline_heap);
    assert_eq!(c.memory_stats().executable_units, 0);
}
