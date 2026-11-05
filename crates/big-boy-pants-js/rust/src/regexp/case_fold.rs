//! ECMAScript Canonicalize: Unicode simple folding or legacy code-unit uppercase.

#[path = "case_fold_data.rs"]
mod data;
pub(super) use data::SIMPLE_FOLD;

pub(super) fn mappings(unicode: bool) -> &'static [(u32, u32)] {
    if unicode {
        SIMPLE_FOLD
    } else {
        data::LEGACY_UPPER
    }
}

/// Canonicalize for ignore-case matching. Legacy mode uppercases one UTF-16
/// code unit, retaining it for multi-unit results or non-ASCII to ASCII mappings.
pub fn canonicalize(cp: u32, unicode: bool) -> u32 {
    let table = mappings(unicode);
    table
        .binary_search_by_key(&cp, |&(source, _)| source)
        .map_or(cp, |index| table[index].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_uppercase() {
        for (source, target) in [
            ('a', 'A'),
            ('é', 'É'),
            ('ς', 'Σ'),
            ('σ', 'Σ'),
            ('ß', 'ß'),
            ('ẞ', 'ẞ'),
            ('ſ', 'ſ'),
            ('K', 'K'),
            ('İ', 'İ'),
            ('ı', 'ı'),
            ('ﬅ', 'ﬅ'),
            ('ᾀ', 'ᾀ'),
        ] {
            assert_eq!(canonicalize(source as u32, false), target as u32);
        }
        assert_eq!(canonicalize(0xd800, false), 0xd800);
        assert_eq!(canonicalize(0x10400, false), 0x10400);
        for pair in data::LEGACY_UPPER.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        for &(source, target) in data::LEGACY_UPPER {
            assert!(source <= 0xffff && target <= 0xffff);
            assert!(source < 128 || target >= 128);
            assert_eq!(canonicalize(source, false), target);
            assert_eq!(canonicalize(target, false), target);
        }
    }

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
