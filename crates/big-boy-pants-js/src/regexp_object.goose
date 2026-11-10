import std;
import .program;
import .source_vm;
import .realm;
import .array_methods;
import .array_callbacks;
import .runtime_compile;
import .function_calls;
import .regexp_match;

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
// literal makes a new object sharing the immutable program. exec and test
// (light-cljs-933) are at the end of this file, the matcher in
// regexp_match.goose; the symbol methods are light-cljs-770's later
// children's: pending properties of RegExp.prototype.
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
fn regexp_native(b: Builtin) -> bool { return b == Builtin.RegExp || b == Builtin.RegExpPrototypeFlags || b == Builtin.RegExpPrototypeToString || regexp_matching(b); }
// Whether built-in b is RegExp.prototype.exec or test (light-cljs-933).
inline fn regexp_matching(b: Builtin) -> bool { return b == Builtin.RegExpPrototypeExec || b == Builtin.RegExpPrototypeTest; }

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
    if regexp_matching(b) { start_exec(job, b, receiver, arg(args, 0), base); return; }
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
        RegExpPrototypeExec, RegExpPrototypeTest => { exec_step(job, frame, b); },
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

// ---- exec and test (light-cljs-933)
// RegExp.prototype.exec (22.2.6.2) and test (22.2.6.16), the Pants
// engine's vm/native/regexp.rs Exec and Test states and regexp/object.rs
// exec_with_capture_budget; QuickJS's js_regexp_exec and js_regexp_test.
//
// exec: RequireInternalSlot(R, [[RegExpMatcher]]), then ToString(string),
// then RegExpBuiltinExec. test: R must be an object, then ToString(string),
// then RegExpExec (22.2.7.1): Get(R, "exec"); a callable one is called with
// R and S and its result must be an object or null (a TypeError otherwise),
// and test answers whether it is not null; any other value needs R's
// [[RegExpMatcher]] (a TypeError otherwise) and runs RegExpBuiltinExec.
// %RegExp.prototype.exec% itself is not called: its steps are these, and
// test needs no match result, so it builds none. RegExpBuiltinExec
// (22.2.7.2): ToLength(Get(R, "lastIndex")), read and converted on every
// call (a valueOf runs), used only with `g` or `y` (otherwise the search
// starts at 0); a search (regexp_match.goose) from it, every later
// position too unless sticky; a failed `g` or `y` search sets lastIndex 0,
// a match its end, by Set (a non-writable lastIndex is a TypeError; a
// RegExp the job did not allocate faults NativeBoundary, decision 3). The
// result is a new array of the groups' strings (undefined for unmatched
// ones), then `index`, `input`, `groups` (a null-prototype object of the
// named groups, or undefined when the pattern names none; of duplicate
// names in exclusive alternatives, the one that matched) and with `d`
// `indices`: an array of [start, end] pairs (undefined for unmatched
// groups) with its own `groups` naming the same pairs. Every one is a
// writable, enumerable, configurable data property, as
// CreateDataProperty makes them. A group's string is a view of the input's
// run (only a header row is new), copied when a UTF-16 slice fits Latin-1;
// its code units are charged anyway (the Pants engine's materialization
// charge, bbp-290), against the search's allowance and the job's work.
//
// Conversions plan nothing (coerce.goose): exec's receiver check and
// test's object check come before ToString of the argument. A call whose
// conversions and `exec` read run no JavaScript (a primitive argument, the
// realm's exec, a primitive lastIndex) runs to its result in place, with
// no frame; otherwise a RegExp frame (Native.RegExp, its built-in the
// payload) continues from the step that needs JavaScript, its phase (pc)
// an exec_pc:: naming the value it awaits on its operand top; receiver R,
// input S once converted.
let exec_pc::Input = 0;      // the argument, converted by ToString in a Convert frame
let exec_pc::Exec = 1;       // test: Get(R, "exec") from its getter
let exec_pc::Result = 2;     // test: a callable exec's result
let exec_pc::LastIndex = 3;  // Get(R, "lastIndex") (or its getter's value), then its ToLength

// Start exec or test (b) with receiver r and argument `input`, operand
// base `base` (native_start's contract: an answer in place goes there).
fn start_exec(job: Job&, b: Builtin, r: Value, input: Value, base: i64) {
    let heap .= job.heap;
    if b == Builtin.RegExpPrototypeTest {
        if !r.is_object() { type_error(job, "RegExp.prototype.test called on a non-object"); return; }
    } else if !is_regexp_object(heap, r) { type_error(job, "RegExp.prototype.exec called on an object that is not a RegExp"); return; }
    let v = form_value(heap, input);
    if v.is_object() {
        let at = exec_frame(job, -1, b, r, Value.Undefined, base, exec_pc::Input);
        if at < 0 || !push_operand(job, v) { return; }
        let ready, s = regexp_text(job, b);
        return;
    }
    let s = primitive_string(job, b, v);
    if heap.fault != Fault.None { return; }
    exec_string(job, -1, b, r, s, base);
}
// The frame of exec or test at `at`, or with `at` -1 a new one at operand
// base `base` (the operands cut to it) in phase `pc`: its row, or -1 at
// the frame limit.
fn exec_frame(job: Job&, at: i64, b: Builtin, r: Value, s: Value, base: i64, pc: i64) -> i64 {
    if at >= 0 { job.frames[at].pc = pc; job.frames[at].input = s; return at; }
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return -1; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: pc, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: r, input: s, native: Native.RegExp { which: b } });
    return job.frames.len - 1;
}
// The answer of exec or test: the frame at `at` finishes with it, or with
// no frame it goes at the call's operand base (or completes the job, when
// a forwarding call that was its entry popped its frame first).
fn exec_answer(job: Job&, at: i64, base: i64, v: Value) {
    if job.heap.fault != Fault.None { return; }
    if at >= 0 { finish_native(job, job.frames[at], v); return; }
    job.operands.resize(base);
    if job.frames.len == 0 { deliver(job, v); } else { push_operand(job, v); }
}
// S is the argument's string: test reads `exec`; exec reads lastIndex.
fn exec_string(job: Job&, at: i64, b: Builtin, r: Value, s: Value, base: i64) {
    let heap .= job.heap;
    if b == Builtin.RegExpPrototypeExec { exec_last_index(job, at, b, r, s, base); return; }
    let k = key_named(heap, "exec");
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, r, k);
    if heap.fault != Fault.None { return; }
    if getter.is_object() {
        let f = exec_frame(job, at, b, r, s, base, exec_pc::Exec);
        if f >= 0 { run_getter(job, b, getter, r); }
        return;
    }
    exec_method(job, at, b, r, s, v, base);
}
// Whether f is %RegExp.prototype.exec%.
fn builtin_exec_function(heap: const Heap&, f: Value) -> bool { return f.is_object() && node(heap, f.id()).kind == node_kind::Builtin && builtin_of(heap, f.id()) == Builtin.RegExpPrototypeExec; }
// test's RegExpExec with `exec`'s value f.
fn exec_method(job: Job&, at: i64, b: Builtin, r: Value, s: Value, f: Value, base: i64) {
    let heap .= job.heap;
    if is_callable(heap, f) && !builtin_exec_function(heap, f) {
        let x = exec_frame(job, at, b, r, s, base, exec_pc::Result);
        if x >= 0 { call_accessor(job, f, r, [s]); }
        return;
    }
    if !is_regexp_object(heap, r) { type_error(job, "RegExp.prototype.test called on an object that is neither a RegExp nor has a callable exec"); return; }
    exec_last_index(job, at, b, r, s, base);
}
// Get(R, "lastIndex"): an own data property of every RegExp, but read as
// any property is.
fn exec_last_index(job: Job&, at: i64, b: Builtin, r: Value, s: Value, base: i64) {
    let heap .= job.heap;
    var v = Value.Undefined; var getter = Value.Undefined;
    if template_layout(heap, r) { v = node(heap, r.id()).slots[0]; }
    else {
        v, getter = lookup(heap, r, last_index_key(heap));
        if heap.fault != Fault.None { return; }
    }
    if getter.is_object() {
        let f = exec_frame(job, at, b, r, s, base, exec_pc::LastIndex);
        if f >= 0 { run_getter(job, b, getter, r); }
        return;
    }
    let x = form_value(heap, v);
    if x.is_object() {
        // Its ToNumber runs in a Convert frame above the RegExp frame.
        let f = exec_frame(job, at, b, r, s, base, exec_pc::LastIndex);
        if f < 0 || !push_operand(job, x) { return; }
        let last, done = length_value(job, b);
        return;
    }
    let n = primitive_number(job, b, x);
    if heap.fault != Fault.None { return; }
    exec_answer(job, at, base, builtin_exec(job, b, r, s, if !(n > 0) { 0.0 } else { min(trunc(n), max_safe_integer()) }));
}
// One step of an exec or test frame: the value its phase awaits arrived.
fn exec_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if job.operands.len <= frame.operand_base { heap.fault = Fault.InvalidState; return; }
    match frame.pc {
        exec_pc::Input => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return; }
            exec_string(job, at, b, frame.receiver, s, frame.operand_base);
        },
        exec_pc::Exec => { let f = job.operands.pop(); exec_method(job, at, b, frame.receiver, frame.input, f, frame.operand_base); },
        exec_pc::Result => {
            let v = job.operands.pop();
            if v.is_null() { finish_native(job, frame, boolean(false)); return; }
            if !v.is_object() { type_error(job, "RegExp exec method returned something other than an object or null"); return; }
            finish_native(job, frame, boolean(true));
        },
        exec_pc::LastIndex => {
            let last, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return; }
            exec_answer(job, at, frame.operand_base, builtin_exec(job, b, frame.receiver, frame.input, last));
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// The interned key "lastIndex": the RegExp template's one property's.
fn last_index_key(heap: const Heap&) -> Value { return heap.shapes[node(heap, regexp_template_row()).shape].key; }
// Whether RegExp r's own properties are still laid out as the realm's
// template made them: only lastIndex, writable, in its first inline slot,
// which Get and Set then read and write in place (what lookup and
// set_or_throw would find there, without a key's hashing).
fn template_layout(heap: const Heap&, r: Value) -> bool {
    let n .= node(heap, r.id());
    return n.kind == node_kind::RegExp && !n.initialized && n.shape == node(heap, regexp_template_row()).shape;
}
// Set(R, "lastIndex", n, true).
fn set_last_index(job: Job&, b: Builtin, r: Value, n: i64) -> bool {
    let heap .= job.heap;
    let k = last_index_key(heap);
    if template_layout(heap, r) && writable(job, r.id()) { node_mut(heap, r.id()).slots[0] = number(n); record(job, r, k, number(n)); return heap.fault == Fault.None; }
    // lastIndex is a non-configurable data property: never a setter.
    if set_or_throw(job, b, r, k, number(n)) { heap.fault = Fault.InvalidState; }
    return heap.fault == Fault.None;
}
// The search's allowance ran out: the job's work limit when the job's
// remaining work was its allowance, else the Pants engine's RangeError.
// The rest of the allowance, `unspent`, is charged to the job's work.
fn step_limit(job: Job&, m: const Matcher&, unspent: i64) {
    let heap .= job.heap;
    if m.allowance < max_match_steps() { heap.fault = Fault.WorkLimit; return; }
    if !charge(heap, unspent) { return; }
    job.detail.clear(); for c in "regular expression step limit exceeded" { job.detail.push(c); }
    heap.fault = Fault.RangeError;
}
// RegExpBuiltinExec(R, S) with lastIndex `last` (ToLength's): exec's
// match result or null, or test's boolean; Undefined with a fault.
fn builtin_exec(job: Job&, b: Builtin, r: Value, s: Value, last: f64) -> Value {
    let heap .= job.heap;
    let test = b == Builtin.RegExpPrototypeTest;
    let p = regexp_program(heap, r.id());
    let stateful = p.flags & (regexp_flag::Global | regexp_flag::Sticky) != 0;
    let h = header(heap, s.sid());
    let len: i64 = h.len;
    var from: i64 = 0;
    if stateful {
        if last > len {
            if !set_last_index(job, b, r, 0) { return Value.Undefined; }
            return if test { boolean(false) } else { Value.Null };
        }
        from = last as i64;
    }
    var m = Matcher {};
    m.allowance = min(max_match_steps(), heap.limits.work - heap.work);
    let first: i64 = p.start; let count: i64 = p.instructions + p.members;
    let ops = heap.code.pattern_ops[first..(first + count)];
    let status = if h.wide { search(m, ops, p, wide_run(heap, h), from) } else { search(m, ops, p, narrow_run(heap, h), from) };
    if status == match_status::Limit { step_limit(job, m, m.allowance); return Value.Undefined; }
    if status == match_status::IgnoreCase { return unsupported_step(job, b, "case-insensitive matching"); }
    if status == match_status::Invalid { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if !charge(heap, m.used) { return Value.Undefined; }
    if status == match_status::None {
        if stateful && !set_last_index(job, b, r, 0) { return Value.Undefined; }
        return if test { boolean(false) } else { Value.Null };
    }
    if stateful && !set_last_index(job, b, r, m.caps[1]) { return Value.Undefined; }
    // The groups' code units, charged after lastIndex's write (as the
    // Pants engine does) and before anything is made.
    var units: i64 = 0;
    for g in p.captures { let i: i64 = g; if m.caps[2 * i] >= 0 { units += m.caps[2 * i + 1] - m.caps[2 * i]; } }
    if units > m.allowance - m.used { step_limit(job, m, m.allowance - m.used); return Value.Undefined; }
    if !charge(heap, units) { return Value.Undefined; }
    if test { return boolean(true); }
    return match_array(job, s, p, m.caps);
}
// The code units [start, end) of string s: s when whole, a view of its run
// (a new header row only), or a Latin-1 copy of a UTF-16 slice that fits
// one, so every string keeps its canonical encoding. No work is charged.
fn substring(heap: Heap&, s: Value, start: i64, end: i64) -> Value {
    let h = header(heap, s.sid());
    if start == 0 && end == h.len { return s; }
    if !h.wide { return string_ref(heap, h.start + start, end - start, false); }
    if fits_latin1(wide_run(heap, h)[start..end]) { return import_text(heap, wide_run(heap, h)[start..end]); }
    return string_ref(heap, h.start + start, end - start, true);
}
// The interned key of capture-name units.
fn name_key(heap: Heap&, units: const u16[:]) -> Value {
    if fits_latin1(units) { var bytes: u8[>..] = []; for u in units { bytes.push(u as u8); } return key_named(heap, bytes); }
    let t = string(heap, units);
    return if heap.fault == Fault.None { intern(heap, t) } else { Value.Undefined };
}
// A new array of `values`.
fn new_array(heap: Heap&, values: const Value[:]) -> Value {
    let start = add_values(heap, values);
    if start < 0 { if heap.fault == Fault.None { memory(heap); } return Value.Undefined; }
    return allocate(heap, Node { kind: node_kind::Array, start: start as u32, count: values.len as u32, length: values.len as u32 });
}
// CreateDataProperty of a new key on a new object.
fn result_property(heap: Heap&, o: Value, name: const u8[:], v: Value) -> bool {
    let k = key_named(heap, name);
    return heap.fault == Fault.None && add_own(heap, o.id(), k, v, attrs_data());
}
// The match result of S's match by program p, its groups' offsets `caps`.
fn match_array(job: Job&, s: Value, p: program::Pattern, caps: const i64[:]) -> Value {
    let heap .= job.heap;
    let groups: i64 = p.captures;
    var values: Value[>..] = [];
    for g in groups {
        values.push(if caps[2 * g] < 0 { Value.Undefined } else { substring(heap, s, caps[2 * g], caps[2 * g + 1]) });
        if heap.fault != Fault.None { return Value.Undefined; }
    }
    let a = new_array(heap, values);
    if heap.fault != Fault.None || !result_property(heap, a, "index", number(caps[0])) || !result_property(heap, a, "input", s) { return Value.Undefined; }
    // The pairs of `d`: [start, end] arrays, undefined for unmatched groups.
    let indices = p.flags & regexp_flag::HasIndices != 0;
    var pairs: Value[>..] = [];
    if indices {
        for g in groups {
            if caps[2 * g] < 0 { pairs.push(Value.Undefined); continue; }
            let start = caps[2 * g] as f64; let end = caps[2 * g + 1] as f64;
            pairs.push(new_array(heap, [number(start), number(end)]));
            if heap.fault != Fault.None { return Value.Undefined; }
        }
    }
    // Named groups: one key per name in first-declaration order, the value
    // of the group that matched (duplicate names are in exclusive
    // alternatives, so at most one did). Each name's interned key row and
    // group, sorted, finds every name's first group without comparing
    // each name with every earlier one (one work unit a name).
    var keys:       Value[>..] = []; var named: Value[>..] = []; var named_pairs: Value[>..] = [];
    var group_keys: Value[>..] = []; var order: i64[>..] = [];
    let names: i64 = p.start + p.instructions + p.members;
    for g in groups {
        group_keys.push(Value.Undefined);
        if g == 0 || heap.code.pattern_ops[names + g - 1].c == 0 { continue; }
        let row = heap.code.pattern_ops[names + g - 1];
        let at: i64 = p.units + row.a; let n: i64 = row.b;
        let k = name_key(heap, heap.code.units[at..(at + n)]);
        if heap.fault != Fault.None || !charge(heap) { return Value.Undefined; }
        group_keys[g] = k; let sid: i64 = k.sid();
        order.push(sid * 4294967296 + g);
    }
    sort(order) { x, y => x < y };
    // first[g]: the first group named as g is; slot[g]: its key's position.
    var first: i64[>..] = []; first.resize(groups, -1); var slot: i64[>..] = []; slot.resize(groups, -1);
    for x, i in order { let g = x % 4294967296; first[g] = if i > 0 && order[i - 1] / 4294967296 == x / 4294967296 { first[order[i - 1] % 4294967296] } else { g }; }
    for g in 1..groups {
        if first[g] < 0 { continue; }
        if first[g] == g { slot[g] = keys.len; keys.push(group_keys[g]); named.push(values[g]); named_pairs.push(if indices { pairs[g] } else { Value.Undefined }); continue; }
        let j = slot[first[g]];
        if !values[g].is_undefined() { named[j] = values[g]; if indices { named_pairs[j] = pairs[g]; } }
    }
    var group_object = Value.Undefined; var pair_groups = Value.Undefined;
    let has_names = keys.len > 0;
    if has_names {
        group_object = allocate(heap, Node { kind: node_kind::Object, proto: Value.Null });
        if heap.fault != Fault.None { return Value.Undefined; }
        for k, j in keys { if !add_own(heap, group_object.id(), k, named[j], attrs_data()) { return Value.Undefined; } }
    }
    if !result_property(heap, a, "groups", group_object) { return Value.Undefined; }
    if indices {
        let pa = new_array(heap, pairs);
        if heap.fault != Fault.None { return Value.Undefined; }
        if has_names {
            pair_groups = allocate(heap, Node { kind: node_kind::Object, proto: Value.Null });
            if heap.fault != Fault.None { return Value.Undefined; }
            for k, j in keys { if !add_own(heap, pair_groups.id(), k, named_pairs[j], attrs_data()) { return Value.Undefined; } }
        }
        if !result_property(heap, pa, "groups", pair_groups) || !result_property(heap, a, "indices", pa) { return Value.Undefined; }
    }
    return a;
}
