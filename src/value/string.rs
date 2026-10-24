//! JavaScript string values (clj-quickjs-65).
//!
//! # Representation
//!
//! A JavaScript string is a primitive: an immutable sequence of UTF-16 code
//! units, compared by content. [`JsString`] stores one as a thin `Arc`
//! (one pointer, so a raw VM value stays two words) to an immutable body
//! that owns the code units. The body is **independently owned immutable
//! `Arc` storage outside the heap**, like executable code and collection trie
//! nodes (`docs/architecture.md`, "String values"):
//!
//! * a string is not a heap object and has no heap edges, so the collector
//!   neither traces nor sweeps it; a string lives exactly as long as some
//!   value (frame slot, operand, capture, collection entry, root, constant
//!   pool, host copy) holds its `Arc`. That is ordinary Rust ownership of an
//!   acyclic leaf buffer, not reference counting of heap objects: no string
//!   can reference another value, so no cycle can form through one;
//! * it is persistent and shareable by construction: nothing can change its
//!   contents, so publishing one, storing one in a persistent collection or
//!   reading it from another task shares the same body without copying and
//!   without any publication walk (`crate::value::share`);
//! * equality, hashing and ordering read only the body, never the heap, so
//!   `===`, map keys (SameValueZero) and future relational comparison need
//!   no heap lock.
//!
//! # UTF-16 code units
//!
//! The body stores code units (`u16`), not Rust `str` bytes, because
//! JavaScript defines `length`, indexing and relational order on code units
//! and a string may contain lone surrogates. Today every string comes from
//! Rust or source text (Unicode scalar values, encoded with
//! [`str::encode_utf16`]), so lone surrogates cannot arise yet;
//! [`JsString::from_code_units`] already accepts any sequence, which is what
//! lone surrogate escapes and string indexing need. Consequently:
//!
//! * `length` is [`JsString::len`], the number of stored code units (O(1));
//! * relational order is [`Ord`]: lexicographic over code units, the
//!   ECMAScript IsLessThan order for two strings (QuickJS `js_string_compare`,
//!   `research/quickjs/quickjs.c`, revision 7d1c13c5d8f8, compares code units
//!   too), not Unicode scalar order;
//! * conversion to Rust text ([`JsString::to_string_lossy`], `Display`)
//!   replaces a lone surrogate with U+FFFD.
//!
//! Every string uses the 16-bit form; a narrow (Latin-1) body as in QuickJS's
//! `JSString`, or a rope body for repeated concatenation, can be added inside
//! the body without changing [`JsString`]'s interface or the value model.
//! Concatenation copies both operands into a new flat body today.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Maximum string length in code units: `2^30 - 1`, as QuickJS's
/// `JS_STRING_LEN_MAX`. A concatenation that would exceed it fails instead of
/// allocating (QuickJS raises an InternalError "string too long").
pub const MAX_STRING_LEN: usize = (1 << 30) - 1;

/// An immutable JavaScript string value: a sequence of UTF-16 code units.
/// See the module docs. Cloning is O(1) and shares the body.
#[derive(Clone)]
pub struct JsString(Arc<Body>);

/// The immutable body of a string.
struct Body {
    units: Box<[u16]>,
}

impl JsString {
    /// A string from arbitrary code units, including lone surrogates.
    /// `None` when longer than [`MAX_STRING_LEN`].
    pub fn from_code_units(units: impl Into<Box<[u16]>>) -> Option<JsString> {
        let units = units.into();
        (units.len() <= MAX_STRING_LEN).then(|| JsString(Arc::new(Body { units })))
    }

    /// The number of UTF-16 code units (JavaScript `length`).
    pub fn len(&self) -> usize {
        self.0.units.len()
    }

    /// True for the empty string `""`.
    pub fn is_empty(&self) -> bool {
        self.0.units.is_empty()
    }

    /// The code units, in order.
    pub fn code_units(&self) -> &[u16] {
        &self.0.units
    }

    /// The contents as Rust text; a lone surrogate becomes U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0.units)
    }

    /// The concatenation `self + other`, or `None` when the result would be
    /// longer than [`MAX_STRING_LEN`]. Concatenating with `""` returns the
    /// other operand's body unchanged.
    pub fn concat(&self, other: &JsString) -> Option<JsString> {
        if other.is_empty() {
            return Some(self.clone());
        }
        if self.is_empty() {
            return Some(other.clone());
        }
        let len = self.len().checked_add(other.len())?;
        if len > MAX_STRING_LEN {
            return None;
        }
        let mut units = Vec::with_capacity(len);
        units.extend_from_slice(self.code_units());
        units.extend_from_slice(other.code_units());
        JsString::from_code_units(units)
    }

    /// True when both values share one body (an O(1) identity test; equal
    /// contents in different bodies are still `==`).
    pub(crate) fn same_body(&self, other: &JsString) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Number of values sharing this body (tests of release).
    #[cfg(test)]
    pub(crate) fn body_refs(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    /// A private, deterministic content hash (FNV-1a over the code units),
    /// used by persistent map keys: equal strings hash equally.
    pub(crate) fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &u in self.code_units() {
            h ^= u64::from(u);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^ (self.len() as u64).rotate_left(32)
    }
}

impl From<&str> for JsString {
    /// # Panics
    ///
    /// If `s` encodes to more than [`MAX_STRING_LEN`] code units.
    fn from(s: &str) -> Self {
        let units: Vec<u16> = s.encode_utf16().collect();
        JsString::from_code_units(units).expect("string longer than MAX_STRING_LEN")
    }
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        self.same_body(other) || self.code_units() == other.code_units()
    }
}

impl Eq for JsString {}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.code_units().iter().copied().eq(other.encode_utf16())
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.code_units().hash(state);
    }
}

/// Lexicographic order over code units: JavaScript's relational order for
/// two strings. See the module docs.
impl Ord for JsString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.code_units().cmp(other.code_units())
    }
}

impl PartialOrd for JsString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string_lossy())
    }
}

/// JavaScript Number::toString(x) in radix 10 (ECMA-262 Number::toString,
/// QuickJS `js_dtoa2` via `JS_ToStringInternal`, `research/quickjs/quickjs.c`,
/// revision 7d1c13c5d8f8): `"NaN"`, `"0"` for both zeros, `"Infinity"` /
/// `"-Infinity"`, otherwise the shortest decimal digits that round-trip to
/// `x`, laid out in plain decimal when `1e-6 <= |x| < 1e21` and in exponent
/// form (`"1e+21"`, `"1e-7"`, `"1.5e-7"`) otherwise.
///
/// Only the layout below is JavaScript's own; Rust's `{}` layout differs
/// (`1e21` prints `1000000000000000000000`, `1e-7` prints `0.0000001`) and
/// is not used. The digits are [`shortest_digits`]. Radix 2..36 is
/// clj-quickjs-149.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let (digits, n) = shortest_digits(x.abs());
    let k = digits.len() as i32;
    let mut out = String::with_capacity(digits.len() + 8);
    if x < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        // An integer: the digits, then n - k zeros.
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        // The point falls inside the digits.
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        // A small fraction: "0.", -n zeros, the digits.
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        // Exponent form: d[.ddd]e±(n - 1).
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n > 0 { '+' } else { '-' });
        out.push_str(&(n - 1).unsigned_abs().to_string());
    }
    out
}

/// The digits `s` (as ASCII, no trailing zero) and exponent `n` of a finite
/// positive `x` per Number::toString: `x = 0.s × 10^n` where `s` has as few
/// digits `k` as possible such that it round-trips to `x`; among several
/// such `s`, the one closest to `x`, and of two equally close, the even one.
///
/// Rust's `{:e}` gives the shortest round-trip digits, so it fixes `k`, but
/// on an exact decimal tie it rounds up (`1000000000000000.25` gives
/// `...3`, JavaScript requires `...2`). Rust's `{:.*e}` with `k - 1`
/// fractional digits is the correctly rounded `k`-digit decimal of the
/// exact binary value, ties to even, i.e. the closest `k`-digit candidate;
/// it is used whenever it round-trips. It may not (next to a power of two
/// the round-trip interval is narrower below `x` than above, so the closest
/// candidate can fall outside it); then the round-trip candidates all lie on
/// the wide side and `{:e}`'s choice stands.
fn shortest_digits(x: f64) -> (String, i32) {
    let parse = |sci: &str| -> (String, i32) {
        let (mantissa, exponent) = sci
            .split_once('e')
            .expect("`{:e}` of a finite number has an exponent");
        // `mantissa` is `d` or `d.ddd`, all ASCII.
        let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        while digits.len() > 1 && digits.ends_with('0') {
            digits.pop();
        }
        let n = exponent
            .parse::<i32>()
            .expect("`{:e}` exponent is an integer")
            + 1;
        (digits, n)
    };
    let shortest = format!("{x:e}");
    let (digits, n) = parse(&shortest);
    let closest = format!("{x:.*e}", digits.len() - 1);
    if closest != shortest && closest.parse::<f64>() == Ok(x) {
        return parse(&closest);
    }
    (digits, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_matches_javascript() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.1, "0.1"),
            (0.5, "0.5"),
            (123.456, "123.456"),
            (-123.456, "-123.456"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (-1e21, "-1e+21"),
            (1.5e21, "1.5e+21"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (-1.5e-7, "-1.5e-7"),
            (0.000001, "0.000001"),
            (0.0000015, "0.0000015"),
            (1.5e300, "1.5e+300"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (9007199254740992.0, "9007199254740992"),
            (19686109595169230000.0, "19686109595169230000"),
            (123e-20, "1.23e-18"),
            (2147483648.0, "2147483648"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            // Exact decimal ties between two shortest candidates take the
            // even one (Rust's `{:e}` alone rounds these up).
            (1e15 + 0.25, "1000000000000000.2"),
            (-(1e15 + 0.25), "-1000000000000000.2"),
            (1e15 + 0.75, "1000000000000000.8"),
            (0.5e-6, "5e-7"),
            (0.25, "0.25"),
        ];
        for &(x, expected) in cases {
            assert_eq!(number_to_string(x), expected, "Number::toString({x:e})");
        }
    }

    #[test]
    fn number_to_string_round_trips_with_shortest_length() {
        // A deterministic sample of bit patterns across the whole range:
        // every result parses back to the same number and is never longer
        // than Rust's shortest round-trip digits.
        let mut bits: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..200_000 {
            bits = bits
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let x = f64::from_bits(bits);
            if !x.is_finite() || x == 0.0 {
                continue;
            }
            let s = number_to_string(x);
            assert_eq!(s.parse::<f64>(), Ok(x), "{s} for {x:e}");
            let (digits, _) = shortest_digits(x.abs());
            let rust_digits = format!("{:e}", x.abs())
                .split('e')
                .next()
                .unwrap()
                .replace('.', "");
            assert_eq!(digits.len(), rust_digits.len(), "{x:e}");
        }
    }

    #[test]
    fn length_and_order_are_in_code_units() {
        let emoji = JsString::from("\u{1F600}");
        assert_eq!(emoji.len(), 2, "a supplementary character is two units");
        // U+FF61 sorts after U+1F600 by code units (0xFF61 > 0xD83D), though
        // not by scalar value.
        let [halfwidth, a, ab, b] = ["\u{FF61}", "a", "ab", "b"].map(JsString::from);
        assert!(halfwidth > emoji);
        assert!(a < ab);
        assert!(b > ab);
    }

    #[test]
    fn lone_surrogates_are_kept_and_shown_as_replacement() {
        let s = JsString::from_code_units(vec![0x61, 0xD800]).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s.code_units(), &[0x61, 0xD800]);
        assert_eq!(s.to_string_lossy(), "a\u{FFFD}");
        assert_ne!(s, JsString::from("a\u{FFFD}"));
    }

    #[test]
    fn equality_and_hash_follow_content() {
        let a = JsString::from("ab");
        let b = JsString::from("a").concat(&JsString::from("b")).unwrap();
        assert!(!a.same_body(&b));
        assert_eq!(a, b);
        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), JsString::from("ba").content_hash());
        assert_eq!(a, "ab");
    }

    #[test]
    fn concatenation_with_empty_shares_the_body() {
        let a = JsString::from("x");
        let empty = JsString::from("");
        assert!(a.concat(&empty).unwrap().same_body(&a));
        assert!(empty.concat(&a).unwrap().same_body(&a));
    }
}
