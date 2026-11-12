import std;
import math;
import .source_vm;
import .realm;
import .coerce;
import .array_callbacks;
import .array_methods;
import .regexp_object;
import .regexp_replace;
import .string_case;

namespace source;

// String code units, statics and search (light-cljs-903; decisions 2, 4,
// 5 of docs/javascript-coverage.md). Semantics: the Pants engine's
// native/string_methods.rs, string_raw.rs and code-unit native states;
// QuickJS js_string_charAt/charCodeAt/codePointAt/indexOf/includes,
// fromCharCode/fromCodePoint/raw/isWellFormed/toWellFormed.
//
// Accessors are plain functions after the receiver/position conversion
// plan. All other methods use Native.StringMethod. Its roots are ordinary
// frame Values and operands, so reclaim.goose's mark_frame/relink_frame and
// continuations.goose's SavedFrame preserve them without a new heap table.
// Every conversion/property read returns to the dispatch loop. Scans and
// construction process at most string_quantum() units plus a pair's second
// surrogate per step, charging
// each unit (including unsuccessful comparisons) to the work limit.
//
// Statics: argument_count arguments retained at operand_base; index the
// next argument, and the operands after the arguments the output pieces.
// raw: the same argument run (template included), receiver the raw object,
// callee its captured length, index its next element; input the piece being
// charged and output its cursor. Search: receiver S, input searchValue then
// its string, callee position; index the candidate, argument_count its
// matched prefix. Well-formedness: receiver S, index next unit, operands
// the output pieces. All pieces use the canonical job-string encoding.
let string_pc::Points = 0;
let string_pc::PointValue = 1;
let string_pc::Match = 2;
let string_pc::Needle = 3;
let string_pc::Position = 4;
let string_pc::Scan = 5;
let string_pc::Raw = 6;
let string_pc::Length = 7;
let string_pc::Piece = 8;
let string_pc::PieceCopy = 9;
let string_pc::Substitution = 10;
let string_pc::SubstitutionCopy = 11;
let string_pc::WellFormed = 12;
let string_pc::Finish = 13;
inline fn string_quantum() -> i64 { return 1024; }
inline fn string_access(b: Builtin) -> bool { return b == Builtin.StringPrototypeCharAt || b == Builtin.StringPrototypeCharCodeAt || b == Builtin.StringPrototypeCodePointAt || b == Builtin.StringPrototypeAt; }
inline fn string_predicate(b: Builtin) -> bool { return b == Builtin.StringPrototypeIncludes || b == Builtin.StringPrototypeStartsWith || b == Builtin.StringPrototypeEndsWith; }
inline fn string_search(b: Builtin) -> bool { return b == Builtin.StringPrototypeIndexOf || b == Builtin.StringPrototypeLastIndexOf || string_predicate(b); }
inline fn string_points(b: Builtin) -> bool { return b == Builtin.StringFromCharCode || b == Builtin.StringFromCodePoint; }
inline fn string_well_formed(b: Builtin) -> bool { return b == Builtin.StringPrototypeIsWellFormed || b == Builtin.StringPrototypeToWellFormed; }
inline fn string_native(b: Builtin) -> bool { return string_points(b) || string_search(b) || string_well_formed(b) || b == Builtin.StringRaw || string_pad(b) || b == Builtin.StringPrototypeSplit || string_locale_case(b); }
inline fn string_lead(u: i64) -> bool { return u >= 0xd800 && u <= 0xdbff; }
inline fn string_trail(u: i64) -> bool { return u >= 0xdc00 && u <= 0xdfff; }

fn string_receiver(job: Job&, b: Builtin, v: Value) -> Value {
    let r = form_value(job.heap, v);
    if r.is_undefined() || r.is_null() { return type_error(job, "String method requires a receiver"); }
    return primitive_string(job, b, r);
}
fn string_access_builtin(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let s = string_receiver(job, b, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    var p = to_integer_or_infinity(primitive_number(job, b, arg(args, 0)));
    if heap.fault != Fault.None { return Value.Undefined; }
    let len = str_len(heap, s);
    if b == Builtin.StringPrototypeAt && p < 0 { p += len; }
    if !(p >= 0 && p < len) {
        return if b == Builtin.StringPrototypeCharAt { key_named(heap, "") } else if b == Builtin.StringPrototypeCharCodeAt { number(0.0 / 0.0) } else { Value.Undefined };
    }
    let i = p as i64;
    if b == Builtin.StringPrototypeCharAt || b == Builtin.StringPrototypeAt { return unit_string(heap, s, i); }
    let u: i64 = unit(heap, s, i);
    if b == Builtin.StringPrototypeCodePointAt && string_lead(u) && i + 1 < len {
        let v: i64 = unit(heap, s, i + 1);
        if string_trail(v) { return number(0x10000 + ((u - 0xd800) << 10) + v - 0xdc00); }
    }
    return number(u);
}
// Operand top's ToNumber; an object's conversion runs above this frame.
fn string_number(job: Job&, b: Builtin) -> bool, f64 {
    let heap .= job.heap;
    let v = form_value(heap, job.operands.pop());
    if v.is_object() {
        if !convertible(heap, v) { unsupported_step(job, b, "ToNumber of a light-cljs runtime value"); return false, 0; }
        job.operands.push(v); start_convert(job, Hint.Number, true, 1, 1);
        return false, 0;
    }
    return true, primitive_number(job, b, v);
}
fn start_string_method(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    if string_pad(b) || b == Builtin.StringPrototypeSplit { start_string_transform(job, b, receiver, args, base); return; }
    // toLocaleUpperCase and toLocaleLowerCase (light-cljs-905; string_case.goose).
    if string_locale_case(b) { start_locale_case(job, b, receiver, args, base); return; }
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    let needle = arg(args, 0); let position = arg(args, 1);
    if string_points(b) || b == Builtin.StringRaw {
        if args.len > heap.limits.stack - job.locals.len - base { heap.fault = Fault.StackLimit; return; }
        var copied: Value[>..] = []; copied.append(args);
        let o = if b == Builtin.StringRaw { to_object(job, arg(copied, 0)) } else { Value.Undefined };
        if heap.fault != Fault.None { return; }
        job.operands.resize(base); job.operands.append(copied);
        job.frames.push(Frame { function: -1, pc: if b == Builtin.StringRaw { string_pc::Raw } else { string_pc::Points }, local_base: job.locals.len, operand_base: base, receiver: o, callee: Value.Undefined, argument_count: copied.len, native: Native.StringMethod { which: b } });
        if b == Builtin.StringRaw { get_named(job, b, o, "raw"); }
        return;
    }
    let s = string_receiver(job, b, receiver);
    if heap.fault != Fault.None { return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: if string_well_formed(b) { string_pc::WellFormed } else { string_pc::Needle }, local_base: job.locals.len, operand_base: base, receiver: s, input: needle, callee: position, native: Native.StringMethod { which: b } });
    let at = job.frames.len - 1;
    if string_search(b) {
        if string_predicate(b) && start_is_regexp(job, b, needle) { job.frames[at].pc = string_pc::Match; }
        else { push_operand(job, needle); }
    }
}
fn string_method_step(job: Job&, frame: Frame, b: Builtin) {
    if string_pad(b) || b == Builtin.StringPrototypeSplit { string_transform_step(job, frame, b); return; }
    if string_locale_case(b) { locale_case_step(job, frame, b); return; }
    if frame.pc == string_pc::Finish {
        let base = frame.operand_base + (if string_points(b) || b == Builtin.StringRaw { frame.argument_count } else { 0 });
        let s = concat_pieces(job, job.operands[base..]);
        if job.heap.fault == Fault.None { finish_native(job, frame, s); }
        return;
    }
    if string_points(b) { string_points_step(job, frame, b); }
    else if b == Builtin.StringRaw { string_raw_step(job, frame); }
    else if string_search(b) { string_search_step(job, frame, b); }
    else { string_well_formed_step(job, frame, b); }
}
// Convert and validate one argument before a later argument is touched.
// Consecutive primitives can share a bounded output chunk.
fn string_points_step(job: Job&, f: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1; let base = f.operand_base;
    if f.pc == string_pc::PointValue {
        let v = job.operands.pop(); job.operands[base + f.index] = v; job.frames[at].pc = string_pc::Points;
        return;
    }
    var units: u16[>..] = []; var i = f.index;
    while i < f.argument_count && units.len < string_quantum() {
        let v = form_value(heap, job.operands[base + i]);
        if v.is_object() { break; }
        if !charge(heap) { return; }
        let n = primitive_number(job, b, v);
        if heap.fault != Fault.None { return; }
        if b == Builtin.StringFromCharCode { units.push(to_uint16(n) as u16); }
        else {
            if !(n >= 0 && n <= 0x10ffff) || trunc(n) != n { range_error(job, "invalid code point"); return; }
            let cp = n as i64;
            if cp > 0xffff { units.push((0xd800 + ((cp - 0x10000) >> 10)) as u16); units.push((0xdc00 + ((cp - 0x10000) & 1023)) as u16); }
            else { units.push(cp as u16); }
        }
        i += 1;
    }
    if units.len > 0 {
        let s = import_text(heap, units);
        if heap.fault != Fault.None || !push_operand(job, s) { return; }
    }
    job.frames[at].index = i;
    if i == f.argument_count {
        // Keep the completed pieces visible at a budget cut before their
        // final copy, including the peak operand height for stack limits.
        job.frames[at].pc = string_pc::Finish;
        return;
    }
    let v = form_value(heap, job.operands[base + i]);
    if v.is_object() {
        if !convertible(heap, v) { unsupported_step(job, b, "ToNumber of a light-cljs runtime value"); return; }
        job.frames[at].pc = string_pc::PointValue;
        if push_operand(job, v) { start_convert(job, Hint.Number, true, 1, 1); }
    }
}
inline fn string_search_result(b: Builtin, p: i64) -> Value { return if string_predicate(b) { boolean(p >= 0) } else { number(p) }; }
fn string_search_step(job: Job&, f: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match f.pc {
        string_pc::Match => {
            let m = job.operands.pop();
            if is_regexp_answer(heap, f.input, m) { type_error(job, "String search predicate does not accept a RegExp"); return; }
            job.frames[at].pc = string_pc::Needle; push_operand(job, f.input);
        },
        string_pc::Needle => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            job.frames[at].input = s; job.frames[at].pc = string_pc::Position;
            push_operand(job, f.callee);
        },
        string_pc::Position => {
            let ready, n = string_number(job, b);
            if !ready || heap.fault != Fault.None { return; }
            let len = str_len(heap, f.receiver); let size = str_len(heap, f.input);
            let p = if b == Builtin.StringPrototypeEndsWith && f.callee.is_undefined() || b == Builtin.StringPrototypeLastIndexOf && n != n { len } else { to_integer_or_infinity(n) };
            var start = min(max(p, 0.0), len) as i64;
            if b == Builtin.StringPrototypeEndsWith { start -= size; }
            if size == 0 { finish_native(job, f, string_search_result(b, start)); return; }
            if b == Builtin.StringPrototypeLastIndexOf { start = min(start, len - size); }
            if start < 0 || start > len - size { finish_native(job, f, string_search_result(b, -1)); return; }
            job.frames[at].pc = string_pc::Scan; job.frames[at].index = start; job.frames[at].argument_count = 0;
        },
        string_pc::Scan => { string_scan(job, f, b); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
fn string_scan(job: Job&, f: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let len = str_len(heap, f.receiver); let size = str_len(heap, f.input);
    var p = f.index; var matched = f.argument_count;
    for visited in string_quantum() {
        if p < 0 || p > len - size { finish_native(job, f, string_search_result(b, -1)); return; }
        if !charge(heap) { return; }
        if unit(heap, f.receiver, p + matched) == unit(heap, f.input, matched) {
            matched += 1;
            if matched == size { finish_native(job, f, string_search_result(b, p)); return; }
        } else {
            if b == Builtin.StringPrototypeStartsWith || b == Builtin.StringPrototypeEndsWith { finish_native(job, f, boolean(false)); return; }
            p += if b == Builtin.StringPrototypeLastIndexOf { -1 } else { 1 }; matched = 0;
        }
    }
    job.frames[at].index = p; job.frames[at].argument_count = matched;
}
fn string_raw_element(job: Job&, at: i64) {
    let f = job.frames[at]; job.frames[at].pc = string_pc::Piece;
    let k = index_key(job.heap, f.index);
    if job.heap.fault == Fault.None { get_key(job, Builtin.StringRaw, f.receiver, k); }
}
fn string_raw_finish(job: Job&, f: Frame) {
    let s = concat_pieces(job, job.operands[(f.operand_base + f.argument_count)..]);
    if job.heap.fault == Fault.None { finish_native(job, f, s); }
}
fn string_raw_step(job: Job&, f: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1; let b = Builtin.StringRaw;
    match f.pc {
        string_pc::Raw => {
            let raw = to_object(job, job.operands.pop());
            if heap.fault != Fault.None { return; }
            job.frames[at].receiver = raw; job.frames[at].pc = string_pc::Length; get_named(job, b, raw, "length");
        },
        string_pc::Length => {
            let ready, n = string_number(job, b);
            if !ready || heap.fault != Fault.None { return; }
            let len = to_length(n);
            if len == 0 { string_raw_finish(job, f); return; }
            job.frames[at].callee = number(len); string_raw_element(job, at);
        },
        string_pc::Piece, string_pc::Substitution => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            job.frames[at].input = s; job.frames[at].output = number(0);
            job.frames[at].pc = if f.pc == string_pc::Piece { string_pc::PieceCopy } else { string_pc::SubstitutionCopy };
        },
        string_pc::PieceCopy, string_pc::SubstitutionCopy => {
            let len = str_len(heap, f.input); let cursor = f.output.num() as i64;
            let end = min(len, cursor + string_quantum());
            if !charge(heap, end - cursor) { return; }
            if end < len { job.frames[at].output = number(end); return; }
            if !push_operand(job, f.input) { return; }
            if f.pc == string_pc::PieceCopy {
                if f.index + 1 >= f.callee.num() { job.frames[at].pc = string_pc::Finish; return; }
                if f.index + 1 < f.argument_count {
                    job.frames[at].pc = string_pc::Substitution;
                    push_operand(job, job.operands[f.operand_base + f.index + 1]); return;
                }
            }
            job.frames[at].index += 1; string_raw_element(job, at);
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
fn string_well_formed_step(job: Job&, f: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1; let len = str_len(heap, f.receiver);
    var i = f.index; var units: u16[>..] = [];
    let end = min(len, i + string_quantum());
    while i < end {
        if !charge(heap) { return; }
        let u: i64 = unit(heap, f.receiver, i); i += 1;
        if string_lead(u) && i < len && string_trail(unit(heap, f.receiver, i)) {
            if !charge(heap) { return; }
            if b == Builtin.StringPrototypeToWellFormed { units.push(u as u16); units.push(unit(heap, f.receiver, i)); }
            i += 1;
        } else if string_lead(u) || string_trail(u) {
            if b == Builtin.StringPrototypeIsWellFormed { finish_native(job, f, boolean(false)); return; }
            units.push(0xfffd);
        } else if b == Builtin.StringPrototypeToWellFormed { units.push(u as u16); }
    }
    if units.len > 0 {
        let s = import_text(heap, units);
        if heap.fault != Fault.None || !push_operand(job, s) { return; }
    }
    if i == len {
        if b == Builtin.StringPrototypeIsWellFormed { finish_native(job, f, boolean(true)); }
        else { job.frames[at].index = i; job.frames[at].pc = string_pc::Finish; }
    } else { job.frames[at].index = i; }
}

// Extraction and construction after the conversion plan (light-cljs-904).
// No user code runs here. Substrings share their source's unit run when
// the encoding stays canonical. Scans and all result units are charged;
// length and memory checks precede construction of a long result.
inline fn max_string_length() -> i64 { return 1073741823; }
inline fn string_pad(b: Builtin) -> bool { return b == Builtin.StringPrototypePadStart || b == Builtin.StringPrototypePadEnd; }
fn string_result_room(job: Job&, len: f64) -> bool {
    if len > max_string_length() { range_error(job, "string too long"); return false; }
    let heap .= job.heap;
    if !units_fit(heap, len as i64) || heap.strings.len - heap.image.strings >= heap.limits.units { return memory(heap); }
    return charge(heap, len as i64);
}
inline fn string_relative(n: f64, len: i64) -> i64 { return if n < 0 { max(len + n, 0.0) as i64 } else { min(n, len) as i64 }; }
fn string_extract(job: Job&, b: Builtin, s: Value, args: const Value[:]) -> Value {
    let heap .= job.heap; let len = str_len(heap, s);
    let first = to_integer_or_infinity(primitive_number(job, b, arg(args, 0)));
    let end_arg = form_value(heap, arg(args, 1));
    let second = if end_arg.is_undefined() { len as f64 } else { to_integer_or_infinity(primitive_number(job, b, end_arg)) };
    if heap.fault != Fault.None { return Value.Undefined; }
    var start = if b == Builtin.StringPrototypeSubstring { min(max(first, 0.0), len) as i64 } else { string_relative(first, len) };
    var end = if b == Builtin.StringPrototypeSubstring { min(max(second, 0.0), len) as i64 }
        else if b == Builtin.StringPrototypeSubstr { start + min(max(second, 0.0), len - start) as i64 }
        else { string_relative(second, len) };
    if b == Builtin.StringPrototypeSubstring && start > end { let swap = start; start = end; end = swap; }
    end = max(start, end);
    if !charge(heap, end - start) { return Value.Undefined; }
    return substring(heap, s, start, end);
}
fn string_repeat(job: Job&, b: Builtin, s: Value, count: Value) -> Value {
    let heap .= job.heap;
    let n = to_integer_or_infinity(primitive_number(job, b, count));
    if heap.fault != Fault.None { return Value.Undefined; }
    if n < 0 || n == 1.0 / 0.0 { range_error(job, "invalid repeat count"); return Value.Undefined; }
    let len = str_len(heap, s);
    if n == 0 || len == 0 { return key_named(heap, ""); }
    if n > max_string_length() / (len as f64) { range_error(job, "string too long"); return Value.Undefined; }
    let total = (n * len) as i64;
    if !string_result_room(job, total) { return Value.Undefined; }
    var units: u16[>..] = [];
    for i in n as i64 { append_utf16(heap, s, units); }
    return import_text(heap, units);
}
fn string_trim(job: Job&, b: Builtin, s: Value) -> Value {
    let heap .= job.heap; let len = str_len(heap, s);
    if !charge(heap, len) { return Value.Undefined; }
    var start: i64 = 0; var end = len;
    if b != Builtin.StringPrototypeTrimEnd { while start < end && str_white_space(unit(heap, s, start)) { start += 1; } }
    if b != Builtin.StringPrototypeTrimStart { while end > start && str_white_space(unit(heap, s, end - 1)) { end -= 1; } }
    return substring(heap, s, start, end);
}
fn string_concat(job: Job&, b: Builtin, s: Value, args: const Value[:]) -> Value {
    let heap .= job.heap; var pieces: Value[>..] = [s]; var total = str_len(heap, s);
    for a in args {
        let piece = primitive_string(job, b, a);
        if heap.fault != Fault.None { return Value.Undefined; }
        let n = str_len(heap, piece);
        if n > max_string_length() - total { range_error(job, "string too long"); return Value.Undefined; }
        if !charge(heap, n) { return Value.Undefined; }
        total += n; pieces.push(piece);
    }
    if !charge(heap, str_len(heap, s)) { return Value.Undefined; }
    return concat_pieces(job, pieces);
}
struct HtmlMethod { which: Builtin, tag: u8[..6], attribute: u8[..5] = "" }
let HTML_METHODS: HtmlMethod[13] = [
    HtmlMethod { which: Builtin.StringPrototypeAnchor, tag: "a", attribute: "name" },
    HtmlMethod { which: Builtin.StringPrototypeBig, tag: "big" }, HtmlMethod { which: Builtin.StringPrototypeBlink, tag: "blink" },
    HtmlMethod { which: Builtin.StringPrototypeBold, tag: "b" }, HtmlMethod { which: Builtin.StringPrototypeFixed, tag: "tt" },
    HtmlMethod { which: Builtin.StringPrototypeFontcolor, tag: "font", attribute: "color" }, HtmlMethod { which: Builtin.StringPrototypeFontsize, tag: "font", attribute: "size" },
    HtmlMethod { which: Builtin.StringPrototypeItalics, tag: "i" }, HtmlMethod { which: Builtin.StringPrototypeLink, tag: "a", attribute: "href" },
    HtmlMethod { which: Builtin.StringPrototypeSmall, tag: "small" }, HtmlMethod { which: Builtin.StringPrototypeStrike, tag: "strike" },
    HtmlMethod { which: Builtin.StringPrototypeSub, tag: "sub" }, HtmlMethod { which: Builtin.StringPrototypeSup, tag: "sup" },
];
fn string_html(job: Job&, b: Builtin, s: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    for m in HTML_METHODS {
        if m.which != b { continue; }
        let a = if m.attribute.len > 0 { primitive_string(job, b, arg(args, 0)) } else { Value.Undefined };
        if heap.fault != Fault.None { return Value.Undefined; }
        var size = 5 + 2 * m.tag.len + str_len(heap, s);
        if m.attribute.len > 0 {
            let len = str_len(heap, a);
            if !charge(heap, len) { return Value.Undefined; }
            size += m.attribute.len + 4 + len;
            for i in len { if unit(heap, a, i) == '"' { size += 5; } }
        }
        if !string_result_room(job, size) { return Value.Undefined; }
        var out: u16[>..] = []; out.push('<'); for c in m.tag { out.push(c); }
        if m.attribute.len > 0 {
            out.push(' '); for c in m.attribute { out.push(c); } out.push('='); out.push('"');
            for i in str_len(heap, a) { let u = unit(heap, a, i); if u == '"' { for c in "&quot;" { out.push(c); } } else { out.push(u); } }
            out.push('"');
        }
        out.push('>'); append_utf16(heap, s, out); out.push('<'); out.push('/'); for c in m.tag { out.push(c); } out.push('>');
        return import_text(heap, out);
    }
    heap.fault = Fault.InvalidState; return Value.Undefined;
}
fn string_transform_builtin(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let s = string_receiver(job, b, receiver);
    if job.heap.fault != Fault.None { return Value.Undefined; }
    match b {
        StringPrototypeSubstring, StringPrototypeSubstr, StringPrototypeSlice => { return string_extract(job, b, s, args); },
        StringPrototypeConcat => { return string_concat(job, b, s, args); },
        StringPrototypeRepeat => { return string_repeat(job, b, s, arg(args, 0)); },
        StringPrototypeTrim, StringPrototypeTrimStart, StringPrototypeTrimEnd => { return string_trim(job, b, s); },
        _ => { return string_html(job, b, s, args); },
    }
}

// Padding converts the filler only after the target length requires it.
// Split has no plan: GetMethod(@@split) precedes ToString of the receiver,
// then ToUint32(limit), then ToString(separator), even when the limit is 0.
// All state is held by ordinary frame Values/operands, including the fresh
// result array. Reclamation and saved continuations trace it normally.
let string_pc::PadFill = 14;
let string_pc::SplitMethod = 15;
let string_pc::SplitDelegate = 16;
let string_pc::SplitText = 17;
let string_pc::SplitLimit = 18;
let string_pc::SplitSeparator = 19;
let string_pc::SplitScan = 20;
fn start_string_transform(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let o = form_value(heap, receiver); let first = form_value(heap, arg(args, 0)); let second = form_value(heap, arg(args, 1));
    if o.is_null() || o.is_undefined() { type_error(job, "String method requires a receiver"); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    if string_pad(b) {
        let s = string_receiver(job, b, o);
        if heap.fault != Fault.None { return; }
        let len = to_length(primitive_number(job, b, first));
        if heap.fault != Fault.None { return; }
        if len <= str_len(heap, s) { job.operands.resize(base); deliver(job, s); return; }
        job.operands.resize(base);
        job.frames.push(Frame { function: -1, pc: string_pc::PadFill, local_base: job.locals.len, operand_base: base, receiver: s, callee: number(len), native: Native.StringMethod { which: b } });
        push_operand(job, if second.is_undefined() { key_named(heap, " ") } else { second });
        return;
    }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: if first.is_undefined() || first.is_null() { string_pc::SplitText } else { string_pc::SplitMethod }, local_base: job.locals.len, operand_base: base, receiver: o, input: first, callee: second, native: Native.StringMethod { which: b } });
    if first.is_undefined() || first.is_null() { push_operand(job, o); }
    else { get_any(job, b, first, Value.Symbol { s: well_known::Split }); }
}
fn string_pad_result(job: Job&, b: Builtin, s: Value, filler: Value, target: f64) -> Value {
    let heap .= job.heap; let len = str_len(heap, s); let fill = str_len(heap, filler);
    if fill == 0 { return s; }
    if !string_result_room(job, target) { return Value.Undefined; }
    let extra = (target as i64) - len;
    var out: u16[>..] = [];
    if b == Builtin.StringPrototypePadEnd { append_utf16(heap, s, out); }
    for i in extra { out.push(unit(heap, filler, i % fill)); }
    if b == Builtin.StringPrototypePadStart { append_utf16(heap, s, out); }
    return import_text(heap, out);
}
fn string_transform_step(job: Job&, f: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match f.pc {
        string_pc::PadFill => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            let result = string_pad_result(job, b, f.receiver, s, f.callee.num());
            if heap.fault == Fault.None { finish_native(job, f, result); }
        },
        string_pc::SplitMethod => {
            let m = form_value(heap, job.operands.pop());
            if m.is_undefined() || m.is_null() { job.frames[at].pc = string_pc::SplitText; push_operand(job, f.receiver); return; }
            if !is_callable(heap, m) { type_error(job, "Symbol.split is not a function"); return; }
            job.frames[at].pc = string_pc::SplitDelegate;
            call_accessor(job, m, f.input, [f.receiver, f.callee]);
        },
        string_pc::SplitDelegate => { finish_native(job, f, job.operands.pop()); },
        string_pc::SplitText => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            job.frames[at].receiver = s; job.frames[at].pc = string_pc::SplitLimit; push_operand(job, f.callee);
        },
        string_pc::SplitLimit => {
            let ready, n = string_number(job, b);
            if !ready || heap.fault != Fault.None { return; }
            job.frames[at].callee = number(if f.callee.is_undefined() { 4294967295 } else { to_uint32(n) });
            job.frames[at].pc = string_pc::SplitSeparator; push_operand(job, f.input);
        },
        string_pc::SplitSeparator => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            let array = new_array(heap, []);
            if heap.fault != Fault.None { return; }
            if f.callee.num() == 0 { finish_native(job, f, array); return; }
            if f.input.is_undefined() {
                if set_element(heap, array, 0, f.receiver) { finish_native(job, f, array); }
                return;
            }
            job.frames[at].input = s; job.frames[at].output = array;
            job.frames[at].pc = string_pc::SplitScan;
            push_operand(job, number(0)); // the end of the last separator
        },
        string_pc::SplitScan => { string_split_scan(job, f); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
fn string_split_piece(job: Job&, f: Frame, start: i64, end: i64) -> bool {
    let heap .= job.heap;
    if !charge(heap, end - start + 1) { return false; }
    let s = substring(heap, f.receiver, start, end);
    return heap.fault == Fault.None && set_element(heap, f.output, node(heap, f.output.id()).length, s);
}
fn string_split_scan(job: Job&, f: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let len = str_len(heap, f.receiver); let size = str_len(heap, f.input);
    if size == 0 {
        if f.index == len { finish_native(job, f, f.output); return; }
        if !string_split_piece(job, f, f.index, f.index + 1) { return; }
        job.frames[at].index += 1;
        if node(heap, f.output.id()).length >= f.callee.num() { finish_native(job, f, f.output); }
        return;
    }
    var p = f.index; var matched = f.argument_count;
    for visited in string_quantum() {
        if p > len - size {
            if string_split_piece(job, f, job.operands[f.operand_base].num() as i64, len) { finish_native(job, f, f.output); }
            return;
        }
        if !charge(heap) { return; }
        if unit(heap, f.receiver, p + matched) == unit(heap, f.input, matched) {
            matched += 1;
            if matched == size {
                if !string_split_piece(job, f, job.operands[f.operand_base].num() as i64, p) { return; }
                p += size; job.operands[f.operand_base] = number(p);
                job.frames[at].index = p; job.frames[at].argument_count = 0;
                if node(heap, f.output.id()).length >= f.callee.num() { finish_native(job, f, f.output); }
                return;
            }
        } else { p += 1; matched = 0; }
    }
    job.frames[at].index = p; job.frames[at].argument_count = matched;
}
