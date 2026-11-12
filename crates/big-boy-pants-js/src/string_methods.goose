import std;
import math;
import .source_vm;
import .realm;
import .coerce;
import .array_callbacks;
import .array_methods;
import .regexp_object;
import .regexp_replace;

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
inline fn string_native(b: Builtin) -> bool { return string_points(b) || string_search(b) || string_well_formed(b) || b == Builtin.StringRaw; }
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
