use light_cljs::{
    Token,
    TokenKind::{self, *},
    Tokenizer,
};

fn tokens(source: &str) -> Vec<(TokenKind, std::ops::Range<usize>, &str)> {
    Tokenizer::new(source)
        .map(|token| {
            let text = &source[token.span.clone()];
            (token.kind, token.span, text)
        })
        .collect()
}

#[test]
fn example() {
    assert_eq!(
        tokens("(+ 1 2)"),
        vec![
            (OpenParen, 0..1, "("),
            (Atom, 1..2, "+"),
            (Atom, 3..4, "1"),
            (Atom, 5..6, "2"),
            (CloseParen, 6..7, ")"),
        ]
    );
}

#[test]
fn empty_and_all_ascii_whitespace() {
    for source in ["", " \t\n\r\x0b\x0c ", ",,,", " ,\t,\n,\r,\x0b,\x0c,"] {
        assert!(tokens(source).is_empty());
    }
}

#[test]
fn adjacent_nested_and_unmatched_parentheses() {
    assert_eq!(
        tokens("()((a)))("),
        vec![
            (OpenParen, 0..1, "("),
            (CloseParen, 1..2, ")"),
            (OpenParen, 2..3, "("),
            (OpenParen, 3..4, "("),
            (Atom, 4..5, "a"),
            (CloseParen, 5..6, ")"),
            (CloseParen, 6..7, ")"),
            (CloseParen, 7..8, ")"),
            (OpenParen, 8..9, "("),
        ]
    );
}

#[test]
fn atoms_at_eof_and_commas() {
    assert_eq!(
        tokens("a,,b,c"),
        vec![(Atom, 0..1, "a"), (Atom, 3..4, "b"), (Atom, 5..6, "c"),]
    );
    assert_eq!(tokens("last"), vec![(Atom, 0..4, "last")]);
    assert_eq!(
        tokens("(a)b"),
        vec![
            (OpenParen, 0..1, "("),
            (Atom, 1..2, "a"),
            (CloseParen, 2..3, ")"),
            (Atom, 3..4, "b"),
        ]
    );
}

#[test]
fn utf8_uses_byte_spans() {
    assert_eq!(
        tokens("(café,λ)🦀"),
        vec![
            (OpenParen, 0..1, "("),
            (Atom, 1..6, "café"),
            (Atom, 7..9, "λ"),
            (CloseParen, 9..10, ")"),
            (Atom, 10..14, "🦀"),
        ]
    );
}

#[test]
fn repeated_eof_is_fused() {
    fn assert_fused(_: impl std::iter::FusedIterator) {}
    assert_fused(Tokenizer::new(""));
    for source in ["", " ", "a", "λ", "()", "a ,\n"] {
        let mut tokenizer = Tokenizer::new(source);
        while tokenizer.next().is_some() {}
        for _ in 0..8 {
            assert_eq!(tokenizer.next(), None);
        }
    }
}

// An independent character-based oracle for the byte scanner.
fn reference(source: &str) -> Vec<Token> {
    let mut result = Vec::new();
    let mut atom_start = None;
    for (offset, ch) in source.char_indices() {
        if matches!(ch, '(' | ')' | ',' | ' ' | '\t'..='\r') {
            if let Some(start) = atom_start.take() {
                result.push(Token {
                    kind: Atom,
                    span: start..offset,
                });
            }
            if matches!(ch, '(' | ')') {
                result.push(Token {
                    kind: if ch == '(' { OpenParen } else { CloseParen },
                    span: offset..offset + 1,
                });
            }
        } else {
            atom_start.get_or_insert(offset);
        }
    }
    if let Some(start) = atom_start {
        result.push(Token {
            kind: Atom,
            span: start..source.len(),
        });
    }
    result
}

#[test]
fn spans_are_sliceable_and_agree_with_character_scanner() {
    let pieces = [
        "(", ")", " ", ",", "\t", "\x0b", "x", "café", "λ", "🦀", "\u{a0}", "e\u{301}", "\0",
        "foo", "z",
    ];
    // Every combination of four pieces, including multibyte characters directly
    // next to delimiters, other multibyte characters, within the original lexical subset.
    for mut index in 0..pieces.len().pow(4) {
        let mut source = std::string::String::new();
        for _ in 0..4 {
            source.push_str(pieces[index % pieces.len()]);
            index /= pieces.len();
        }
        let actual: Vec<_> = Tokenizer::new(&source).collect();
        assert_eq!(actual, reference(&source), "source: {source:?}");
        let mut end = 0;
        for token in actual {
            assert!(token.span.start >= end);
            assert!(token.span.start < token.span.end);
            assert!(source.get(token.span.clone()).is_some());
            end = token.span.end;
        }
    }
}
