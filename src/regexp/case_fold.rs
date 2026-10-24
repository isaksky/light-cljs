//! Unicode simple folding (C/S), with no multi-character or Turkic mappings.
//! Non-Unicode ignore-case matching remains explicitly unsupported by compile.

#[path = "case_fold_data.rs"]
mod data;
pub(super) use data::SIMPLE_FOLD;

/// Canonicalize a character for Unicode ignore-case matching. Non-Unicode
/// callers leave characters unchanged; the matcher rejects legacy `i` first.
pub fn canonicalize(cp: u32, unicode: bool) -> u32 {
    if !unicode {
        return cp;
    }
    SIMPLE_FOLD
        .binary_search_by_key(&cp, |&(source, _)| source)
        .map_or(cp, |index| SIMPLE_FOLD[index].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_folding() {
        for (source, target) in [
            ('A', 'a'),
            ('ſ', 's'),
            ('K', 'k'),
            ('ẞ', 'ß'),
            ('Σ', 'σ'),
            ('ς', 'σ'),
            ('σ', 'σ'),
            ('İ', 'İ'),
        ] {
            assert_eq!(canonicalize(source as u32, true), target as u32);
        }
        assert_eq!(canonicalize(0xd800, true), 0xd800);
        assert_eq!(canonicalize(0x10400, true), 0x10428);
        for pair in SIMPLE_FOLD.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        for &(source, target) in SIMPLE_FOLD {
            assert_eq!(canonicalize(source, true), target);
            assert_eq!(canonicalize(target, true), target);
        }
    }
}
