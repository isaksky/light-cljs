import std;
import .source_vm;
import .realm;
import .coerce;
import .array_methods;
import .regexp_object;
import .proxy;
import .string_methods;
import .unicode_data;
import .language_tag;

namespace source;

// String case mapping and normalization (light-cljs-905; decisions 2, 5
// and 6 of docs/javascript-coverage.md). Semantics: the Pants engine's
// native/string_methods.rs (its case, locale and normalization stages) and
// value/string_unicode.rs; QuickJS js_string_toLowerCase (the four case
// methods by its magic argument), js_string_normalize and libunicode.c.
// The Unicode 17.0.0 tables are static data (unicode_data.goose, generated
// by scripts/unicode-data.bb), read by binary search, never heap rows.
//
// toUpperCase, toLowerCase and normalize are plain functions after their
// conversion plans (the receiver's ToString, normalize's form's), as is a
// locale-sensitive case method whose locales are undefined, a primitive or
// a string. One whose locales are an object reads the list
// (CanonicalizeLocaleList) in a StringMethod frame, after the receiver's
// conversion: receiver S, input the list, callee its length, index the
// next element and output the first element's rules (case_rules::), each
// step one Get, HasProperty or conversion, its JavaScript (getters, proxy
// traps, an element's toString) running above the frame. Every unit read,
// code point visited and unit made is charged to the work limit.
let string_pc::LocaleLength = 21;
let string_pc::LocaleElement = 22;
let string_pc::LocalePresent = 23;
let string_pc::LocaleValue = 24;
// The language rules a locale selects: none, Turkish and Azeri (dotted and
// dotless i), Lithuanian (the dot above). The default locale is the root
// until the Intl area (light-cljs-774) settles Goose's.
let case_rules::Root = 0;
let case_rules::Turkic = 1;
let case_rules::Lithuanian = 2;
// normalize's forms, in the Pants engine's order.
let normal_form::Nfc = 0;
let normal_form::Nfd = 1;
let normal_form::Nfkc = 2;
let normal_form::Nfkd = 3;
inline fn string_locale_case(b: Builtin) -> bool { return b == Builtin.StringPrototypeToLocaleUpperCase || b == Builtin.StringPrototypeToLocaleLowerCase; }
inline fn string_lowers(b: Builtin) -> bool { return b == Builtin.StringPrototypeToLowerCase || b == Builtin.StringPrototypeToLocaleLowerCase; }

// Unicode lookups (string_unicode.rs). The row of code point c in an
// ascending table, or -1.
fn unicode_row(codes: const u32[:], c: i64) -> i64 {
    var lo: i64 = 0; var hi = codes.len;
    while lo < hi { let mid = lo + (hi - lo) / 2; if codes[mid] < c { lo = mid + 1; } else { hi = mid; } }
    return if lo < codes.len && codes[lo] == c { lo } else { -1 };
}
// Whether c lies in one of the ascending inclusive ranges [first[i], last[i]].
fn unicode_in(first: const u32[:], last: const u32[:], c: i64) -> bool {
    var lo: i64 = 0; var hi = last.len;
    while lo < hi { let mid = lo + (hi - lo) / 2; if last[mid] < c { lo = mid + 1; } else { hi = mid; } }
    return lo < last.len && first[lo] <= c;
}
fn unicode_class(c: i64) -> i64 { let i = unicode_row(unicode::CLASS_CODES, c); return if i < 0 { 0 } else { unicode::CLASSES[i] }; }
fn unicode_cased(c: i64) -> bool { return unicode_in(unicode::CASED_FIRST, unicode::CASED_LAST, c); }
fn unicode_case_ignorable(c: i64) -> bool { return unicode_in(unicode::CASE_IGNORABLE_FIRST, unicode::CASE_IGNORABLE_LAST, c); }
fn unicode_soft_dotted(c: i64) -> bool { return unicode_in(unicode::SOFT_DOTTED_FIRST, unicode::SOFT_DOTTED_LAST, c); }
// The canonical composition of a and b (Hangul syllables algorithmically),
// or -1.
fn unicode_compose(a: i64, b: i64) -> i64 {
    if a >= 0x1100 && a < 0x1113 && b >= 0x1161 && b < 0x1176 { return 0xac00 + (a - 0x1100) * 588 + (b - 0x1161) * 28; }
    if a >= 0xac00 && a <= 0xd7a3 && (a - 0xac00) % 28 == 0 && b >= 0x11a8 && b < 0x11c3 { return a + b - 0x11a7; }
    let key: u64 = (a as u64) * 0x200000 + (b as u64);
    var lo: i64 = 0; var hi = unicode::COMPOSE_PAIRS.len;
    while lo < hi { let mid = lo + (hi - lo) / 2; if unicode::COMPOSE_PAIRS[mid] < key { lo = mid + 1; } else { hi = mid; } }
    return if lo < unicode::COMPOSE_PAIRS.len && unicode::COMPOSE_PAIRS[lo] == key { unicode::COMPOSED[lo] } else { -1 };
}
// Code point c in UTF-16, appended.
fn put_point(out: u16[>..]&, c: i64) {
    if c > 0xffff { out.push((0xd800 + ((c - 0x10000) >> 10)) as u16); out.push((0xdc00 + ((c - 0x10000) & 1023)) as u16); }
    else { out.push(c as u16); }
}
// The code points of UTF-16 units: a surrogate pair is one, a lone
// surrogate itself.
fn code_points(units: const u16[:], out: u32[>..]&) {
    var i: i64 = 0;
    while i < units.len {
        var c: i64 = units[i]; i += 1;
        if string_lead(c) && i < units.len && string_trail(units[i]) { c = 0x10000 + ((c - 0xd800) << 10) + units[i] - 0xdc00; i += 1; }
        out.push(c as u32);
    }
}
// A mapping table's code points for c (NAME_CODES, NAME_STARTS, NAME_RUN),
// appended as UTF-16 units; false when c maps to itself.
fn mapped_units(codes: const u32[:], starts: const u16[:], run: const u32[:], c: i64, out: u16[>..]&) -> bool {
    let i = unicode_row(codes, c);
    if i < 0 { return false; }
    for k in (starts[i] as i64)..(starts[i + 1] as i64) { put_point(out, run[k]); }
    return true;
}

// Case mapping (the Pants engine's case_step and lookahead; QuickJS
// js_string_toLowerCase): each code point's full mapping, Final_Sigma for
// U+03A3 when lowering, and the Turkic and Lithuanian rules.
fn case_default(out: u16[>..]&, lower: bool, c: i64) {
    if c < 0x80 {
        out.push((if lower && c >= 'A' && c <= 'Z' { c + 32 } else if !lower && c >= 'a' && c <= 'z' { c - 32 } else { c }) as u16);
        return;
    }
    let mapped = if lower { mapped_units(unicode::LOWER_CODES, unicode::LOWER_STARTS, unicode::LOWER_RUN, c, out) } else { mapped_units(unicode::UPPER_CODES, unicode::UPPER_STARTS, unicode::UPPER_RUN, c, out) };
    if !mapped { put_point(out, c); }
}
// Final_Sigma's context of the U+03A3 at `at`: the nearest code point
// before it that is not Case_Ignorable is Cased, and the nearest after it
// is not (or there is none). Each visit is a work unit; the scans of two
// sigmas never overlap, since a sigma is not Case_Ignorable.
fn final_sigma(heap: Heap&, points: const u32[:], at: i64) -> bool {
    var before = false; var k = at - 1;
    while k >= 0 {
        if !charge(heap) { return false; }
        if !unicode_case_ignorable(points[k]) { before = unicode_cased(points[k]); break; }
        k -= 1;
    }
    if !before { return false; }
    k = at + 1;
    while k < points.len {
        if !charge(heap) { return false; }
        if !unicode_case_ignorable(points[k]) { return !unicode_cased(points[k]); }
        k += 1;
    }
    return true;
}
// String s mapped to upper or lower case under `rules`, a new string in its
// canonical encoding (Undefined with a fault).
fn string_case(job: Job&, s: Value, lower: bool, rules: i64) -> Value {
    let heap .= job.heap; let len = str_len(heap, s);
    if !charge(heap, len) { return Value.Undefined; }
    var units:  u16[>..] = []; append_utf16(heap, s, units);
    var points: u32[>..] = []; code_points(units, points);
    let n = points.len;
    var out: u16[>..] = [];
    // `skip` is a U+0307 the rules drop; the code points below `through`
    // are copied unchanged.
    var skip: i64 = -1; var through: i64 = 0;
    for at in n {
        let c: i64 = points[at];
        if at == skip { skip = -1; continue; }
        if at < through { put_point(out, c); continue; }
        if lower && c == 0x3a3 {
            let last = final_sigma(heap, points, at);
            if heap.fault != Fault.None { return Value.Undefined; }
            put_point(out, if last { 0x3c2 } else { 0x3c3 });
        } else if lower && (rules == case_rules::Turkic && c == 0x49 || rules == case_rules::Lithuanian && (c == 0x49 || c == 0x4a || c == 0x12e)) || !lower && rules == case_rules::Lithuanian && unicode_soft_dotted(c) {
            // Look past the Case_Ignorable code points that follow: a
            // U+0307 the Turkic rules (unless an accent above blocks it)
            // and Lithuanian uppercasing drop, an accent above after which
            // Lithuanian lowercasing keeps the dot.
            var look = at + 1; var blocked = false; var dot = false;
            while look < n && unicode_case_ignorable(points[look]) {
                if !charge(heap) { return Value.Undefined; }
                let d: i64 = points[look];
                if d == 0x307 && (rules == case_rules::Turkic || !lower) { if !blocked { skip = look; } break; }
                if unicode_class(d) == 230 {
                    if lower && rules != case_rules::Turkic { dot = true; break; }
                    if rules == case_rules::Turkic { blocked = true; }
                }
                look += 1;
            }
            if skip >= 0 || !lower { through = look; }
            if rules == case_rules::Turkic { put_point(out, if skip >= 0 { 0x69 } else { 0x131 }); }
            else { case_default(out, lower, c); if dot { put_point(out, 0x307); } }
        } else if rules == case_rules::Turkic && lower && c == 0x130 { put_point(out, 0x69); }
        else if rules == case_rules::Turkic && !lower && c == 0x69 { put_point(out, 0x130); }
        else if rules == case_rules::Lithuanian && lower && (c == 0xcc || c == 0xcd || c == 0x128) {
            put_point(out, 0x69); put_point(out, 0x307); put_point(out, if c == 0xcc { 0x300 } else if c == 0xcd { 0x301 } else { 0x303 });
        } else { case_default(out, lower, c); }
    }
    if !string_result_room(job, out.len) { return Value.Undefined; }
    return import_text(heap, out);
}

// Normalization (the Pants engine's normalize_step; QuickJS
// js_string_normalize): full canonical or compatibility decomposition
// (Hangul syllables algorithmically), canonical ordering, then for NFC and
// NFKC composition with exclusions. A decomposed code point is held as its
// combining class above bit 21, so ordering and composition read the class
// once.
inline fn class_bits() -> i64 { return 21; }
inline fn point_of(p: u32) -> i64 { return p & 0x1fffff; }
inline fn class_of(p: u32) -> i64 { return p >> class_bits(); }
fn push_classed(points: u32[>..]&, c: i64) { points.push(((unicode_class(c) << class_bits()) | c) as u32); }
// A decomposition table's code points for c, appended with their classes;
// false when c has none.
fn mapped_points(codes: const u32[:], starts: const u16[:], run: const u32[:], c: i64, points: u32[>..]&) -> bool {
    let i = unicode_row(codes, c);
    if i < 0 { return false; }
    for k in (starts[i] as i64)..(starts[i + 1] as i64) { push_classed(points, run[k]); }
    return true;
}
// Whether a string of `units` UTF-16 units fits a result: past the maximum
// length a RangeError, past the job's units its memory limit.
fn string_case_room(job: Job&, units: i64) -> bool {
    if units > max_string_length() { range_error(job, "string too long"); return false; }
    if !units_fit(job.heap, units) { return memory(job.heap); }
    return true;
}
fn string_normalize(job: Job&, s: Value, form: i64) -> Value {
    let heap .= job.heap; let len = str_len(heap, s);
    if !charge(heap, len) { return Value.Undefined; }
    var units: u16[>..] = []; append_utf16(heap, s, units);
    // Below U+0300 every code point is NFC (no combining class, no
    // composition takes one as its second), below U+00C0 NFD, below U+00A0
    // NFKD and NFKC (unicode-data.bb checks these bounds against the tables).
    let quick: i64 = if form == normal_form::Nfc { 0x300 } else if form == normal_form::Nfd { 0xc0 } else { 0xa0 };
    var unchanged = true;
    for u in units { if u >= quick { unchanged = false; break; } }
    if unchanged { return s; }
    let compat = form == normal_form::Nfkc || form == normal_form::Nfkd;
    var points: u32[>..] = [];
    var size:   i64      = 0;
    var i:      i64      = 0;
    while i < units.len {
        var c: i64 = units[i]; i += 1;
        if string_lead(c) && i < units.len && string_trail(units[i]) { c = 0x10000 + ((c - 0xd800) << 10) + units[i] - 0xdc00; i += 1; }
        let before = points.len;
        let mapped = if compat { mapped_points(unicode::NFKD_CODES, unicode::NFKD_STARTS, unicode::NFKD_RUN, c, points) } else { mapped_points(unicode::NFD_CODES, unicode::NFD_STARTS, unicode::NFD_RUN, c, points) };
        if !mapped && c >= 0xac00 && c <= 0xd7a3 {
            let x = c - 0xac00;
            push_classed(points, 0x1100 + x / 588); push_classed(points, 0x1161 + (x % 588) / 28);
            if x % 28 != 0 { push_classed(points, 0x11a7 + x % 28); }
        } else if !mapped { push_classed(points, c); }
        for k in before..points.len { size += if point_of(points[k]) > 0xffff { 2 } else { 1 }; }
        if !charge(heap, points.len - before) || !string_case_room(job, size) { return Value.Undefined; }
    }
    // Canonical ordering: a stable sort of each run of non-starters by class.
    var at: i64 = 0;
    while at < points.len {
        if class_of(points[at]) == 0 { at += 1; continue; }
        var end = at + 1;
        while end < points.len && class_of(points[end]) != 0 { end += 1; }
        if end - at > 1 {
            if !charge(heap, end - at) { return Value.Undefined; }
            stable_sort(points[at..end]) { a, b => class_of(a) < class_of(b) };
        }
        at = end;
    }
    if form == normal_form::Nfc || form == normal_form::Nfkc {
        // A code point composes with the last starter unless a code point
        // between them has a class of zero or at least its own.
        var starter: i64 = -1; var last: i64 = 0; var kept: i64 = 0;
        for k in points.len {
            let p = points[k]; let cc = class_of(p);
            let composed = if starter >= 0 && (last == 0 || last < cc) { unicode_compose(point_of(points[starter]), point_of(p)) } else { -1 };
            if composed >= 0 { points[starter] = composed as u32; continue; }
            if cc == 0 { starter = kept; }
            points[kept] = p; kept += 1; last = cc;
        }
        if !charge(heap, points.len) { return Value.Undefined; }
        points.resize(kept);
    }
    var out: u16[>..] = [];
    for p in points { put_point(out, point_of(p)); }
    if !string_result_room(job, out.len) { return Value.Undefined; }
    return import_text(heap, out);
}
// normalize's form named by string f ("NFC", "NFD", "NFKC", "NFKD"), or -1.
fn normal_form_named(heap: const Heap&, f: Value) -> i64 {
    let len = str_len(heap, f);
    if len < 3 || len > 4 || unit(heap, f, 0) != 'N' || unit(heap, f, 1) != 'F' { return -1; }
    let k = len == 4;
    if k && unit(heap, f, 2) != 'K' { return -1; }
    let last = unit(heap, f, len - 1);
    if last == 'C' { return if k { normal_form::Nfkc } else { normal_form::Nfc }; }
    if last == 'D' { return if k { normal_form::Nfkd } else { normal_form::Nfd }; }
    return -1;
}

// toUpperCase, toLowerCase and normalize: plain functions after their
// plans converted the receiver (and normalize's form).
fn string_case_builtin(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let s = string_receiver(job, b, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    if b != Builtin.StringPrototypeNormalize { return string_case(job, s, string_lowers(b), case_rules::Root); }
    let f = form_value(heap, arg(args, 0));
    if f.is_undefined() { return string_normalize(job, s, normal_form::Nfc); }
    let text = primitive_string(job, b, f);
    if heap.fault != Fault.None { return Value.Undefined; }
    let form = normal_form_named(heap, text);
    if form < 0 { range_error(job, "invalid normalization form"); return Value.Undefined; }
    return string_normalize(job, s, form);
}

// The rules locale tag `tag` selects (its language subtag); a tag that is
// not structurally valid is a RangeError (-1).
fn case_locale_rules(job: Job&, tag: Value) -> i64 {
    let heap .= job.heap; let len = str_len(heap, tag);
    var units: u16[>..] = [];
    if len < 512 {
        if !charge(heap, len) { return -1; }
        append_utf16(heap, tag, units);
    }
    let n = locale::language_subtag(units);
    if n < 0 { range_error(job, "invalid language tag"); return -1; }
    if n != 2 { return case_rules::Root; }
    let a = units[0] | 0x20; let z = units[1] | 0x20;
    if a == 't' && z == 'r' || a == 'a' && z == 'z' { return case_rules::Turkic; }
    if a == 'l' && z == 't' { return case_rules::Lithuanian; }
    return case_rules::Root;
}
// toLocaleUpperCase and toLocaleLowerCase (ECMA-402 TransformCase), their
// receiver converted by their plan before the locales are read: undefined
// and primitives other than strings make an empty list (the Pants engine's
// list stage: an intrinsic wrapper prototype has no `length`), null is a
// TypeError, a string is a list of itself; an object's list is read in a
// StringMethod frame.
fn start_locale_case(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let s = string_receiver(job, b, receiver);
    if heap.fault != Fault.None { return; }
    let locales = form_value(heap, arg(args, 0));
    if heap.fault != Fault.None { return; }
    job.operands.resize(base);
    if !locales.is_object() {
        if locales.is_null() { type_error(job, "locales cannot be null"); return; }
        let rules = if locales.is_string() { case_locale_rules(job, locales) } else { case_rules::Root };
        if heap.fault != Fault.None { return; }
        let r = string_case(job, s, string_lowers(b), rules);
        if heap.fault == Fault.None { deliver(job, r); }
        return;
    }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: string_pc::LocaleLength, local_base: job.locals.len, operand_base: base, receiver: s, input: locales, callee: Value.Undefined, output: Value.Undefined, native: Native.StringMethod { which: b } });
    get_named(job, b, locales, "length");
}
// One step of a locale list's frame: ToLength of `length`, then for each
// index HasProperty, Get, the element's type check, its ToString and the
// tag's check, the first element's rules kept; past the length the case
// mapping.
fn locale_case_step(job: Job&, f: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match f.pc {
        string_pc::LocaleLength => {
            let ready, n = string_number(job, b);
            if !ready || heap.fault != Fault.None { return; }
            job.frames[at].callee = number(to_length(n)); job.frames[at].index = 0;
            job.frames[at].pc = string_pc::LocaleElement;
        },
        string_pc::LocaleElement => {
            if (f.index as f64) >= f.callee.num() {
                let r = string_case(job, f.receiver, string_lowers(b), if f.output.is_number() { f.output.num() as i64 } else { case_rules::Root });
                if heap.fault == Fault.None { finish_native(job, f, r); }
                return;
            }
            let key = index_key(heap, f.index);
            if heap.fault != Fault.None { return; }
            let has = has_property(heap, f.input, key);
            // A proxy's [[HasProperty]] answers at LocalePresent.
            if proxy_pending(heap, proxy_method::HasProperty) { job.frames[at].pc = string_pc::LocalePresent; proxy_take(job, proxy_method::HasProperty); return; }
            if heap.fault != Fault.None { return; }
            locale_element(job, at, b, has);
        },
        string_pc::LocalePresent => { locale_element(job, at, b, truthy(heap, job.operands.pop())); },
        string_pc::LocaleValue => {
            let v = form_value(heap, job.operands[job.operands.len - 1]);
            if heap.fault != Fault.None { return; }
            if !v.is_string() && !v.is_object() { type_error(job, "locale list element must be a string or object"); return; }
            let ready, tag = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            let rules = case_locale_rules(job, tag);
            if heap.fault != Fault.None { return; }
            if f.output.is_undefined() { job.frames[at].output = number(rules); }
            job.frames[at].index += 1; job.frames[at].pc = string_pc::LocaleElement;
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// Element `index` of the list is present (Get, its value or getter's at
// LocaleValue) or not (the next index).
fn locale_element(job: Job&, at: i64, b: Builtin, present: bool) {
    let heap .= job.heap;
    if !present { job.frames[at].index += 1; job.frames[at].pc = string_pc::LocaleElement; return; }
    job.frames[at].pc = string_pc::LocaleValue;
    let key = index_key(heap, job.frames[at].index);
    if heap.fault == Fault.None { get_key(job, b, job.frames[at].input, key); }
}
