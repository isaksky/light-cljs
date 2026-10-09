use light_cljs::interpreter::{
    Context, Form, FormKind, LowerErrorKind as L, Name, Origin, ReadError, ReadErrorKind as E,
    ReaderLimits, RootedValue, UnsupportedSyntax as U, ValueView,
};
use light_cljs::{
    parse,
    parser::{CollectionKind, ErrorKind, Node, NodeId, NodeKind, SyntaxTree},
};
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn read(c: &Context, text: &str) -> Result<Form, ReadError> {
    let source = c.source("reader.clj", text);
    c.reader(&source)
        .unwrap()
        .read_form()?
        .ok_or_else(|| panic!("expected a form"))
}
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
fn expected(id: &str, actual: &RootedValue) {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let case = cases.iter().find(|c| c.id == id).unwrap();
    assert_eq!(
        case.outcome,
        protocol::Outcome::Returned(wire(actual)),
        "{id}"
    );
}
fn symbol(form: &Form) -> Name {
    let ValueView::Symbol(name) = form.data().inspect() else {
        panic!("expected symbol")
    };
    name
}
fn error(text: &str, kind: L) -> ReadError {
    let e = read(&Context::new(), text).unwrap_err();
    assert_eq!(e.kind, E::Lower(kind), "{text}");
    e
}

#[test]
fn ordinary_source_values_and_quote_nesting_match_pinned_sci() {
    let c = Context::new();
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    for id in [
        "sci-reader-ordinary",
        "sci-reader-quotes",
        "sci-reader-deref",
        "sci-reader-var",
        "sci-reader-var-data",
        "sci-reader-discard",
        "sci-reader-numeric-keys",
        "sci-reader-parse-form",
    ] {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        let form = read(&c, case.source.strip_prefix('\'').unwrap()).unwrap();
        expected(id, &form.data());
    }
    let nil = read(&c, "nil").unwrap();
    let empty = read(&c, "()").unwrap();
    assert!(!nil.data().value_eq(&empty.data()));
    assert_eq!(empty.data().inspect(), ValueView::List(0));
    let quoted = read(&c, "''[x (observe 9)]").unwrap();
    assert_eq!(quoted.node().origin, Origin::Quote);
    assert_eq!(quoted.children()[0].span().text(), "''[x (observe 9)]");
    assert_eq!(quoted.children()[1].node().origin, Origin::Quote);
    assert_eq!(
        quoted.children()[1].children()[1].children()[0]
            .span()
            .text(),
        "x"
    );
    assert_eq!(symbol(&quoted.children()[0]), Name::new(None, "quote"));
}

#[test]
fn incremental_read_adapts_sci_parse_test_and_eof_value_assertions() {
    let c = Context::new();
    let source = c.source("parse.clj", "(defn foo [] 1) (foo) nil 42");
    let mut r = c.reader(&source).unwrap();
    expected(
        "sci-reader-parse-form",
        &r.read_form().unwrap().unwrap().data(),
    );
    assert_eq!(r.read_form().unwrap().unwrap().span().text(), "(foo)");
    let nil = r.read_form().unwrap().unwrap().data();
    let number = r.read_form().unwrap().unwrap().data();
    assert!(r.read_form().unwrap().is_none());
    assert!(r.read_form().unwrap().is_none());
    expected(
        "sci-reader-eof",
        &c.vector(&[nil, number, c.keyword(Name::new(None, "eof"))])
            .unwrap(),
    );
    let source = c.source("source.clj", "1 {:a   1} [1  2 3]");
    let mut r = c.reader(&source).unwrap();
    for text in ["1", "{:a   1}", "[1  2 3]"] {
        assert_eq!(r.read_form().unwrap().unwrap().span().text(), text);
    }
    assert!(r.read_form().unwrap().is_none());
}

#[test]
fn metadata_shorthand_merging_and_target_validation_match_pinned_reader() {
    let c = Context::new();
    for (id, source, tag_only) in [
        ("sci-reader-meta-keyword", "^:private x", false),
        ("sci-reader-meta-symbol", "^String x", true),
        ("sci-reader-meta-string", "^\"String\" []", true),
        ("sci-reader-meta-merge", "^{:a 1} ^{:a 2 :b 3} []", false),
    ] {
        let form = read(&c, source).unwrap();
        let meta = form.data().metadata().unwrap();
        expected(
            id,
            &if tag_only {
                meta.get(&c.keyword(Name::new(None, "tag")))
                    .unwrap()
                    .unwrap()
            } else {
                meta.clone()
            },
        );
        assert!(form.metadata().unwrap().data().value_eq(&meta));
        assert_eq!(form.node().origin, Origin::Metadata);
    }
    for source in [
        "^:x nil",
        "^:x true",
        "^:x 1",
        "^:x :a",
        "^:x \"a\"",
        "^:x \\a",
        "#_^:x 1 2",
    ] {
        error(source, L::InvalidMetadataTarget);
    }
    for source in [
        "^:x ()",
        "^:x []",
        "^:x {}",
        "^:x #{}",
        "^:x symbol",
        "^:x 'a",
    ] {
        assert!(read(&c, source).is_ok(), "{source}");
    }
    let form = read(&c, "^{:m (observe 1)} [2]").unwrap();
    assert_eq!(
        form.metadata().unwrap().children()[1].span().text(),
        "(observe 1)"
    );
    let e = read(&c, "^42 x").unwrap_err();
    assert_eq!(e.kind, E::Syntax(ErrorKind::InvalidMetadata));
    assert_eq!(e.span.text(), "42");
}

#[test]
fn duplicate_literals_use_value_equality_ignore_metadata_and_keep_related_locations() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    for id in [
        "sci-reader-duplicate-map",
        "sci-reader-duplicate-set",
        "sci-reader-duplicate-meta",
        "sci-reader-duplicate-discard",
        "sci-reader-anonymous-duplicate",
    ] {
        let case = cases.iter().find(|c| c.id == id).unwrap();
        assert_eq!(
            case.outcome,
            protocol::Outcome::Error {
                phase: protocol::Phase::Read,
                category: protocol::Category::Reader
            }
        );
        let e = read(
            &Context::new(),
            case.source.strip_prefix('\'').unwrap_or(case.source),
        )
        .unwrap_err();
        assert!(matches!(
            e.kind,
            E::Lower(L::DuplicateMapKey | L::DuplicateSetMember)
        ));
    }
    for (source, kind) in [
        ("{:a 1 :a 2}", L::DuplicateMapKey),
        ("{[1] 2 (1) 3}", L::DuplicateMapKey),
        ("{1 2 2/2 3}", L::DuplicateMapKey),
        ("{^:a [1] 2 ^:b (1) 3}", L::DuplicateMapKey),
        ("#{1 1}", L::DuplicateSetMember),
        ("#{[1] (1)}", L::DuplicateSetMember),
        ("'#{'x (quote x)}", L::DuplicateSetMember),
        ("#_#{1 1} 2", L::DuplicateSetMember),
        ("[#_{:a 1 :a 2} 3]", L::DuplicateMapKey),
        ("#_#_#{1 1} 2 3", L::DuplicateSetMember),
    ] {
        let e = error(source, kind);
        assert!(e.related.is_some());
    }
    let e = error("{\n :a 1\n :a 2}", L::DuplicateMapKey);
    assert_eq!(e.span.text(), ":a");
    assert_eq!(
        e.span.source().line_column(e.span.bytes().start),
        Some((3, 2))
    );
    assert_eq!(e.related.unwrap().bytes(), 3..5);
    for source in [
        "{1 1 1.0 2}",
        "#{1 1.0}",
        "#{##NaN ##NaN}",
        "{##NaN 1 ##NaN 2}",
    ] {
        assert!(read(&Context::new(), source).is_ok(), "{source}");
    }
}

#[test]
fn anonymous_parameter_relationships_gaps_rest_and_discard_match_sci() {
    let c = Context::new();
    let f = read(&c, "#(% %3 %1 %&)").unwrap();
    let parts = f.children();
    let params = parts[1].children();
    let body = parts[2].children();
    expected(
        "sci-reader-anonymous",
        &c.vector(&[
            parts[0].data(),
            c.integer(params.len() as i64),
            c.boolean(
                params[0].data().value_eq(&body[0].data())
                    && params[0].data().value_eq(&body[2].data()),
            ),
            c.boolean(params[2].data().value_eq(&body[1].data())),
            c.boolean(params[4].data().value_eq(&body[3].data())),
            params[3].data(),
        ])
        .unwrap(),
    );
    assert_eq!(f.node().origin, Origin::AnonymousFunction);
    assert_eq!(body[1].span().text(), "%3");
    assert_eq!(params[1].span().text(), "#(% %3 %1 %&)");
    for (source, id) in [
        ("#(%2 #_%4)", "sci-reader-anonymous-discard"),
        ("#(%0 %-1)", "sci-reader-anonymous-nonpositive"),
    ] {
        let f = read(&c, source).unwrap().children();
        expected(
            id,
            &c.vector(&[
                c.integer(f[1].children().len() as i64),
                c.integer(f[2].children().len() as i64),
            ])
            .unwrap(),
        );
    }
    let f = read(&c, "#(quote %3)").unwrap().children();
    expected(
        "sci-reader-anonymous-quoted",
        &c.vector(&[
            c.integer(f[1].children().len() as i64),
            c.boolean(
                f[1].children()[2]
                    .data()
                    .value_eq(&f[2].children()[1].data()),
            ),
        ])
        .unwrap(),
    );
    let f = read(&c, "#(%1/1 %3/2)").unwrap().children();
    expected(
        "sci-reader-anonymous-ratio",
        &c.vector(&[c.integer(f[1].children().len() as i64), f[2].data()])
            .unwrap(),
    );
    expected(
        "sci-reader-anonymous-empty",
        &read(&c, "(#())").unwrap().data(),
    );
    // SCI uses canonical placeholder symbols; equal expansions are equal data.
    let second = read(&c, "#(%)").unwrap().children()[1].children()[0].data();
    assert!(second.value_eq(&params[0].data()));
    expected(
        "sci-reader-anonymous-equal",
        &c.boolean(
            read(&c, "#(%)")
                .unwrap()
                .data()
                .value_eq(&read(&c, "#(%)").unwrap().data()),
        ),
    );
    error("#{#(%) #(%)}", L::DuplicateSetMember);
    for (source, n) in [("#(%+3)", 3), ("#(%010)", 10), ("#(%01)", 1)] {
        assert_eq!(read(&c, source).unwrap().children()[1].children().len(), n);
    }
    for source in ["#(%1N)", "#(%0x3)", "#(%2r11)", "#(%9223372036854775808)"] {
        error(source, L::InvalidAnonymousArgument);
    }
    error("#(%1025)", L::AnonymousParameterLimit);
    assert!(read(&c, "#(#_%1N #_%0x3)").is_ok());
    let source = c.source("bounded.clj", "#(%4)");
    let e = c
        .reader_with_limits(
            &source,
            ReaderLimits {
                max_anonymous_parameters: 3,
            },
        )
        .unwrap()
        .read_form()
        .unwrap_err();
    assert_eq!(e.kind, E::Lower(L::AnonymousParameterLimit));
    assert_eq!(e.span.text(), "%4");
    assert_eq!(
        read(&c, "#(#())").unwrap_err().kind,
        E::Syntax(ErrorKind::NestedAnonymousFunction)
    );
}

#[test]
fn numeric_conversion_is_checked_and_utf16_is_exact() {
    let c = Context::new();
    for (text, n) in [
        ("+0", 0),
        ("-0x8000000000000000", i64::MIN),
        ("+9223372036854775807", i64::MAX),
        ("18446744073709551614/2", i64::MAX),
        ("-18446744073709551616/2", i64::MIN),
        ("0/999999999999999999999999999999999999", 0),
    ] {
        assert_eq!(
            read(&c, text).unwrap().data().inspect(),
            ValueView::Integer(n)
        );
    }
    for text in [
        "9223372036854775808",
        "-9223372036854775809",
        "0xffffffffffffffff",
        "1e999",
        "1/9223372036854775808",
        "999999999999999999999999999999999999999999999999/2",
    ] {
        error(text, L::NumericOverflow);
    }
    error("1N", L::Unsupported(U::BigInteger));
    error("1M", L::Unsupported(U::Decimal));
    assert_eq!(
        read(&c, "\"a\\ud800\\udc00\\udfff\"")
            .unwrap()
            .data()
            .inspect(),
        ValueView::String(vec![97, 0xd800, 0xdc00, 0xdfff].into())
    );
    for (text, value) in [
        ("##Inf", f64::INFINITY),
        ("##-Inf", f64::NEG_INFINITY),
        ("-0.0", -0.0),
    ] {
        let ValueView::Double(actual) = read(&c, text).unwrap().data().inspect() else {
            panic!()
        };
        assert_eq!(actual.to_bits(), value.to_bits());
    }
}

#[test]
fn unsupported_features_and_read_eval_have_specific_source_diagnostics() {
    error("#\"x\"", L::Unsupported(U::Regex));
    error("#tag 1", L::UnknownTag(Box::new(Name::new(None, "tag"))));
    let c = Context::new();
    for source in [
        "#=(observe 1)",
        "#_#=(observe 1) 2",
        "#?(:clj 1 :cljs #=(observe 1))",
    ] {
        let e = read(&c, source).unwrap_err();
        assert_eq!(e.kind, E::Syntax(ErrorKind::ReadEvalDisabled));
        assert_eq!(e.span.text(), "#=");
    }
    let source = c.source("errors.clj", "1\n[2)");
    let mut r = c.reader(&source).unwrap();
    assert_eq!(
        r.read_form().unwrap().unwrap().data().inspect(),
        ValueView::Integer(1)
    );
    let e = r.read_form().unwrap_err();
    assert_eq!(e.span.text(), ")");
    assert_eq!(e.related.unwrap().text(), "[");
    assert!(r.read_form().unwrap().is_none());
    let source = c.source("lower.clj", "nil\n^:a 3 4");
    let mut r = c.reader(&source).unwrap();
    assert!(r.read_form().unwrap().is_some());
    let e = r.read_form().unwrap_err();
    assert_eq!(e.span.text(), "3");
    assert!(r.read_form().unwrap().is_none());
    assert_eq!(
        format!("{e}"),
        "lower.clj:2:5: Lower(InvalidMetadataTarget)"
    );
    let source = c.source("eof.clj", "1 #_#{2 2}");
    let mut r = c.reader(&source).unwrap();
    r.read_form().unwrap();
    assert!(r.read_form().is_err());
}

#[test]
fn source_order_and_forms_survive_syntax_source_and_context_lifetimes() {
    let mut c = Context::new();
    let form = {
        let source = c.source("retained.clj", "{:z (observe 1) :a (observe 2)}");
        let tree = parse(source.text()).unwrap();
        c.lower_form(&source, &tree, tree.roots[0]).unwrap()
    };
    let children = form.children();
    assert_eq!(
        children
            .iter()
            .map(|f| f.span().text().to_owned())
            .collect::<Vec<_>>(),
        [":z", "(observe 1)", ":a", "(observe 2)"]
    );
    assert!(matches!(
        form.node().kind,
        FormKind::Collection {
            kind: CollectionKind::Map,
            ..
        }
    ));
    let value = form.data();
    let span = form.span();
    let supplied = c.form_from_data(&value, &span).unwrap();
    assert!(supplied.data().value_eq(&value));
    assert_eq!(supplied.node().origin, Origin::SuppliedData);
    c.collect_garbage();
    c.reset();
    drop(c);
    assert_eq!(value.count(), Some(2));
    assert_eq!(form.span().source().name(), "retained.clj");
    assert_eq!(supplied.data().count(), Some(2));
}

#[test]
fn supplied_data_preserves_metadata_surrogates_and_owner_checks() {
    let c = Context::new();
    let source = c.source("host", "");
    let name = c.symbol(Name::from_utf16(None, &[0xd800]));
    let meta = c
        .map(&[(c.keyword(Name::new(None, "host")), c.boolean(true))])
        .unwrap();
    let value = c
        .with_meta(&c.list(&[name, c.string(vec![0xdc00])]).unwrap(), &meta)
        .unwrap();
    let form = c
        .form_from_data(&value, &source.span(0..0).unwrap())
        .unwrap();
    assert!(form.data().value_eq(&value));
    assert!(form.metadata().unwrap().data().value_eq(&meta));
    let foreign = Context::new();
    assert!(foreign.reader(&source).is_err());
    assert_eq!(
        foreign
            .form_from_data(&value, &source.span(0..0).unwrap())
            .unwrap_err()
            .kind,
        E::Lower(L::WrongOwner)
    );
    let tree = SyntaxTree {
        nodes: vec![Node {
            kind: NodeKind::Collection {
                kind: CollectionKind::List,
                children: vec![NodeId(9)],
            },
            span: 0..0,
        }],
        roots: vec![NodeId(0)],
    };
    assert_eq!(
        c.lower_form(&source, &tree, NodeId(0)).unwrap_err().kind,
        E::Lower(L::InvalidSyntax)
    );
}

#[test]
fn retained_discard_side_table_is_opt_in_and_handles_symbolic_literal_discards() {
    let c = Context::new();
    let source = c.source("discard.clj", "#_[:x #_1 2] ## #_3 Inf");
    let plain = parse(source.text()).unwrap();
    assert_eq!(plain.nodes.len(), 1);
    let mut parser = light_cljs::Parser::new(source.text()).retaining_discards();
    let root = parser.read_form().unwrap().unwrap();
    assert!(
        parser
            .discarded()
            .iter()
            .all(|id| id.0 < parser.nodes().len())
    );
    assert_eq!(
        parser.nodes()[root.0].kind,
        NodeKind::SymbolicValue(light_cljs::parser::SymbolicValue::Infinity)
    );
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    assert_eq!(form.data().inspect(), ValueView::Double(f64::INFINITY));
    assert_eq!(
        form.nodes().len(),
        1,
        "discarded forms are not retained by semantic storage"
    );
}

#[test]
fn discarded_forms_are_reclaimed_and_invalid_truncations_never_panic() {
    let mut c = Context::new();
    for _ in 0..20 {
        let source = c.source(
            "repl.clj",
            "#_[:discarded {:key \"value\"}] ^:kept '(a [1 2])",
        );
        let mut reader = c.reader(&source).unwrap();
        let form = reader.read_form().unwrap().unwrap();
        c.collect_garbage();
        assert_eq!(
            form.children()[1].children()[0].data().inspect(),
            ValueView::Symbol(Name::new(None, "a"))
        );
        assert!(reader.read_form().unwrap().is_none());
    }
    c.collect_garbage();
    assert_eq!(c.memory_stats().heap_values, 0);
    assert_eq!(c.memory_stats().sources, 0);
    assert_eq!(c.memory_stats().names, 0);
    assert_eq!(c.memory_stats().executable_units, 0);
    let sample = "#_[#_λ 3] [^:x 'λ #(%3 %& #_%9) {\"a\\ud800\" 6/4} ## #_9 Inf]";
    for end in (0..=sample.len()).filter(|&i| sample.is_char_boundary(i)) {
        let source = c.source("truncated.clj", &sample[..end]);
        let mut reader = c.reader(&source).unwrap();
        loop {
            match reader.read_form() {
                Ok(Some(_)) => (),
                Ok(None) => break,
                Err(e) => {
                    assert!(source.text().get(e.span.bytes()).is_some());
                    if let Some(related) = e.related {
                        assert!(source.text().get(related.bytes()).is_some());
                    }
                    assert!(reader.read_form().unwrap().is_none());
                    break;
                }
            }
        }
    }
}
