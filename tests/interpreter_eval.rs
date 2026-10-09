use light_cljs::interpreter::{
    AnalyzedForm, Context, EvalError, EvalErrorKind as E, EvalPhase as P, Name, RootedValue,
    ValueView, eval_string,
};
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
fn outcome(result: Result<RootedValue, EvalError>) -> protocol::Outcome {
    use protocol::{Category as C, Outcome as O, Phase as P};
    match result {
        Ok(value) => O::Returned(wire(&value)),
        Err(error) => {
            let (phase, category) = match error.kind {
                E::Read(_) => (P::Read, C::Reader),
                E::UnresolvedSymbol(_) => (P::Analysis, C::UnresolvedSymbol),
                E::InvalidForm(_) => (P::Analysis, C::InvalidForm),
                E::DuplicateMapKey | E::DuplicateSetMember => (P::Execution, C::DuplicateKey),
                other => panic!("unexpected native error {other:?}"),
            };
            assert_eq!(format!("{:?}", error.phase), format!("{phase:?}"));
            O::Error { phase, category }
        }
    }
}
fn analyze(c: &mut Context, text: &str) -> AnalyzedForm {
    let source = c.source("analysis.clj", text);
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    c.analyze_form(&form).unwrap()
}

#[test]
fn primitive_programs_execute_against_selected_pinned_sci_outcomes() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| {
        c.id.starts_with("sci-primitive-")
            || [
                "sci-scalars",
                "sci-integers",
                "sci-doubles",
                "sci-ratios",
                "sci-utf16",
                "sci-characters",
                "sci-list",
                "sci-map",
                "sci-set",
                "sci-do-nil",
                "sci-if",
                "sci-invalid-form",
                "sci-context-isolated",
                "sci-read-eval-disabled",
                "sci-after-error",
                "sci-reader-quotes",
                "sci-reader-deref",
                "sci-reader-var",
                "sci-reader-var-data",
                "sci-reader-discard",
                "sci-reader-numeric-keys",
                "sci-reader-parse-form",
                "sci-reader-duplicate-map",
                "sci-reader-duplicate-set",
                "sci-reader-duplicate-discard",
            ]
            .contains(&c.id)
    }) {
        assert_eq!(
            outcome(eval_string(case.source)),
            case.outcome,
            "{}: {}",
            case.id,
            case.source
        );
        assert!(case.effects.is_empty() && case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 36, "keep the tested scope explicit");
}

#[test]
fn public_results_and_code_own_values_after_input_and_reader_drop() {
    let mut c = Context::new();
    let mut input = String::from("(let* [x [\"\\uD800\" 'unknown]] x)");
    let result = c.eval_string(&input).unwrap();
    input.clear();
    c.collect_garbage();
    assert_eq!(
        result.nth(0).unwrap().unwrap().inspect(),
        ValueView::String(vec![0xd800].into())
    );
    let analyzed = analyze(&mut c, "(let* [x ^{:tag :kept} [\"saved\"] y x] y)");
    assert_eq!(analyzed.local_count(), 2);
    assert!(analyzed.expression_count() > 2);
    c.collect_garbage();
    for _ in 0..3 {
        let value = c.eval_analyzed(&analyzed).unwrap();
        assert_eq!(value.metadata().unwrap().count(), Some(1));
        assert_eq!(
            value.nth(0).unwrap().unwrap().inspect(),
            ValueView::String("saved".encode_utf16().collect())
        );
    }
    let old = analyzed.unit().id();
    c.reset();
    assert_eq!(
        analyzed.span().text(),
        "(let* [x ^{:tag :kept} [\"saved\"] y x] y)"
    );
    assert_eq!(c.eval_analyzed(&analyzed).unwrap_err().kind, E::WrongOwner);
    assert!(c.unit_by_id(old).is_err());
    drop(c);
    assert_eq!(
        result.nth(1).unwrap().unwrap().inspect(),
        ValueView::Symbol(Name::new(None, "unknown"))
    );
}

#[test]
fn lexical_initializers_shadowing_and_empty_bodies_are_resolved_once() {
    let mut c = Context::new();
    for text in [
        "",
        " ; comment",
        "#_42",
        "(do)",
        "(let* [])",
        "(let* [x 1])",
        "(if false :yes)",
    ] {
        assert_eq!(
            c.eval_string(text).unwrap().inspect(),
            ValueView::Nil,
            "{text}"
        );
    }
    let value = c
        .eval_string("(let* [x 1] [(let* [x [x] x [x]] x) x])")
        .unwrap();
    assert_eq!(
        wire(&value).wire(),
        "(vector (vector (vector (int 1))) (int 1))"
    );
    let mut text = String::from("(let* [x 1");
    for _ in 0..32 {
        text.push_str(" x [x]");
    }
    text.push_str("] x)");
    let code = analyze(&mut c, &text);
    assert_eq!(code.local_count(), 33);
    let mut value = c.eval_analyzed(&code).unwrap();
    for _ in 0..32 {
        value = value.nth(0).unwrap().unwrap();
    }
    assert_eq!(value.inspect(), ValueView::Integer(1));
    assert!(matches!(
        c.eval_string("x").unwrap_err().kind,
        E::UnresolvedSymbol(_)
    ));
}

#[test]
fn collections_evaluate_members_then_metadata_and_quote_retains_data() {
    let mut c = Context::new();
    let value = c
        .eval_string("(let* [x :evaluated] ^{:tag x} [x])")
        .unwrap();
    let tag = c.keyword(Name::new(None, "tag"));
    assert_eq!(
        value
            .metadata()
            .unwrap()
            .get(&tag)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Keyword(Name::new(None, "evaluated"))
    );
    let quoted = c.eval_string("'^:private (missing (if))").unwrap();
    assert_eq!(quoted.count(), Some(2));
    assert_eq!(quoted.metadata().unwrap().count(), Some(1));
    let quoted = c.eval_string("'^{:tag unknown} [unknown]").unwrap();
    assert_eq!(
        quoted
            .metadata()
            .unwrap()
            .get(&tag)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Symbol(Name::new(None, "unknown"))
    );
    // Empty-list metadata stays data, matching SCI's constant treatment.
    c.eval_string("^{:tag unknown} ()").unwrap();
}

#[test]
fn errors_preserve_phases_exact_spans_and_source_lifetimes() {
    let mut c = Context::new();
    for (text, span) in [
        ("(if true 1 missing)", "missing"),
        ("(if false missing 1)", "missing"),
        ("(let* [x x] x)", "x"),
        ("(let* [x later later 1] x)", "later"),
        ("(let* [x 1] user/x)", "user/x"),
        ("(if missing)", "missing"),
    ] {
        let error = c.eval_string(text).unwrap_err();
        assert_eq!(error.phase, P::Analysis);
        assert!(matches!(error.kind, E::UnresolvedSymbol(_)));
        assert_eq!(error.span.text(), span);
    }
    for text in [
        "(if)",
        "(if 1)",
        "(if 1 2 3 4)",
        "(quote)",
        "(quote 1 2)",
        "(let*)",
        "(let* ())",
        "(let* [x])",
        "(let* [1 2])",
        "(let* [a/x 2])",
        "(let* [[x] []])",
    ] {
        let error = c.eval_string(text).unwrap_err();
        assert_eq!(error.phase, P::Analysis, "{text}");
        assert!(matches!(error.kind, E::InvalidForm(_)), "{text}: {error:?}");
    }
    let source = c.source("owned.clj", "[\"λ\"\n missing]");
    let error = c.eval_source(&source).unwrap_err();
    drop(source);
    c.reset();
    drop(c);
    assert_eq!(error.span.bytes(), 7..14);
    assert_eq!(error.span.source().line_column(7), Some((2, 2)));
    assert!(error.to_string().starts_with("owned.clj:2:2: Analysis:"));
    assert_eq!(eval_string("[1").unwrap_err().phase, P::Read);
}

#[test]
fn execution_errors_clear_scratch_roots_and_context_remains_usable() {
    let mut c = Context::new();
    c.eval_string("").unwrap();
    c.collect_garbage();
    let baseline = c.memory_stats();
    for _ in 0..30 {
        let error = c
            .eval_string("(let* [x [1] y '(1)] [\"temporary\" {x :first y :second}])")
            .unwrap_err();
        assert_eq!(error.phase, P::Execution);
        assert_eq!(error.kind, E::DuplicateMapKey);
        assert_eq!(error.span.text(), "y");
        assert_eq!(error.related.unwrap().text(), "x");
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
        c.collect_garbage();
    }
    c.collect_garbage();
    let stats = c.memory_stats();
    assert_eq!(
        (
            stats.heap_values,
            stats.names,
            stats.sources,
            stats.executable_units,
            stats.roots
        ),
        (
            baseline.heap_values,
            baseline.names,
            baseline.sources,
            baseline.executable_units,
            baseline.roots
        )
    );
    let other = Context::new();
    let source = other.source("foreign.clj", "42");
    assert_eq!(c.eval_source(&source).unwrap_err().kind, E::WrongOwner);
    let form = other.reader(&source).unwrap().read_form().unwrap().unwrap();
    assert_eq!(c.eval_form(&form).unwrap_err().kind, E::WrongOwner);
}

#[test]
fn deeply_nested_accepted_source_evaluates_within_the_parser_bound() {
    let source = format!("{}42{}", "[".repeat(120), "]".repeat(120));
    let mut result = eval_string(&source).unwrap();
    for _ in 0..120 {
        result = result.nth(0).unwrap().unwrap();
    }
    assert_eq!(result.inspect(), ValueView::Integer(42));
}
