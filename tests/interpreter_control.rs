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
fn control_programs_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for case in cases.iter().filter(|c| c.id.starts_with("sci-control-")) {
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
    assert_eq!(count, 132);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_control_macro_is_registered_and_attributed() {
    use light_cljs::interpreter::{Name, Origin};
    for (name, program) in [
        ("and", "(and true 42)"),
        ("or", "(or nil 42)"),
        ("if-not", "(if-not false 42)"),
        ("when", "(when true 42)"),
        ("when-not", "(when-not false 42)"),
        ("if-let", "(if-let [x 42] x)"),
        ("if-some", "(if-some [x 42] x)"),
        ("when-let", "(when-let [x 42] x)"),
        ("when-some", "(when-some [x 42] x)"),
        ("when-first", "(when-first [x [42]] x)"),
        ("cond", "(cond true 42)"),
        ("condp", "(condp = 42 42 42)"),
        ("->", "(-> 41 inc)"),
        ("->>", "(->> 41 inc)"),
        ("as->", "(as-> 41 x (inc x))"),
        ("cond->", "(cond-> 41 true inc)"),
        ("cond->>", "(cond->> 41 true inc)"),
        ("some->", "(some-> 41 inc)"),
        ("some->>", "(some->> 41 inc)"),
        ("comment", "(comment unresolved)"),
        ("assert", "(assert true)"),
        ("case", "(case 1 1 42)"),
    ] {
        let mut c = Context::new();
        c.set_macro_diagnostics(true);
        let value = c.eval_string(program).unwrap();
        assert!(matches!(
            value.inspect(),
            ValueView::Integer(42) | ValueView::Nil
        ));
        let stat = c
            .macro_expansion_stats()
            .iter()
            .find(|s| s.name == Name::new(Some("clojure.core"), name))
            .unwrap();
        assert!(stat.expansions >= 1, "{name}");
        assert!(stat.generated_forms > 0, "{name}");
        let var = c
            .resolve_var(&Name::new(Some("clojure.core"), name))
            .unwrap()
            .unwrap();
        assert_eq!(
            var.metadata()
                .unwrap()
                .get(&c.keyword(Name::new(None, "macro")))
                .unwrap()
                .unwrap()
                .inspect(),
            ValueView::Boolean(true)
        );
        let quoted = c.eval_string(&format!("'{program}")).unwrap();
        let expanded = c.macroexpand(&quoted, None).unwrap();
        let source = c.source("owned-control.clj", "");
        let form = c
            .form_from_data(&expanded, &source.span(0..0).unwrap())
            .unwrap();
        let code = c.analyze_form(&form).unwrap();
        drop((quoted, expanded, form, source));
        c.collect_garbage();
        assert!(c.eval_analyzed(&code).unwrap().value_eq(&value));
        let bad = c.eval_string(name).unwrap_err();
        assert_eq!(bad.phase, EvalPhase::Analysis);
        assert_eq!(bad.origin, Origin::Source);
    }
}

#[test]
fn malformed_macros_and_case_primitives_fail_at_the_origin_before_effects() {
    use light_cljs::interpreter::Origin;
    for program in [
        "(if-not)",
        "(if-not true 1 2 3)",
        "(when)",
        "(when-not)",
        "(if-let)",
        "(if-some [x 1])",
        "(when-let)",
        "(when-some)",
        "(when-first)",
        "(if-let [x (observe 1) y 2] x)",
        "(if-some (x 1) x)",
        "(when-let [] nil)",
        "(when-some [x] nil)",
        "(when-first [x [1] y 2] x)",
        "(cond (observe 1) 2 false)",
        "(condp =)",
        "(->)",
        "(->>)",
        "(as-> 1)",
        "(cond-> (observe 1) true)",
        "(cond->> (observe 1) true)",
        "(some->)",
        "(some->>)",
        "(assert)",
        "(assert true 1 2)",
        "(case)",
        "(case (observe 1) (1 2) :x 2 :y)",
    ] {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let source = c.source("malformed-controls.clj", program);
        let error = c.eval_source(&source).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Macroexpansion, "{program}: {error}");
        assert_eq!(error.span.text(), program);
        assert!(matches!(error.origin, Origin::MacroExpansion(_)));
        TRACE.with(|t| assert!(t.borrow().is_empty()));
    }
    for program in [
        "(case* 1)",
        "(case* 1 nil)",
        "(case* 1 [1])",
        "(case* 1 [1 1 1 2])",
    ] {
        let mut c = Context::new();
        let error = c.eval_string(program).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Analysis);
        assert!(matches!(error.kind, E::InvalidForm(_)));
    }
}

#[test]
fn control_failures_preserve_original_subforms_and_generated_call_locations() {
    use light_cljs::interpreter::{Name, Origin};
    let mut c = Context::new();
    for program in [
        "(and true (/ 1 0))",
        "(if-let [x (/ 1 0)] x)",
        "(condp = 1 1 (/ 1 0))",
        "(case 1 1 (/ 1 0))",
        "(assert false (/ 1 0))",
    ] {
        let error = c.eval_string(program).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Execution);
        assert_eq!(error.span.text(), "(/ 1 0)");
    }
    for (name, program) in [
        ("assert", "(assert false :message)"),
        ("case", "(case 3 1 :one)"),
        ("condp", "(condp = 3 1 :one)"),
        ("->", "(-> 1 (nth 0))"),
    ] {
        let source = c.source("generated-control.clj", program);
        let error = c.eval_source(&source).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Execution);
        assert_eq!(error.span.text(), program);
        assert_eq!(
            error.origin,
            Origin::MacroExpansion(std::rc::Rc::new(Name::new(Some("clojure.core"), name)))
        );
        assert_eq!(error.span.source().name(), "generated-control.clj");
    }
    assert_eq!(
        c.eval_string("(try (case 0 1 2) (catch Exception e :caught))")
            .unwrap()
            .inspect(),
        ValueView::Keyword(Name::new(None, "caught"))
    );
    assert_eq!(
        c.eval_string("(try (assert false 42) (catch AssertionError e (ex-message e)))")
            .unwrap()
            .inspect(),
        ValueView::String("Assert failed".encode_utf16().collect::<Vec<_>>().into())
    );
}

#[test]
fn case_constants_and_branches_survive_gc_and_repeat_without_macro_execution() {
    let mut c = Context::new();
    c.set_macro_diagnostics(true);
    let source = c.source("case-closure.clj", "(let [offset 40] (fn [x] (case x (:a :b) (+ offset 2) [1 2] :vector {:x [1]} :map #{[1 2]} :set :none)))");
    let closure = c.eval_source(&source).unwrap();
    let before: u64 = c.macro_expansion_stats().iter().map(|s| s.expansions).sum();
    drop(source);
    c.collect_garbage();
    for (input, expected) in [
        (":b", "42"),
        ("'(1 2)", ":vector"),
        ("'{:x (1)}", ":map"),
        ("'#{(1 2)}", ":set"),
        (":c", ":none"),
    ] {
        let argument = c.eval_string(input).unwrap();
        let expected = c.eval_string(expected).unwrap();
        assert!(c.invoke(&closure, &[argument]).unwrap().value_eq(&expected));
        c.collect_garbage();
    }
    assert_eq!(
        c.macro_expansion_stats()
            .iter()
            .map(|s| s.expansions)
            .sum::<u64>(),
        before
    );
    assert_eq!(
        c.eval_string("(loop [x 0] (case x 100000 x (recur (inc x))))")
            .unwrap()
            .inspect(),
        ValueView::Integer(100000)
    );
}

#[test]
fn expansion_and_case_lookup_obey_shared_resource_limits() {
    use light_cljs::interpreter::{ErrorClass, ResourceLimits};
    for program in [
        "(and 1 2 (or nil false 42))",
        "(if-let [[x] [42]] (when-some [y x] y))",
        "(condp = 2 1 1 2 :>> (fn [x] 42))",
        "(as-> 1 x (cond-> x true inc true (+ 40)))",
        "(some->> [20 22] (apply +) (identity))",
        "(try (assert false [1 2]) (catch AssertionError e 42))",
        "(case {:x [1 2]} (:a :b) 0 {:x (1 2)} 42 -1)",
    ] {
        let mut baseline = Context::new();
        baseline.eval_string(program).unwrap();
        let usage = baseline.resource_usage();
        for allocation in [false, true] {
            let limit = if allocation {
                usage.allocation
            } else {
                usage.fuel
            };
            for budget in (0..limit.min(128)).chain([limit - 1, limit, limit + 1]) {
                let mut c = Context::new();
                c.set_resource_limits(ResourceLimits {
                    fuel: (!allocation).then_some(budget),
                    allocation: allocation.then_some(budget),
                    ..ResourceLimits::default()
                });
                match c.eval_string(program) {
                    Ok(v) => assert_eq!(v.inspect(), ValueView::Integer(42)),
                    Err(e) => assert_eq!(
                        e.class(),
                        ErrorClass::Control,
                        "{allocation}/{budget}/{program}: {e}"
                    ),
                }
            }
        }
    }
    let mut c = Context::new();
    c.set_resource_limits(ResourceLimits {
        evaluation_depth: 8,
        ..ResourceLimits::default()
    });
    assert_eq!(
        c.eval_string("(and 1 2 3 4 5 6 7 8 9 10)")
            .unwrap_err()
            .class(),
        ErrorClass::Control
    );
}

#[test]
fn wide_case_analysis_preserves_native_source_order_and_dispatches_every_branch() {
    // SCI's internal integer-key imap changes iteration order beyond eight
    // constants. The native profile deliberately retains source order instead
    // of importing the JVM hash-map layout for analysis-time macro effects.
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    TRACE.with(|t| t.borrow_mut().clear());
    c.eval_string("(defmacro marked [x] (observe x) x) (def choose (fn [x] (case x :a (marked 1) :b (marked 2) :c (marked 3) :d (marked 4) :e (marked 5) :f (marked 6) :g (marked 7) :h (marked 8) :i (marked 9) (marked 0))))").unwrap();
    TRACE.with(|t| {
        assert_eq!(
            *t.borrow(),
            (0..=9).map(protocol::Value::Int).collect::<Vec<_>>()
        )
    });
    for (i, key) in ["a", "b", "c", "d", "e", "f", "g", "h", "i"]
        .iter()
        .enumerate()
    {
        assert_eq!(
            c.eval_string(&format!("(choose :{key})"))
                .unwrap()
                .inspect(),
            ValueView::Integer(i as i64 + 1)
        );
    }
    assert_eq!(
        c.eval_string("(choose :other)").unwrap().inspect(),
        ValueView::Integer(0)
    );
    TRACE.with(|t| assert_eq!(t.borrow().len(), 10));
}
