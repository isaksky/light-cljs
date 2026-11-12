import std;

namespace locale;

// The structural check of a locale tag that CanonicalizeLocaleList makes
// (ECMA-402 IsStructurallyValidLanguageTag over a Unicode BCP 47 locale
// identifier), as the Pants engine's rust/src/intl/tag.rs LanguageTag::parse
// accepts one (light-cljs-905, for toLocaleUpperCase and
// toLocaleLowerCase): a language of 2, 3 or 5 to 8 letters; an optional
// script (4 letters) and region (2 letters or 3 digits); distinct variants
// (5 to 8 alphanumerics, or 4 starting with a digit); distinct singleton
// extensions, a `u` extension's attributes and keys with types of 3 to 8
// alphanumerics (and the legacy `co-phonebook` and `co-dictionary`), a `t` extension's
// language and fields, any other extension's subtags of 2 to 8
// alphanumerics; then `x` private use subtags of 1 to 8 alphanumerics.
// Case is not significant; at most 511 ASCII units and 63 subtags. This
// covers validation and the language subtag only: the Intl area
// (light-cljs-774) adds the canonical form (aliases, subtag order) on top.
//
// language_subtag answers the length of the language subtag (its first
// subtag, which canonicalization keeps for every language the case
// methods tailor), or -1 for a tag that is not structurally valid.
fn language_subtag(units: const u16[:]) -> i64 {
    if units.len == 0 || units.len >= 512 { return -1; }
    var text: u8[>..] = [];
    for u in units {
        if u >= 0x80 { return -1; }
        text.push(if u >= 'A' && u <= 'Z' { (u + 32) as u8 } else { u as u8 });
    }
    var starts: i64[>..] = []; var ends: i64[>..] = [];
    var begin:  i64      = 0;
    for c, i in text { if c == '-' { starts.push(begin); ends.push(i); begin = i + 1; } }
    starts.push(begin); ends.push(text.len);
    let n = starts.len;
    if n >= 64 { return -1; }
    for k in n { if ends[k] == starts[k] { return -1; } }
    var i = base_end(text, starts, ends, 0, n);
    if i < 0 { return -1; }
    var seen: u64 = 0;
    while i < n {
        let singleton = text[starts[i]..ends[i]];
        if singleton.len != 1 || !alnum(singleton) { return -1; }
        let c = singleton[0];
        let shift: i64 = if digit(c) { c - '0' + 26 } else { c - 'a' };
        let bit:   u64 = (1 as u64) << shift;
        if seen & bit != 0 { return -1; }
        seen = seen | bit;
        i += 1;
        let start = i;
        if c == 'x' {
            if i == n { return -1; }
            for k in i..n { if ends[k] - starts[k] > 8 || !alnum(text[starts[k]..ends[k]]) { return -1; } }
            return ends[0];
        }
        while i < n && ends[i] - starts[i] != 1 { i += 1; }
        if start == i { return -1; }
        if c == 'u' {
            if !unicode_extension(text, starts, ends, start, i) { return -1; }
        } else {
            for k in start..i { let len = ends[k] - starts[k]; if len < 2 || len > 8 || !alnum(text[starts[k]..ends[k]]) { return -1; } }
            if c == 't' && !transform_extension(text, starts, ends, start, i) { return -1; }
        }
    }
    return ends[0];
}
// The index after the language, script, region and variant subtags among
// subtags [from, to), or -1 when they are not valid.
fn base_end(text: const u8[:], starts: const i64[:], ends: const i64[:], from: i64, to: i64) -> i64 {
    if !language(text[starts[from]..ends[from]]) { return -1; }
    var i = from + 1;
    if i < to && script(text[starts[i]..ends[i]]) { i += 1; }
    if i < to && region(text[starts[i]..ends[i]]) { i += 1; }
    let variants = i;
    while i < to && ends[i] - starts[i] != 1 {
        let v = text[starts[i]..ends[i]];
        if !variant(v) { return -1; }
        for k in variants..i { if same(text[starts[k]..ends[k]], v) { return -1; } }
        i += 1;
    }
    return i;
}
// A `u` extension's subtags [from, to): attributes, then keys (an
// alphanumeric and a letter), each with an optional type.
fn unicode_extension(text: const u8[:], starts: const i64[:], ends: const i64[:], from: i64, to: i64) -> bool {
    var j = from;
    while j < to && ends[j] - starts[j] != 2 {
        if !unicode_type(text[starts[j]..ends[j]]) { return false; }
        j += 1;
    }
    while j < to {
        let key = text[starts[j]..ends[j]];
        if !alnum(key) || !letter(key[1]) { return false; }
        j += 1;
        let first = j;
        while j < to && ends[j] - starts[j] != 2 { j += 1; }
        if first < j {
            let value = text[starts[first]..ends[j - 1]];
            // The legacy collation types phonebook and dictionary are
            // aliases (phonebk, dict) before the type's length is checked.
            if !(same(key, "co") && (same(value, "phonebook") || same(value, "dictionary"))) && !unicode_type(value) { return false; }
        }
    }
    return true;
}
// A `t` extension's subtags [from, to): an optional language tag (no
// extensions), then fields of a key (a letter and a digit) and one or more
// types.
fn transform_extension(text: const u8[:], starts: const i64[:], ends: const i64[:], from: i64, to: i64) -> bool {
    var first = from;
    while first < to && !field_key(text[starts[first]..ends[first]]) { first += 1; }
    if first > from && base_end(text, starts, ends, from, first) != first { return false; }
    var k = first;
    while k < to {
        k += 1;
        let values = k;
        while k < to && !field_key(text[starts[k]..ends[k]]) {
            if !unicode_type(text[starts[k]..ends[k]]) { return false; }
            k += 1;
        }
        if k == values { return false; }
    }
    return true;
}
inline fn letter(c: u8) -> bool { return c >= 'a' && c <= 'z'; }
inline fn digit(c: u8) -> bool { return c >= '0' && c <= '9'; }
fn alpha(s: const u8[:]) -> bool { for c in s { if !letter(c) { return false; } } return true; }
fn alnum(s: const u8[:]) -> bool { for c in s { if !letter(c) && !digit(c) { return false; } } return true; }
fn same(a: const u8[:], b: const u8[:]) -> bool {
    if a.len != b.len { return false; }
    for c, i in a { if c != b[i] { return false; } }
    return true;
}
fn language(s: const u8[:]) -> bool { return (s.len >= 2 && s.len <= 3 || s.len >= 5 && s.len <= 8) && alpha(s); }
fn script(s: const u8[:]) -> bool { return s.len == 4 && alpha(s); }
fn region(s: const u8[:]) -> bool {
    if s.len == 2 { return alpha(s); }
    if s.len != 3 { return false; }
    for c in s { if !digit(c) { return false; } }
    return true;
}
fn variant(s: const u8[:]) -> bool { return alnum(s) && (s.len >= 5 && s.len <= 8 || s.len == 4 && digit(s[0])); }
inline fn field_key(s: const u8[:]) -> bool { return s.len == 2 && letter(s[0]) && digit(s[1]); }
// Hyphen-separated subtags of 3 to 8 alphanumerics.
fn unicode_type(s: const u8[:]) -> bool {
    var len: i64 = 0;
    for c in s {
        if c == '-' { if len < 3 || len > 8 { return false; } len = 0; }
        else if letter(c) || digit(c) { len += 1; }
        else { return false; }
    }
    return len >= 3 && len <= 8;
}
