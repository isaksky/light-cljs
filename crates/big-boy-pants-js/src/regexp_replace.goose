import std;
import .source_vm;
import .realm;
import .array_methods;
import .array_callbacks;
import .function_calls;
import .regexp_match;
import .regexp_object;

namespace source;

// String.prototype.replace and replaceAll and RegExp.prototype[@@replace]
// (light-cljs-934, the third child of light-cljs-770; decisions 2, 4 and 5
// of docs/javascript-coverage.md; ECMAScript 22.1.3.19, 22.1.3.20,
// 22.1.3.19.1 GetSubstitution and 22.2.6.11; the Pants engine's
// vm/native/string_regexp.rs and string_replace.rs, QuickJS's
// js_string_replace, js_regexp_Symbol_replace and GetSubstitution).
//
// Each runs as a RegExp native frame (Native.RegExp, its built-in the
// payload) whose phase (pc) names the one value it awaits on its operand
// top: a property read (a getter runs above the frame), a conversion (an
// object's ToPrimitive runs in a Convert frame above it, and the phase runs
// again on the primitive), a call (the replacer, a user exec, a delegated
// @@replace method) or a setter's return. A value that arrives without
// running JavaScript is taken at once: a step chains phases until one runs
// JavaScript, at most replace_chain() of them, and it ends after a search
// and after each replaced occurrence, so every budget cut, limit and
// suspension falls between two reads, conversions, calls or occurrences.
// GetSubstitution and the string scans are plain functions (decision 5);
// the result is a new job string in its canonical encoding (decision 2).
//
// replace(searchValue, replaceValue) and replaceAll: RequireObjectCoercible
// of the receiver; a searchValue neither undefined nor null has, for
// replaceAll, IsRegExp (regexp_object.goose start_is_regexp), and when it
// is one Get(searchValue, "flags"), RequireObjectCoercible and ToString,
// a TypeError without "g"; then GetMethod(searchValue, @@replace) (GetV:
// a primitive's prototype is read), and a method found is called with the
// receiver and replaceValue, its result the answer (the realm's own
// @@replace replaces this frame with its own, at the same operand base).
// Otherwise ToString of the receiver, then of searchValue, then of
// replaceValue unless it is callable; then the occurrences of the search
// string, the first or (replaceAll) every one, advancing by max(1, its
// length), each replaced by GetSubstitution of the template (no captures,
// no groups) or by ToString of the replacer's result for (matched,
// position, string). The receiver's ToString follows the @@replace lookup,
// so it is no light-cljs-900 plan (coerce.goose).
//   receiver   O, then (from Search on) its string S
//   input      searchValue, then (from Replacement on) the search string
//   callee     replaceValue, then (from Scan on) the template or the replacer
//   output     the allowance spent, a number (replace_spend)
//   index      Scan: where the next search starts; Callback: the occurrence
//   argument_count  the end of the last replaced occurrence
//   operands   from Scan on the output's pieces (S's slices and
//              replacements, none empty), then the awaited value
let replace_pc::Match = 0;        // replaceAll: Get(searchValue, @@match)
let replace_pc::Flags = 1;        // replaceAll: Get(searchValue, "flags"), then its ToString
let replace_pc::Method = 2;       // GetMethod(searchValue, @@replace)
let replace_pc::Delegate = 3;     // that method's result, the answer
let replace_pc::Text = 4;         // ToString(O)
let replace_pc::Search = 5;       // ToString(searchValue)
let replace_pc::Replacement = 6;  // ToString(replaceValue)
let replace_pc::Scan = 7;         // no value: the next occurrence
let replace_pc::Callback = 8;     // the replacer's result, then its ToString
//
// RegExp.prototype[@@replace](string, replaceValue) on any object rx:
// ToString(string); IsCallable(replaceValue), or its ToString; Get(rx,
// "flags") and ToString; with "g", Set(rx, "lastIndex", 0, true). Then
// RegExpExec(rx, S) until null, or once without "g": Get(rx, "exec"), a
// callable one other than the realm's exec called with S (an object or
// null required), otherwise rx's [[RegExpMatcher]] (a TypeError without
// one) runs RegExpBuiltinExec (regexp_object.goose builtin_exec) after
// ToLength(Get(rx, "lastIndex")); with "g" each result's ToString(Get(r,
// "0")), and an empty one advances lastIndex: ToLength(Get(rx,
// "lastIndex")), AdvanceStringIndex (code points with "u" or "v") and Set.
// Every result is collected before any is used, so an exec runs before
// any replacer. Then for each result: ToLength(Get(r, "length")),
// ToString(Get(r, "0")), ToIntegerOrInfinity(Get(r, "index")) clamped to
// S, Get(r, n) and ToString of each capture but undefined, Get(r,
// "groups"); the replacer called with (matched, captures..., position, S
// [, groups]) and its result converted, or with a template ToObject of
// defined groups, Get(groups, name) and ToString of each `$<name>` (in the
// template's order, before the rest of GetSubstitution, which is plain);
// a result whose position is before the end of the last one used is
// dropped.
//   receiver   rx                    input   the argument, then (from Replacement on) S
//   callee     replaceValue, then (from Flags on) the template or the replacer
//   output     the allowance spent, a number (replace_spend)
//   index      collecting: symbol_flag:: bits; rendering: the result i
//   argument_count  R, the results collected
//   operands   symbol_registers() numbers (symbol_reg::), then R slots
//              (each a result, and from Render on, below i, the output's
//              piece for that result), then while rendering result i its
//              scratch: matched, the captures converted so far, groups and
//              the `$<name>` values read so far; then the awaited value
let symbol_pc::Input = 0;         // ToString(string)
let symbol_pc::Replacement = 1;   // ToString(replaceValue)
let symbol_pc::Flags = 2;         // Get(rx, "flags"), then its ToString
let symbol_pc::Reset = 3;         // a lastIndex setter's return (dropped), then exec
let symbol_pc::Exec = 4;          // Get(rx, "exec")
let symbol_pc::ExecResult = 5;    // the exec's result (a search's, or a user exec's)
let symbol_pc::LastIndex = 6;     // Get(rx, "lastIndex"), then ToLength, then the search
let symbol_pc::MatchText = 7;     // "g": Get(result, "0"), then ToString
let symbol_pc::Advance = 8;       // an empty match: Get(rx, "lastIndex"), then ToLength
let symbol_pc::AdvanceSet = 9;    // the lastIndex setter's return (dropped)
let symbol_pc::Render = 10;       // no value: result i, or the answer
let symbol_pc::Length = 11;       // Get(result, "length"), then ToLength
let symbol_pc::Matched = 12;      // Get(result, "0"), then ToString
let symbol_pc::Position = 13;     // Get(result, "index"), then ToIntegerOrInfinity
let symbol_pc::Capture = 14;      // Get(result, n), then ToString unless undefined
let symbol_pc::Groups = 15;       // Get(result, "groups")
let symbol_pc::Callback = 16;     // the replacer's result, then ToString
let symbol_pc::Named = 17;        // Get(groups, name), then ToString unless undefined
// The flags a collection needs (the frame's index while collecting).
let symbol_flag::Global = 1;
let symbol_flag::Unicode = 2;
// The frame's registers, numbers at its operand base.
let symbol_reg::Next = 0;         // rendering: the end of the last result used
let symbol_reg::Position = 1;     // result i's position
let symbol_reg::Captures = 2;     // result i's capture count
let symbol_reg::Template = 3;     // where the next `$<name>` read starts in the template
let symbol_reg::Close = 4;        // the template's last ">", -1 for none
let symbol_reg::Charged = 5;      // the template work spent before the substitution
inline fn symbol_registers() -> i64 { return 6; }
//
// Work. Each operation has the Pants engine's replacement allowance,
// max_replace_work() units (MAX_REPLACE_WORK), spent as it does: two units
// an occurrence of a search string and the matched string as a replacer's
// argument; one an exec and one a result collected, and a built-in
// search's result its groups' units, their count and one; a result's
// capture count, its matched string and captures as converted; a template
// unit, a `$<`'s scan for its ">", and every unit a replacement appends;
// the output's units of the string since the last replacement (with a
// RegExp, as the Pants engine's render does, the replacement's again).
// A template's tokens before a `$<name>` and the name's value are spent
// before the next name is read, so the allowance runs out where the Pants
// engine's left-to-right expansion does. Past it, the catchable RangeError
// "string replacement work limit exceeded" (QuickJS has no such limit: the
// oracles disagree, and Goose follows the Pants engine), unless the job's
// remaining work runs out first (WorkLimit). Every unit it spends but a
// built-in search's groups (charged by the search) is charged to the job's
// work as well, as are the scans for a search string, which one step runs
// for at most scan_quantum() units and the next continues. An operation
// that replaces nothing answers its string without spending its length.
// The Pants engine's 65,536-unit argument limit, which guards its rope
// flattening, has no counterpart: Goose strings are flat and bounded by
// the unit limit.

// Whether built-in b is one of these (regexp_object.goose regexp_native).
inline fn replace_native(b: Builtin) -> bool { return b == Builtin.StringPrototypeReplace || b == Builtin.StringPrototypeReplaceAll || b == Builtin.RegExpPrototypeReplace; }
inline fn max_replace_work() -> i64 { return 1000000; }
inline fn replace_chain() -> i64 { return 64; }
inline fn scan_quantum() -> i64 { return 65536; }

// Start built-in b (replace_native) with `receiver` and `args`, its operand
// base `base` (native_start's contract).
fn start_replace(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let o = form_value(heap, receiver); let first = form_value(heap, arg(args, 0)); let second = form_value(heap, arg(args, 1));
    if b == Builtin.RegExpPrototypeReplace { start_symbol_replace(job, o, first, second, base); return; }
    if o.tag() <= value_tag::Null { type_error(job, if b == Builtin.StringPrototypeReplace { "String.prototype.replace called on null or undefined" } else { "String.prototype.replaceAll called on null or undefined" }); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: replace_pc::Method, local_base: job.locals.len, operand_base: base, callee: second, receiver: o, input: first, output: number(0), native: Native.RegExp { which: b } });
    let at = job.frames.len - 1;
    if first.tag() <= value_tag::Null { replace_ask_text(job, at); }
    else if b == Builtin.StringPrototypeReplaceAll && first.is_object() { job.frames[at].pc = replace_pc::Match; start_is_regexp(job, b, first); }
    else { replace_ask_method(job, at, b, first); }
    replace_run(job, at, b);
}
// One step of the frame at the top (regexp_object.goose regexp_step).
fn replace_step(job: Job&, b: Builtin) { replace_run(job, job.frames.len - 1, b); }
// Run the frame at `at` while it stays at the top and its values arrive
// without JavaScript, at most replace_chain() phases, until one ends the step.
fn replace_run(job: Job&, at: i64, b: Builtin) {
    var n: i64 = 0;
    while job.heap.fault == Fault.None && job.frames.len - 1 == at && n < replace_chain() {
        let done = if b == Builtin.RegExpPrototypeReplace { symbol_replace_phase(job, at) } else { string_replace_phase(job, at, b) };
        if done { return; }
        n += 1;
    }
}

// Spend n units of the allowance of the frame at `at` (the job's work too,
// unless `job_work` is false): false with the fault. Whichever runs out
// first decides it: past the allowance its rest is charged and the
// RangeError raised; past the job's remaining work, WorkLimit.
fn replace_spend(job: Job&, at: i64, n: i64, job_work: bool = true) -> bool {
    let heap .= job.heap;
    if heap.fault != Fault.None { return false; }
    if n < 0 || !job.frames[at].output.is_number() { heap.fault = Fault.InvalidState; return false; }
    let spent = job.frames[at].output.num() as i64;
    if spent < 0 || spent > max_replace_work() { heap.fault = Fault.InvalidState; return false; }
    let left = max_replace_work() - spent;
    if n > left && (!job_work || left <= heap.limits.work - heap.work) {
        if job_work && !charge(heap, left) { return false; }
        return replace_limit(job, at);
    }
    if job_work && !charge(heap, n) { return false; }
    job.frames[at].output = number(spent + n);
    return true;
}
// The allowance's RangeError for the frame at `at`, worded as the Pants
// engine words it: a replacement's, or the other symbol methods'
// (light-cljs-937; regexp_symbols.goose).
fn replace_limit(job: Job&, at: i64) -> bool {
    let replacing = match job.frames[at].native { RegExp r => replace_native(r.which), _ => true };
    job.detail.clear(); for c in (if replacing { "string replacement work limit exceeded" } else { "RegExp symbol work limit exceeded" }) { job.detail.push(c); }
    job.heap.fault = Fault.RangeError;
    return false;
}
// The units the frame at `at` may still spend, within the job's work.
fn replace_budget(job: const Job&, at: i64) -> i64 {
    let left = max_replace_work() - (job.frames[at].output.num() as i64);
    return min(left, job.heap.limits.work - job.heap.work);
}
// Get(o, k) of any value o (GetV: a primitive's realm prototype), for the
// frame at the top.
fn get_any(job: Job&, b: Builtin, o: Value, k: Value) {
    let heap .= job.heap;
    if o.is_bigint() { unsupported_step(job, b, "a BigInt search value"); return; }
    let v, getter = read(heap, o, k);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { run_getter(job, b, getter, o); } else { push_operand(job, v); }
}
// ToNumber of the value at the operand top, or false when it is an object,
// whose ToNumber a Convert frame above runs first (array_methods.goose
// length_value's contract).
fn number_value(job: Job&, b: Builtin) -> f64, bool {
    let heap .= job.heap;
    let v = form_value(heap, job.operands.pop());
    if v.is_object() {
        if !convertible(heap, v) { unsupported_step(job, b, "ToNumber of a light-cljs runtime value"); return 0, false; }
        job.operands.push(v); start_convert(job, Hint.Number, true, 1, 1); return 0, false;
    }
    let n = primitive_number(job, b, v);
    return n, heap.fault == Fault.None;
}
// Whether string s has code unit c.
fn has_unit(heap: const Heap&, s: Value, c: i64) -> bool { for i in str_len(heap, s) { if unit(heap, s, i) == c { return true; } } return false; }
// Units [start, end) of string s, appended.
fn append_range(heap: const Heap&, s: Value, start: i64, end: i64, out: u16[>..]&) {
    if start >= end { return; }
    let h = header(heap, s.sid());
    if h.wide { out.append(wide_run(heap, h)[start..end]); } else { for c in narrow_run(heap, h)[start..end] { out.push(c); } }
}
// The strings of `pieces` as one, in its canonical encoding, charging no
// work (each piece's units were spent as it was made): "" for none, a
// single non-empty piece itself.
fn concat_pieces(job: Job&, pieces: const Value[:]) -> Value {
    let heap .= job.heap;
    var total: i64 = 0; var count: i64 = 0; var wide = false; var only = Value.Undefined;
    for p in pieces {
        if !p.is_string() { heap.fault = Fault.InvalidState; return Value.Undefined; }
        let h = header(heap, p.sid());
        if h.len == 0 { continue; }
        total += h.len; count += 1; only = p;
        if h.wide { wide = true; }
    }
    if count == 0 { return key_named(heap, ""); }
    if count == 1 { return only; }
    if !units_fit(heap, total) || heap.strings.len - heap.image.strings >= heap.limits.units { memory(heap); return Value.Undefined; }
    if wide {
        var out: u16[>..] = [];
        for p in pieces { append_utf16(heap, p, out); }
        return import_text(heap, out);
    }
    var bytes: u8[>..] = [];
    for p in pieces { bytes.append(narrow_run(heap, header(heap, p.sid()))); }
    return string_ref(heap, add_latin1(heap, bytes), bytes.len, false);
}

// ---- GetSubstitution (22.1.3.19.1)
// A template's token at a unit: one unit copied (a "$" that starts no
// other token too), "$$", "$&", "$`", "$'", a capture ("$n" or "$nn", the
// two-digit one when that capture exists), or "$<name>" when the result
// has groups and a ">" follows (the name's units [a, b)).
let token::Literal = 0;
let token::Dollar = 1;
let token::Match = 2;
let token::Prefix = 3;
let token::Suffix = 4;
let token::Capture = 5;
let token::Named = 6;
// A token: its kind, the unit after it, its operands, and the units a
// "$<" scanned for its ">" (the Pants engine's charge: through the ">", or
// the rest of the template).
struct Token { kind: i64, next: i64, a: i64 = 0, b: i64 = 0, scanned: i64 = 0 }
// The token at unit i of template t (n units) for a result with `captures`
// captures, `named` when it has groups; `close` is t's last ">" (-1 for none).
fn template_token(heap: const Heap&, t: Value, i: i64, n: i64, captures: i64, named: bool, close: i64) -> Token {
    if unit(heap, t, i) != 36 || i + 1 == n { return Token { kind: token::Literal, next: i + 1 }; }
    let d: i64 = unit(heap, t, i + 1);
    if d == 36 { return Token { kind: token::Dollar, next: i + 2 }; }
    if d == 38 { return Token { kind: token::Match, next: i + 2 }; }
    if d == 96 { return Token { kind: token::Prefix, next: i + 2 }; }
    if d == 39 { return Token { kind: token::Suffix, next: i + 2 }; }
    if d >= 48 && d <= 57 {
        var index = d - 48; var next = i + 2;
        if i + 2 < n {
            let e: i64 = unit(heap, t, i + 2);
            if e >= 48 && e <= 57 && index * 10 + e - 48 >= 1 && index * 10 + e - 48 <= captures { index = index * 10 + e - 48; next = i + 3; }
        }
        if index >= 1 && index <= captures { return Token { kind: token::Capture, next: next, a: index }; }
        return Token { kind: token::Literal, next: i + 1 };
    }
    if d == 60 && named {
        if close < i + 2 { return Token { kind: token::Literal, next: i + 1, scanned: n - i - 2 }; }
        var j = i + 2;
        while unit(heap, t, j) != 62 { j += 1; }
        return Token { kind: token::Named, next: j + 1, a: i + 2, b: j, scanned: j - i - 1 };
    }
    return Token { kind: token::Literal, next: i + 1 };
}
// The last ">" of template t, -1 for none.
fn template_close(heap: const Heap&, t: Value) -> i64 {
    var j = str_len(heap, t) - 1;
    while j >= 0 && unit(heap, t, j) != 62 { j -= 1; }
    return j;
}
// GetSubstitution of template t for `matched` at `position` in s, with
// `captures` (strings, or undefined for ""), and with `named` the values
// of the template's `$<name>` tokens in order (strings, or undefined for
// ""): its units appended to out. Returns the work (a unit a token, a
// `$<`'s scan and the units appended), stopping once it passes `budget`.
fn substitute(heap: const Heap&, t: Value, s: Value, position: i64, matched: Value, captures: const Value[:], named: bool, values: const Value[:], out: u16[>..]&, budget: i64) -> i64 {
    let n = str_len(heap, t); let len = str_len(heap, s);
    let tail = min(position + str_len(heap, matched), len);
    let close = if named { template_close(heap, t) } else { -1 };
    var i: i64 = 0; var k: i64 = 0; var work: i64 = 0;
    while i < n && work <= budget {
        let token = template_token(heap, t, i, n, captures.len, named, close);
        let before = out.len;
        match token.kind {
            token::Dollar => { out.push(36); },
            token::Match => { append_range(heap, matched, 0, str_len(heap, matched), out); },
            token::Prefix => { append_range(heap, s, 0, position, out); },
            token::Suffix => { append_range(heap, s, tail, len, out); },
            token::Capture => { let c = captures[token.a - 1]; if c.is_string() { append_utf16(heap, c, out); } },
            token::Named => { if k < values.len && values[k].is_string() { append_utf16(heap, values[k], out); } k += 1; },
            _ => { out.push(unit(heap, t, i)); },
        }
        work += 1 + token.scanned + out.len - before;
        i = token.next;
    }
    return work;
}
// The work substitute spends on a token other than a `$<name>`: one, its
// scan, and the units it appends.
fn token_work(heap: const Heap&, token: Token, s: Value, position: i64, matched: Value, captures: const Value[:]) -> i64 {
    let len = str_len(heap, s);
    let units: i64 = match token.kind {
        token::Match => str_len(heap, matched),
        token::Prefix => position,
        token::Suffix => len - min(position + str_len(heap, matched), len),
        token::Capture => str_len(heap, captures[token.a - 1]),
        token::Named => 0,
        _ => 1,
    };
    return 1 + token.scanned + units;
}
// GetSubstitution as a new string for the frame at `at`, its work spent
// but the `charged` units the `$<name>` reads spent already.
fn replacement_text(job: Job&, at: i64, t: Value, s: Value, position: i64, matched: Value, captures: const Value[:], named: bool, values: const Value[:], charged: i64) -> Value {
    let heap .= job.heap;
    if !t.is_string() || !s.is_string() || !matched.is_string() || position < 0 || position > str_len(heap, s) || charged < 0 { heap.fault = Fault.InvalidState; return Value.Undefined; }
    // A template without "$" is its own replacement: its units spent as
    // substitute would (a token and a unit each), and no copy made.
    if !has_unit(heap, t, 36) {
        if !replace_spend(job, at, 2 * str_len(heap, t)) { return Value.Undefined; }
        return t;
    }
    var out: u16[>..] = [];
    let work = substitute(heap, t, s, position, matched, captures, named, values, out, replace_budget(job, at) + charged);
    if work < charged { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if !replace_spend(job, at, work - charged) { return Value.Undefined; }
    return import_text(heap, out);
}

// ---- String.prototype.replace and replaceAll
// The first occurrence of `needle` in `hay` at or after `from`
// (StringIndexOf), trying candidates while the work stays within
// `quantum`: its position, -1 for none, or -2 when the quantum ran out
// before the candidate it returns next; and the work, a unit a candidate
// tried and a unit a further unit compared.
fn find_units<A, B>(hay: const A[:], needle: const B[:], from: i64, quantum: i64) -> i64, i64, i64 {
    let n = hay.len; let m = needle.len;
    if from < 0 || from > n { return -1, from, 1; }
    if m == 0 { return from, from, 1; }
    let first: i64 = needle[0];
    var work: i64 = 1; var i = from;
    while m <= n - i {
        if work >= quantum { return -2, i, work; }
        work += 1;
        let c: i64 = hay[i];
        if c == first {
            var j: i64 = 1;
            while j < m { let x: i64 = hay[i + j]; let y: i64 = needle[j]; if x != y { break; } j += 1; }
            work += j - 1;
            if j == m { return i, i, work; }
        }
        i += 1;
    }
    return -1, i, work;
}
// StringIndexOf(s, t, from) for at most scan_quantum() units of work (or
// the job's remaining work, when less, so running out of it is WorkLimit),
// charged to the job: the position, -1 for none, or -2 and where the
// search continues.
fn index_of(heap: Heap&, s: Value, t: Value, from: i64) -> i64, i64 {
    let hs = header(heap, s.sid()); let ht = header(heap, t.sid());
    let quantum = min(scan_quantum(), heap.limits.work - heap.work + 1);
    var p: i64 = -1; var next: i64 = from; var work: i64 = 0;
    if hs.wide {
        if ht.wide { p, next, work = find_units(wide_run(heap, hs), wide_run(heap, ht), from, quantum); } else { p, next, work = find_units(wide_run(heap, hs), narrow_run(heap, ht), from, quantum); }
    } else {
        if ht.wide { p, next, work = find_units(narrow_run(heap, hs), wide_run(heap, ht), from, quantum); } else { p, next, work = find_units(narrow_run(heap, hs), narrow_run(heap, ht), from, quantum); }
    }
    if !charge(heap, work) { return -1, from; }
    return p, next;
}
fn replace_ask_method(job: Job&, at: i64, b: Builtin, search: Value) {
    job.frames[at].pc = replace_pc::Method;
    get_any(job, b, search, Value.Symbol { s: well_known::Replace });
}
fn replace_ask_text(job: Job&, at: i64) {
    job.frames[at].pc = replace_pc::Text;
    push_operand(job, job.frames[at].receiver);
}
// One phase of a replace or replaceAll frame at `at`: true when the step ends.
fn string_replace_phase(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap; let f = job.frames[at];
    if f.pc == replace_pc::Scan { return replace_scan(job, at, b); }
    if job.operands.len <= f.operand_base || !f.output.is_number() { heap.fault = Fault.InvalidState; return true; }
    match f.pc {
        replace_pc::Match => {
            let m = job.operands.pop();
            if is_regexp_answer(heap, f.input, m) { job.frames[at].pc = replace_pc::Flags; get_named(job, b, f.input, "flags"); } else { replace_ask_method(job, at, b, f.input); }
        },
        replace_pc::Flags => {
            if form_value(heap, job.operands[job.operands.len - 1]).tag() <= value_tag::Null { type_error(job, "RegExp flags are undefined or null"); return true; }
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            if !has_unit(heap, s, 103) { type_error(job, "replaceAll requires a global RegExp"); return true; }
            replace_ask_method(job, at, b, f.input);
        },
        replace_pc::Method => {
            let m = form_value(heap, job.operands.pop());
            if m.tag() <= value_tag::Null { replace_ask_text(job, at); return false; }
            if !is_callable(heap, m) { type_error(job, "Symbol.replace is not a function"); return true; }
            // The realm's @@replace answers in this frame's place: its
            // frame replaces this one and runs from the next step.
            if f.input.is_object() && m.is_object() && node(heap, m.id()).kind == node_kind::Builtin && builtin_of(heap, m.id()) == Builtin.RegExpPrototypeReplace {
                job.frames.pop(); job.operands.resize(f.operand_base);
                push_symbol_replace(job, f.input, f.receiver, f.callee, f.operand_base);
                return true;
            }
            job.frames[at].pc = replace_pc::Delegate;
            call_accessor(job, m, f.input, [f.receiver, f.callee]);
        },
        replace_pc::Delegate => { finish_native(job, f, job.operands.pop()); return true; },
        replace_pc::Text => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            job.frames[at].receiver = s; job.frames[at].pc = replace_pc::Search;
            push_operand(job, f.input);
        },
        replace_pc::Search => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            job.frames[at].input = s;
            if is_callable(heap, f.callee) { replace_begin_scan(job, at); } else { job.frames[at].pc = replace_pc::Replacement; push_operand(job, f.callee); }
        },
        replace_pc::Replacement => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            job.frames[at].callee = s; replace_begin_scan(job, at);
        },
        replace_pc::Callback => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None || !replace_spend(job, at, str_len(heap, s)) { return true; }
            replace_commit(job, at, b, f.index, s);
            return true;
        },
        _ => { heap.fault = Fault.InvalidState; return true; },
    }
    return false;
}
fn replace_begin_scan(job: Job&, at: i64) { job.frames[at].pc = replace_pc::Scan; job.frames[at].index = 0; job.frames[at].argument_count = 0; }
// The next occurrence, from the frame's index: replaced by its template at
// once, or its replacer called; none left, the answer.
fn replace_scan(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap; let f = job.frames[at];
    let s = f.receiver; let t = f.input;
    // The scan resumes at index, at most S's length plus one (past every
    // occurrence).
    if !s.is_string() || !t.is_string() || !f.output.is_number() || f.index < 0 || f.index > str_len(heap, s) + 1 || f.argument_count < 0 || f.argument_count > str_len(heap, s) || job.operands.len < f.operand_base { heap.fault = Fault.InvalidState; return true; }
    let p, next = index_of(heap, s, t, f.index);
    if heap.fault != Fault.None { return true; }
    // The quantum ran out: the next step continues from the first
    // candidate not tried.
    if p == -2 { job.frames[at].index = next; return true; }
    if p < 0 { replace_finish(job, at); return true; }
    if !replace_spend(job, at, 2) { return true; }
    if is_callable(heap, f.callee) {
        if !replace_spend(job, at, str_len(heap, t)) { return true; }
        job.frames[at].index = p; job.frames[at].pc = replace_pc::Callback;
        call_accessor(job, f.callee, Value.Undefined, [t, number(p), s]);
        return true;
    }
    let r = replacement_text(job, at, f.callee, s, p, t, [], false, [], 0);
    if heap.fault != Fault.None { return true; }
    replace_commit(job, at, b, p, r);
    return true;
}
// Replace the occurrence at p by string r: S's units since the last one,
// then r, become pieces. r's units were spent as it was made (a template)
// or converted (a replacer's result), so only S's are spent here, as the
// Pants engine's string replacement spends them.
fn replace_commit(job: Job&, at: i64, b: Builtin, p: i64, r: Value) {
    let heap .= job.heap; let f = job.frames[at];
    let s = f.receiver; let len = str_len(heap, s); let m = str_len(heap, f.input);
    let next = f.argument_count;
    if !s.is_string() || !r.is_string() || p < next || p + m > len { heap.fault = Fault.InvalidState; return; }
    if !replace_spend(job, at, p - next) { return; }
    if p > next { let piece = substring(heap, s, next, p); if heap.fault != Fault.None || !push_operand(job, piece) { return; } }
    if str_len(heap, r) > 0 && !push_operand(job, r) { return; }
    job.frames[at].argument_count = p + m;
    job.frames[at].index = if b == Builtin.StringPrototypeReplaceAll { p + max(1, m) } else { len + 1 };
    job.frames[at].pc = replace_pc::Scan;
}
// No occurrence left: the pieces and S's rest, or S itself when nothing
// was replaced.
fn replace_finish(job: Job&, at: i64) {
    let heap .= job.heap; let f = job.frames[at];
    let s = f.receiver; let len = str_len(heap, s); let next = f.argument_count;
    if next == 0 && job.operands.len == f.operand_base { finish_native(job, f, s); return; }
    if !replace_spend(job, at, len - next) { return; }
    if len > next { let piece = substring(heap, s, next, len); if heap.fault != Fault.None || !push_operand(job, piece) { return; } }
    let r = concat_pieces(job, job.operands[f.operand_base..]);
    if heap.fault == Fault.None { finish_native(job, f, r); }
}

// ---- RegExp.prototype[@@replace]
// Start it on rx with the argument `string` and replaceValue, operand base
// `base`, and run it as far as it goes without JavaScript.
fn start_symbol_replace(job: Job&, rx: Value, string: Value, replacement: Value, base: i64) {
    let at = push_symbol_replace(job, rx, string, replacement, base);
    if at >= 0 { replace_run(job, at, Builtin.RegExpPrototypeReplace); }
}
// Its frame, awaiting the argument's ToString (the argument on its operand
// top): its row, or -1 with the fault.
fn push_symbol_replace(job: Job&, rx: Value, string: Value, replacement: Value, base: i64) -> i64 {
    let heap .= job.heap;
    if !rx.is_object() { type_error(job, "RegExp.prototype[Symbol.replace] called on a non-object"); return -1; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return -1; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: symbol_pc::Input, local_base: job.locals.len, operand_base: base, callee: replacement, receiver: rx, input: string, output: number(0), native: Native.RegExp { which: Builtin.RegExpPrototypeReplace } });
    for r in symbol_registers() { if !push_operand(job, number(if r == symbol_reg::Close { -1 } else { 0 })) { return -1; } }
    if !push_operand(job, string) { return -1; }
    return job.frames.len - 1;
}
// Register r of the frame at `at`, -1 (with InvalidState) when it holds no number.
fn register(job: Job&, at: i64, r: i64) -> i64 {
    let v = job.operands[job.frames[at].operand_base + r];
    if !v.is_number() { job.heap.fault = Fault.InvalidState; return -1; }
    return v.num() as i64;
}
inline fn set_register(job: Job&, at: i64, r: i64, n: i64) { job.operands[job.frames[at].operand_base + r] = number(n); }
// The first result slot of the frame at `at` and its scratch's start.
inline fn result_base(job: const Job&, at: i64) -> i64 { return job.frames[at].operand_base + symbol_registers(); }
inline fn scratch_base(job: const Job&, at: i64) -> i64 { return result_base(job, at) + job.frames[at].argument_count; }
// Get(rx, "lastIndex"): a RegExp in the realm template's layout reads its
// first inline slot (regexp_object.goose template_layout).
fn ask_last_index(job: Job&, b: Builtin, rx: Value) {
    let heap .= job.heap;
    if template_layout(heap, rx) { push_operand(job, node(heap, rx.id()).slots[0]); return; }
    get_key(job, b, rx, last_index_key(heap));
}
// Set(rx, "lastIndex", n, true): true when its setter now runs above the
// frame; false when written, or with the fault.
fn put_last_index(job: Job&, b: Builtin, rx: Value, n: f64) -> bool { return put_last_index_value(job, b, rx, number(n)); }
// Set(rx, "lastIndex", v, true) of any value v (@@search's restore,
// light-cljs-937), as put_last_index.
fn put_last_index_value(job: Job&, b: Builtin, rx: Value, v: Value) -> bool {
    let heap .= job.heap;
    let k = last_index_key(heap);
    if template_layout(heap, rx) && writable(job, rx.id()) { node_mut(heap, rx.id()).slots[0] = v; record(job, rx, k, v); return false; }
    return set_or_throw(job, b, rx, k, v);
}
// AdvanceStringIndex(S, i, unicode).
fn advance_string_index(heap: const Heap&, s: Value, i: f64, unicode: bool) -> f64 {
    let h = header(heap, s.sid());
    let len: i64 = h.len;
    if !unicode || !h.wide || i + 1 >= len { return i + 1; }
    return advance_index(wide_run(heap, h), i as i64, true);
}
// RegExpExec's first read: Get(rx, "exec"), after spending the exec's unit.
fn symbol_ask_exec(job: Job&, at: i64) {
    if !replace_spend(job, at, 1) { return; }
    job.frames[at].pc = symbol_pc::Exec;
    get_named(job, Builtin.RegExpPrototypeReplace, job.frames[at].receiver, "exec");
}
fn symbol_begin_render(job: Job&, at: i64) {
    job.frames[at].pc = symbol_pc::Render; job.frames[at].index = 0;
    set_register(job, at, symbol_reg::Next, 0);
}
// One phase of the @@replace frame at `at`: true when the step ends.
fn symbol_replace_phase(job: Job&, at: i64) -> bool {
    let heap .= job.heap; let b = Builtin.RegExpPrototypeReplace; let f = job.frames[at];
    if f.argument_count < 0 || job.operands.len < scratch_base(job, at) || !f.output.is_number() || !f.receiver.is_object() { heap.fault = Fault.InvalidState; return true; }
    if f.pc == symbol_pc::Render { return symbol_render(job, at); }
    if job.operands.len <= scratch_base(job, at) { heap.fault = Fault.InvalidState; return true; }
    if f.pc >= symbol_pc::Length { return symbol_render_phase(job, at); }
    if f.pc > symbol_pc::Replacement && !f.input.is_string() { heap.fault = Fault.InvalidState; return true; }
    match f.pc {
        symbol_pc::Input => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            job.frames[at].input = s;
            if is_callable(heap, f.callee) { job.frames[at].pc = symbol_pc::Flags; get_named(job, b, f.receiver, "flags"); }
            else { job.frames[at].pc = symbol_pc::Replacement; push_operand(job, f.callee); }
        },
        symbol_pc::Replacement => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            job.frames[at].callee = s; job.frames[at].pc = symbol_pc::Flags;
            get_named(job, b, f.receiver, "flags");
        },
        symbol_pc::Flags => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            let bits = (if has_unit(heap, s, 103) { symbol_flag::Global } else { 0 }) | (if has_unit(heap, s, 117) || has_unit(heap, s, 118) { symbol_flag::Unicode } else { 0 });
            job.frames[at].index = bits;
            if bits & symbol_flag::Global == 0 { symbol_ask_exec(job, at); return false; }
            job.frames[at].pc = symbol_pc::Reset;
            let zero: f64 = 0;
            if !put_last_index(job, b, f.receiver, zero) && heap.fault == Fault.None { symbol_ask_exec(job, at); }
        },
        symbol_pc::Reset, symbol_pc::AdvanceSet => { job.operands.pop(); symbol_ask_exec(job, at); },
        symbol_pc::Exec => {
            let e = form_value(heap, job.operands.pop());
            if is_callable(heap, e) && !builtin_exec_function(heap, e) {
                job.frames[at].pc = symbol_pc::ExecResult;
                call_accessor(job, e, f.receiver, [f.input]);
                return false;
            }
            if !is_regexp_object(heap, f.receiver) { type_error(job, "RegExp exec method called on an object that is not a RegExp"); return true; }
            job.frames[at].pc = symbol_pc::LastIndex;
            ask_last_index(job, b, f.receiver);
        },
        symbol_pc::LastIndex => {
            let last, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            if !is_regexp_object(heap, f.receiver) { heap.fault = Fault.InvalidState; return true; }
            let r = builtin_exec(job, b, f.receiver, f.input, last);
            if heap.fault != Fault.None { return true; }
            // A match's groups, their count and one, from the allowance
            // (the Pants engine's exec_with_capture_budget); the search
            // charged their units to the job's work.
            if r.is_object() {
                let start: i64 = node(heap, r.id()).start; let count: i64 = node(heap, r.id()).count;
                var units: i64 = count + 1;
                for v in run(heap, start, count) { units += str_len(heap, v); }
                if !replace_spend(job, at, units, false) { return true; }
            }
            job.frames[at].pc = symbol_pc::ExecResult;
            push_operand(job, r);
            return true;
        },
        symbol_pc::ExecResult => {
            let r = form_value(heap, job.operands.pop());
            if !r.is_null() && !r.is_object() { type_error(job, "RegExp exec method returned something other than an object or null"); return true; }
            if r.is_null() { symbol_begin_render(job, at); return false; }
            if !replace_spend(job, at, 1) || !push_operand(job, r) { return true; }
            job.frames[at].argument_count = f.argument_count + 1;
            if f.index & symbol_flag::Global == 0 { symbol_begin_render(job, at); return false; }
            let k = index_key(heap, 0);
            if heap.fault != Fault.None { return true; }
            job.frames[at].pc = symbol_pc::MatchText;
            get_key(job, b, r, k);
        },
        symbol_pc::MatchText => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            if str_len(heap, s) > 0 { symbol_ask_exec(job, at); return false; }
            job.frames[at].pc = symbol_pc::Advance;
            ask_last_index(job, b, f.receiver);
        },
        symbol_pc::Advance => {
            let last, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            let next = advance_string_index(heap, f.input, last, f.index & symbol_flag::Unicode != 0);
            job.frames[at].pc = symbol_pc::AdvanceSet;
            if !put_last_index(job, b, f.receiver, next) && heap.fault == Fault.None { symbol_ask_exec(job, at); }
        },
        _ => { heap.fault = Fault.InvalidState; return true; },
    }
    return false;
}
// Result i's first read, or with every result used the answer: the
// pieces and S's rest, or S itself when there were no results.
fn symbol_render(job: Job&, at: i64) -> bool {
    let heap .= job.heap; let f = job.frames[at];
    let i = f.index; let results = result_base(job, at);
    if !f.input.is_string() || i < 0 || i > f.argument_count || job.operands.len != scratch_base(job, at) { heap.fault = Fault.InvalidState; return true; }
    if i < f.argument_count {
        let r = job.operands[results + i];
        if !r.is_object() { heap.fault = Fault.InvalidState; return true; }
        job.frames[at].pc = symbol_pc::Length;
        get_named(job, Builtin.RegExpPrototypeReplace, r, "length");
        return false;
    }
    let s = f.input; let len = str_len(heap, s);
    let next = register(job, at, symbol_reg::Next);
    // A matched string longer than the rest of S leaves next past its end.
    if heap.fault != Fault.None || next < 0 { heap.fault = Fault.InvalidState; return true; }
    if f.argument_count == 0 { finish_native(job, f, s); return true; }
    if len > next && !replace_spend(job, at, len - next) { return true; }
    if len > next { let piece = substring(heap, s, next, len); if heap.fault != Fault.None || !push_operand(job, piece) { return true; } }
    let r = concat_pieces(job, job.operands[results..]);
    if heap.fault == Fault.None { finish_native(job, f, r); }
    return true;
}
// Ask for capture n of result r, or after the last its `groups`.
fn symbol_ask_capture(job: Job&, at: i64, r: Value, n: i64) {
    let heap .= job.heap; let b = Builtin.RegExpPrototypeReplace;
    let m = register(job, at, symbol_reg::Captures);
    if heap.fault != Fault.None { return; }
    if n > m { job.frames[at].pc = symbol_pc::Groups; get_named(job, b, r, "groups"); return; }
    let k = index_key(heap, n);
    if heap.fault != Fault.None { return; }
    job.frames[at].pc = symbol_pc::Capture;
    get_key(job, b, r, k);
}
// One render phase (Length to Named) of the frame at `at` for result i.
fn symbol_render_phase(job: Job&, at: i64) -> bool {
    let heap .= job.heap; let b = Builtin.RegExpPrototypeReplace; let f = job.frames[at];
    let i = f.index; let scratch = scratch_base(job, at);
    if !f.input.is_string() || i < 0 || i >= f.argument_count || !job.operands[result_base(job, at) + i].is_object() { heap.fault = Fault.InvalidState; return true; }
    let r = job.operands[result_base(job, at) + i];
    // The scratch below the awaited value: matched, then the captures, groups and named values.
    let held = job.operands.len - 1 - scratch;
    match f.pc {
        symbol_pc::Length => {
            if held != 0 { heap.fault = Fault.InvalidState; return true; }
            let len, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            let m = if len >= 1 { len - 1 } else { 0 };
            if m > max_replace_work() { replace_limit(job, at); return true; }
            if !replace_spend(job, at, m as i64) { return true; }
            set_register(job, at, symbol_reg::Captures, m as i64);
            let k = index_key(heap, 0);
            if heap.fault != Fault.None { return true; }
            job.frames[at].pc = symbol_pc::Matched;
            get_key(job, b, r, k);
        },
        symbol_pc::Matched => {
            if held != 0 { heap.fault = Fault.InvalidState; return true; }
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None || !replace_spend(job, at, str_len(heap, s)) || !push_operand(job, s) { return true; }
            job.frames[at].pc = symbol_pc::Position;
            get_named(job, b, r, "index");
        },
        symbol_pc::Position => {
            if held != 1 { heap.fault = Fault.InvalidState; return true; }
            let n, done = number_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            // ToIntegerOrInfinity (NaN is 0), clamped to S.
            let p = if n != n { 0.0 } else { min(max(trunc(n), 0.0), str_len(heap, f.input)) };
            set_register(job, at, symbol_reg::Position, p as i64);
            symbol_ask_capture(job, at, r, 1);
        },
        symbol_pc::Capture => {
            let m = register(job, at, symbol_reg::Captures);
            if heap.fault != Fault.None || held < 1 || held > m { heap.fault = Fault.InvalidState; return true; }
            if form_value(heap, job.operands[job.operands.len - 1]).is_undefined() { job.operands[job.operands.len - 1] = Value.Undefined; }
            else {
                let ready, s = regexp_text(job, b);
                if !ready || heap.fault != Fault.None || !replace_spend(job, at, str_len(heap, s)) || !push_operand(job, s) { return true; }
            }
            symbol_ask_capture(job, at, r, held + 1);
        },
        symbol_pc::Groups => {
            let m = register(job, at, symbol_reg::Captures);
            if heap.fault != Fault.None || held != m + 1 { heap.fault = Fault.InvalidState; return true; }
            let g = form_value(heap, job.operands[job.operands.len - 1]);
            job.operands[job.operands.len - 1] = g;
            if is_callable(heap, f.callee) {
                let p = register(job, at, symbol_reg::Position);
                if heap.fault != Fault.None { return true; }
                var args: Value[>..] = [];
                args.append(job.operands[scratch..(scratch + 1 + m)]);
                args.push(number(p)); args.push(f.input);
                if !g.is_undefined() { args.push(g); }
                job.frames[at].pc = symbol_pc::Callback;
                call_accessor(job, f.callee, Value.Undefined, args);
                return false;
            }
            if !f.callee.is_string() { heap.fault = Fault.InvalidState; return true; }
            if !g.is_undefined() {
                let o = to_object(job, g);
                if heap.fault != Fault.None { return true; }
                job.operands[job.operands.len - 1] = o;
                set_register(job, at, symbol_reg::Close, template_close(heap, f.callee));
            }
            set_register(job, at, symbol_reg::Template, 0); set_register(job, at, symbol_reg::Charged, 0);
            symbol_next_named(job, at);
        },
        // A name's value, spent as it is appended, before the next name is read.
        symbol_pc::Named => {
            let m = register(job, at, symbol_reg::Captures); let charged = register(job, at, symbol_reg::Charged);
            if heap.fault != Fault.None || held < m + 2 || charged < 0 { heap.fault = Fault.InvalidState; return true; }
            if form_value(heap, job.operands[job.operands.len - 1]).is_undefined() { job.operands[job.operands.len - 1] = Value.Undefined; }
            else {
                let ready, s = regexp_text(job, b);
                if !ready || heap.fault != Fault.None || !replace_spend(job, at, str_len(heap, s)) || !push_operand(job, s) { return true; }
                set_register(job, at, symbol_reg::Charged, charged + str_len(heap, s));
            }
            symbol_next_named(job, at);
        },
        symbol_pc::Callback => {
            let m = register(job, at, symbol_reg::Captures);
            if heap.fault != Fault.None || held != m + 2 { heap.fault = Fault.InvalidState; return true; }
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None || !replace_spend(job, at, str_len(heap, s)) { return true; }
            symbol_commit(job, at, s);
            return true;
        },
        _ => { heap.fault = Fault.InvalidState; return true; },
    }
    return false;
}
// The template's next `$<name>` from the Template register: Get(groups,
// name), or with none left the substitution and its commit.
// The tokens before it and its own scan are spent first (the Charged
// register counts them), so the allowance runs out before a later name is
// read, where the Pants engine's left-to-right expansion stops.
fn symbol_next_named(job: Job&, at: i64) {
    let heap .= job.heap; let f = job.frames[at];
    let scratch = scratch_base(job, at);
    let m = register(job, at, symbol_reg::Captures); let from = register(job, at, symbol_reg::Template); let close = register(job, at, symbol_reg::Close);
    let charged = register(job, at, symbol_reg::Charged); let p = register(job, at, symbol_reg::Position);
    if heap.fault != Fault.None { return; }
    let groups = job.operands[scratch + 1 + m];
    let t = f.callee; let n = str_len(heap, t);
    if !t.is_string() || from < 0 || from > n || close >= n || charged < 0 || p < 0 || p > str_len(heap, f.input) { heap.fault = Fault.InvalidState; return; }
    let matched = job.operands[scratch];
    var captures: Value[>..] = []; captures.append(job.operands[(scratch + 1)..(scratch + 1 + m)]);
    var i = from; var work: i64 = 0;
    if !groups.is_undefined() {
        while i < n {
            let token = template_token(heap, t, i, n, m, true, close);
            work += token_work(heap, token, f.input, p, matched, captures);
            if token.kind == token::Named {
                if !replace_spend(job, at, work) { return; }
                set_register(job, at, symbol_reg::Charged, charged + work); set_register(job, at, symbol_reg::Template, token.next);
                let k = name_key(heap, utf16_range(heap, t, token.a, token.b));
                if heap.fault != Fault.None { return; }
                job.frames[at].pc = symbol_pc::Named;
                get_key(job, Builtin.RegExpPrototypeReplace, groups, k);
                return;
            }
            // No ">" after this "$<": no later one has one either.
            if token.scanned > 0 { break; }
            i = token.next;
        }
    }
    set_register(job, at, symbol_reg::Template, n);
    var values: Value[>..] = []; values.append(job.operands[(scratch + 2 + m)..]);
    let r = replacement_text(job, at, t, f.input, p, matched, captures, !groups.is_undefined(), values, charged);
    if heap.fault != Fault.None { return; }
    symbol_commit(job, at, r);
}
// Units [start, end) of string s.
fn utf16_range(heap: const Heap&, s: Value, start: i64, end: i64) -> u16[>..] { var out: u16[>..] = []; append_range(heap, s, start, end, out); return out; }
// Result i's replacement r: unless its position is before the end of the
// last result used, S's units since that end and r become its slot's
// piece (otherwise ""); the scratch goes, and the next step renders i + 1.
fn symbol_commit(job: Job&, at: i64, r: Value) {
    let heap .= job.heap; let f = job.frames[at];
    let s = f.input; let scratch = scratch_base(job, at);
    let next = register(job, at, symbol_reg::Next); let p = register(job, at, symbol_reg::Position);
    if heap.fault != Fault.None { return; }
    if !r.is_string() || !s.is_string() || p < 0 || p > str_len(heap, s) || next < 0 || job.operands.len <= scratch { heap.fault = Fault.InvalidState; return; }
    let matched = job.operands[scratch];
    var piece = key_named(heap, "");
    if heap.fault != Fault.None { return; }
    if p >= next {
        if !replace_spend(job, at, p - next + str_len(heap, r)) { return; }
        if p > next {
            let before = substring(heap, s, next, p);
            if heap.fault != Fault.None { return; }
            piece = if str_len(heap, r) == 0 { before } else { concat_pieces(job, [before, r]) };
        } else { piece = r; }
        if heap.fault != Fault.None { return; }
        set_register(job, at, symbol_reg::Next, p + str_len(heap, matched));
    }
    job.operands.resize(scratch);
    job.operands[result_base(job, at) + f.index] = piece;
    job.frames[at].index = f.index + 1; job.frames[at].pc = symbol_pc::Render;
}
