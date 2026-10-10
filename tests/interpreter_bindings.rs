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
fn binding_programs_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for case in cases.iter().filter(|c| c.id.starts_with("sci-binding-")) {
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
    assert_eq!(count, 114);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn malformed_bindings_fail_before_initializers_and_excluded_extensions_are_explicit() {
    use light_cljs::interpreter::Origin;
    for expression in [
        "(let (x 1) x)",
        "(let [x] x)",
        "(let [42 (observe 1)] nil)",
        "(let [:x (observe 1)] nil)",
        "(let [[x &] (observe [1])] nil)",
        "(let [[x & xs y] (observe [1])] nil)",
        "(let [[x :as] (observe [1])] nil)",
        "(let [{:keys x} (observe {})] nil)",
        "(let [{:keys [42]} (observe {})] nil)",
        "(let [{:keys [x] :or 1} (observe {})] nil)",
        "(let [{:keys! [x]} (observe {})] nil)",
        "(let [{:select m} (observe {})] nil)",
        "(let [{:all m} (observe {})] nil)",
        "(let [{:excess m} (observe {})] nil)",
        "(let [{:missing m} (observe {})] nil)",
        "(let [{:keys [x & :y]} (observe {})] nil)",
        "(fn [:x] x)",
        "(fn [& :xs] xs)",
        "(defn f [:x] x)",
        "(let [{:as [x]} (observe {})] nil)",
        "(fn [x &])",
        "(fn [x & xs y])",
        "(fn)",
        "(fn (x x))",
        "(fn [[x & xs y]] nil)",
        "(letfn [x 1] x)",
        "(letfn [(42 [] 1)] nil)",
        "(defn 42 [] 1)",
        "(loop [[x & xs y] (observe [1])] nil)",
    ] {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let source = c.source("malformed.clj", expression);
        let error = c.eval_source(&source).unwrap_err();
        assert_eq!(
            error.phase,
            EvalPhase::Macroexpansion,
            "{expression}: {error}"
        );
        assert!(
            matches!(error.kind, E::InvalidForm(_)),
            "{expression}: {error}"
        );
        assert!(matches!(error.origin, Origin::MacroExpansion(_)));
        assert_eq!(error.span.source().name(), "malformed.clj");
        TRACE.with(|t| assert!(t.borrow().is_empty(), "{expression}"));
    }
}

#[test]
fn expansion_is_executable_direct_data_and_fn_defn_have_distinct_recursion() {
    use light_cljs::interpreter::Name;
    let mut c = Context::new();
    for program in [
        "'(let [[x y] [20 22]] (+ x y))",
        "'(loop [[n x] [3 0]] (if (zero? n) x (recur [(dec n) (+ x 14)])))",
        "'(letfn [(f [[x]] (+ x 2))] (f [40]))",
        "'(do (defn answer [{:keys [x]}] (+ x 2)) (answer {:x 40}))",
    ] {
        let form = c.eval_string(program).unwrap();
        let expanded = c.macroexpand(&form, None).unwrap();
        let source = c.source("owned-expansion.clj", "");
        let form = c
            .form_from_data(&expanded, &source.span(0..0).unwrap())
            .unwrap();
        let code = c.analyze_form(&form).unwrap();
        drop(form);
        drop(source);
        drop(expanded);
        c.collect_garbage();
        assert_eq!(
            c.eval_analyzed(&code).unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
    c.set_macro_diagnostics(true);
    c.eval_string("(defn f [[x]] {:pre [(pos? x)]} (+ x 1)) (f [1])")
        .unwrap();
    for name in ["defn", "fn", "assert"] {
        let stat = c
            .macro_expansion_stats()
            .iter()
            .find(|s| s.name == Name::new(Some("clojure.core"), name))
            .unwrap();
        assert!(stat.expansions > 0 && stat.generated_forms > 0);
    }
}

#[test]
fn functions_and_destructured_local_cycles_keep_sources_alive_and_are_reclaimed() {
    let mut c = Context::new();
    let source = c.source("bindings.clj", "(let [{:keys [x]} {:x 20}] (letfn [(f [[y]] (if y (g {:y y}) f)) (g [{:keys [y]}] (+ x y))] f))");
    let f = c.eval_source(&source).unwrap();
    drop(source);
    c.collect_garbage();
    let arg = c.vector(&[c.integer(22)]).unwrap();
    assert_eq!(
        c.invoke(&f, &[arg]).unwrap().inspect(),
        ValueView::Integer(42)
    );
    let empty = c.vector(&[]).unwrap();
    let self_value = c.invoke(&f, &[empty]).unwrap();
    assert!(self_value.value_eq(&f));
    drop(f);
    drop(self_value);
    c.collect_garbage();
    assert_eq!(c.memory_stats().executable_units, 0);
}

#[test]
fn prepost_assertions_keep_body_once_error_provenance_and_tail_rules() {
    use light_cljs::interpreter::{ErrorClass, Origin};
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    let source = c.source(
        "preconditions.clj",
        "(defn f [[x]] {:pre [(pos? x)]} x)\n(f [-1])",
    );
    let error = c.eval_source(&source).unwrap_err();
    assert_eq!(error.phase, EvalPhase::Execution);
    assert_eq!(error.kind, E::Thrown(ExceptionCategory::AssertionError));
    assert_eq!(error.class(), ErrorClass::Language);
    assert_eq!(error.span.source().name(), "preconditions.clj");
    assert!(matches!(error.origin, Origin::MacroExpansion(_)));
    assert!(!error.frames.is_empty());
    assert_eq!(
        c.eval_string("(try (f [-1]) (catch Throwable e :caught))")
            .unwrap()
            .inspect(),
        ValueView::Keyword(light_cljs::interpreter::Name::new(None, "caught"))
    );
    let error = c
        .eval_string("(fn [x] {:post [(pos? %)]} (recur x))")
        .unwrap_err();
    assert_eq!(error.phase, EvalPhase::Analysis);
    assert!(matches!(
        error.kind,
        E::InvalidForm("recur requires tail position")
    ));
    // Original initializer lists retain their precise source through expansion.
    let source = c.source("initializers.clj", "(let [[x] [(/ 1 0)]] x)");
    let error = c.eval_source(&source).unwrap_err();
    assert_eq!(error.span.text(), "(/ 1 0)");
    let source = c.source("destructuring.clj", "(let [[x] 3] x)");
    let error = c.eval_source(&source).unwrap_err();
    assert_eq!(error.span.text(), "(let [[x] 3] x)");
}

#[test]
fn binding_expansion_and_recur_share_budgets_and_reanalyze_only_at_entry() {
    use light_cljs::interpreter::{ErrorClass, ResourceLimits};
    let program = "(let [{:keys [x]} {:x 20}] ((fn [[y]] {:pre [(pos? y)]} (+ x y)) [22]))";
    let mut baseline = Context::new();
    baseline.eval_string(program).unwrap();
    let usage = baseline.resource_usage();
    for allocation in [false, true] {
        let limit = if allocation {
            usage.allocation
        } else {
            usage.fuel
        };
        // Sweep every small boundary plus the complete operation's admission edge.
        for budget in (0..limit.min(1000)).chain([limit - 1, limit, limit + 1]) {
            let mut c = Context::new();
            c.set_resource_limits(ResourceLimits {
                fuel: (!allocation).then_some(budget),
                allocation: allocation.then_some(budget),
                ..ResourceLimits::default()
            });
            match c.eval_string(program) {
                Ok(v) => assert_eq!(v.inspect(), ValueView::Integer(42)),
                Err(e) => assert_eq!(e.class(), ErrorClass::Control, "{allocation}/{budget}: {e}"),
            }
        }
    }
    let mut c = Context::new();
    c.set_macro_diagnostics(true);
    let source = c.source(
        "recur.clj",
        "(loop [[n x] [100000 0]] (if (zero? n) x (recur [(dec n) (inc x)])))",
    );
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let code = c.analyze_form(&form).unwrap();
    let expansions: u64 = c.macro_expansion_stats().iter().map(|s| s.expansions).sum();
    assert_eq!(
        c.eval_analyzed(&code).unwrap().inspect(),
        ValueView::Integer(100000)
    );
    assert_eq!(
        c.macro_expansion_stats()
            .iter()
            .map(|s| s.expansions)
            .sum::<u64>(),
        expansions
    );
    c.set_resource_limits(ResourceLimits {
        fuel: Some(100),
        ..ResourceLimits::default()
    });
    assert_eq!(
        c.eval_analyzed(&code).unwrap_err().class(),
        ErrorClass::Control
    );
}
