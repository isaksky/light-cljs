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
//!   `===`, map keys (SameValueZero) and relational comparison need
//!   no heap lock.
//!
//! # UTF-16 code units
//!
//! The body stores code units (`u16`), not Rust `str` bytes, because
//! JavaScript defines `length`, indexing and relational order on code units
//! and a string may contain lone surrogates. [`JsString::from_code_units`]
//! accepts any sequence: the lexer builds string literals with it, so a lone
//! surrogate escape is a lone code unit and two surrogate escapes that spell
//! a pair are the same string as the astral character (clj-quickjs-141).
//! Consequently:
//!
//! * `length` is [`JsString::len`], the number of stored code units (O(1));
//! * relational order is [`Ord`]: lexicographic over code units, the
//!   ECMAScript IsLessThan order for two strings (QuickJS `js_string_compare`,
//!   `research/quickjs/quickjs.c`, revision 7d1c13c5d8f8, compares code units
//!   too), not Unicode scalar order;
//! * equality, hashing and the map-key hash compare code-unit sequences,
//!   never how a string was produced;
//! * conversion to Rust text ([`JsString::to_string_lossy`], `Display`)
//!   replaces a lone surrogate with U+FFFD; [`JsString::code_units`] and
//!   `Debug` (a quoted literal that writes a lone surrogate as `\u{d800}`)
//!   are exact. None of them panics.
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
    // Keeping the allocation as a Vec lets the incremental builder transfer
    // ownership without a potentially linear shrink/copy at its final step.
    units: Vec<u16>,
}

impl JsString {
    /// A string from arbitrary code units, including lone surrogates.
    /// `None` when longer than [`MAX_STRING_LEN`].
    pub fn from_code_units(units: impl Into<Box<[u16]>>) -> Option<JsString> {
        let units = units.into();
        (units.len() <= MAX_STRING_LEN).then(|| {
            JsString(Arc::new(Body {
                units: units.into_vec(),
            }))
        })
    }

    /// The number of UTF-16 code units (JavaScript `length`).
    pub fn len(&self) -> usize {
        self.0.units.len()
    }

    /// True for the empty string `""`.
    pub fn is_empty(&self) -> bool {
        self.0.units.is_empty()
    }

    /// The one-code-unit string at `index` (`s[index]`, `s.charAt(index)`),
    /// or `None` at or past the end. Constant time: no scan of the string.
    pub fn code_unit_at(&self, index: usize) -> Option<JsString> {
        let unit = *self.0.units.get(index)?;
        Some(JsString(Arc::new(Body { units: vec![unit] })))
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

/// Incremental flat-string construction. Each part names a source, starting
/// unit, and output length. A part longer than the source suffix repeats it
/// (padding). All storage is task-owned Rust data with no collector edges.
#[derive(Debug)]
pub(crate) struct StringBuild {
    parts: Vec<(JsString, usize, usize)>,
    part: usize,
    offset: usize,
    units: Vec<u16>,
}

impl StringBuild {
    pub(crate) fn new(parts: Vec<(JsString, usize, usize)>) -> Option<Self> {
        let len = parts
            .iter()
            .try_fold(0usize, |n, (_, _, len)| n.checked_add(*len))?;
        if len > MAX_STRING_LEN {
            return None;
        }
        let mut units = Vec::new();
        // Reserve once so bounded copying never triggers a growing-buffer
        // copy. Allocation itself is not interruptible; no units are filled.
        if !matches!(parts.as_slice(), [(text, 0, n)] if *n == text.len()) {
            units.try_reserve_exact(len).ok()?;
        }
        Some(Self {
            parts,
            part: 0,
            offset: 0,
            units,
        })
    }

    /// Copy at most `budget` units/part boundaries; moving the finished Vec
    /// into immutable Arc storage does no final linear copy.
    pub(crate) fn advance(&mut self, budget: usize) -> Option<JsString> {
        if let [(text, 0, len)] = self.parts.as_slice()
            && *len == text.len()
        {
            return Some(text.clone());
        }
        for _ in 0..budget {
            let Some((text, start, len)) = self.parts.get(self.part) else {
                return Some(JsString(Arc::new(Body {
                    units: std::mem::take(&mut self.units),
                })));
            };
            if self.offset == *len {
                self.part += 1;
                self.offset = 0;
            } else {
                self.units
                    .push(text.code_units()[start + self.offset % (text.len() - start)]);
                self.offset += 1;
            }
        }
        None
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

/// Incremental KMP search over UTF-16 units. Prefix construction and scanning
/// both resume between bounded batches of comparisons. Buffers are immutable
/// leaf strings and Rust-owned indices, with no collector edges.
#[derive(Debug)]
pub(crate) struct StringSearch {
    text: JsString,
    needle: JsString,
    prefix: Vec<usize>,
    matched: usize,
    next: usize,
    end: usize,
    last: bool,
    result: isize,
    done: bool,
}

impl StringSearch {
    /// Position is already clamped to [0, text.len()]. For reverse search,
    /// scan forward through the last eligible match and retain its start.
    pub(crate) fn new(text: JsString, needle: JsString, position: usize, last: bool) -> Self {
        let done = needle.is_empty() || needle.len() > text.len();
        let result = if needle.is_empty() {
            position as isize
        } else {
            -1
        };
        let end = if last {
            position.saturating_add(needle.len()).min(text.len())
        } else {
            text.len()
        };
        Self {
            text,
            needle,
            prefix: vec![0],
            matched: 0,
            next: if last { 0 } else { position },
            end,
            last,
            result,
            done,
        }
    }

    /// At most `comparisons` code-unit comparisons, including KMP fallback
    /// work. O(text.len() + needle.len()) total, O(needle.len()) index storage.
    pub(crate) fn advance(&mut self, comparisons: usize) -> Option<isize> {
        if self.done {
            return Some(self.result);
        }
        let needle = self.needle.code_units();
        for _ in 0..comparisons {
            if self.prefix.len() < needle.len() {
                if needle[self.prefix.len()] == needle[self.matched] {
                    self.matched += 1;
                    self.prefix.push(self.matched);
                } else if self.matched > 0 {
                    self.matched = self.prefix[self.matched - 1];
                } else {
                    self.prefix.push(0);
                }
                if self.prefix.len() == needle.len() {
                    self.matched = 0;
                }
                continue;
            }
            if self.next == self.end {
                self.done = true;
                return Some(self.result);
            }
            if self.text.code_units()[self.next] == needle[self.matched] {
                self.next += 1;
                self.matched += 1;
                if self.matched == needle.len() {
                    self.result = (self.next - needle.len()) as isize;
                    if !self.last {
                        self.done = true;
                        return Some(self.result);
                    }
                    self.matched = self.prefix[self.matched - 1];
                }
            } else if self.matched > 0 {
                self.matched = self.prefix[self.matched - 1];
            } else {
                self.next += 1;
            }
        }
        None
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

/// Exact: a quoted, escaped literal like `str`'s `Debug`, except that a lone
/// surrogate is written as the escape `\u{d800}` (not U+FFFD), so two
/// different strings never print the same.
impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        let mut run = String::new();
        for unit in char::decode_utf16(self.code_units().iter().copied()) {
            match unit {
                Ok(c) => run.push(c),
                Err(lone) => {
                    write_escaped_run(f, &run)?;
                    run.clear();
                    write!(f, "\\u{{{:x}}}", lone.unpaired_surrogate())?;
                }
            }
        }
        write_escaped_run(f, &run)?;
        f.write_str("\"")
    }
}

/// Writes `run` escaped as `str`'s `Debug` does, without the quotes.
fn write_escaped_run(f: &mut fmt::Formatter<'_>, run: &str) -> fmt::Result {
    if run.is_empty() {
        return Ok(());
    }
    let quoted = format!("{run:?}");
    f.write_str(&quoted[1..quoted.len() - 1])
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
/// is not used. The digits are [`shortest_digits`]. Other radixes use
/// [`number_to_string_radix`].
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

/// Number formatting in radix 2..36 (clj-quickjs-149). Radix 10 retains
/// Number::toString's decimal layout; other radixes never use exponents.
///
/// Semantic reference: `js_dtoa`'s free-format path in
/// research/quickjs/dtoa.c, revision 7d1c13c5d8f8. Written afresh using exact
/// remainders and binary64 rounding boundaries rather than floating-point
/// repeated multiplication. Integers are emitted exactly at every magnitude.
/// A fractional prefix stops as soon as rounding it up or down identifies x;
/// when both do, choose the nearest, ties to even. Boundary inclusion follows
/// the even binary significand, including subnormals and powers of two.
///
/// This leaf has a fixed bound: at most 1024 integer digits or 1075 fractional
/// digits, each operating on 34 u32 limbs (1088 bits). No input-sized loop,
/// JavaScript callback, heap edge or retained native continuation is needed.
///
/// # Panics
/// Panics if radix is outside 2..36. Callers validate it before entry.
pub fn number_to_string_radix(x: f64, radix: u32) -> String {
    assert!((2..=36).contains(&radix));
    if radix == 10 || !x.is_finite() || x == 0.0 {
        return number_to_string(x);
    }
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let bits = x.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mut mantissa = bits & ((1 << 52) - 1);
    let e = if exponent == 0 {
        -1074
    } else {
        mantissa |= 1 << 52;
        exponent - 1023 - 52
    };
    let mut integer = if e >= 0 {
        RadixInteger::shifted(mantissa, e as usize)
    } else {
        RadixInteger::shifted(mantissa.checked_shr((-e) as u32).unwrap_or(0), 0)
    };
    let mut digits = Vec::new();
    loop {
        digits.push(integer.divide(radix) as u8);
        if integer == RadixInteger::ZERO {
            break;
        }
    }
    digits.reverse();
    let mut integer_len = digits.len();
    if e < 0 {
        // One common exact denominator 2^1075 covers every binary64 half-ulp.
        let fractional_bits = if -e >= 64 {
            mantissa
        } else {
            mantissa & ((1 << -e) - 1)
        };
        let mut remainder = RadixInteger::shifted(fractional_bits, (e + 1075) as usize);
        if remainder != RadixInteger::ZERO {
            let upper_bit = (e + 1074) as usize;
            let mut upper = RadixInteger::shifted(1, upper_bit);
            let mut lower = RadixInteger::shifted(
                1,
                upper_bit - usize::from(exponent > 1 && mantissa == 1 << 52),
            );
            let inclusive = mantissa & 1 == 0;
            for _ in 0..1075 {
                remainder.multiply(radix);
                digits.push(remainder.take_digit() as u8);
                lower.multiply(radix);
                upper.multiply(radix);
                let down = remainder.within(&lower, inclusive);
                let up = RadixInteger::DENOMINATOR
                    .minus(&remainder)
                    .within(&upper, inclusive);
                if !down && !up {
                    continue;
                }
                let half = RadixInteger::shifted(1, 1074);
                let round_up = up
                    && (!down
                        || remainder.cmp(&half).is_gt()
                        || (remainder == half
                            && digits
                                .iter()
                                .fold(0, |p, d| (p * radix + u32::from(*d)) & 1)
                                != 0));
                if round_up {
                    let mut at = digits.len();
                    loop {
                        at -= 1;
                        digits[at] += 1;
                        if u32::from(digits[at]) != radix {
                            break;
                        }
                        digits[at] = 0;
                        if at == 0 {
                            // A non-integer binary64 is below 2^52, so its
                            // exact integer part cannot overflow to a new digit
                            // except through this final fractional rounding.
                            digits.insert(0, 1);
                            integer_len += 1;
                            break;
                        }
                    }
                }
                break;
            }
        }
    }
    while digits.len() > integer_len && digits.last() == Some(&0) {
        digits.pop();
    }
    let mut out = String::with_capacity(digits.len() + 2);
    if x < 0.0 {
        out.push('-');
    }
    for (i, digit) in digits.into_iter().enumerate() {
        if i == integer_len {
            out.push('.');
        }
        out.push(DIGITS[digit as usize] as char);
    }
    out
}

/// Fixed storage for radix formatting only. The denominator is 2^1075;
/// multiplying a remainder or boundary by at most 36 fits in 1088 bits.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RadixInteger([u32; 34]);

impl RadixInteger {
    const ZERO: Self = Self([0; 34]);
    const DENOMINATOR: Self = {
        let mut limbs = [0; 34];
        limbs[33] = 1 << 19;
        Self(limbs)
    };

    fn shifted(value: u64, shift: usize) -> Self {
        let mut out = Self::ZERO;
        for bit in 0..64 {
            if value & (1 << bit) != 0 {
                let at = shift + bit;
                out.0[at / 32] |= 1 << (at % 32);
            }
        }
        out
    }

    fn multiply(&mut self, radix: u32) {
        let mut carry = 0;
        for limb in &mut self.0 {
            carry += u64::from(*limb) * u64::from(radix);
            *limb = carry as u32;
            carry >>= 32;
        }
        debug_assert_eq!(carry, 0);
    }

    fn divide(&mut self, radix: u32) -> u32 {
        let mut remainder = 0;
        for limb in self.0.iter_mut().rev() {
            let value = (remainder << 32) | u64::from(*limb);
            *limb = (value / u64::from(radix)) as u32;
            remainder = value % u64::from(radix);
        }
        remainder as u32
    }

    fn take_digit(&mut self) -> u32 {
        let digit = self.0[33] >> 19;
        self.0[33] &= (1 << 19) - 1;
        digit
    }

    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.iter().rev().cmp(other.0.iter().rev())
    }

    fn within(&self, boundary: &Self, inclusive: bool) -> bool {
        let order = self.cmp(boundary);
        order.is_lt() || (inclusive && order.is_eq())
    }

    fn minus(&self, other: &Self) -> Self {
        let mut out = *self;
        let mut borrow = false;
        for (limb, rhs) in out.0.iter_mut().zip(other.0) {
            let (value, a) = limb.overflowing_sub(rhs);
            let (value, b) = value.overflowing_sub(u32::from(borrow));
            *limb = value;
            borrow = a || b;
        }
        debug_assert!(!borrow);
        out
    }
}

/// True for a StrWhiteSpaceChar code unit (ECMA-262 StringToNumber):
/// WhiteSpace (TAB, VT, FF, ZWNBSP U+FEFF and every Unicode `Zs` character:
/// space, NBSP U+00A0, U+1680, U+2000..=U+200A, U+202F, U+205F, U+3000) or a
/// LineTerminator (LF, CR, U+2028, U+2029). All of them are BMP characters,
/// so one code unit decides. (QuickJS tests the same set, the
/// regular-expression `\s` characters, with `lre_is_space`,
/// `research/quickjs/libunicode.h`, revision 7d1c13c5d8f8.)
fn is_str_white_space(u: u16) -> bool {
    matches!(
        u,
        0x09..=0x0D
            | 0x20
            | 0xA0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

/// JavaScript StringToNumber (ECMA-262 §7.1.4.1.1; QuickJS `js_atof` with
/// the flags `JS_ToNumberHintFree` passes, `research/quickjs/quickjs.c`,
/// revision 7d1c13c5d8f8). Leading and trailing StrWhiteSpace is ignored;
/// what remains must be, in full:
///
/// * nothing: `0`;
/// * `Infinity`, `+Infinity` or `-Infinity` (case-sensitive);
/// * an unsigned `0x`/`0X`, `0o`/`0O` or `0b`/`0B` integer with at least one
///   digit, of any length, correctly rounded (round half to even);
/// * a decimal literal with an optional sign, fraction and exponent: `12`,
///   `-1.5`, `.5`, `5.`, `1e3`, `-0` (negative zero), correctly rounded.
///   Leading zeros are decimal (`"010"` is 10, no legacy octal).
///
/// Anything else is NaN: numeric separators (`1_000`), a BigInt suffix
/// (`1n`), a signed radix prefix (`-0x1`), trailing garbage (`12px`),
/// another spelling of infinity (`infinity`, `inf`), or a non-ASCII digit.
pub fn string_to_number(s: &JsString) -> f64 {
    let units = s.code_units();
    let start = units
        .iter()
        .position(|&u| !is_str_white_space(u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|&u| !is_str_white_space(u))
        .map_or(start, |i| i + 1);
    let trimmed = &units[start..end];
    if trimmed.is_empty() {
        return 0.0;
    }
    // Every accepted form is ASCII; anything else is NaN.
    let Some(text) = trimmed
        .iter()
        .map(|&u| u8::try_from(u).ok().filter(u8::is_ascii))
        .collect::<Option<Vec<u8>>>()
    else {
        return f64::NAN;
    };
    match text.as_slice() {
        b"Infinity" | b"+Infinity" => return f64::INFINITY,
        b"-Infinity" => return f64::NEG_INFINITY,
        [b'0', b'x' | b'X', digits @ ..] => return radix_integer(digits, 4),
        [b'0', b'o' | b'O', digits @ ..] => return radix_integer(digits, 3),
        [b'0', b'b' | b'B', digits @ ..] => return radix_integer(digits, 1),
        _ => {}
    }
    if !is_str_decimal_literal(&text) {
        return f64::NAN;
    }
    // The text is ASCII and matches StrDecimalLiteral without `Infinity`,
    // a subset of the grammar Rust's correctly rounded `f64` parser
    // accepts, with the same meaning.
    std::str::from_utf8(&text)
        .ok()
        .and_then(|t| t.parse::<f64>().ok())
        .unwrap_or(f64::NAN)
}

/// True when `t` is `[+-]? (D+ ('.' D*)? | '.' D+) ([eE] [+-]? D+)?` with
/// ASCII decimal digits `D`.
fn is_str_decimal_literal(t: &[u8]) -> bool {
    let mut prefix = DecimalPrefix::default();
    t.iter().all(|&c| prefix.push(u16::from(c))) && prefix.complete()
}

/// The shared StrDecimalLiteral prefix grammar (Infinity is handled by
/// callers). State 3 is a point with no digit yet; 5/6 are an incomplete
/// exponent, which parseFloat discards but StringToNumber rejects.
#[derive(Debug, Default)]
struct DecimalPrefix(u8);

impl DecimalPrefix {
    fn push(&mut self, c: u16) -> bool {
        let digit = (48..=57).contains(&c);
        self.0 = match (self.0, c) {
            (0, 43 | 45) => 1,
            (0..=2, _) if digit => 2,
            (0 | 1, 46) => 3,
            (2, 46) => 4,
            (3 | 4, _) if digit => 4,
            (2 | 4, 69 | 101) => 5,
            (5, 43 | 45) => 6,
            (5..=7, _) if digit => 7,
            _ => return false,
        };
        true
    }

    fn complete(&self) -> bool {
        matches!(self.0, 2 | 4 | 7)
    }
}

/// Budgeted parseInt/parseFloat scanning over immutable leaf storage. No
/// heap edges. Each advance inspects at most `budget` input units; integer
/// arithmetic touches at most 32 limbs per unit and decimal finalization
/// parses at most 1100 retained digits plus a sticky digit and exponent.
#[derive(Debug)]
pub(crate) struct NumberParse {
    text: JsString,
    next: usize,
    started: bool,
    negative: bool,
    radix: Option<u32>,
    integer: [u32; 32],
    any: bool,
    decimal: DecimalPrefix,
    digits: String,
    fraction: i64,
    dropped: i64,
    sticky: bool,
    exponent: i64,
    exponent_negative: bool,
}

impl NumberParse {
    /// `None` selects parseFloat; `Some(0)` detects hex for parseInt.
    pub(crate) fn new(text: JsString, radix: Option<u32>) -> Self {
        Self {
            text,
            radix,
            next: 0,
            started: false,
            negative: false,
            integer: [0; 32],
            any: false,
            decimal: DecimalPrefix::default(),
            digits: String::new(),
            fraction: 0,
            dropped: 0,
            sticky: false,
            exponent: 0,
            exponent_negative: false,
        }
    }

    pub(crate) fn advance(&mut self, budget: usize) -> Option<f64> {
        for _ in 0..budget {
            let Some(&c) = self.text.code_units().get(self.next) else {
                return Some(self.finish());
            };
            if !self.started {
                self.next += 1;
                if is_str_white_space(c) {
                    continue;
                }
                self.started = true;
                self.negative = c == 45;
                if !matches!(c, 43 | 45) {
                    self.next -= 1;
                }
                let rest = &self.text.code_units()[self.next..];
                if let Some(radix) = &mut self.radix {
                    if matches!(*radix, 0 | 16)
                        && rest.first() == Some(&48)
                        && matches!(rest.get(1), Some(88 | 120))
                    {
                        *radix = 16;
                        self.next += 2;
                    } else if *radix == 0 {
                        *radix = 10;
                    }
                } else if rest.len() >= 8 && rest[..8].iter().copied().eq("Infinity".encode_utf16())
                {
                    return Some(self.signed(f64::INFINITY));
                }
                continue;
            }
            self.next += 1;
            if let Some(radix) = self.radix {
                let digit = match c {
                    48..=57 => u32::from(c - 48),
                    65..=90 => u32::from(c - 65) + 10,
                    97..=122 => u32::from(c - 97) + 10,
                    _ => return Some(self.finish()),
                };
                if digit >= radix {
                    return Some(self.finish());
                }
                self.any = true;
                let mut carry = u64::from(digit);
                for limb in &mut self.integer {
                    carry += u64::from(*limb) * u64::from(radix);
                    *limb = carry as u32;
                    carry >>= 32;
                }
                if carry != 0 {
                    // Every possible extension is already beyond 2^1024.
                    return Some(self.signed(f64::INFINITY));
                }
            } else {
                let before = self.decimal.0;
                // The optional sign was consumed above; a second sign is invalid.
                if (before == 0 && matches!(c, 43 | 45)) || !self.decimal.push(c) {
                    return Some(self.finish());
                }
                if self.decimal.0 == 7 {
                    // Greater than twice MAX_STRING_LEN, so saturation cannot
                    // hide cancellation against a long fraction/significand.
                    self.exponent = (self.exponent * 10 + i64::from(c - 48)).min(4_000_000_000);
                } else if before == 5 && c == 45 {
                    self.exponent_negative = true;
                } else if (48..=57).contains(&c) {
                    self.any = true;
                    if matches!(before, 3 | 4) {
                        self.fraction += 1;
                    }
                    if c != 48 || !self.digits.is_empty() {
                        if self.digits.len() < 1100 {
                            self.digits.push(c as u8 as char);
                        } else {
                            self.dropped += 1;
                            self.sticky |= c != 48;
                        }
                    }
                }
            }
        }
        None
    }

    fn signed(&self, n: f64) -> f64 {
        if self.negative { -n } else { n }
    }

    fn finish(&self) -> f64 {
        if !self.any {
            return f64::NAN;
        }
        if self.radix.is_some() {
            // Stream the exact (at most 1024-bit) integer into the same
            // top-bits/guard/sticky rounding rule used by StringToNumber.
            let mut mant = 0u64;
            let mut dropped = 0u64;
            let mut sticky = false;
            for limb in self.integer.iter().rev() {
                for k in (0..32).rev() {
                    let bit = u64::from((limb >> k) & 1);
                    if mant >> 63 == 0 {
                        mant = (mant << 1) | bit;
                    } else {
                        dropped += 1;
                        sticky |= bit != 0;
                    }
                }
            }
            return self.signed(round_integer(mant, dropped, sticky));
        }
        if self.digits.is_empty() {
            return self.signed(0.0);
        }
        let mut shift = self.dropped - self.fraction;
        if self.decimal.0 == 7 {
            shift += if self.exponent_negative {
                -self.exponent
            } else {
                self.exponent
            };
        }
        let mut digits = self.digits.clone();
        if self.sticky {
            // A binary64 rounding boundary is a dyadic rational with at
            // most 1075 fractional decimal places and fewer than 1100
            // significant digits. Beyond 1100 digits only nonzeroness can
            // affect which side of that boundary this value occupies.
            digits.push('1');
            shift -= 1;
        }
        digits.push('e');
        digits.push_str(&shift.to_string());
        self.signed(digits.parse::<f64>().expect("validated decimal"))
    }
}

/// The value of the unsigned integer whose digits in radix
/// `2^bits_per_digit` (2, 8 or 16) are `digits`, correctly rounded to the
/// nearest `f64` (ties to even), or NaN when `digits` is empty or holds a
/// character that is not a digit of the radix. Any length is accepted;
/// past 2^1024 the value is Infinity.
fn radix_integer(digits: &[u8], bits_per_digit: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let radix = 1u32 << bits_per_digit;
    // The first 64 significant bits, the number of later bits (`dropped`)
    // and whether any of those was 1 (`sticky`).
    let mut mant: u64 = 0;
    let mut dropped: u64 = 0;
    let mut sticky = false;
    for &c in digits {
        let Some(d) = char::from(c).to_digit(radix) else {
            return f64::NAN;
        };
        for k in (0..bits_per_digit).rev() {
            let bit = u64::from((d >> k) & 1);
            if mant >> 63 == 0 {
                mant = (mant << 1) | bit;
            } else {
                dropped += 1;
                sticky |= bit == 1;
            }
        }
    }
    round_integer(mant, dropped, sticky)
}

fn round_integer(mant: u64, dropped: u64, sticky: bool) -> f64 {
    if mant == 0 {
        return 0.0;
    }
    // Keep the top 53 significant bits of `mant`; the bits below them and
    // `sticky` round half to even. (Bits are dropped only once `mant` holds
    // 64 significant bits, so `shift` is 0 only when nothing was dropped.)
    let shift = 11u32.saturating_sub(mant.leading_zeros());
    let mut m = mant >> shift;
    if shift > 0 {
        let rest = mant & ((1u64 << shift) - 1);
        let half = 1u64 << (shift - 1);
        if rest > half || (rest == half && (sticky || m & 1 == 1)) {
            m += 1;
        }
    }
    // `m <= 2^53` (a carry may reach 2^53 exactly), so it is exact in f64.
    let exponent = u64::from(shift) + dropped;
    if exponent > 1023 {
        return f64::INFINITY;
    }
    // 2^exponent, exact for 0 <= exponent <= 1023; the product rounds to
    // Infinity on overflow and is otherwise exact.
    let scale = f64::from_bits((exponent + 1023) << 52);
    m as f64 * scale
}

#[cfg(test)]
mod tests {
    fn parsed(text: &str, radix: Option<u32>) -> f64 {
        let mut parser = NumberParse::new(JsString::from(text), radix);
        for _ in 0..text.len() + 2 {
            if let Some(number) = parser.advance(1) {
                return number;
            }
        }
        panic!("parser did not finish");
    }

    #[test]
    fn parse_integer_all_radixes_round_exact_integers_once() {
        for radix in 2..=36u32 {
            for n in [
                0,
                1,
                (1u128 << 53) + 1,
                (1u128 << 53) + 3,
                (1u128 << 100) + (1u128 << 46) + 1,
                u128::MAX,
            ] {
                let mut digits = Vec::new();
                let mut rest = n;
                loop {
                    digits
                        .push(char::from_digit((rest % u128::from(radix)) as u32, radix).unwrap());
                    rest /= u128::from(radix);
                    if rest == 0 {
                        break;
                    }
                }
                let text: String = digits.into_iter().rev().collect();
                assert_eq!(parsed(&text, Some(radix)), n as f64, "{text} radix {radix}");
            }
        }
        for text in ["1".repeat(309), "9".repeat(309), "0".repeat(2000)] {
            assert_eq!(parsed(&text, Some(10)), text.parse::<f64>().unwrap());
        }
        assert_eq!(parsed(&"f".repeat(256), Some(16)), f64::INFINITY);
        let below_overflow = format!("{}{}", "1".repeat(53), "0".repeat(971));
        assert_eq!(parsed(&below_overflow, Some(2)), f64::MAX);
    }

    #[test]
    fn parse_float_retained_digits_preserve_rounding_boundaries() {
        let midpoint = "1.00000000000000011102230246251565404236316680908203125";
        let cases = [
            format!("{midpoint}{}", "0".repeat(2000)),
            format!("{midpoint}{}1", "0".repeat(2000)),
            format!("{}e-2000", "1234567890".repeat(200)),
            format!("0.{}123e2003", "0".repeat(2000)),
            format!("{}e-9999999999999999999999999999", "9".repeat(2000)),
            format!("0.{}1e9999999999999999999999999999", "0".repeat(2000)),
            "2.4703282292062327e-324".to_string(),
            "2.4703282292062328e-324".to_string(),
            "1.7976931348623157e308".to_string(),
            "1.7976931348623159e308".to_string(),
        ];
        for text in cases {
            assert_eq!(
                parsed(&text, None).to_bits(),
                text.parse::<f64>().unwrap().to_bits(),
                "{text}"
            );
        }
    }

    #[test]
    fn incremental_string_build_checks_limits_and_moves_completed_storage() {
        let source = JsString::from("ab");
        assert!(StringBuild::new(vec![(source.clone(), 0, MAX_STRING_LEN + 1)]).is_none());
        assert!(
            StringBuild::new(vec![
                (source.clone(), 0, usize::MAX),
                (source.clone(), 0, 1)
            ])
            .is_none()
        );
        let mut unchanged = StringBuild::new(vec![(source.clone(), 0, 2)]).unwrap();
        assert!(unchanged.units.is_empty());
        assert!(unchanged.advance(1).unwrap().same_body(&source));

        let mut build = StringBuild::new(vec![(source, 1, 513)]).unwrap();
        let allocation = build.units.as_ptr();
        assert!(build.advance(256).is_none());
        assert_eq!(build.units.len(), 256);
        assert!(build.advance(256).is_none());
        assert_eq!(build.units.len(), 512);
        let result = build.advance(256).unwrap();
        assert_eq!(result.len(), 513);
        assert!(result.code_units().iter().all(|unit| *unit == b'b' as u16));
        assert_eq!(result.code_units().as_ptr(), allocation);
    }

    #[test]
    fn incremental_search_matches_code_unit_windows() {
        use super::{JsString, StringSearch};

        // Exhaust all short binary strings, including a lone surrogate.
        // Slice windows are an independent oracle for both search directions.
        let strings = |max_len| {
            (0..=max_len)
                .flat_map(|len| {
                    (0..1usize << len).map(move |bits| {
                        JsString::from_code_units(
                            (0..len)
                                .map(|i| if bits & (1 << i) == 0 { 97 } else { 0xd800 })
                                .collect::<Vec<_>>(),
                        )
                        .unwrap()
                    })
                })
                .collect::<Vec<_>>()
        };
        for text in strings(5) {
            for needle in strings(4) {
                for position in 0..=text.len() {
                    for last in [false, true] {
                        let expected = if needle.is_empty() {
                            position as isize
                        } else {
                            let mut candidates = text
                                .code_units()
                                .windows(needle.len())
                                .enumerate()
                                .filter(|(i, units)| {
                                    *units == needle.code_units()
                                        && if last { *i <= position } else { *i >= position }
                                })
                                .map(|(i, _)| i as isize);
                            if last {
                                candidates.next_back()
                            } else {
                                candidates.next()
                            }
                            .unwrap_or(-1)
                        };
                        let mut search =
                            StringSearch::new(text.clone(), needle.clone(), position, last);
                        let mut result = None;
                        // One comparison per step forces every KMP state to
                        // survive a boundary, including prefix fallback.
                        for _ in 0..64 {
                            result = search.advance(1);
                            if result.is_some() {
                                break;
                            }
                        }
                        assert_eq!(
                            result,
                            Some(expected),
                            "{text:?} / {needle:?}, {position}, last={last}"
                        );
                    }
                }
            }
        }
    }

    use super::*;

    /// Upstream rows adapt QuickJS test_builtin.js::test_number,
    /// revision 7d1c13c5d8f8; copyright and MIT notice are retained with
    /// tests/js_source.rs::string_code_unit_access. Other rows are synthetic.
    #[test]
    fn number_to_string_with_radix() {
        for (x, radix, expected) in [
            (1.0 - 2.0f64.powi(-53), 12, "0.bbbbbbbbbbbbbba".to_string()),
            (1.3, 7, "1.2046204620462046205".to_string()),
            (1.3, 35, "1.ahhhhhhhhhm".to_string()),
            (1000000000000000128.0, 10, "1000000000000000100".to_string()),
            (255.0, 16, "ff".to_string()),
            (-255.0, 2, "-11111111".to_string()),
            (0.5, 2, "0.1".to_string()),
            (
                0.1,
                2,
                "0.0001100110011001100110011001100110011001100110011001101".to_string(),
            ),
            (2.0f64.powi(60), 2, format!("1{}", "0".repeat(60))),
            (35.0, 36, "z".to_string()),
            (-0.0, 2, "0".to_string()),
            (f64::NAN, 2, "NaN".to_string()),
            (-f64::INFINITY, 16, "-Infinity".to_string()),
        ] {
            assert_eq!(number_to_string_radix(x, radix), expected, "{x:e}, {radix}");
        }
    }

    #[test]
    fn number_to_string_radix_rounds_halfway_coefficients_to_even() {
        // In odd bases, the parity of the whole coefficient matters.
        // 3^33 is odd, so both values lie exactly halfway between radix
        // coefficients. 33 fractional digits first fit their binary64
        // intervals: 1/(2*3^32) > half an ulp, 1/(2*3^33) < half an ulp.
        // The lower coefficient for 1.5 is even; for 2.5 it is odd.
        for (x, expected) in [
            (1.5, format!("1.{}", "1".repeat(33))),
            (2.5, format!("2.{}2", "1".repeat(32))),
        ] {
            assert_eq!(number_to_string_radix(x, 3), expected);
            assert_eq!(number_to_string_radix(-x, 3), format!("-{expected}"));
        }
    }

    #[test]
    fn radix_extremes_and_exact_integers() {
        assert_eq!(
            number_to_string_radix(f64::from_bits(1), 2),
            format!("0.{}1", "0".repeat(1073))
        );
        assert_eq!(
            number_to_string_radix(f64::MAX, 2),
            format!("{}{}", "1".repeat(53), "0".repeat(971))
        );
        assert_eq!(
            number_to_string_radix(f64::MIN_POSITIVE, 16),
            format!("0.{}4", "0".repeat(255))
        );
        assert_eq!(
            number_to_string_radix(1000000000000000128.0, 16),
            "de0b6b3a7640080"
        );
        // Every digit and radix, checked with independent native integer arithmetic.
        for radix in 2..=36 {
            for x in [1u64, 35, 255, 123456789, (1 << 53) - 1] {
                let text = number_to_string_radix(x as f64, radix);
                assert_eq!(u64::from_str_radix(&text, radix), Ok(x));
            }
            for x in [
                f64::from_bits(1),
                f64::MIN_POSITIVE,
                0.1,
                0.5,
                1.3,
                1.0 - f64::EPSILON / 2.0,
            ] {
                let text = number_to_string_radix(x, radix);
                assert!(text.len() <= 1077, "{x:e} base {radix}: {text}");
                assert!(!text.ends_with('0'), "{x:e} base {radix}: {text}");
            }
        }
    }

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
    fn string_to_number_follows_the_string_numeric_literal_grammar() {
        let n = |s: &str| string_to_number(&JsString::from(s));
        for (s, expected) in [
            ("", 0.0),
            (
                " \t\n\r\u{0B}\u{0C}\u{A0}\u{FEFF}\u{1680}\u{2000}\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}",
                0.0,
            ),
            ("\u{3000}42\u{2029}", 42.0),
            ("010", 10.0),
            ("+7", 7.0),
            ("-.5e1", -5.0),
            ("1E+2", 100.0),
            ("1e-2", 0.01),
            ("0.1", 0.1),
            ("1e400", f64::INFINITY),
            ("-1e400", f64::NEG_INFINITY),
            ("1e-400", 0.0),
            ("0x0", 0.0),
            ("0x00000000000000000000000000001", 1.0),
            ("0xFFFFFFFFFFFFFFFF", 18446744073709551615.0),
            ("0B101", 5.0),
            ("0O777", 511.0),
            // 2^53 + 1 is a tie between 2^53 and 2^53 + 2: even wins.
            ("0x20000000000001", 9007199254740992.0),
            // 2^53 + 3 ties up to 2^53 + 4.
            ("0x20000000000003", 9007199254740996.0),
        ] {
            assert_eq!(n(s), expected, "{s:?}");
        }
        // Long radix integers round correctly at any length.
        let zeros = |k: usize| "0".repeat(k);
        for (s, expected) in [
            // Just above the 2^53 + 1 tie (times 2^64), a later 1 bit
            // rounds up.
            (
                format!("0x20000000000001{}1", zeros(15)),
                9007199254740994.0 * 18446744073709551616.0,
            ),
            // (2^53 - 1) * 2^971 is the largest finite value.
            (format!("0xfffffffffffff8{}", zeros(242)), f64::MAX),
            (format!("0xfffffffffffffb{}", "f".repeat(242)), f64::MAX),
            // Halfway between it and 2^1024 rounds to even: Infinity.
            (format!("0xfffffffffffffc{}", zeros(242)), f64::INFINITY),
            (format!("0x1{}", zeros(256)), f64::INFINITY),
            (format!("0b1{}", zeros(1023)), 2f64.powi(1023)),
            (format!("0o1{}", zeros(341)), 2f64.powi(1023)),
        ] {
            assert_eq!(n(&s), expected, "{s:?}");
        }
        assert!(n("-0").is_sign_negative() && n("-0") == 0.0);
        assert!(n("0").is_sign_positive());
        for s in [
            ".",
            "+",
            "-",
            "e5",
            "1e",
            "1e+",
            "..5",
            "1.2.3",
            "0x",
            "0xg",
            "0b2",
            "0o8",
            "-0x1",
            "+0x1",
            "1_000",
            "1n",
            "12px",
            "infinity",
            "inf",
            "NaN",
            "+-1",
            "1 2",
            "0x1.8",
            "\u{FF11}",
            "Infinityx",
            "- 1",
        ] {
            assert!(n(s).is_nan(), "{s:?} gives {}", n(s));
        }
    }

    #[test]
    fn length_and_order_are_in_code_units() {
        let emoji = JsString::from("\u{1F600}");
        assert_eq!(emoji.len(), 2, "a supplementary character is two units");
        // Indexing is per code unit: each half of the pair is a lone surrogate.
        let units = |s: Option<JsString>| s.map(|s| s.code_units().to_vec());
        assert_eq!(units(emoji.code_unit_at(0)), Some(vec![0xD83D]));
        assert_eq!(units(emoji.code_unit_at(1)), Some(vec![0xDE00]));
        assert_eq!(units(emoji.code_unit_at(2)), None);
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
        assert_eq!(s.to_string(), "a\u{FFFD}");
        assert_ne!(s, JsString::from("a\u{FFFD}"));
        // `Debug` is exact: a lone surrogate is an escape, everything else
        // is escaped as `str`'s `Debug` escapes it.
        assert_eq!(format!("{s:?}"), r#""a\u{d800}""#);
        let mixed =
            JsString::from_code_units(vec![0xDC00, 0x0A, 0x22, 0xD83D, 0xDE00, 0xDBFF]).unwrap();
        assert_eq!(
            format!("{mixed:?}"),
            "\"\\u{dc00}\\n\\\"\u{1F600}\\u{dbff}\""
        );
        assert_eq!(format!("{:?}", JsString::from("")), r#""""#);
        assert_eq!(
            format!("{:?}", JsString::from("a\tb")),
            format!("{:?}", "a\tb")
        );
        // Halves joined by concatenation form the pair.
        let high = JsString::from_code_units(vec![0xDBFF]).unwrap();
        let low = JsString::from_code_units(vec![0xDFFF]).unwrap();
        assert_eq!(high.concat(&low).unwrap(), JsString::from("\u{10FFFF}"));
        assert_ne!(high, low);
        assert!(high < low);
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
