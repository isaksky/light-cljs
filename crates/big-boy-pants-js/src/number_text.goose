import std;

namespace number;

// Number::toString(x) in radix 10 (ECMAScript 6.1.6.1.20; the Pants
// engine's rust/src/value/string.rs number_to_string, QuickJS js_dtoa via
// JS_ToStringInternal), appended to out as ASCII bytes (light-cljs-826).
//
// The digits are Goose's own float text: the fewest significant digits that
// read back as x and, of those, the ones nearest x, a tie going to the even
// digit (Burger and Dybvig's free-format algorithm in the Goose runtime), as
// ECMAScript asks of s and k. Only their layout differs from C's: with
// x = 0.d1..dk * 10^n, an integer below 10^21 is written out in full, a
// fraction from 10^-6 up has its point inside or before the digits, and
// everything else is d1[.d2..dk]e±(n - 1).
fn append_decimal(out: u8[>..]&, x: f64) {
    if x != x { out.append("NaN"); return; }
    if x == 0 { out.push('0'); return; }
    if x > 1.7976931348623157e308 { out.append("Infinity"); return; }
    if x < -1.7976931348623157e308 { out.append("-Infinity"); return; }
    if x < 0 { out.push('-'); }
    var digits: u8[>..] = [];
    let n = shortest(if x < 0 { -x } else { x }, digits);
    let k = digits.len;
    if k <= n && n <= 21 {
        out.append(digits); for i in (n - k) { out.push('0'); }
    } else if 0 < n && n <= 21 {
        out.append(digits[..n]); out.push('.'); out.append(digits[n..]);
    } else if -6 < n && n <= 0 {
        out.append("0."); for i in (-n) { out.push('0'); } out.append(digits);
    } else {
        out.push(digits[0]);
        if k > 1 { out.push('.'); out.append(digits[1..]); }
        out.push('e'); out.push(if n > 0 { '+' } else { '-' });
        format(out, if n > 0 { n - 1 } else { 1 - n });
    }
}
// The digits d1..dk of a finite positive x, without leading or trailing
// zeros, and n with x = 0.d1..dk * 10^n, read off Goose's float text: its
// mantissa digits with an optional point, then an optional exponent.
fn shortest(x: f64, digits: u8[>..]&) -> i64 {
    var t:   u8[>..] = []; format(t, x);
    var all: u8[>..] = []; var point: i64 = -1; var i: i64 = 0;
    while i < t.len && t[i] != 'e' { if t[i] == '.' { point = all.len; } else { all.push(t[i]); } i += 1; }
    if point < 0 { point = all.len; }
    var exponent: i64 = 0;
    if i < t.len {
        i += 1; let negative = t[i] == '-'; if t[i] == '-' || t[i] == '+' { i += 1; }
        while i < t.len { exponent = exponent * 10 + (t[i] - '0'); i += 1; }
        if negative { exponent = -exponent; }
    }
    var lead: i64 = 0; while lead < all.len - 1 && all[lead] == '0' { lead += 1; }
    var end = all.len; while end > lead + 1 && all[end - 1] == '0' { end -= 1; }
    digits.append(all[lead..end]); return point + exponent - lead;
}
// StringToNumber of a StringNumericLiteral (ECMAScript 7.1.4.1.1; the Pants
// engine's rust/src/value/string.rs string_to_number, QuickJS js_atof with
// JS_ToNumberHintFree's flags), given the string's code units with leading
// and trailing StrWhiteSpace trimmed, every unit ASCII (light-cljs-827).
// Accepted in full, everything else being NaN: nothing (0); `Infinity` with
// an optional sign; an unsigned `0x`, `0o` or `0b` integer of any length;
// a decimal `[+-]? (D+ ('.' D*)? | '.' D+) ([eE] [+-]? D+)?`, leading zeros
// decimal (`010` is 10). Both are correctly rounded, ties to even: the
// decimal by Goose's parse_flt, which reads only text this grammar accepted
// (it would take other spellings), the radix integer by radix_integer.
fn parse_literal(t: const u8[:]) -> f64 {
    if t.len == 0 { return 0.0; }
    if t == "Infinity" || t == "+Infinity" { return 1.0 / 0.0; }
    if t == "-Infinity" { return -1.0 / 0.0; }
    if t.len >= 2 && t[0] == '0' {
        let p = t[1];
        if p == 'x' || p == 'X' { return radix_integer(t[2..], 4); }
        if p == 'o' || p == 'O' { return radix_integer(t[2..], 3); }
        if p == 'b' || p == 'B' { return radix_integer(t[2..], 1); }
    }
    if !decimal_literal(t) { return 0.0 / 0.0; }
    var copy: u8[>..] = []; copy.append(t);
    let v, ok = parse_flt(copy);
    return if ok { v } else { 0.0 / 0.0 };
}
inline fn is_digit(c: u8) -> bool { return c >= '0' && c <= '9'; }
// Whether t is a StrDecimalLiteral other than `Infinity`.
fn decimal_literal(t: const u8[:]) -> bool {
    var i: i64 = 0;
    if i < t.len && (t[i] == '+' || t[i] == '-') { i += 1; }
    var digits: i64 = 0;
    while i < t.len && is_digit(t[i]) { i += 1; digits += 1; }
    if i < t.len && t[i] == '.' { i += 1; while i < t.len && is_digit(t[i]) { i += 1; digits += 1; } }
    if digits == 0 { return false; }
    if i < t.len && (t[i] == 'e' || t[i] == 'E') {
        i += 1;
        if i < t.len && (t[i] == '+' || t[i] == '-') { i += 1; }
        var exponent: i64 = 0;
        while i < t.len && is_digit(t[i]) { i += 1; exponent += 1; }
        if exponent == 0 { return false; }
    }
    return i == t.len;
}
// The value of digits in radix 2^bits (one digit at least, any number),
// correctly rounded to a double, ties to even; NaN for no digit or a digit
// outside the radix. The first significant bits accumulate exactly, up to
// 60 of them; a later digit only adds its bits to the scale and sets the
// sticky bit if it is not zero, which is all rounding to 53 bits needs.
fn radix_integer(digits: const u8[:], bits: i64) -> f64 {
    if digits.len == 0 { return 0.0 / 0.0; }
    let one: u64 = 1; let radix = one << bits;
    var m: u64 = 0; var scale: i64 = 0; var sticky = false;
    for c in digits {
        let d: u64 = if is_digit(c) { c - '0' } else if c >= 'a' && c <= 'f' { c - 'a' + 10 } else if c >= 'A' && c <= 'F' { c - 'A' + 10 } else { 16 };
        if d >= radix { return 0.0 / 0.0; }
        if m < one << 56 { m = (m << bits) | d; } else { scale += bits; if d != 0 { sticky = true; } }
    }
    var length: i64 = 0; while length < 64 && (m >> length) != 0 { length += 1; }
    if length > 53 {
        let excess = length - 53;
        let low = m & ((one << excess) - 1); let half = one << (excess - 1);
        m = m >> excess; scale += excess;
        if low > half || low == half && (sticky || (m & 1) == 1) { m += 1; }
    }
    // m is at most 2^53, exact as a double, and so is each product below
    // until one passes the largest double: Infinity, as rounding asks.
    var v: f64 = m as i64;
    while scale >= 32 && v != 1.0 / 0.0 { v = v * 4294967296.0; scale -= 32; }
    while scale > 0 && v != 1.0 / 0.0 { v = v * 2.0; scale -= 1; }
    return v;
}
// parseInt and parseFloat after leading StrWhiteSpace (light-cljs-900;
// ECMAScript 19.2.5 and 19.2.4; the Pants engine's rust/src/value/string.rs
// NumberParse, QuickJS's js_parseInt and js_parseFloat through js_atof):
// `t` holds the string's units from its first unit that is not
// StrWhiteSpace, as far as they can belong to a number (the caller charges
// them as it scans them).
//
// parse_int(t, r), r being ToInt32 of the radix: an optional sign; radix 0
// is 10, or 16 after a `0x` or `0X` prefix, which radix 16 also skips; a
// radix outside 2 to 36 is NaN; then the longest prefix of digits of the
// radix (letters for 10 to 35, either case), none being NaN. The digits
// accumulate exactly in 32-bit limbs, up to 1,024 bits (more is Infinity),
// and round to the nearest double, ties to even, so the result is exact up
// to 2^53 and correctly rounded beyond; "-0" is -0.
fn parse_int(t: const u16[:], radix_in: i64) -> f64 {
    var i: i64 = 0; var negative = false;
    if i < t.len && (t[i] == '+' || t[i] == '-') { negative = t[i] == '-'; i += 1; }
    var radix = radix_in; var strip = true;
    if radix != 0 {
        if radix < 2 || radix > 36 { return 0.0 / 0.0; }
        if radix != 16 { strip = false; }
    } else { radix = 10; }
    if strip && t.len - i >= 2 && t[i] == '0' && (t[i + 1] == 'x' || t[i + 1] == 'X') { i += 2; radix = 16; }
    var limbs: u64[>..] = []; var any = false;
    let r: u64 = radix as u64;
    while i < t.len {
        let c = t[i];
        let d: i64 = if c >= '0' && c <= '9' { c - '0' } else if c >= 'a' && c <= 'z' { c - 'a' + 10 } else if c >= 'A' && c <= 'Z' { c - 'A' + 10 } else { 99 };
        if d >= radix { break; }
        any = true;
        var carry: u64 = d as u64;
        for k in limbs.len { carry += limbs[k] * r; limbs[k] = carry & 4294967295; carry = carry >> 32; }
        if carry != 0 {
            // Past 2^1024 every longer prefix is too.
            if limbs.len == 32 { return if negative { -1.0 / 0.0 } else { 1.0 / 0.0 }; }
            limbs.push(carry);
        }
        i += 1;
    }
    if !any { return 0.0 / 0.0; }
    let v = limbs_value(limbs);
    return if negative { -v } else { v };
}
// The integer of little-endian 32-bit limbs, rounded to the nearest double,
// ties to even: its first 64 significant bits, how many follow and whether
// any of those is 1.
fn limbs_value(limbs: const u64[:]) -> f64 {
    if limbs.len == 0 { return 0.0; }
    if limbs.len <= 2 {
        let x: u64 = if limbs.len == 2 { (limbs[1] << 32) | limbs[0] } else { limbs[0] };
        if x <= 9007199254740992 { let small: i64 = x as i64; let v: f64 = small; return v; }
    }
    var mant: u64 = 0; var dropped: i64 = 0; var sticky = false;
    var k = limbs.len - 1;
    while k >= 0 {
        var bit: i64 = 31;
        while bit >= 0 {
            let b = (limbs[k] >> bit) & 1;
            if (mant >> 63) == 0 { mant = (mant << 1) | b; } else { dropped += 1; if b != 0 { sticky = true; } }
            bit -= 1;
        }
        k -= 1;
    }
    return round_integer(mant, dropped, sticky);
}
// mant * 2^dropped, rounded to 53 significant bits (ties to even, `sticky`
// marking a nonzero bit among the dropped ones).
fn round_integer(mant: u64, dropped: i64, sticky: bool) -> f64 {
    if mant == 0 { return 0.0; }
    var length: i64 = 0; while length < 64 && (mant >> length) != 0 { length += 1; }
    let shift = if length > 53 { length - 53 } else { 0 };
    let one: u64 = 1;
    var m = mant >> shift;
    if shift > 0 {
        let rest = mant & ((one << shift) - 1); let half = one << (shift - 1);
        if rest > half || rest == half && (sticky || (m & 1) == 1) { m += 1; }
    }
    var scale = shift + dropped;
    if scale > 1023 { return 1.0 / 0.0; }
    // m is at most 2^53, exact as a double, and so is each product until
    // one passes the largest double: Infinity, as rounding asks.
    let mi: i64 = m as i64;
    var v: f64 = mi;
    while scale >= 32 && v != 1.0 / 0.0 { v = v * 4294967296.0; scale -= 32; }
    while scale > 0 && v != 1.0 / 0.0 { v = v * 2.0; scale -= 1; }
    return v;
}
// parse_float(t): an optional sign, then `Infinity` or the longest prefix
// that is a StrUnsignedDecimalLiteral, `(D+ ('.' D*)? | '.' D+) ([eE]
// [+-]? D+)?` (an exponent without digits is not part of it), correctly
// rounded; no prefix is NaN, and no hexadecimal, octal or binary form is
// read ("0x1" is 0). Of the significant digits the first 1,100 are kept and
// any later nonzero one is a sticky 1: a double's rounding boundary has
// fewer significant digits, so only that nonzeroness can decide a side.
fn parse_float(t: const u16[:]) -> f64 {
    var i: i64 = 0; var negative = false;
    if i < t.len && (t[i] == '+' || t[i] == '-') { negative = t[i] == '-'; i += 1; }
    let infinity = "Infinity";
    if t.len - i >= infinity.len {
        var same = true; for c, j in infinity { if t[i + j] != c { same = false; } }
        if same { return if negative { -1.0 / 0.0 } else { 1.0 / 0.0 }; }
    }
    // States: 0 nothing, 2 integer digits, 3 a point without digits before
    // it, 4 a point after digits or fraction digits, 5 `e`, 6 the exponent's
    // sign, 7 exponent digits; 2, 4 and 7 end a literal.
    var state:    i64     = 0; var any = false;
    var digits:   u8[>..] = []; var fraction: i64 = 0; var dropped: i64 = 0; var sticky = false;
    var exponent: i64     = 0; var exponent_negative = false;
    while i < t.len {
        let c = t[i]; let digit = c >= '0' && c <= '9';
        var next: i64 = -1;
        if digit { next = if state <= 2 { 2 } else if state <= 4 { 4 } else { 7 }; }
        else if c == '.' { next = if state == 0 { 3 } else if state == 2 { 4 } else { -1 }; }
        else if c == 'e' || c == 'E' { next = if state == 2 || state == 4 { 5 } else { -1 }; }
        else if c == '+' || c == '-' { next = if state == 5 { 6 } else { -1 }; }
        if next < 0 { break; }
        if next == 7 { exponent = min(exponent * 10 + (c - '0'), 4000000000); }
        else if state == 5 && c == '-' { exponent_negative = true; }
        else if digit {
            any = true;
            if next == 4 { fraction += 1; }
            if c != '0' || digits.len > 0 {
                if digits.len < 1100 { digits.push(c as u8); } else { dropped += 1; if c != '0' { sticky = true; } }
            }
        }
        state = next; i += 1;
    }
    if !any { return 0.0 / 0.0; }
    let zero: f64 = 0.0;
    if digits.len == 0 { return if negative { -zero } else { zero }; }
    var shift = dropped - fraction;
    if state == 7 { shift += if exponent_negative { -exponent } else { exponent }; }
    if sticky { digits.push('1'); shift -= 1; }
    digits.push('e'); format(digits, shift);
    let v, ok = parse_flt(digits);
    if !ok { return 0.0 / 0.0; }
    return if negative { -v } else { v };
}
