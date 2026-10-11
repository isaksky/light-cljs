use light_cljs::{
    CollectionKind as C, ErrorKind as E, LineIndex, NodeId, NodeKind as N, NumberKind as Num,
    Parser, PrefixKind as P, StringValue, SyntaxTree, parse,
};

fn root_kind(source: &str) -> N {
    let tree = parse(source).unwrap_or_else(|e| panic!("{source:?}: {e:?}"));
    assert_eq!(tree.roots.len(), 1, "{source:?}");
    tree.nodes[tree.roots[0].0].kind.clone()
}

fn children(tree: &SyntaxTree, id: NodeId) -> &[NodeId] {
    let N::Collection { children, .. } = &tree.nodes[id.0].kind else {
        panic!()
    };
    children
}

fn error(source: &str, expected: E) {
    let err = parse(source).unwrap_err();
    assert_eq!(err.kind, expected, "{source:?}: {err:?}");
    assert!(source.get(err.span).is_some());
    if let Some(span) = err.related {
        assert!(source.get(span).is_some());
    }
}

fn check_tree(source: &str, tree: &SyntaxTree) {
    for (index, node) in tree.nodes.iter().enumerate() {
        assert!(node.span.start < node.span.end);
        assert!(source.get(node.span.clone()).is_some());
        let child_ids: Vec<_> = match &node.kind {
            N::Collection { children, .. } => children.clone(),
            N::Prefix { form, .. } => vec![*form],
            N::Metadata { metadata, target } => vec![*metadata, *target],
            N::Tagged { tag, form } => vec![*tag, *form],
            N::Conditional { branches, .. } => vec![*branches],
            N::NamespacedMap { map, .. } => vec![*map],
            N::Argument {
                index: Some(id), ..
            } => vec![*id],
            _ => vec![],
        };
        for id in child_ids {
            assert!(id.0 < index);
            let span = &tree.nodes[id.0].span;
            assert!(node.span.start <= span.start && span.end <= node.span.end);
        }
    }
    for root in &tree.roots {
        assert!(root.0 < tree.nodes.len());
    }
}

#[test]
fn empty_comments_commas_and_fused_eof() {
    for source in ["", " \t,\n\u{2003}", "; hi", "#!/bin/cljs\r\n#_ 1"] {
        assert!(parse(source).unwrap().nodes.is_empty());
        let mut parser = Parser::new(source);
        for _ in 0..4 {
            assert_eq!(parser.read_form(), Ok(None));
        }
    }
}

#[test]
fn example_arena_and_incremental_top_level_forms() {
    let source = "(+ 1 2) café";
    let mut parser = Parser::new(source);
    let first = parser.read_form().unwrap().unwrap();
    assert_eq!(first, NodeId(3));
    assert_eq!(parser.nodes().len(), 4);
    assert_eq!(parser.nodes()[first.0].span, 0..7);
    assert_eq!(parser.nodes()[0].kind, N::Symbol);
    assert_eq!(parser.nodes()[1].kind, N::Number(Num::Integer));
    let second = parser.read_form().unwrap().unwrap();
    assert_eq!(&source[parser.nodes()[second.0].span.clone()], "café");
    assert_eq!(parser.read_form(), Ok(None));
    assert_eq!(parser.read_form(), Ok(None));
}

#[test]
fn collections_and_wide_siblings() {
    let source = "([] {} #{λ café} (()()))";
    let tree = parse(source).unwrap();
    check_tree(source, &tree);
    assert_eq!(children(&tree, tree.roots[0]).len(), 4);
    for (source, expected) in [
        ("()", C::List),
        ("[]", C::Vector),
        ("{}", C::Map),
        ("#{}", C::Set),
        ("#()", C::AnonymousFn),
    ] {
        assert!(
            matches!(root_kind(source), N::Collection { kind, children } if kind == expected && children.is_empty())
        );
    }
    let wide = format!("[{}]", "x ".repeat(100_000));
    let tree = parse(&wide).unwrap();
    assert_eq!(children(&tree, tree.roots[0]).len(), 100_000);
}

#[test]
fn symbols_keywords_literals_and_numbers() {
    for symbol in [
        "+", "-", "/", "ns//", "foo'", "foo#", "x:kw", "%1:kw", "café", "λ", "𐐀", "nilx",
    ] {
        assert_eq!(root_kind(symbol), N::Symbol);
    }
    for keyword in [":x", ":ns/name", ":1", ":/", ":a:b"] {
        assert_eq!(
            root_kind(keyword),
            N::Keyword {
                auto_resolve: false
            }
        );
    }
    for keyword in ["::x", "::alias/name"] {
        assert_eq!(root_kind(keyword), N::Keyword { auto_resolve: true });
    }
    assert_eq!(root_kind("nil"), N::Nil);
    assert_eq!(root_kind("true"), N::Boolean(true));
    assert_eq!(root_kind("false"), N::Boolean(false));
    for (kind, values) in [
        (
            Num::Integer,
            &[
                "0",
                "-0",
                "+42",
                "077",
                "0xff",
                "0XFE",
                "2r101",
                "36rN",
                "36r10N",
                "9999999999999999999999999999",
            ][..],
        ),
        (Num::BigInteger, &["42N", "0xFFN", "077N"][..]),
        (Num::Float, &["1.0", "2.", "2e3", "-1E-3", "08.0"][..]),
        (Num::Decimal, &["1M", "1.25M", "1e-10M", "09M"][..]),
        (Num::Ratio, &["1/2", "-13/2", "+01/002"][..]),
    ] {
        for value in values {
            assert_eq!(root_kind(value), N::Number(kind), "{value}");
        }
    }
    for value in [
        "1x", "09", "09N", "0x", "0xG", "1e", "2e+", "1.2.3", "2r102", "2r10N", "1r0", "37r0",
        "1/0", "1/000", "1/-2", "١", "1Nf",
    ] {
        error(value, E::InvalidNumber);
    }
    for value in ["a/", "/a", "a/b/c", "a/1", "a:"] {
        error(value, E::InvalidSymbol);
    }
    for value in [":", "::", ":::x", ":a:", ":a/", ":a/1"] {
        error(value, E::InvalidKeyword);
    }
}

#[test]
fn strings_decode_utf16_and_preserve_unescaped_spans() {
    let source = "\"café λ 😀\"";
    let N::String(value) = root_kind(source) else {
        panic!()
    };
    assert_eq!(value, StringValue::Source(1..source.len() - 1));
    assert_eq!(
        value.to_utf16(source),
        "café λ 😀".encode_utf16().collect::<Vec<_>>()
    );
    for (source, expected) in [
        (r#""\t\r\n\\\"\b\f\u03bb\141""#, "\t\r\n\\\"\u{8}\u{c}λa"),
        ("\"a\r\nb\r\x0cc\rd\"", "a\nb\nc\nd"),
        (r#""\1 \12 \1234""#, "\u{1} \n S4"),
        (r#""\uＦＦ４１\u٠٠٤١""#, "ａA"),
    ] {
        let N::String(value) = root_kind(source) else {
            panic!()
        };
        assert_eq!(
            value.to_utf16(source),
            expected.encode_utf16().collect::<Vec<_>>()
        );
    }
    let source = r#""\uD800\uDC00\uDFFF""#;
    let N::String(value) = root_kind(source) else {
        panic!()
    };
    assert_eq!(value.to_utf16(source), [0xd800, 0xdc00, 0xdfff]);
    for source in [
        r#""\q""#,
        r#""\u123""#,
        r#""\u12zz""#,
        r#""\400""#,
        r#""\8""#,
        r#""\12a""#,
    ] {
        error(source, E::InvalidEscape);
    }
}

#[test]
fn characters_and_raw_regexes() {
    for (source, expected) in [
        (r"\λ", 'λ'),
        (r"\)", ')'),
        (r"\space", ' '),
        (r"\newline", '\n'),
        (r"\return", '\r'),
        (r"\o141", 'a'),
        (r"\u03bb", 'λ'),
        ("\\\r\n", '\n'),
        (r"\uＦＦ４１", 'ａ'),
    ] {
        assert_eq!(root_kind(source), N::Character(expected));
    }
    for source in [
        r"\unknown",
        r"\uD800",
        r"\u123",
        r"\u12345",
        r"\o400",
        r"\o7777",
        r"\😀",
    ] {
        error(source, E::InvalidCharacter);
    }
    assert_eq!(root_kind(r##"#"\d+\s*[()\"]""##), N::Regex);
    // Target-specific regex validation is explicitly deferred.
    assert_eq!(root_kind(r##"#"[""##), N::Regex);
}

#[test]
fn prefixes_metadata_tags_namespaces_and_specials() {
    for (source, expected) in [
        ("'x", P::Quote),
        ("`x", P::SyntaxQuote),
        ("~x", P::Unquote),
        ("~@x", P::UnquoteSplicing),
        ("@x", P::Deref),
        ("#'x", P::VarQuote),
    ] {
        assert!(matches!(root_kind(source), N::Prefix { kind, .. } if kind == expected));
    }
    for source in ["^:private x", "#^String x", "^{:doc \"x\"} [a]"] {
        assert!(matches!(root_kind(source), N::Metadata { .. }));
    }
    for source in ["#js [1 2]", "#app/tag {:x 1}", "#uuid \"x\""] {
        assert!(matches!(root_kind(source), N::Tagged { .. }));
    }
    for (source, auto, ns) in [
        ("#:app{:x 1}", false, "app"),
        ("#::alias {:x 2}", true, "alias"),
        ("#::{:x 3}", true, ""),
        ("#:1foo#x{:a 1}", false, "1foo#x"),
    ] {
        let N::NamespacedMap {
            auto_resolve,
            namespace,
            ..
        } = root_kind(source)
        else {
            panic!()
        };
        assert_eq!(auto_resolve, auto);
        assert_eq!(&source[namespace], ns);
    }
    for source in ["##Inf", "##-Inf", "##NaN", "## #_ x Inf"] {
        let tree = parse(source).unwrap();
        assert_eq!(tree.nodes.len(), 1);
        assert!(matches!(root_kind(source), N::SymbolicValue(_)));
    }
    error("^1 x", E::InvalidMetadata);
    error("#42 x", E::InvalidTag);
    error("#:{}", E::InvalidNamespace);
    error("#:a/b {}", E::InvalidNamespace);
    error("#::a []", E::InvalidNamespace);
    error("##Other", E::InvalidSymbolicValue);
    error("#=(+ 1 2)", E::ReadEvalDisabled);
}

#[test]
fn discards_consume_whole_forms_without_retaining_nodes() {
    for source in [
        "#_1 :kept",
        "#_#_1 2 :kept",
        "#_ '(a [b]) :kept",
        "#_#_#_1 2 3 :kept",
        "#_ ; hi\n ^:x y :kept",
    ] {
        let tree = parse(source).unwrap();
        assert_eq!(tree.nodes.len(), 1, "{source}");
        assert_eq!(&source[tree.nodes[0].span.clone()], ":kept");
    }
    let source = "[1 #_ [a b] 2 #_ 3]";
    let tree = parse(source).unwrap();
    assert_eq!(children(&tree, tree.roots[0]).len(), 2);
    assert_eq!(tree.nodes.len(), 3);
    assert!(matches!(root_kind("'#_x y"), N::Prefix { .. }));
    error("[#_1] #_", E::UnexpectedEof("a form"));
    error("[#_]", E::UnexpectedDelimiter(']'));
    error("#_ \"\\q\" x", E::InvalidEscape);
}

#[test]
fn anonymous_arguments_use_contextual_boundaries() {
    let source = "#(% %1 %&rest %1:kw)";
    let tree = parse(source).unwrap();
    let members = children(&tree, tree.roots[0]);
    let spellings: Vec<_> = members
        .iter()
        .map(|id| &source[tree.nodes[id.0].span.clone()])
        .collect();
    assert_eq!(spellings, ["%", "%1", "%&", "rest", "%1", ":kw"]);
    assert!(matches!(
        tree.nodes[members[0].0].kind,
        N::Argument {
            rest: false,
            index: None
        }
    ));
    assert!(matches!(
        tree.nodes[members[2].0].kind,
        N::Argument {
            rest: true,
            index: None
        }
    ));
    check_tree(source, &tree);
    assert_eq!(root_kind("%&rest"), N::Symbol);
    assert!(parse("#(%1/1)").is_ok());
    // Reduction/type checking of ratio indices belongs to expansion.
    assert!(parse("#(%3/2)").is_ok());
    assert_eq!(parse("#(%2r10) #(%-1) %").unwrap().roots.len(), 3);
    let source = "#(%0 %-1 %-2 %& %2)";
    let tree = parse(source).unwrap();
    check_tree(source, &tree);
    for (id, text) in children(&tree, tree.roots[0])
        .iter()
        .zip(["%0", "%-1", "%-2", "%&", "%2"])
    {
        assert_eq!(&source[tree.nodes[id.0].span.clone()], text);
        assert!(matches!(tree.nodes[id.0].kind, N::Argument { .. }));
    }
    error("#(#())", E::NestedAnonymousFunction);
    error("#(%foo)", E::InvalidArgument);
    error("#(%1.2)", E::InvalidArgument);
}

#[test]
fn conditionals_preserve_branches_and_defer_collection_arity() {
    assert!(matches!(
        root_kind("#?(:cljs x :clj y)"),
        N::Conditional {
            splicing: false,
            ..
        }
    ));
    for source in [
        "[#?@(:cljs [1 2])]",
        "{#?@(:cljs [:a 1])}",
        "{#?(:cljs :a) 1}",
        "{#?(:cljs [:a 1])}",
    ] {
        let tree = parse(source).unwrap();
        check_tree(source, &tree);
    }
    for source in [
        "#?@(:cljs [])",
        "#?[]",
        "#?(:cljs)",
        "#?(x 1)",
        "#?(:else 1)",
    ] {
        error(source, E::InvalidConditional);
    }
    error("{:a}", E::OddMap);
    error("#:a{:b}", E::OddMap);
}

#[test]
fn errors_have_locations_openers_and_terminal_behavior() {
    let err = parse("(λ\n ]").unwrap_err();
    assert_eq!(
        err.kind,
        E::MismatchedDelimiter {
            expected: ')',
            found: ']'
        }
    );
    assert_eq!(err.span, 5..6);
    assert_eq!(err.related, Some(0..1));
    error(")", E::UnexpectedDelimiter(')'));
    error("[", E::UnexpectedEof("a closing delimiter"));
    error("'", E::UnexpectedEof("a form"));
    error("\"x", E::Lexical(light_cljs::LexError::UnterminatedString));
    let mut parser = Parser::new("x [)");
    let first = parser.read_form().unwrap().unwrap();
    assert!(parser.read_form().is_err());
    assert_eq!(parser.nodes().len(), 1);
    assert_eq!(parser.nodes()[first.0].kind, N::Symbol);
    for _ in 0..3 {
        assert_eq!(parser.read_form(), Ok(None));
    }
    let source = "λ\r\nx\r\x0cy\rz\n";
    let lines = LineIndex::new(source);
    for (offset, expected) in [
        (0, (1, 1)),
        (2, (1, 3)),
        (4, (2, 1)),
        (7, (3, 1)),
        (9, (4, 1)),
        (11, (5, 1)),
    ] {
        assert_eq!(lines.line_column(offset), Some(expected));
    }
    assert_eq!(lines.line_column(source.len() + 1), None);
}

#[test]
fn nesting_limit_includes_collections_prefixes_and_discards() {
    for source in [
        format!("{}x{}", "(".repeat(1000), ")".repeat(1000)),
        format!("{}x", "'".repeat(1000)),
        format!("{}x", "#_".repeat(1000)),
    ] {
        error(&source, E::NestingLimit);
    }
    let mut parser = Parser::with_nesting_limit("[x]", 1);
    assert_eq!(parser.read_form().unwrap_err().kind, E::NestingLimit);
    assert!(
        Parser::with_nesting_limit("[]", 1)
            .read_form()
            .unwrap()
            .is_some()
    );
    assert!(
        Parser::with_nesting_limit("", 0)
            .read_form()
            .unwrap()
            .is_none()
    );
}

#[test]
fn malformed_combinations_and_truncations_keep_valid_spans() {
    let fragments = [
        "λ", "😀", "1", "0x", ":", "(", ")", "[", "]", "{", "}", "\"", "\\", "#_", "'", "#?", "#(",
        "%&rest", ",", "\n", ";",
    ];
    for a in fragments {
        for b in fragments {
            for c in fragments {
                let source = format!("{a}{b}{c}");
                match parse(&source) {
                    Ok(tree) => check_tree(&source, &tree),
                    Err(err) => {
                        assert!(source.get(err.span).is_some());
                        if let Some(span) = err.related {
                            assert!(source.get(span).is_some());
                        }
                    }
                }
            }
        }
    }
    let source = include_str!("fixtures/parser_forms.cljs");
    for (end, _) in source
        .char_indices()
        .chain(std::iter::once((source.len(), '\0')))
    {
        match parse(&source[..end]) {
            Ok(tree) => check_tree(&source[..end], &tree),
            Err(err) => assert!(source[..end].get(err.span).is_some()),
        }
    }
}

#[test]
fn every_reader_family_truncation_preserves_incremental_progress_and_fused_failure() {
    let forms = [
        r#"["λ😀\uD800\uDC00" \u03bb \o141 36r10N]"#,
        "^:flag 'café",
        "`[x# ~λ ~@xs]",
        "#(vector % %3 %&)",
        "#js {:λ [1 2]}",
        "#unknown/tag [λ]",
        "#::alias{:λ 1}",
        "[#_ #unknown/tag 1 2]",
        "[#?@(:cljs [λ] :default [2])]",
        "#?(:cljs 1 :clj [2])",
        "{1 2 1N 3}",
        "#{1 1.0}",
    ];
    for form in forms {
        for end in form
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(form.len()))
        {
            let source = format!("kept {}", &form[..end]);
            let mut parser = Parser::new(&source);
            let kept = parser.read_form().unwrap().unwrap();
            assert_eq!(parser.nodes()[kept.0].span, 0..4);
            match parser.read_form() {
                Ok(Some(id)) => {
                    assert!(parser.nodes()[id.0].span.start >= 5);
                    assert!(source.get(parser.nodes()[id.0].span.clone()).is_some());
                    assert_eq!(parser.read_form(), Ok(None));
                }
                Ok(None) => {}
                Err(error) => {
                    assert!(source.get(error.span).is_some(), "{source}");
                    if let Some(span) = error.related {
                        assert!(source.get(span).is_some());
                    }
                    assert_eq!(
                        parser.nodes().len(),
                        1,
                        "failed form must roll back: {source}"
                    );
                }
            }
            for _ in 0..3 {
                assert_eq!(parser.read_form(), Ok(None));
            }
        }
    }
}
