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
