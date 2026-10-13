use light_cljs::{
    LexError::*,
    TokenKind::{self, *},
    Tokenizer,
};

fn check(source: &str, expected: &[(TokenKind, &str)]) {
    let tokens: Vec<_> = Tokenizer::new(source).collect();
    let actual: Vec<_> = tokens
        .iter()
        .map(|t| (t.kind, &source[t.span.clone()]))
        .collect();
    assert_eq!(actual, expected, "{source:?}");
    let mut cursor = 0;
    for (token, (_, text)) in tokens.iter().zip(expected) {
        cursor += source[cursor..].find(text).unwrap();
        assert_eq!(token.span, cursor..cursor + text.len());
        cursor += text.len();
    }
}

#[test]
fn all_collection_delimiters_without_matching() {
    check(
        "([{}])}{][)( #{a} #(%)",
        &[
            (OpenParen, "("),
            (OpenBracket, "["),
            (OpenBrace, "{"),
            (CloseBrace, "}"),
            (CloseBracket, "]"),
            (CloseParen, ")"),
            (CloseBrace, "}"),
            (OpenBrace, "{"),
            (CloseBracket, "]"),
            (OpenBracket, "["),
            (CloseParen, ")"),
            (OpenParen, "("),
            (SetStart, "#{"),
            (Atom, "a"),
            (CloseBrace, "}"),
            (AnonymousFnStart, "#("),
            (Atom, "%"),
            (CloseParen, ")"),
        ],
    );
}

#[test]
fn atoms_keep_classification_and_context_for_the_reader() {
    let words = [
        "nil",
        "true",
        "false",
        "42",
        "-1.5e+2",
        "+2",
        "0xFF",
        "2r101",
        "3/4",
        "10N",
        "1M",
        ":keyword",
        "::auto",
        "::alias/name",
        "ns/symbol",
        "/",
        "+",
        "-",
        "foo#",
        "foo'",
        "foo:bar",
        "foo%bar",
        "%",
        "%1",
        "%&",
        "café",
        "λ",
        "🦀",
    ];
    let source = words.join(", ");
    check(&source, &words.map(|w| (Atom, w)));
}

#[test]
fn number_candidates_have_different_terminators_than_symbols() {
    check(
        "1:kw 2'x -3#tag +4% 5@x 6^x 7`x 8~x foo#'bar:kw%",
        &[
            (Atom, "1"),
            (Atom, ":kw"),
            (Atom, "2"),
            (Quote, "'"),
            (Atom, "x"),
            (Atom, "-3"),
            (Dispatch, "#"),
            (Atom, "tag"),
            (Atom, "+4"),
            (Atom, "%"),
            (Atom, "5"),
            (Deref, "@"),
            (Atom, "x"),
            (Atom, "6"),
            (Metadata, "^"),
            (Atom, "x"),
            (Atom, "7"),
            (SyntaxQuote, "`"),
            (Atom, "x"),
            (Atom, "8"),
            (Unquote, "~"),
            (Atom, "x"),
            (Atom, "foo#'bar:kw%"),
        ],
    );
}

#[test]
fn quoted_literals_preserve_raw_escapes_and_embedded_syntax() {
    for literal in [
        r#""""#,
        r#""a \"quoted\" café \\ \n \u03bb \141""#,
        "\"line one\nline two\r\n\tλ\"",
        r#""(;#{[]},'~@^)""#,
        // Literal validity belongs to the reader, including unknown escapes.
        r#""\q \uZZZZ \400""#,
    ] {
        check(literal, &[(String, literal)]);
        let regex = format!("#{literal}");
        check(&regex, &[(Regex, &regex)]);
    }
    check(
        "\"λ\"x#\"re\"",
        &[(String, "\"λ\""), (Atom, "x#"), (String, "\"re\"")],
    );
    check(
        r##"#"\s+\d{2}\"[()];""##,
        &[(Regex, r##"#"\s+\d{2}\"[()];""##)],
    );
}

#[test]
fn backslash_parity_controls_string_and_regex_endings() {
    for count in 0..16 {
        let content = format!("\"{}\"", "\\".repeat(count));
        let kind = if count % 2 == 0 {
            String
        } else {
            Error(UnterminatedString)
        };
        check(&content, &[(kind, &content)]);
        let regex = format!("#{content}");
        let kind = if count % 2 == 0 {
            Regex
        } else {
            Error(UnterminatedRegex)
        };
        check(&regex, &[(kind, &regex)]);
    }
}

#[test]
fn character_literals_include_delimiters_and_names() {
    let literals = [
        r"\)",
        r"\(",
        r"\;",
        "\\\"",
        r"\\",
        r"\[",
        r"\]",
        r"\{",
        r"\}",
        r"\,",
        r"\@",
        r"\#",
        r"\^",
        r"\`",
        r"\~",
        r"\'",
        r"\%",
        r"\space",
        r"\newline",
        r"\tab",
        r"\backspace",
        r"\formfeed",
        r"\return",
        r"\u03bb",
        r"\o141",
        r"\λ",
        r"\🦀",
        r"\not-a-character",
        r"\uZZZZ",
    ];
    check(&literals.join(" "), &literals.map(|s| (Character, s)));
    check(
        "\\)x\\;y",
        &[
            (Character, "\\)"),
            (Atom, "x"),
            (Character, "\\;"),
            (Atom, "y"),
        ],
    );
    for literal in [
        "\\ ",
        "\\\t",
        "\\\n",
        "\\\r",
        "\\\r\n",
        "\\\r\x0c",
        "\\\u{2003}",
    ] {
        let source = format!("{literal}x");
        check(&source, &[(Character, literal), (Atom, "x")]);
    }
    check(
        "\\café(x)",
        &[
            (Character, "\\café"),
            (OpenParen, "("),
            (Atom, "x"),
            (CloseParen, ")"),
        ],
    );
}

#[test]
fn line_comments_preserve_text_and_stop_at_cr_or_lf() {
    check(
        "a; \"λ [#_\r\nb#! /bin/cljs\rc;tail",
        &[
            (Atom, "a"),
            (Comment, "; \"λ [#_"),
            (Atom, "b#!"),
            (Atom, "/bin/cljs"),
            (Atom, "c"),
            (Comment, ";tail"),
        ],
    );
    check(
        "#!/bin/cljs\n(x) #! end",
        &[
            (Comment, "#!/bin/cljs"),
            (OpenParen, "("),
            (Atom, "x"),
            (CloseParen, ")"),
            (Comment, "#! end"),
        ],
    );
    check(
        ";a\u{2028}b\n;",
        &[(Comment, ";a\u{2028}b"), (Comment, ";")],
    );
}

#[test]
fn quote_metadata_and_deref_prefixes() {
    check(
        "'x `x ~x ~@x @x ^:private x #^String x #'ns/x",
        &[
            (Quote, "'"),
            (Atom, "x"),
            (SyntaxQuote, "`"),
            (Atom, "x"),
            (Unquote, "~"),
            (Atom, "x"),
            (UnquoteSplicing, "~@"),
            (Atom, "x"),
            (Deref, "@"),
            (Atom, "x"),
            (Metadata, "^"),
            (Atom, ":private"),
            (Atom, "x"),
            (Metadata, "#^"),
            (Atom, "String"),
            (Atom, "x"),
            (VarQuote, "#'"),
            (Atom, "ns/x"),
        ],
    );
}

#[test]
fn discard_and_reader_conditionals_only_emit_prefixes() {
    check(
        "#_#_(x)y #?(:cljs a) #?@(:cljs [b]) #=(+ 1 2)",
        &[
            (Discard, "#_"),
            (Discard, "#_"),
            (OpenParen, "("),
            (Atom, "x"),
            (CloseParen, ")"),
            (Atom, "y"),
            (ReaderConditional, "#?"),
            (OpenParen, "("),
            (Atom, ":cljs"),
            (Atom, "a"),
            (CloseParen, ")"),
            (ReaderConditionalSplicing, "#?@"),
            (OpenParen, "("),
            (Atom, ":cljs"),
            (OpenBracket, "["),
            (Atom, "b"),
            (CloseBracket, "]"),
            (CloseParen, ")"),
            (ReadEval, "#="),
            (OpenParen, "("),
            (Atom, "+"),
            (Atom, "1"),
            (Atom, "2"),
            (CloseParen, ")"),
        ],
    );
}

#[test]
fn dispatch_tags_namespaced_maps_and_symbolic_values() {
    check(
        "#js [] #uuid \"id\" #foo/bar{} #λ x #:ns{} #::alias{} #::{} ##Inf ##-Inf ## NaN",
        &[
            (Dispatch, "#"),
            (Atom, "js"),
            (OpenBracket, "["),
            (CloseBracket, "]"),
            (Dispatch, "#"),
            (Atom, "uuid"),
            (String, "\"id\""),
            (Dispatch, "#"),
            (Atom, "foo/bar"),
            (OpenBrace, "{"),
            (CloseBrace, "}"),
            (Dispatch, "#"),
            (Atom, "λ"),
            (Atom, "x"),
            (NamespacedMap, "#:ns"),
            (OpenBrace, "{"),
            (CloseBrace, "}"),
            (AutoNamespacedMap, "#::alias"),
            (OpenBrace, "{"),
            (CloseBrace, "}"),
            (AutoNamespacedMap, "#::"),
            (OpenBrace, "{"),
            (CloseBrace, "}"),
            (SymbolicValue, "##"),
            (Atom, "Inf"),
            (SymbolicValue, "##"),
            (Atom, "-Inf"),
            (SymbolicValue, "##"),
            (Atom, "NaN"),
        ],
    );
}

#[test]
fn whitespace_matches_jvm_reader_including_unicode_exceptions() {
    let whitespace = [
        "\t", "\n", "\x0b", "\x0c", "\r", "\u{1c}", "\u{1d}", "\u{1e}", "\u{1f}", " ", ",",
        "\u{1680}", "\u{2000}", "\u{2001}", "\u{2002}", "\u{2003}", "\u{2004}", "\u{2005}",
        "\u{2006}", "\u{2008}", "\u{2009}", "\u{200a}", "\u{2028}", "\u{2029}", "\u{205f}",
        "\u{3000}",
    ];
    for separator in whitespace {
        check(separator, &[]);
        check(&format!("λ{separator}café"), &[(Atom, "λ"), (Atom, "café")]);
    }
    for nonspace in [
        "\u{85}", "\u{a0}", "\u{180e}", "\u{2007}", "\u{202f}", "\u{200b}", "\u{feff}",
    ] {
        let source = format!("λ{nonspace}café");
        check(&source, &[(Atom, &source)]);
    }
}

#[test]
fn lexical_errors_have_spans_and_iteration_recovers_or_finishes() {
    for (source, error) in [
        ("\"λ", UnterminatedString),
        ("\"a\\", UnterminatedString),
        ("#\"λ", UnterminatedRegex),
        ("#\"a\\", UnterminatedRegex),
        ("\\", MissingCharacter),
        ("#", MissingDispatch),
    ] {
        check(source, &[(Error(error), source)]);
        let mut lexer = Tokenizer::new(source);
        assert!(lexer.next().is_some());
        for _ in 0..3 {
            assert_eq!(lexer.next(), None);
        }
    }
    check("#<x>", &[(Error(UnreadableForm), "#<"), (Atom, "x>")]);
    // Missing following forms are reader errors, not lexical errors.
    for source in [
        "'", "`", "~", "~@", "@", "^", "#_", "#?", "#?@", "#:ns", "##", "#=",
    ] {
        assert!(Tokenizer::new(source).all(|t| !matches!(t.kind, Error(_))));
    }
}

fn java_space(ch: char) -> bool {
    matches!(ch, ',' | '\t'..='\r' | '\u{1c}'..=' ' | '\u{1680}' |
        '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{205f}' | '\u{3000}')
}

fn invariants(source: &str) {
    let mut lexer = Tokenizer::new(source);
    let mut end = 0;
    let mut count = 0;
    for token in lexer.by_ref() {
        count += 1;
        assert!(count <= source.len()); // Always advances, even on malformed input.
        assert!(token.span.start >= end && token.span.start < token.span.end);
        assert!(
            source.get(token.span.clone()).is_some(),
            "{source:?}: {token:?}"
        );
        assert!(source[end..token.span.start].chars().all(java_space));
        end = token.span.end;
    }
    assert!(source[end..].chars().all(java_space));
    for _ in 0..3 {
        assert_eq!(lexer.next(), None);
    }
}

#[test]
fn malformed_and_utf8_combinations_always_progress_with_sliceable_spans() {
    let pieces = [
        "(", ")", "[", "]", "{", "}", "#", "'", "\"", "\\", ";", "~", "@", "^", ":", "_", "?", "%",
        " ", ",", "\n", "\r", "\0", "a", "1", "-1", "λ", "café", "🦀", "\u{2003}", "\u{a0}",
    ];
    for mut index in 0..pieces.len().pow(3) {
        let mut source = std::string::String::new();
        for _ in 0..3 {
            source.push_str(pieces[index % pieces.len()]);
            index /= pieces.len();
        }
        invariants(&source);
    }
    // Every truncation at a UTF-8 boundary of more complex literal combinations.
    let source = "#_[:kw \"λ\\\"café\" #\"\\s+🦀\" \\u03bb ;comment\r\n#?@(:cljs [x])]";
    for end in source.char_indices().map(|(i, _)| i).chain([source.len()]) {
        invariants(&source[..end]);
    }
}

#[test]
fn unicode_classes_agree_with_the_vendored_reader_snapshot() {
    let reference = include_str!("../reference/reader_classes.txt");
    let mut spaces = Vec::new();
    let mut digits = Vec::new();
    let mut digit_section = false;
    for line in reference.lines() {
        if line == ":digit" {
            digit_section = true;
            continue;
        }
        if line == ":whitespace" {
            continue;
        }
        let (start, end) = line.split_once("..").unwrap();
        let start = u32::from_str_radix(&start[2..], 16).unwrap();
        let end = u32::from_str_radix(&end[2..], 16).unwrap();
        if digit_section {
            digits.push(start..=end);
        } else {
            spaces.push(start..=end);
        }
    }
    for ch in (0..=0xffff).filter_map(char::from_u32) {
        let source = ch.to_string();
        let whitespace = spaces.iter().any(|r| r.contains(&(ch as u32)));
        assert_eq!(
            Tokenizer::new(&source).next().is_none(),
            whitespace,
            "U+{:04X}",
            ch as u32
        );
        // Skip macro-leading cases whose first token is not an ordinary atom.
        if whitespace || "\";@^`~()[]{}\\#'".contains(ch) {
            continue;
        }
        let number = digits.iter().any(|r| r.contains(&(ch as u32)));
        let source = format!("{ch}:kw");
        if number {
            check(&source, &[(Atom, &ch.to_string()), (Atom, ":kw")]);
            let signed = format!("-{ch}:kw");
            check(&signed, &[(Atom, &format!("-{ch}")), (Atom, ":kw")]);
        } else {
            check(&source, &[(Atom, &source)]);
        }
    }
    // A supplementary decimal digit is two chars to the JVM reader; a Roman
    // numeral is not a decimal digit. Both remain symbol-like atom candidates.
    check("𝟙:kw Ⅻ:kw", &[(Atom, "𝟙:kw"), (Atom, "Ⅻ:kw")]);
}

#[test]
fn namespaced_map_headers_keep_symbol_boundaries_even_when_digit_leading() {
    check(
        "#:1foo#x{:a 1} #::alias'{}",
        &[
            (NamespacedMap, "#:1foo#x"),
            (OpenBrace, "{"),
            (Atom, ":a"),
            (Atom, "1"),
            (CloseBrace, "}"),
            (AutoNamespacedMap, "#::alias'"),
            (OpenBrace, "{"),
            (CloseBrace, "}"),
        ],
    );
}

#[test]
fn context_sensitive_percent_notation_is_preserved_for_the_reader() {
    check(
        "#(%&rest %1:kw) %&rest %1:kw",
        &[
            (AnonymousFnStart, "#("),
            (Atom, "%&rest"),
            (Atom, "%1:kw"),
            (CloseParen, ")"),
            (Atom, "%&rest"),
            (Atom, "%1:kw"),
        ],
    );
}
