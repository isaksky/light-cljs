//! Unicode 17 binary properties and General_Category. Script/scx are deferred.

#[path = "property_data.rs"]
mod data;

struct PropertyData {
    aliases: &'static str,
    category: bool,
    ranges: &'static [(u32, u32)],
    // Canonical targets contributed by nonidentity simple-fold sources.
    folded: &'static [(u32, u32)],
    folded_complement: &'static [(u32, u32)],
}

/// A validated property identity into immutable generated data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnicodeProperty(usize);

/// Exact ECMAScript spellings only; no UAX44 loose matching. A lone name may
/// denote a category value or binary property. Only gc accepts `name=value`.
pub fn lookup(name: &str) -> Option<UnicodeProperty> {
    let (name, category_only) = if let Some((key, value)) = name.split_once('=') {
        if !matches!(key, "gc" | "General_Category") {
            return None;
        }
        (value, true)
    } else {
        (name, false)
    };
    data::PROPERTIES
        .iter()
        .position(|p| (!category_only || p.category) && p.aliases.split(',').any(|a| a == name))
        .map(UnicodeProperty)
}

impl UnicodeProperty {
    pub(super) fn ranges(self) -> &'static [(u32, u32)] {
        data::PROPERTIES[self.0].ranges
    }

    /// The matcher passes Canonicalize(input) in ignore-case mode. Complement
    /// membership is computed before adding its own folding contributions.
    pub fn contains(self, cp: u32, inverted: bool, ignore_case: bool) -> bool {
        if cp > 0x10ffff {
            return false;
        }
        let p = &data::PROPERTIES[self.0];
        member(cp, p.ranges) != inverted
            || ignore_case
                && member(
                    cp,
                    if inverted {
                        p.folded_complement
                    } else {
                        p.folded
                    },
                )
    }
}

fn member(cp: u32, ranges: &[(u32, u32)]) -> bool {
    let i = ranges.partition_point(|&(_, end)| end < cp);
    ranges.get(i).is_some_and(|&(start, _)| cp >= start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_range_invariants() {
        assert_eq!(data::PROPERTIES.len(), 91); // 53 binary and 38 GC values.
        for (i, p) in data::PROPERTIES.iter().enumerate() {
            for alias in p.aliases.split(',') {
                assert_eq!(lookup(alias), Some(UnicodeProperty(i)), "{alias}");
                if p.category {
                    for key in ["gc", "General_Category"] {
                        assert_eq!(lookup(&format!("{key}={alias}")), Some(UnicodeProperty(i)));
                    }
                } else {
                    assert_eq!(lookup(&format!("gc={alias}")), None);
                }
            }
            for ranges in [p.ranges, p.folded, p.folded_complement] {
                for &(start, end) in ranges {
                    assert!(start <= end && end <= 0x10ffff);
                }
                for pair in ranges.windows(2) {
                    assert!(pair[0].1 + 1 < pair[1].0);
                }
            }
        }
    }
}
