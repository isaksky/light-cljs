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
