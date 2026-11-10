//! Unicode 17 binary, General_Category, Script and Script_Extensions properties.

#[path = "property_data.rs"]
mod data;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PropertyKind {
    CodePoint,
    String,
}

/// Recognize syntax independently of the matcher's available range tables.
/// ECMAScript requires exact property keys and Unicode aliases.
pub(super) fn syntax_kind(name: &str) -> Option<PropertyKind> {
    if lookup(name).is_some() {
        return Some(PropertyKind::CodePoint);
    }
    if super::string_property::lookup(name).is_some() {
        return Some(PropertyKind::String);
    }
    None
}

struct PropertyData {
    aliases: &'static str,
    group: PropertyGroup,
    ranges: &'static [(u32, u32)],
    // Canonical targets contributed by nonidentity simple-fold sources.
    folded: &'static [(u32, u32)],
    folded_complement: &'static [(u32, u32)],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PropertyGroup {
    Binary,
    Category,
    Script,
    ScriptExtensions,
}

/// A validated property identity into immutable generated data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnicodeProperty(usize);

/// Exact ECMAScript spellings only; no UAX44 loose matching. A lone name may
/// denote a category value or binary property. Scripts require a property key.
pub fn lookup(name: &str) -> Option<UnicodeProperty> {
    let (name, group) = if let Some((key, value)) = name.split_once('=') {
        let group = match key {
            "gc" | "General_Category" => PropertyGroup::Category,
            "sc" | "Script" => PropertyGroup::Script,
            "scx" | "Script_Extensions" => PropertyGroup::ScriptExtensions,
            _ => return None,
        };
        (value, Some(group))
    } else {
        (name, None)
    };
    data::PROPERTIES
        .iter()
        .position(|p| {
            group.map_or(
                matches!(p.group, PropertyGroup::Binary | PropertyGroup::Category),
                |g| p.group == g,
            ) && p.aliases.split(',').any(|a| a == name)
        })
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

impl UnicodeProperty {
    /// Every code point [`contains`](Self::contains) accepts with these
    /// flags, as sorted, disjoint, non-adjacent inclusive ranges: the
    /// property's ranges (their complement in 0..=0x10FFFF when inverted),
    /// plus under ignore case the folded targets of the same polarity. The
    /// Goose exporter lowers property escapes to these ranges, so the Goose
    /// VM carries no Unicode property tables (light-cljs-932).
    pub(crate) fn lowered(self, inverted: bool, ignore_case: bool) -> Vec<(u32, u32)> {
        let p = &data::PROPERTIES[self.0];
        let mut ranges = Vec::with_capacity(p.ranges.len() + 1);
        if inverted {
            let mut next = 0u32;
            for &(start, end) in p.ranges {
                if start > next {
                    ranges.push((next, start - 1));
                }
                next = end.saturating_add(1);
            }
            if next <= 0x10ffff {
                ranges.push((next, 0x10ffff));
            }
        } else {
            ranges.extend_from_slice(p.ranges);
        }
        if ignore_case {
            ranges.extend_from_slice(if inverted {
                p.folded_complement
            } else {
                p.folded
            });
        }
        ranges.retain(|&(start, _)| start <= 0x10ffff);
        for range in &mut ranges {
            range.1 = range.1.min(0x10ffff);
        }
        ranges.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            match merged.last_mut() {
                Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
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
        assert_eq!(data::PROPERTIES.len(), 91 + 2 * 176);
        for (i, p) in data::PROPERTIES.iter().enumerate() {
            for alias in p.aliases.split(',') {
                let keys: &[&str] = match p.group {
                    PropertyGroup::Binary => &[""],
                    PropertyGroup::Category => &["", "gc", "General_Category"],
                    PropertyGroup::Script => &["sc", "Script"],
                    PropertyGroup::ScriptExtensions => &["scx", "Script_Extensions"],
                };
                for key in [
                    "",
                    "gc",
                    "General_Category",
                    "sc",
                    "Script",
                    "scx",
                    "Script_Extensions",
                ] {
                    let name = if key.is_empty() {
                        alias.to_owned()
                    } else {
                        format!("{key}={alias}")
                    };
                    if keys.contains(&key) {
                        assert_eq!(lookup(&name), Some(UnicodeProperty(i)), "{name}");
                    } else {
                        assert_ne!(lookup(&name), Some(UnicodeProperty(i)), "{name}");
                    }
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

    /// The Goose exporter's lowering (light-cljs-932) accepts exactly what
    /// `contains` does, at every range boundary and its neighbours.
    #[test]
    fn lowered_ranges_agree_with_contains() {
        for i in 0..data::PROPERTIES.len() {
            let p = UnicodeProperty(i);
            let d = &data::PROPERTIES[i];
            for inverted in [false, true] {
                for ignore_case in [false, true] {
                    let lowered = p.lowered(inverted, ignore_case);
                    for pair in lowered.windows(2) {
                        assert!(pair[0].1 + 1 < pair[1].0);
                    }
                    let mut probes = vec![0, 0x10ffff];
                    for ranges in [d.ranges, d.folded, d.folded_complement, &lowered[..]] {
                        for &(start, end) in ranges {
                            probes.extend([start.saturating_sub(1), start, end, end + 1]);
                        }
                    }
                    for cp in probes.into_iter().filter(|&cp| cp <= 0x10ffff) {
                        assert_eq!(
                            member(cp, &lowered),
                            p.contains(cp, inverted, ignore_case),
                            "{} {cp:#x} {inverted} {ignore_case}",
                            d.aliases
                        );
                    }
                }
            }
        }
    }
}
