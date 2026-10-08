#[path = "../benches/support/variants.rs"]
mod variants;

#[path = "../benches/support/subset.rs"]
mod subset;
use subset::{Token, Tokenizer};
use variants::{CompactTokenizer, MatchTokenizer, SourceTooLarge};

fn check(source: &str) {
    let expected: Vec<_> = Tokenizer::new(source).collect();
    let mut baseline = MatchTokenizer::new(source);
    let actual: Vec<_> = baseline.by_ref().collect();
    assert_eq!(actual, expected);
    let mut compact = CompactTokenizer::new(source).unwrap();
    let actual: Vec<_> = compact
        .by_ref()
        .map(|token| Token {
            kind: token.kind,
            span: token.span.start as usize..token.span.end as usize,
        })
        .collect();
    assert_eq!(actual, expected);
    for _ in 0..3 {
        assert_eq!(baseline.next(), None);
        assert_eq!(compact.next(), None);
    }
}

#[test]
fn experimental_scanners_agree_with_library() {
    check("");
    check("(+ 1 2)");
    // All ASCII bytes exercise the entire delimiter classification domain.
    // Non-ASCII byte values are also exercised through valid UTF-8 encodings.
    for ch in (0..=0x10ffff).filter_map(char::from_u32) {
        let source = format!("(x{ch}λ),{ch} {ch}");
        check(&source);
    }
}

#[test]
fn compact_source_limit_is_checked_before_narrowing() {
    assert_eq!(CompactTokenizer::validate_len(0), Ok(()));
    assert_eq!(CompactTokenizer::validate_len(u32::MAX as usize), Ok(()));
    if let Ok(too_large) = usize::try_from(u64::from(u32::MAX) + 1) {
        assert_eq!(
            CompactTokenizer::validate_len(too_large),
            Err(SourceTooLarge)
        );
        assert_eq!(
            CompactTokenizer::validate_len(usize::MAX),
            Err(SourceTooLarge)
        );
    }
}
