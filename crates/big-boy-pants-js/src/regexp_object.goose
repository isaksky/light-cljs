import std;
import .program;
import .source_vm;
import .realm;
import .array_methods;
import .array_callbacks;
import .runtime_compile;

namespace source;

// RegExp objects (light-cljs-932; decisions 1, 2, 4 and 6 of
// docs/javascript-coverage.md; the Pants engine's regexp/object.rs and
// vm/native/regexp.rs, QuickJS's js_regexp_constructor, js_regexp_get_flag,
// js_regexp_get_flags, js_regexp_get_source and js_regexp_toString).
//
// bbp's Rust frontend compiles every pattern: a literal's in the image
// (GSP version 7, program::Pattern, Code.patterns), a `new RegExp`'s at run
// time (runtime_compile.goose compile_pattern), whose rows are job code
// rows the worker's reset truncates. Goose parses no pattern and carries no
// Unicode table. A RegExp is one node_kind::RegExp row: `function` its
// program's row in Code.patterns (its flags are the program's), `value` its
// pattern text (the source string, which `source` escapes), `proto` its
// prototype, and, as an ordinary object, own properties, the first
// `lastIndex` (writable, neither enumerable nor configurable), from the
// realm's template's shape (regexp_template_row). Each evaluation of a
// literal makes a new object sharing the immutable program. Matching (exec,
// test and the symbol methods) is light-cljs-770's later children's: they
// are pending properties of RegExp.prototype.
//
// The constructor (22.2.4.1) runs as a RegExp native frame (decision 4),
// since IsRegExp, the pattern's `constructor`, `source` and `flags`,
// newTarget's `prototype` and the two ToString conversions can run
// JavaScript; each step consumes the one value it asked for. The flags
// getter (22.2.6.4) and toString (22.2.6.17) read generically, so they are
// frames too. The flag getters, `source` and RegExp[@@species] cannot run
// JavaScript: plain built-ins.

// A RegExp constructor frame's phase (its pc): the value it awaits on its
// operand top.
let regexp_pc::Match = 0;        // Get(pattern, @@match), IsRegExp's read
let regexp_pc::Constructor = 1;  // Get(pattern, "constructor")
let regexp_pc::Source = 2;       // Get(pattern, "source")
let regexp_pc::Flags = 3;        // Get(pattern, "flags"), or the flags argument
let regexp_pc::Prototype = 4;    // Get(newTarget, "prototype"), or undefined
let regexp_pc::PatternText = 5;  // ToString of the pattern
let regexp_pc::FlagsText = 6;    // ToString of the flags
// A constructor frame's state bits (its index): called without new and
// with undefined flags (the identity rule), the pattern is a RegExp, IsRegExp
// answered true, and `callee` now holds the prototype (not newTarget).
let regexp_state::Identity = 1;
let regexp_state::Branded = 2;
let regexp_state::IsRegExp = 4;
let regexp_state::Prototype = 8;
struct FlagName { name: u8[..12], letter: u8 }
// RegExp.prototype.flags's reads, in order (22.2.6.4), with their letters.
let FLAG_NAMES: FlagName[8] = [
    FlagName { name: "hasIndices", letter: 'd' }, FlagName { name: "global", letter: 'g' }, FlagName { name: "ignoreCase", letter: 'i' }, FlagName { name: "multiline", letter: 'm' },
    FlagName { name: "dotAll", letter: 's' }, FlagName { name: "unicode", letter: 'u' }, FlagName { name: "unicodeSets", letter: 'v' }, FlagName { name: "sticky", letter: 'y' },
];
// The flag getters in FLAG_NAMES order, each a regexp_flag:: bit.
let FLAG_GETTERS: Builtin[8] = [Builtin.RegExpPrototypeHasIndices, Builtin.RegExpPrototypeGlobal, Builtin.RegExpPrototypeIgnoreCase, Builtin.RegExpPrototypeMultiline, Builtin.RegExpPrototypeDotAll, Builtin.RegExpPrototypeUnicode, Builtin.RegExpPrototypeUnicodeSets, Builtin.RegExpPrototypeSticky];

// Whether v is a RegExp object (has [[RegExpMatcher]]).
fn is_regexp_object(heap: const Heap&, v: Value) -> bool { return v.is_object() && v.id() < heap.nodes.len && node(heap, v.id()).kind == node_kind::RegExp; }
// RegExp o's program row.
fn regexp_program(heap: const Heap&, o: i64) -> const program::Pattern& { return heap.code.patterns[node(heap, o).function]; }
// Whether built-in b is an accessor's getter: RegExp.prototype's flags,
// source and flag getters and RegExp[@@species] (realm.goose build_realm).
fn regexp_accessor(b: Builtin) -> bool {
    if b == Builtin.RegExpPrototypeFlags || b == Builtin.RegExpPrototypeSource || b == Builtin.RegExpSpecies { return true; }
    for g in FLAG_GETTERS { if g == b { return true; } }
    return false;
}
// Whether built-in b runs as a RegExp native frame (native_start).
fn regexp_native(b: Builtin) -> bool { return b == Builtin.RegExp || b == Builtin.RegExpPrototypeFlags || b == Builtin.RegExpPrototypeToString; }

// The row of literal instruction row `op`'s program (Code.patterns, in
// ascending `op`; a run-time row compiled when the code had `op`
// instructions is skipped), or -1.
fn literal_program(code: const Code&, op: i64) -> i64 {
    var lo: i64 = 0; var hi = code.patterns.len;
    while lo < hi { let mid = lo + (hi - lo) / 2; if code.patterns[mid].op < op { lo = mid + 1; } else { hi = mid; } }
    while lo < code.patterns.len && code.patterns[lo].op == op { if code.patterns[lo].literal { return lo; } lo += 1; }
    return -1;
}
// Raise program p's failure: a SyntaxError with the parser's message, the
// compilation step limit's RangeError, or, for a feature bbp's Rust matcher
// compiler refuses (lookbehind, light-cljs-770's eighth child), an
// UnsupportedOperand naming it (decision 1).
fn pattern_failure(job: Job&, p: program::Pattern) {
    let heap .= job.heap;
    job.detail.clear();
    if p.failure == regexp_failure::Unsupported { for c in "RegExp: " { job.detail.push(c); } }
    job.detail.append(heap.code.units[p.units..(p.units + p.unit_count)]);
    heap.fault = if p.failure == regexp_failure::Syntax { Fault.SyntaxError } else if p.failure == regexp_failure::Limit { Fault.RangeError } else { Fault.UnsupportedOperand };
}
// A new RegExp of program row `program` with pattern text `source` and
// prototype `proto`, lastIndex 0.
fn new_regexp(job: Job&, program: i64, source: Value, proto: Value) -> Value {
    let heap .= job.heap;
    let t .= node(heap, regexp_template_row());
    return allocate(heap, Node { kind: node_kind::RegExp, function: program as u32, key_count: 1, value: source, shape: t.shape, proto: proto, slots: [number(0), Value.Undefined] });
}
// RegExpLiteral (light-cljs-932) at instruction row `at`, whose constant
// `encoded` is the literal's flags, NUL and pattern: a new RegExp of the
// literal's program, its pattern text a view of the constant's units; or
// the program's failure, raised now.
fn regexp_literal(job: Job&, at: i64, encoded: Value) {
    let heap .= job.heap;
    let r = literal_program(heap.code, at);
    if r < 0 || !encoded.is_string() { heap.fault = Fault.InvalidState; return; }
    let p = heap.code.patterns[r];
    if p.failure != regexp_failure::None { pattern_failure(job, p); return; }
    let s = header(heap, encoded.sid()); var split: i64 = 0;
    while split < s.len && unit(heap, encoded, split) != 0 { split += 1; }
    if split >= s.len { heap.fault = Fault.InvalidState; return; }
    let source = string_ref(heap, s.start + split + 1, s.len - split - 1, s.wide);
    if heap.fault != Fault.None { return; }
    let o = new_regexp(job, r, source, object(regexp_prototype_row()));
    if heap.fault == Fault.None { push_operand(job, o); }
}
// The flags text of regexp_flag:: bits, in dgimsuvy order.
fn flags_text(heap: Heap&, bits: i64) -> Value {
    var text: u8[>..] = [];
    for f, i in FLAG_NAMES { if bits & (1 << i) != 0 { text.push(f.letter); } }
    return key_named(heap, text);
}
// EscapeRegExpPattern (22.2.6.13.1; the Pants engine's object.rs
// `source`): "(?:)" for the empty pattern; otherwise the pattern with a
// slash outside a class escaped and each line terminator written as its
// escape (a backslash before one is not doubled), so `/${source}/` parses
// to the same pattern.
fn escape_pattern(heap: Heap&, pattern: Value) -> Value {
    let n = str_len(heap, pattern);
    if n == 0 { return key_named(heap, "(?:)"); }
    if !charge(heap, n) { return Value.Undefined; }
    var out: u16[>..] = []; var escaped = false; var class = false;
    for i in n {
        let c = unit(heap, pattern, i);
        if c == 10 || c == 13 || c == 8232 || c == 8233 {
            if escaped { out.pop(); }
            let text: const u8[:] = if c == 10 { "\\n" } else if c == 13 { "\\r" } else if c == 8232 { "\\u2028" } else { "\\u2029" };
            for b in text { out.push(b); }
            escaped = false; continue;
        }
        if !escaped && c == 47 && !class { out.push(92); }
        if !escaped && c == 91 { class = true; }
        if !escaped && c == 93 { class = false; }
        out.push(c);
        escaped = !escaped && c == 92;
    }
    return string(heap, out);
}
// The plain RegExp built-ins: the flag getters and `source` (a RegExp's,
// RegExp.prototype's own answer, a TypeError otherwise) and
// RegExp[@@species] (its receiver).
fn regexp_builtin(job: Job&, b: Builtin, receiver: Value) -> Value {
    let heap .= job.heap;
    if b == Builtin.RegExpSpecies { return receiver; }
    let own = is_regexp_object(heap, receiver);
    let prototype = receiver.is_object() && receiver.id() == regexp_prototype_row();
    if b == Builtin.RegExpPrototypeSource {
        if own { return escape_pattern(heap, node(heap, receiver.id()).value); }
        if prototype { return key_named(heap, "(?:)"); }
        return type_error(job, "RegExp.prototype.source getter called on a non-RegExp");
    }
    for g, i in FLAG_GETTERS {
        if g != b { continue; }
        if own { return boolean(regexp_program(heap, receiver.id()).flags & (1 << i) != 0); }
        if prototype { return Value.Undefined; }
        return type_error(job, "RegExp flag getter called on a non-RegExp");
    }
    heap.fault = Fault.InvalidState; return Value.Undefined;
}
// The operand top as a string for built-in b's frame: ToString of a
// primitive (a symbol's TypeError), or an object's conversion started in a
// Convert frame above (false: the step runs again on the string).
fn regexp_text(job: Job&, b: Builtin) -> bool, Value {
    let heap .= job.heap;
    let v = form_value(heap, job.operands.pop());
    if v.is_object() {
        if !convertible(heap, v) { unsupported_step(job, b, "ToString of a light-cljs runtime value"); return false, Value.Undefined; }
        job.operands.push(v); start_convert(job, Hint.String, true, 1, 1);
        return false, Value.Undefined;
    }
    return true, primitive_string(job, b, v);
}
// Get(o, k) for the frame at the top, k any key: the value goes on its
// operands, or a getter (or a proxy's [[Get]]) runs above it and its
// result arrives there.
fn get_key(job: Job&, b: Builtin, o: Value, k: Value) {
    let v, getter = lookup(job.heap, o, k);
    if job.heap.fault != Fault.None { return; }
    if getter.is_object() { run_getter(job, b, getter, o); } else { push_operand(job, v); }
}
// IsRegExp(v) (7.2.8), a reusable step for native frames (light-cljs-903's
// startsWith, endsWith and includes and the String methods of
// light-cljs-770's later children): a primitive is no RegExp and pushes
// nothing (false); for an object Get(v, @@match) goes on the frame's
// operands (true), a getter running above it, and is_regexp_answer reads it.
fn start_is_regexp(job: Job&, b: Builtin, v: Value) -> bool {
    if !v.is_object() { return false; }
    get_key(job, b, v, Value.Symbol { s: well_known::Match });
    return true;
}
// IsRegExp's answer from `matcher`, Get(v, @@match): its ToBoolean, or when
// undefined whether v has [[RegExpMatcher]].
fn is_regexp_answer(heap: const Heap&, v: Value, matcher: Value) -> bool { return if matcher.is_undefined() { is_regexp_object(heap, v) } else { truthy(heap, matcher) }; }

// Start RegExp built-in b (regexp_native) called with `receiver` and
// `args`, its operand base `base` (native_start).
fn start_regexp_native(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    if b == Builtin.RegExp { start_regexp(job, args, base, Value.Undefined); return; }
    let heap .= job.heap;
    if !receiver.is_object() { type_error(job, if b == Builtin.RegExpPrototypeFlags { "RegExp.prototype.flags getter called on a non-object" } else { "RegExp.prototype.toString called on a non-object" }); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: receiver, native: Native.RegExp { which: b } });
    get_named(job, b, receiver, if b == Builtin.RegExpPrototypeFlags { FLAG_NAMES[0].name } else { "source" });
}
// RegExp(pattern, flags) called (`nt` undefined) or constructed with
// newTarget `nt`, its operand base `base`: the RegExp frame (see above). A
// RegExp pattern gives its pattern text and, with undefined flags, its
// flags now; IsRegExp's @@match read comes first.
fn start_regexp(job: Job&, args: const Value[:], base: i64, nt: Value) {
    let heap .= job.heap;
    let pattern = arg(args, 0); let flags = arg(args, 1);
    var state: i64 = if nt.is_undefined() && flags.is_undefined() { regexp_state::Identity } else { 0 };
    var p = pattern; var f = flags;
    if is_regexp_object(heap, pattern) {
        state |= regexp_state::Branded;
        p = node(heap, pattern.id()).value;
        if flags.is_undefined() { f = flags_text(heap, regexp_program(heap, pattern.id()).flags); if heap.fault != Fault.None { return; } }
    }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: regexp_pc::Match, local_base: job.locals.len, operand_base: base, callee: nt, receiver: pattern, input: p, output: f, index: state, native: Native.RegExp { which: Builtin.RegExp } });
    if !start_is_regexp(job, Builtin.RegExp, pattern) { push_operand(job, Value.Undefined); }
}
// Ask the constructor frame at `at` for its next value, phase `pc`.
fn regexp_read(job: Job&, at: i64, pc: i64) {
    let heap .= job.heap; let b = Builtin.RegExp;
    job.frames[at].pc = pc;
    let f = job.frames[at];
    match pc {
        regexp_pc::Constructor => { get_named(job, b, f.receiver, "constructor"); },
        regexp_pc::Source => { get_named(job, b, f.receiver, "source"); },
        regexp_pc::Flags => { if f.output.is_undefined() { get_named(job, b, f.receiver, "flags"); } else { push_operand(job, f.output); } },
        // GetPrototypeFromConstructor(newTarget) (light-cljs-887's
        // closure_prototype: a closure's lazy `prototype` is made first).
        regexp_pc::Prototype => {
            if !f.callee.is_object() { push_operand(job, Value.Undefined); return; }
            let proto, getter = closure_prototype(job, f.callee);
            if heap.fault != Fault.None { return; }
            if getter.is_object() { run_getter(job, b, getter, f.callee); } else { push_operand(job, if proto.is_object() { proto } else { Value.Undefined }); }
        },
        // RegExpInitialize: an undefined pattern or flags is "".
        regexp_pc::PatternText => { push_operand(job, if f.input.is_undefined() { key_named(heap, "") } else { f.input }); },
        regexp_pc::FlagsText => { push_operand(job, if f.output.is_undefined() { key_named(heap, "") } else { f.output }); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// After IsRegExp and the identity rule: the pattern's `source` and `flags`
// when it is IsRegExp but no RegExp, then newTarget's `prototype`.
fn regexp_after_match(job: Job&, at: i64) {
    let state = job.frames[at].index;
    regexp_read(job, at, if state & regexp_state::IsRegExp != 0 && state & regexp_state::Branded == 0 { regexp_pc::Source } else { regexp_pc::Prototype });
}
// One step of the RegExp frame at the top.
fn regexp_step(job: Job&, frame: Frame, b: Builtin) {
    match b {
        RegExp => { regexp_constructor_step(job, frame); },
        RegExpPrototypeFlags => { regexp_flags_step(job, frame); },
        RegExpPrototypeToString => { regexp_to_string_step(job, frame); },
        _ => { job.heap.fault = Fault.InvalidState; },
    }
}
fn regexp_constructor_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1; let b = Builtin.RegExp;
    if job.operands.len <= frame.operand_base { heap.fault = Fault.InvalidState; return; }
    match frame.pc {
        regexp_pc::Match => {
            let m = job.operands.pop();
            var state = frame.index;
            if is_regexp_answer(heap, frame.receiver, m) { state |= regexp_state::IsRegExp; } else { state &= ~regexp_state::Identity; }
            job.frames[at].index = state;
            if state & regexp_state::Identity != 0 { regexp_read(job, at, regexp_pc::Constructor); } else { regexp_after_match(job, at); }
        },
        // The identity rule: the pattern itself when its `constructor` is
        // %RegExp%.
        regexp_pc::Constructor => {
            let c = job.operands.pop();
            if c.is_object() && c.id() == regexp_row() { finish_native(job, frame, frame.receiver); return; }
            regexp_after_match(job, at);
        },
        regexp_pc::Source => { job.frames[at].input = job.operands.pop(); regexp_read(job, at, regexp_pc::Flags); },
        regexp_pc::Flags => { job.frames[at].output = job.operands.pop(); regexp_read(job, at, regexp_pc::Prototype); },
        regexp_pc::Prototype => {
            let p = job.operands.pop();
            job.frames[at].callee = if p.is_object() { p } else { object(regexp_prototype_row()) };
            job.frames[at].index = frame.index | regexp_state::Prototype;
            regexp_read(job, at, regexp_pc::PatternText);
        },
        regexp_pc::PatternText => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            job.frames[at].input = s; regexp_read(job, at, regexp_pc::FlagsText);
        },
        regexp_pc::FlagsText => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            let r = compile_pattern(job, frame.input, s);
            if r < 0 { return; }
            let o = new_regexp(job, r, frame.input, frame.callee);
            if heap.fault == Fault.None { finish_native(job, frame, o); }
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// RegExp.prototype.flags's frame: pc is the FLAG_NAMES read it awaits,
// argument_count the regexp_flag:: bits of those that were truthy.
fn regexp_flags_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if frame.pc < 0 || frame.pc >= FLAG_NAMES.len || job.operands.len <= frame.operand_base { heap.fault = Fault.InvalidState; return; }
    let v = job.operands.pop();
    let bits = frame.argument_count | (if truthy(heap, v) { 1 << frame.pc } else { 0 });
    if frame.pc + 1 == FLAG_NAMES.len {
        let s = flags_text(heap, bits);
        if heap.fault == Fault.None { finish_native(job, frame, s); }
        return;
    }
    job.frames[at].argument_count = bits; job.frames[at].pc = frame.pc + 1;
    get_named(job, Builtin.RegExpPrototypeFlags, frame.receiver, FLAG_NAMES[frame.pc + 1].name);
}
// RegExp.prototype.toString's frame: pc 0 awaits `source`, pc 1 `flags`
// (`input` the source's string), each converted by ToString.
fn regexp_to_string_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1; let b = Builtin.RegExpPrototypeToString;
    if frame.pc < 0 || frame.pc > 1 || job.operands.len <= frame.operand_base { heap.fault = Fault.InvalidState; return; }
    let ready, s = regexp_text(job, b);
    if !ready || heap.fault != Fault.None { return; }
    if frame.pc == 0 { job.frames[at].input = s; job.frames[at].pc = 1; get_named(job, b, frame.receiver, "flags"); return; }
    if !frame.input.is_string() { heap.fault = Fault.InvalidState; return; }
    var units: u16[>..] = [47]; append_utf16(heap, frame.input, units); units.push(47); append_utf16(heap, s, units);
    let text = string(heap, units);
    if heap.fault == Fault.None { finish_native(job, frame, text); }
}
