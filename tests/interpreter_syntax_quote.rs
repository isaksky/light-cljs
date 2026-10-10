use light_cljs::interpreter::{
    Arity, CollectionError, Context, EvalErrorKind as E, EvalPhase, ExceptionCategory,
    NativeDefinition, NativeEffects, NativeError, NativeMetadata, RootedValue, ValueView,
};
use std::cell::RefCell;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn wire(context: &mut Context, value: &RootedValue) -> protocol::Value {
    use protocol::Value as V;
    match value.inspect() {
        ValueView::CompilerLiteral(_) => panic!("compiler-only reader value in native fixture"),
        ValueView::LazySeq => {
            let seq = context.seq(value).unwrap();
            if matches!(seq.inspect(), ValueView::List(_)) {
                wire(context, &seq)
            } else {
                V::Seq(
                    context
                        .sequence_elements(value)
                        .unwrap()
                        .iter()
                        .map(|v| wire(context, v))
                        .collect(),
                )
            }
        }
        ValueView::Delay => panic!("delay outside fixture transport"),
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
        ValueView::List(_) => V::List(
            value
                .elements()
                .unwrap()
                .iter()
                .map(|v| wire(context, v))
                .collect(),
        ),
        ValueView::Seq(_) => V::Seq(
            value
                .elements()
                .unwrap()
                .iter()
                .map(|v| wire(context, v))
                .collect(),
        ),
        ValueView::Vector(_) => V::Vector(
            value
                .elements()
                .unwrap()
                .iter()
                .map(|v| wire(context, v))
                .collect(),
        ),
        ValueView::Map(_) => {
            let mut pairs: Vec<_> = value
                .entries()
                .unwrap()
                .iter()
                .map(|(k, v)| (wire(context, k), wire(context, v)))
                .collect();
            pairs.sort_by_key(|(k, v)| format!("(entry {} {})", k.wire(), v.wire()));
            V::Map(pairs)
        }
        ValueView::Set(_) => {
            let mut values: Vec<_> = value
                .elements()
                .unwrap()
                .iter()
                .map(|v| wire(context, v))
                .collect();
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
        let observed = wire(context, &args[0]);
        TRACE.with(|trace| trace.borrow_mut().push(observed));
        context.collect_garbage();
        Ok(args[0].clone())
    },
};

#[test]
fn syntax_quote_programs_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for case in cases
        .iter()
        .filter(|c| c.id.starts_with("sci-syntax-quote-"))
    {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let actual = match c.eval_string(case.source) {
            Ok(v) => O::Returned(wire(&mut c, &v)),
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
        TRACE.with(|t| assert_eq!(*t.borrow(), case.effects, "{} effects", case.id));
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        assert_eq!(
            c.eval_string("(+ 20 22)").unwrap().inspect(),
            ValueView::Integer(42)
        );
        count += 1;
    }
    assert_eq!(count, 75);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn live_reader_uses_namespace_alias_and_refer_state_at_each_read() {
    use light_cljs::interpreter::{Name, Origin};
    let mut c = Context::new();
    c.eval_string("(create-ns 'a) (create-ns 'b) (alias 't 'a)")
        .unwrap();
    let source = c.source("live.clj", "`t/x `t/x `inc `local");
    let mut reader = c.reader(&source).unwrap();
    let first = reader.read_form().unwrap().unwrap();
    assert_eq!(first.node().origin, Origin::SyntaxQuote);
    c.eval_string("(ns-unalias *ns* 't) (alias 't 'b)").unwrap();
    let second = reader.read_form().unwrap().unwrap();
    assert_eq!(
        c.eval_form(&first).unwrap().inspect(),
        ValueView::Symbol(Name::new(Some("a"), "x"))
    );
    assert_eq!(
        c.eval_form(&second).unwrap().inspect(),
        ValueView::Symbol(Name::new(Some("b"), "x"))
    );
    c.eval_string("(in-ns 'a) (def inc 9) (in-ns 'b) (refer 'a)")
        .unwrap();
    let referred = reader.read_form().unwrap().unwrap();
    assert_eq!(
        c.eval_form(&referred).unwrap().inspect(),
        ValueView::Symbol(Name::new(Some("a"), "inc"))
    );
    let local = reader.read_form().unwrap().unwrap();
    assert_eq!(
        c.eval_form(&local).unwrap().inspect(),
        ValueView::Symbol(Name::new(Some("b"), "local"))
    );
    assert!(reader.read_form().unwrap().is_none());
    let parsed = light_cljs::parse(source.text()).unwrap();
    let lowered = c.lower_form(&source, &parsed, parsed.roots[0]).unwrap();
    assert_eq!(
        c.eval_form(&lowered).unwrap().inspect(),
        ValueView::Symbol(Name::new(Some("t"), "x"))
    );
}

#[test]
fn gensym_scopes_are_fresh_and_retained_code_reuses_its_reading() {
    use light_cljs::interpreter::Name;
    let mut c = Context::new();
    let source = c.source("gensym.clj", "`[x# x# foo/x# foo/x#]");
    let first = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let second = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let code = c.analyze_form(&first).unwrap();
    let a = c.eval_analyzed(&code).unwrap();
    let again = c.eval_analyzed(&code).unwrap();
    let b = c.eval_form(&second).unwrap();
    assert!(a.value_eq(&again));
    assert!(!a.value_eq(&b));
    for i in [0, 2] {
        let left = a.nth(i).unwrap().unwrap();
        assert!(left.value_eq(&a.nth(i + 1).unwrap().unwrap()));
        let ValueView::Symbol(name) = left.inspect() else {
            panic!()
        };
        assert!(name.namespace().is_none());
        assert!(
            String::from_utf16(name.name())
                .unwrap()
                .ends_with("__auto__")
        );
        assert_ne!(name, Name::new(None, "x#"));
    }
    assert!(
        !a.nth(0)
            .unwrap()
            .unwrap()
            .value_eq(&a.nth(2).unwrap().unwrap())
    );
    let nested = c.eval_string("`[x# `[x# x#] x#]").unwrap();
    let mut pending = vec![nested];
    let mut generated = Vec::new();
    while let Some(value) = pending.pop() {
        match value.inspect() {
            ValueView::Symbol(name)
                if String::from_utf16(name.name())
                    .unwrap()
                    .ends_with("__auto__") =>
            {
                generated.push(name)
            }
            ValueView::LazySeq | ValueView::List(_) | ValueView::Seq(_) | ValueView::Vector(_) => {
                pending.extend(c.sequence_elements(&value).unwrap())
            }
            _ => (),
        }
    }
    assert_eq!(generated.len(), 4);
    assert_eq!(generated[0], generated[3]);
    assert_eq!(generated[1], generated[2]);
    assert_ne!(generated[0].name(), generated[1].name());
    assert!(generated[0].namespace().is_none());
    assert_eq!(
        generated[1].namespace(),
        Some("user".encode_utf16().collect::<Vec<_>>().as_slice())
    );
    c.eval_string("(def make (fn* [] `x#))").unwrap();
    assert!(
        c.eval_string("(= (make) (make))")
            .unwrap()
            .value_eq(&c.boolean(true))
    );
}

#[test]
fn quote_values_and_deferred_splices_survive_source_owner_and_reset() {
    let mut c = Context::new();
    let (form, valid, invalid, function) = {
        let source = c.source("owned.clj", "`[kept# ~(+ 1 2)]");
        let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
        let valid = c.eval_string("`(1 ~@[2 3] ~4)").unwrap();
        let invalid = c.eval_string("`(1 ~@7)").unwrap();
        let function = c
            .eval_string("(let* [x \"retained\"] (fn* [] `[~x x#]))")
            .unwrap();
        (form, valid, invalid, function)
    };
    c.collect_garbage();
    assert_eq!(
        c.eval_form(&form)
            .unwrap()
            .nth(1)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Integer(3)
    );
    assert_eq!(
        c.invoke(&function, &[])
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::String("retained".encode_utf16().collect())
    );
    // Syntax quote uses genuine lazy concat now. Realize the retained finite
    // value, and only the valid prefix of the invalid splice, before detach.
    assert_eq!(c.sequence_elements(&valid).unwrap().len(), 4);
    c.seq(&invalid).unwrap();
    c.reset();
    drop(c);
    assert_eq!(valid.elements().unwrap().len(), 4);
    assert_eq!(
        invalid.nth(0).unwrap().unwrap().inspect(),
        ValueView::Integer(1)
    );
    assert!(invalid.nth(1).is_err());
    assert!(invalid.elements().is_err());
    assert_eq!(form.span().source().name(), "owned.clj");
    assert!(form.data().elements().is_ok());
}

#[test]
fn generated_macro_diagnostics_retain_source_call_and_body_origins() {
    use light_cljs::interpreter::{Name, Origin};
    let mut c = Context::new();
    let source = c.source("macro-quote.clj", "(defmacro bad [] `[~(/ 1 0)])\n(bad)");
    let body_error = c.eval_source(&source).unwrap_err();
    assert_eq!(body_error.phase, EvalPhase::Macroexpansion);
    assert_eq!(body_error.span.text(), "(bad)");
    assert!(
        body_error
            .frames
            .iter()
            .any(|frame| frame.span.text() == "(/ 1 0)")
    );
    let generated = c.source(
        "generated-quote.clj",
        "(defmacro broken [] `(+ 1 absent))\n(broken)",
    );
    let error = c.eval_source(&generated).unwrap_err();
    assert_eq!(error.phase, EvalPhase::Analysis);
    assert_eq!(error.span.text(), "(broken)");
    assert_eq!(
        error.origin,
        Origin::MacroExpansion(Name::new(Some("user"), "broken").into())
    );
    assert_eq!(
        error.kind,
        E::UnresolvedSymbol(Name::new(Some("user"), "absent"))
    );
    c.reset();
    drop(c);
    assert!(
        body_error
            .frames
            .iter()
            .any(|frame| frame.span.source().name() == "macro-quote.clj")
    );
    assert_eq!(error.span.source().name(), "generated-quote.clj");
}

#[test]
fn syntax_quote_work_obeys_shared_budgets_and_reclaims_reading_storage() {
    use light_cljs::interpreter::{ErrorClass, ResourceLimits};
    for allocation in 1..350 {
        let mut c = Context::new();
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..ResourceLimits::default()
        });
        let result = c.eval_string("(defmacro m [x] `(let* [x# ~x] [x# x#])) (m 3)");
        if let Err(error) = result {
            assert_eq!(error.class(), ErrorClass::Control, "{allocation}: {error}");
        }
    }
    let mut c = Context::new();
    for _ in 0..30 {
        let source = c.source("repl.clj", "`[x# ~(identity 1) [a b]]");
        let mut reader = c.reader(&source).unwrap();
        let form = reader.read_form().unwrap().unwrap();
        c.eval_form(&form).unwrap();
    }
    c.collect_garbage();
    assert_eq!(c.memory_stats().sources, 0);
    assert_eq!(c.memory_stats().executable_units, 0);
    c.reset();
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, 0);
    assert_eq!(c.memory_stats().namespaces, 0);
}
