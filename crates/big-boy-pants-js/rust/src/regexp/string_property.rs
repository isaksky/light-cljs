//! Unicode 17 / Emoji 17 properties of strings, stored once for the six
//! disjoint sequence categories. RGI_Emoji is their union, without copied data.

#[path = "string_property_data.rs"]
mod data;

struct StringPropertyData {
    name: &'static str,
    // Multi-code-point sequences, each prefixed by its scalar count.
    strings: &'static [u32],
}

/// A validated identity into immutable generated emoji sequence data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnicodeStringProperty(usize);

/// Properties of strings have exact names and no aliases or key=value form.
pub fn lookup(name: &str) -> Option<UnicodeStringProperty> {
    if name == "RGI_Emoji" {
        Some(UnicodeStringProperty(data::PROPERTIES.len()))
    } else {
        data::PROPERTIES
            .iter()
            .position(|p| p.name == name)
            .map(UnicodeStringProperty)
    }
}

impl UnicodeStringProperty {
    /// Singleton code points belong only to Basic_Emoji (and its RGI union).
    pub fn ranges(self) -> &'static [(u32, u32)] {
        if self.0 == 0 || self.0 == data::PROPERTIES.len() {
            data::BASIC_RANGES
        } else {
            &[]
        }
    }

    pub fn strings(self) -> impl Iterator<Item = &'static [u32]> {
        let properties = if self.0 == data::PROPERTIES.len() {
            data::PROPERTIES
        } else {
            &data::PROPERTIES[self.0..self.0 + 1]
        };
        properties.iter().flat_map(|p| {
            let mut remaining = p.strings;
            std::iter::from_fn(move || {
                let (&len, tail) = remaining.split_first()?;
                let sequence = tail.get(..len as usize)?;
                remaining = tail.get(len as usize..)?;
                Some(sequence)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_sequence_data_invariants() {
        let counts = [1400, 12, 665, 259, 3, 1614];
        let mut union = std::collections::BTreeSet::new();
        for (i, data) in data::PROPERTIES.iter().enumerate() {
            let mut remaining = data.strings;
            while let Some((&len, tail)) = remaining.split_first() {
                assert!(len >= 2);
                let (sequence, tail) = tail.split_at(len as usize);
                assert!(sequence.iter().all(|&cp| cp <= 0x10ffff));
                remaining = tail;
            }
            let property = lookup(data.name).unwrap();
            let mut count = 0;
            for &(a, b) in property.ranges() {
                assert!(a <= b && b <= 0x10ffff);
                for cp in a..=b {
                    assert!(union.insert(vec![cp]));
                    count += 1;
                }
            }
            for sequence in property.strings() {
                assert!(union.insert(sequence.to_vec()));
                count += 1;
            }
            assert_eq!(count, counts[i], "{}", data.name);
        }
        let rgi = lookup("RGI_Emoji").unwrap();
        assert_eq!(union.len(), 3953);
        assert_eq!(
            rgi.strings().count(),
            union.iter().filter(|s| s.len() > 1).count()
        );
        assert_eq!(rgi.ranges(), lookup("Basic_Emoji").unwrap().ranges());
        for invalid in ["rgi_emoji", "RGIEmoji", "Basic_Emoji=Yes", "RGI_Emoji=Yes"] {
            assert_eq!(lookup(invalid), None);
        }
    }
}
