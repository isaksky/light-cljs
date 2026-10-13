//! The runtime's `hash`, ported to Rust bit for bit, so `Advanced` output can
//! embed the hash of each pooled constant instead of computing it on first use
//! (light-cljs-219; docs/advanced-optimization.md, "Constant pool extension
//! points").
//!
//! The functions mirror `hash` and the Murmur3 helpers above it in
//! `src/js_runtime.js`, which follow upstream ClojureScript: `int32` arithmetic
//! is wrapping `i32` arithmetic here, JavaScript's `>>>` is a shift of the
//! `u32` bits, and strings are UTF-16 code units, lone surrogates included.
//! [`ConstantHashes`] applies them to the constant pool's entries
//! (`ConstantPart` and `Composite` in `src/emitter.rs`), and
//! `tests/compiler/constant_hashes.rs` checks them against the runtime in Node
//! and QuickJS.

use std::collections::HashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};

use crate::emitter::{Composite, ConstantPart, EmptyCollection};

/// The runtime's `rotateLeft(mixK1)` step: `imul(rotl(imul(k, c1), 15), c2)`.
pub fn mix_k1(value: i32) -> i32 {
    value
        .wrapping_mul(0xcc9e_2d51_u32 as i32)
        .rotate_left(15)
        .wrapping_mul(0x1b87_3593)
}

/// `mixH1`: `(imul(rotl(h ^ k, 13), 5) + 0xe6546b64) | 0`.
pub fn mix_h1(hash: i32, value: i32) -> i32 {
    (hash ^ value)
        .rotate_left(13)
        .wrapping_mul(5)
        .wrapping_add(0xe654_6b64_u32 as i32)
}

/// `fmix`, Murmur3's finalizer over `length`.
pub fn fmix(hash: i32, length: i32) -> i32 {
    let mut hash = (hash ^ length) as u32;
    hash = (hash ^ (hash >> 16)).wrapping_mul(0x85eb_ca6b);
    hash = (hash ^ (hash >> 13)).wrapping_mul(0xc2b2_ae35);
    (hash ^ (hash >> 16)) as i32
}

/// `hashInt` (upstream `m3-hash-int`).
pub fn hash_int(value: i32) -> i32 {
    if value == 0 {
        0
    } else {
        fmix(mix_h1(0, mix_k1(value)), 4)
    }
}

/// `stringHash` (upstream `hash-string*`): `s[0]*31^(n-1) + ... + s[n-1]` over
/// UTF-16 code units, as `int32`. The runtime leaves the last addition
/// unwrapped, but every reader of it converts with `ToInt32` first.
pub fn string_hash(units: &[u16]) -> i32 {
    units.iter().fold(0i32, |hash, &unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    })
}

/// `hashChars` (upstream `m3-hash-unencoded-chars`), the name part of a
/// keyword or symbol hash.
pub fn hash_chars(units: &[u16]) -> i32 {
    let mut hash = 0;
    let (pairs, rest) = units.as_chunks::<2>();
    for [low, high] in pairs {
        hash = mix_h1(hash, mix_k1(i32::from(*low) | (i32::from(*high) << 16)));
    }
    if let [last] = rest {
        hash ^= mix_k1(i32::from(*last));
    }
    fmix(hash, (units.len() as i32).wrapping_mul(2))
}

/// `hashNamed`: the hash of a symbol, which a keyword offsets by
/// `0x9e3779b9`.
fn hash_named(namespace: Option<&[u16]>, name: &[u16]) -> i32 {
    let seed = hash_chars(name);
    seed ^ namespace
        .map_or(0, string_hash)
        .wrapping_add(0x9e37_79b9_u32 as i32)
        .wrapping_add(seed << 6)
        .wrapping_add(seed >> 2)
}

/// The hash of a keyword with this namespace and name.
pub fn keyword(namespace: Option<&[u16]>, name: &[u16]) -> i32 {
    hash_named(namespace, name).wrapping_add(0x9e37_79b9_u32 as i32)
}

/// The hash of a symbol with this namespace and name.
pub fn symbol(namespace: Option<&[u16]>, name: &[u16]) -> i32 {
    hash_named(namespace, name)
}

/// The hash of a string of UTF-16 code units.
pub fn string(units: &[u16]) -> i32 {
    hash_int(string_hash(units))
}

/// `hashDouble`: the native little-endian bytes of the float read as two
/// big-endian `int32`s and combined, as upstream reads them.
pub fn hash_double(value: f64) -> i32 {
    let bytes = value.to_le_bytes();
    i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        ^ i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
}

/// The hash of a number: fixed values for the nonfinite ones, the integer
/// modulo `2^31 - 1` for a safe integer (`-0` included, which hashes as `0`),
/// and [`hash_double`] for any other number.
pub fn number(value: f64) -> i32 {
    const SAFE: f64 = 9_007_199_254_740_991.0;
    if value.is_nan() {
        2_146_959_360
    } else if value == f64::INFINITY {
        2_146_435_072
    } else if value == f64::NEG_INFINITY {
        -1_048_576
    } else if value.fract() == 0.0 && value.abs() <= SAFE {
        // `(Math.floor(value) % 2147483647) | 0`: a truncating remainder,
        // whose magnitude is below 2^31.
        ((value as i64) % 2_147_483_647) as i32
    } else {
        hash_double(value)
    }
}

/// The hash of `true` or `false`.
pub fn boolean(value: bool) -> i32 {
    if value { 1231 } else { 1237 }
}

/// `mixCollectionHash` (upstream `mix-collection-hash`).
pub fn mix_collection(basis: i32, count: usize) -> i32 {
    fmix(mix_h1(0, mix_k1(basis)), count as i32)
}

/// `hashOrderedColl`: the hash of a vector, list or sequence of these hashes.
pub fn ordered(hashes: impl IntoIterator<Item = i32>) -> i32 {
    let (basis, count) = hashes
        .into_iter()
        .fold((1i32, 0usize), |(basis, count), hash| {
            (basis.wrapping_mul(31).wrapping_add(hash), count + 1)
        });
    mix_collection(basis, count)
}

/// `hashUnorderedColl`: the hash of a set of members with these hashes.
pub fn unordered(hashes: impl IntoIterator<Item = i32>) -> i32 {
    let (basis, count) = hashes
        .into_iter()
        .fold((0i32, 0usize), |(basis, count), hash| {
            (basis.wrapping_add(hash), count + 1)
        });
    mix_collection(basis, count)
}

/// The hash of a map with entries of these key and value hashes: the
/// unordered hash of its entries, each the ordered hash of `[key value]`.
pub fn map(entries: impl IntoIterator<Item = (i32, i32)>) -> i32 {
    unordered(entries.into_iter().map(|(key, value)| {
        mix_collection(
            31i32
                .wrapping_mul(31i32.wrapping_add(key))
                .wrapping_add(value),
            2,
        )
    }))
}

/// What the constant pool knows about a table entry, by entry number, to hash
/// the constants that contain it and to tell which of their keys are equal.
#[derive(Clone)]
enum Entry<'a> {
    /// An entry declared by something other than the pool's constants.
    Unknown,
    /// A keyword or quoted symbol.
    Named {
        symbol: bool,
        namespace: Option<&'a [u16]>,
        name: &'a [u16],
    },
    /// A composite constant, with its hash when it has one.
    Composite {
        constructor: Composite,
        parts: Box<[ConstantPart<'a>]>,
        hash: Option<i32>,
    },
}

/// What `=` is for a pair of constants: [`Same::Unknown`] when the pool
/// cannot tell (two NaNs, maps or sets with items, unequal keywords spelled
/// alike), which leaves a literal with such keys without a hash. Equal
/// constants here also hash alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Same {
    Equal,
    Distinct,
    Unknown,
}

/// The hashes of the `Advanced` constant table's entries (see the module
/// documentation). An entry has a hash when every part of it has one and,
/// for a map or set literal, when the pool can tell which of its keys the
/// runtime merges as equal.
#[derive(Default)]
pub(crate) struct ConstantHashes<'a> {
    entries: Vec<Entry<'a>>,
}

impl<'a> ConstantHashes<'a> {
    fn record(&mut self, number: usize, entry: Entry<'a>) {
        if self.entries.len() <= number {
            self.entries.resize(number + 1, Entry::Unknown);
        }
        self.entries[number] = entry;
    }

    /// Record table entry `number` as a keyword or quoted symbol.
    pub(crate) fn named(
        &mut self,
        number: usize,
        symbol: bool,
        namespace: Option<&'a [u16]>,
        name: &'a [u16],
    ) {
        self.record(
            number,
            Entry::Named {
                symbol,
                namespace,
                name,
            },
        );
    }

    /// Record table entry `number` as a composite with this constructor and
    /// these parts, and its hash from [`ConstantHashes::composite_hash`].
    pub(crate) fn composite(
        &mut self,
        number: usize,
        constructor: Composite,
        parts: Box<[ConstantPart<'a>]>,
        hash: Option<i32>,
    ) {
        self.record(
            number,
            Entry::Composite {
                constructor,
                parts,
                hash,
            },
        );
    }

    fn entry(&self, number: usize) -> &Entry<'a> {
        self.entries.get(number).unwrap_or(&Entry::Unknown)
    }

    /// The hash of a part, from its value or its entry.
    pub(crate) fn part(&self, part: ConstantPart<'a>) -> Option<i32> {
        match part {
            ConstantPart::Nil => Some(0),
            ConstantPart::Bool(value) => Some(boolean(value)),
            ConstantPart::Number(bits) => Some(number(f64::from_bits(bits))),
            ConstantPart::String(units) => Some(string(units)),
            ConstantPart::Empty(EmptyCollection::Vector | EmptyCollection::List) => {
                Some(ordered([]))
            }
            ConstantPart::Empty(EmptyCollection::Map | EmptyCollection::Set) => Some(unordered([])),
            ConstantPart::Entry(number) => match self.entry(number) {
                Entry::Unknown => None,
                Entry::Named {
                    symbol: true,
                    namespace,
                    name,
                } => Some(symbol(*namespace, name)),
                Entry::Named {
                    namespace, name, ..
                } => Some(keyword(*namespace, name)),
                Entry::Composite { hash, .. } => *hash,
            },
        }
    }

    /// The hash of the value `constructor` builds from `parts`, which the
    /// runtime's `hash` gives it, or `None` when the pool cannot compute it.
    pub(crate) fn composite_hash(
        &self,
        constructor: Composite,
        parts: &[ConstantPart<'a>],
    ) -> Option<i32> {
        let hashes = || parts.iter().map(|&part| self.part(part));
        match constructor {
            Composite::Vector | Composite::List => {
                hashes().collect::<Option<Vec<_>>>().map(ordered)
            }
            // Positional storage keeps every entry, equal keys included.
            Composite::LiteralSet => hashes().collect::<Option<Vec<_>>>().map(unordered),
            Composite::LiteralMap => {
                let pairs = parts
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&[key, value]| Some((self.part(key)?, self.part(value)?)));
                pairs.collect::<Option<Vec<_>>>().map(map)
            }
            // The builders merge equal keys, keeping the first key and the
            // last value.
            Composite::Set => {
                let members = self.merged(parts.iter().map(|&part| (part, None)))?;
                members
                    .iter()
                    .map(|(member, _)| self.part(*member))
                    .collect::<Option<Vec<_>>>()
                    .map(unordered)
            }
            Composite::ArrayMap | Composite::HashMap => {
                let entries = self.merged(
                    parts
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&[key, value]| (key, Some(value))),
                )?;
                entries
                    .iter()
                    .map(|(key, value)| Some((self.part(*key)?, self.part((*value)?)?)))
                    .collect::<Option<Vec<_>>>()
                    .map(map)
            }
            // Metadata does not take part in the hash.
            Composite::WithMeta => self.part(*parts.first()?),
            Composite::Uuid => match parts {
                [ConstantPart::String(text)] => uuid_text(text).map(|text| string(&text)),
                _ => None,
            },
            Composite::CheckedSet => None,
        }
    }

    /// The entries a builder keeps of `items`, keys with optional values: a
    /// key equal to an earlier one replaces that entry's value. `None` when
    /// the pool cannot tell whether two keys are equal. Scalar, keyword and
    /// symbol keys are found by [`Scalar`], so a large literal is linear;
    /// collection keys, which never equal those, are compared pairwise.
    #[allow(clippy::type_complexity)]
    fn merged(
        &self,
        items: impl Iterator<Item = (ConstantPart<'a>, Option<ConstantPart<'a>>)>,
    ) -> Option<Vec<(ConstantPart<'a>, Option<ConstantPart<'a>>)>> {
        let mut kept: Vec<(ConstantPart<'a>, Option<ConstantPart<'a>>)> = Vec::new();
        let mut scalars: HashMap<Scalar<'a>, usize> = HashMap::new();
        let mut collections: Vec<usize> = Vec::new();
        'items: for (key, value) in items {
            let scalar = self.scalar(key)?;
            let collection = scalar.is_none();
            let candidates = match scalar {
                Some(scalar) => match scalars.entry(scalar) {
                    Occupied(found) => std::slice::from_ref(found.into_mut()),
                    Vacant(slot) => {
                        slot.insert(kept.len());
                        &[]
                    }
                },
                None => &collections[..],
            };
            for &index in candidates {
                match self.same(kept[index].0, key) {
                    Same::Equal => {
                        kept[index].1 = value;
                        continue 'items;
                    }
                    Same::Distinct => {}
                    Same::Unknown => return None,
                }
            }
            if collection {
                collections.push(kept.len());
            }
            kept.push((key, value));
        }
        Some(kept)
    }

    /// The [`Scalar`] of a key, `Some(None)` for a collection or `#uuid`, and
    /// `None` for an unknown entry.
    fn scalar(&self, key: ConstantPart<'a>) -> Option<Option<Scalar<'a>>> {
        Some(Some(match self.unwrapped(key) {
            ConstantPart::Nil => Scalar::Nil,
            ConstantPart::Bool(value) => Scalar::Bool(value),
            // `0` and `-0` are `=`.
            ConstantPart::Number(bits) => Scalar::Number((f64::from_bits(bits) + 0.0).to_bits()),
            ConstantPart::String(units) => Scalar::String(units),
            ConstantPart::Empty(_) => return Some(None),
            ConstantPart::Entry(number) => match self.entry(number) {
                Entry::Unknown => return None,
                Entry::Named {
                    symbol,
                    namespace,
                    name,
                } => Scalar::Named(*symbol, spelling(*namespace, name)),
                Entry::Composite { .. } => return Some(None),
            },
        }))
    }

    /// Whether `=` holds between two constants, for the builders' merging of
    /// equal keys. [`Same::Equal`] only when their hashes are also equal, as
    /// the hash-map builder merges only keys with equal hashes.
    fn same(&self, left: ConstantPart<'a>, right: ConstantPart<'a>) -> Same {
        use ConstantPart as P;
        let decided = |equal: bool| if equal { Same::Equal } else { Same::Distinct };
        let left = self.unwrapped(left);
        let right = self.unwrapped(right);
        match (left, right) {
            (P::Nil, P::Nil) => Same::Equal,
            (P::Bool(left), P::Bool(right)) => decided(left == right),
            // NaN is no other number; whether the hash-map builder finds a
            // NaN key equal to itself is left open.
            (P::Number(left), P::Number(right)) => {
                let (left, right) = (f64::from_bits(left), f64::from_bits(right));
                if left.is_nan() && right.is_nan() {
                    Same::Unknown
                } else {
                    decided(left == right)
                }
            }
            (P::String(left), P::String(right)) => decided(left == right),
            _ => match (self.class(left), self.class(right)) {
                (Some(left_class), Some(right_class)) if left_class != right_class => {
                    Same::Distinct
                }
                (Some(Class::Named), Some(Class::Named)) => self.same_named(left, right),
                (Some(Class::Sequential), Some(Class::Sequential)) => {
                    match (self.sequence(left), self.sequence(right)) {
                        (Some(left), Some(right)) if left.len() != right.len() => Same::Distinct,
                        (Some(left), Some(right)) => left.iter().zip(right.iter()).fold(
                            Same::Equal,
                            |same, (&left, &right)| match (same, self.same(left, right)) {
                                (Same::Distinct, _) | (_, Same::Distinct) => Same::Distinct,
                                (Same::Equal, Same::Equal) => Same::Equal,
                                _ => Same::Unknown,
                            },
                        ),
                        _ => Same::Unknown,
                    }
                }
                (Some(Class::Scalar), Some(Class::Scalar)) => Same::Distinct,
                (Some(Class::Map | Class::Set), Some(Class::Map | Class::Set)) => {
                    // A map or set literal with an item is not empty.
                    match (self.items(left), self.items(right)) {
                        (Some(0), Some(0)) => Same::Equal,
                        (Some(0), Some(_)) | (Some(_), Some(0)) => Same::Distinct,
                        _ => Same::Unknown,
                    }
                }
                (Some(Class::Uuid), Some(Class::Uuid)) => match (left, right) {
                    (P::Entry(left), P::Entry(right)) => {
                        match (self.uuid(left), self.uuid(right)) {
                            (Some(left), Some(right)) => decided(left == right),
                            _ => Same::Unknown,
                        }
                    }
                    _ => Same::Unknown,
                },
                _ => Same::Unknown,
            },
        }
    }

    /// A part with reader metadata removed, which `=` and `hash` ignore.
    fn unwrapped(&self, mut part: ConstantPart<'a>) -> ConstantPart<'a> {
        while let ConstantPart::Entry(number) = part
            && let Entry::Composite {
                constructor: Composite::WithMeta,
                parts,
                ..
            } = self.entry(number)
        {
            part = parts[0];
        }
        part
    }

    /// The `=` class of a part that is not a scalar, or of a scalar
    /// ([`Class::Scalar`]); `None` for an unknown entry.
    fn class(&self, part: ConstantPart<'a>) -> Option<Class> {
        Some(match part {
            ConstantPart::Nil
            | ConstantPart::Bool(_)
            | ConstantPart::Number(_)
            | ConstantPart::String(_) => Class::Scalar,
            ConstantPart::Empty(EmptyCollection::Vector | EmptyCollection::List) => {
                Class::Sequential
            }
            ConstantPart::Empty(EmptyCollection::Map) => Class::Map,
            ConstantPart::Empty(EmptyCollection::Set) => Class::Set,
            ConstantPart::Entry(number) => match self.entry(number) {
                Entry::Unknown => return None,
                Entry::Named { .. } => Class::Named,
                Entry::Composite { constructor, .. } => match constructor {
                    Composite::Vector | Composite::List => Class::Sequential,
                    Composite::ArrayMap | Composite::HashMap | Composite::LiteralMap => Class::Map,
                    Composite::Set | Composite::CheckedSet | Composite::LiteralSet => Class::Set,
                    Composite::Uuid => Class::Uuid,
                    Composite::WithMeta => return None,
                },
            },
        })
    }

    /// `=` of two keywords or symbols, which compare their spellings: equal
    /// spellings of different namespaces and names would hash apart.
    fn same_named(&self, left: ConstantPart<'a>, right: ConstantPart<'a>) -> Same {
        let named = |part| match part {
            ConstantPart::Entry(number) => match self.entry(number) {
                Entry::Named {
                    symbol,
                    namespace,
                    name,
                } => Some((*symbol, *namespace, *name)),
                _ => None,
            },
            _ => None,
        };
        let (Some(left), Some(right)) = (named(left), named(right)) else {
            return Same::Unknown;
        };
        if left.0 != right.0 {
            Same::Distinct
        } else if (left.1, left.2) == (right.1, right.2) {
            Same::Equal
        } else if spelling(left.1, left.2) == spelling(right.1, right.2) {
            Same::Unknown
        } else {
            Same::Distinct
        }
    }

    /// The number of parts of an empty collection or composite entry.
    fn items(&self, part: ConstantPart<'a>) -> Option<usize> {
        match part {
            ConstantPart::Empty(_) => Some(0),
            ConstantPart::Entry(number) => match self.entry(number) {
                Entry::Composite { parts, .. } => Some(parts.len()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The items of a sequential part.
    fn sequence(&self, part: ConstantPart<'a>) -> Option<&[ConstantPart<'a>]> {
        match part {
            ConstantPart::Empty(_) => Some(&[]),
            ConstantPart::Entry(number) => match self.entry(number) {
                Entry::Composite {
                    constructor: Composite::Vector | Composite::List,
                    parts,
                    ..
                } => Some(parts),
                _ => None,
            },
            _ => None,
        }
    }

    /// The text a `#uuid` entry holds.
    fn uuid(&self, number: usize) -> Option<Vec<u16>> {
        match self.entry(number) {
            Entry::Composite {
                constructor: Composite::Uuid,
                parts,
                ..
            } => match **parts {
                [ConstantPart::String(text)] => uuid_text(text),
                _ => None,
            },
            _ => None,
        }
    }
}

/// A key that is not a collection, as builders compare it: equal keys have
/// equal [`Scalar`]s, and equal [`Scalar`]s are equal keys except for NaN and
/// keywords or symbols spelled alike, which [`ConstantHashes::same`] decides.
#[derive(PartialEq, Eq, Hash)]
enum Scalar<'a> {
    Nil,
    Bool(bool),
    /// The bits, with `-0` as `0`.
    Number(u64),
    String(&'a [u16]),
    /// Whether a symbol, and the spelling.
    Named(bool, Vec<u16>),
}

/// A keyword's or symbol's spelling, `namespace/name` or `name`, which `=`
/// compares.
fn spelling(namespace: Option<&[u16]>, name: &[u16]) -> Vec<u16> {
    let mut units = namespace.map_or_else(Vec::new, |namespace| {
        let mut units = namespace.to_vec();
        units.push(u16::from(b'/'));
        units
    });
    units.extend_from_slice(name);
    units
}

/// The classes of values `=` never finds equal across.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Scalar,
    Named,
    Sequential,
    Map,
    Set,
    Uuid,
}

/// The text `uuid` keeps of a `#uuid` literal: `toLowerCase()`, which the
/// pool applies only to ASCII text.
fn uuid_text(text: &[u16]) -> Option<Vec<u16>> {
    text.iter()
        .map(|&unit| {
            u8::try_from(unit)
                .ok()
                .filter(u8::is_ascii)
                .map(|byte| u16::from(byte.to_ascii_lowercase()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    // Values printed by `(hash ...)` in Node at `--optimize none`.
    #[test]
    fn scalars_hash_as_the_runtime_does() {
        assert_eq!(string(&utf16("abc")), 74_834_163);
        assert_eq!(string(&utf16("")), 0);
        assert_eq!(string(&utf16("\u{1F600}")), -1_934_073_014);
        assert_eq!(string(&[0xD800]), 545_030_712);
        assert_eq!(number(1.5), 63_551);
        assert_eq!(number(-0.0), 0);
        assert_eq!(number(f64::NAN), 2_146_959_360);
        assert_eq!(number(f64::INFINITY), 2_146_435_072);
        assert_eq!(number(f64::NEG_INFINITY), -1_048_576);
        assert_eq!(number(9_007_199_254_740_992.0), 16_451);
        assert_eq!(number(-5.0), -5);
        assert_eq!(number(2_147_483_647.0), 0);
        assert_eq!(number(2_147_483_648.0), 1);
        assert_eq!(number(-2_147_483_648.0), -1);
        assert_eq!(number(4_294_967_296.0), 2);
        assert_eq!(number(1e300), -1_601_095_690);
        assert_eq!(number(0.1), 50_340_006);
        assert_eq!(boolean(true), 1231);
        assert_eq!(keyword(None, &utf16("a")), -2_123_407_586);
        assert_eq!(keyword(Some(&utf16("a")), &utf16("b")), 1_482_224_565);
        assert_eq!(symbol(Some(&utf16("a")), &utf16("b")), -1_172_211_204);
        assert_eq!(symbol(None, &utf16("x")), -555_367_584);
    }

    #[test]
    fn collections_hash_as_the_runtime_does() {
        let a = keyword(None, &utf16("a"));
        assert_eq!(ordered([number(1.0), number(2.0)]), 1_185_709_346);
        assert_eq!(map([(a, number(1.0))]), -2_144_769_406);
        assert_eq!(unordered([number(1.0), number(2.0)]), -1_751_407_317);
        assert_eq!(ordered([]), -2_017_569_654);
        assert_eq!(unordered([]), -15_128_758);
        // `{1 :b 1.0 :b}` keeps both entries, `#{1 1.0}` both members.
        let b = keyword(None, &utf16("b"));
        assert_eq!(map([(number(1.0), a), (number(1.0), b)]), 440_366_004);
        assert_eq!(unordered([number(1.0), number(1.0)]), -403_707_546);
        assert_eq!(
            string(&utf16("00000000-0000-0000-0000-00000000000a")),
            1_804_450_500
        );
    }

    #[test]
    fn builders_merge_equal_keys_and_unknown_equality_leaves_no_hash() {
        use ConstantPart as P;
        let (a, b, ab) = (utf16("a"), utf16("b"), utf16("a/b"));
        let number = |value: f64| P::Number(value.to_bits());
        let nan = number(f64::NAN);
        let mut hashes = ConstantHashes::default();
        hashes.named(0, false, None, &a);
        hashes.named(1, false, Some(&a), &b);
        // `:a/b` read whole, which only data can build.
        hashes.named(2, false, None, &ab);
        // Recorded hashes stand in for computed ones.
        hashes.composite(3, Composite::Vector, Box::new([number(1.0)]), Some(7));
        hashes.composite(4, Composite::List, Box::new([number(1.0)]), Some(7));
        hashes.composite(5, Composite::Set, Box::new([number(1.0)]), Some(9));
        let map_hash = |parts: &[ConstantPart]| hashes.composite_hash(Composite::ArrayMap, parts);
        let keyword_a = super::keyword(None, &a);
        // `0` and `-0` merge, keeping the last value; NaN is no other number.
        assert_eq!(
            map_hash(&[number(0.0), P::Bool(true), number(-0.0), P::Entry(0)]),
            Some(map([(0, keyword_a)]))
        );
        assert_eq!(
            map_hash(&[nan, P::Nil, number(1.0), P::Nil]),
            Some(map([(super::number(f64::NAN), 0), (1, 0)]))
        );
        assert_eq!(map_hash(&[nan, P::Nil, nan, P::Nil]), None);
        // `[1]` and `(1)` are `=`; a set key may or may not equal another.
        assert_eq!(
            map_hash(&[P::Entry(3), P::Nil, P::Entry(4), P::Bool(false)]),
            Some(map([(7, 1237)]))
        );
        assert_eq!(map_hash(&[P::Entry(5), P::Nil, P::Entry(5), P::Nil]), None);
        assert_eq!(
            map_hash(&[P::Entry(5), P::Nil, P::Empty(EmptyCollection::Set), P::Nil]),
            Some(map([(9, 0), (unordered([]), 0)]))
        );
        // Keywords spelled alike from other parts would hash apart.
        assert_eq!(map_hash(&[P::Entry(1), P::Nil, P::Entry(2), P::Nil]), None);
        assert_eq!(
            hashes.composite_hash(Composite::Set, &[P::Entry(0), P::Entry(0), P::String(&a)]),
            Some(unordered([keyword_a, string(&a)]))
        );
    }
}
