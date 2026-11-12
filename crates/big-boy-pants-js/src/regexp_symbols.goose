import std;
import .source_vm;
import .realm;
import .array_methods;
import .array_callbacks;
import .function_calls;
import .continuations;
import .iterators;
import .collections;
import .regexp_object;
import .regexp_replace;

namespace source;

// String.prototype.match, matchAll and search, RegExp.prototype[@@match],
// [@@matchAll], [@@search] and [@@split], and %RegExpStringIteratorPrototype%
// (light-cljs-937, the sixth child of light-cljs-770; decisions 2, 4 and 5
// of docs/javascript-coverage.md; ECMAScript 22.1.3.13, .14, .23, 22.2.6.8,
// .9, .12, .14 and 22.2.9; the Pants engine's vm/native/string_regexp.rs,
// QuickJS's js_string_match, js_regexp_Symbol_match,
// js_regexp_Symbol_matchAll, js_regexp_string_iterator_next,
// js_regexp_Symbol_search and js_regexp_Symbol_split).
//
// Each runs as a RegExp native frame (Native.RegExp, its built-in the
// payload), as replace does (regexp_replace.goose): its phase (pc) names
// the one value it awaits on its operand top, a property read (a getter
// runs above the frame), a conversion (a Convert frame above it, the phase
// running again on the primitive), a call or construction, or a setter's
// return. A value that arrives without JavaScript is taken at once; a step
// chains at most replace_chain() phases and ends after every built-in
// search and every element a split appends, so budget cuts, limits and
// suspensions fall between two reads, conversions, calls, searches or
// elements. Conversions plan nothing (coerce.goose).
//
// match(regexp), matchAll(regexp) and search(regexp) of String.prototype:
// RequireObjectCoercible(O); a regexp neither undefined nor null has, for
// matchAll, IsRegExp (an object's @@match read, regexp_object.goose
// start_is_regexp) and when it is one Get(regexp, "flags"),
// RequireObjectCoercible and ToString, a TypeError without "g"; then
// GetMethod(regexp, @@match, @@matchAll or @@search) (GetV: a primitive's
// prototype is read, as V8 and the Pants engine do; QuickJS reads objects
// only), a method found called with O, its result the answer. Otherwise
// ToString(O), then RegExpCreate(regexp, undefined, or "g" for matchAll)
// as Construct(%RegExp%, ...) (both oracles construct, so IsRegExp and the
// RegExp's own reads run as the constructor's frame makes them), then
// Invoke(rx, the symbol, [S]). The realm's own symbol method answers in
// the frame's place: its frame replaces this one, at the same operand base
// (symbol_takeover), as String.prototype.split's @@split delegation
// (string_methods.goose) and replace's @@replace do.
//   receiver   O, then (from Created on) S
//   input      regexp
//   callee     (Invoke) the RegExp made
let dispatch_pc::Match = 0;     // matchAll: Get(regexp, @@match), IsRegExp's read
let dispatch_pc::Flags = 1;     // matchAll: Get(regexp, "flags"), then ToString
let dispatch_pc::Method = 2;    // GetMethod(regexp, the symbol)
let dispatch_pc::Delegate = 3;  // the method's result, the answer
let dispatch_pc::Text = 4;      // ToString(O)
let dispatch_pc::Created = 5;   // RegExpCreate's RegExp
let dispatch_pc::Invoke = 6;    // Get(rx, the symbol), then its call
//
// The symbol methods on any object rx (a TypeError otherwise), each first
// ToString(string):
//   @@match    Get(rx, "flags") and ToString; without "g" RegExpExec once,
//              its result the answer; with it Set(rx, "lastIndex", 0), then
//              RegExpExec until null, each result's ToString(Get(r, "0"))
//              appended to a new array (an empty one advancing lastIndex by
//              Get, ToLength, AdvanceStringIndex and Set, code points with
//              "u" or "v"); null when none was.
//   @@search   previousLastIndex = Get(rx, "lastIndex"), Set to 0 unless
//              SameValue(it, +0), RegExpExec, Get(rx, "lastIndex") again and
//              Set it back unless SameValue, then -1 or Get(result, "index").
//   @@matchAll SpeciesConstructor(rx, %RegExp%) (Get "constructor", then
//              @@species: undefined or null is %RegExp%, a non-constructor a
//              TypeError), ToString(Get(rx, "flags")), Construct(C, [rx,
//              flags]) the matcher, Set(matcher, "lastIndex",
//              ToLength(Get(rx, "lastIndex"))), and a new RegExp String
//              Iterator over the matcher and S.
//   @@split    SpeciesConstructor and flags as @@matchAll, Construct(C, [rx,
//              flags with "y"]) the splitter, ToUint32(limit) (undefined:
//              2^32 - 1; 0 answers []), then the sticky scan: an empty S is
//              [] when the splitter matches it, else [S]; otherwise for each
//              position q, Set(splitter, "lastIndex", q) and RegExpExec; a
//              match whose ToLength(Get(splitter, "lastIndex")), at most
//              |S|, is past the last piece's end p appends S[p, q), moves p
//              there and appends each capture Get(z, i) for i below
//              ToLength(Get(z, "length")); the limit stops it after any
//              element; finally S[p, |S|).
// RegExpExec(R, S) (22.2.7.1), as @@replace runs it: Get(R, "exec"), a
// callable one other than the realm's exec called with S (an object or null
// required), otherwise R's [[RegExpMatcher]] (a TypeError without one)
// after ToLength(Get(R, "lastIndex")) (regexp_object.goose builtin_exec).
//   receiver   R: rx, the splitter (from Created on), the iterator's matcher
//   input      the argument, then (from Input's conversion on) S
//   callee     split: the limit, then (from SplitLoop on) lim, a number;
//              matchAll: from Created the matcher; next: the iterator
//   output     the allowance spent, a number (regexp_replace.goose replace_spend)
//   index      symbol_flag:: bits from the flags (next: the iterator's)
//   argument_count  the elements appended to the result
//   operands   sym_registers() values (sym_reg::), then the awaited value
let sym_pc::Input = 0;          // ToString(string)
let sym_pc::Constructor = 1;    // Get(rx, "constructor")
let sym_pc::Species = 2;        // Get(C, @@species)
let sym_pc::Flags = 3;          // Get(rx, "flags"), then ToString
let sym_pc::Created = 4;        // Construct(C, [rx, flags])'s object
let sym_pc::CopyIndex = 5;      // matchAll: Get(rx, "lastIndex"), then ToLength
let sym_pc::CopySet = 6;        // matchAll: the matcher's lastIndex setter's return
let sym_pc::Reset = 7;          // match "g": the lastIndex setter's return
let sym_pc::Exec = 8;           // Get(R, "exec")
let sym_pc::ExecResult = 9;     // the exec's result (a search's, or a user exec's)
let sym_pc::LastIndex = 10;     // Get(R, "lastIndex"), then ToLength and the search
let sym_pc::MatchText = 11;     // match, next with "g": Get(result, "0"), then ToString
let sym_pc::Advance = 12;       // an empty match: Get(R, "lastIndex"), then ToLength
let sym_pc::AdvanceSet = 13;    // the lastIndex setter's return
let sym_pc::SearchSaved = 14;   // search: Get(rx, "lastIndex"), previousLastIndex
let sym_pc::SearchZero = 15;    // search: the lastIndex setter's return
let sym_pc::SearchCurrent = 16; // search: Get(rx, "lastIndex") after the exec
let sym_pc::SearchRestore = 17; // search: the restoring setter's return
let sym_pc::SearchIndex = 18;   // search: Get(result, "index"), the answer
let sym_pc::SplitLimit = 19;    // split: the limit, then ToUint32
let sym_pc::SplitLoop = 20;     // split: no value; the next position
let sym_pc::SplitSet = 21;      // split: the splitter's lastIndex setter's return
let sym_pc::SplitEnd = 22;      // split: Get(splitter, "lastIndex"), then ToLength
let sym_pc::SplitLength = 23;   // split: Get(z, "length"), then ToLength
let sym_pc::SplitCapture = 24;  // split: Get(z, i)
// The frame's registers, at its operand base.
let sym_reg::Out = 0;           // match, split: the result; search: previousLastIndex; matchAll, split before Created: C
let sym_reg::Result = 1;        // search, split, next: the exec result in use
let sym_reg::P = 2;             // split: the end of the last piece, a number
let sym_reg::Q = 3;             // split: the position tried, a number
let sym_reg::Captures = 4;      // split: the result's capture count, a number
let sym_reg::Capture = 5;       // split: the next capture, a number
inline fn sym_registers() -> i64 { return 6; }
//
// A RegExp String Iterator (22.2.9) is a node_kind::Iterator row of
// iteration::RegExpString (iterators.goose), whose internal slots are its
// fields (decision 2): `value` the matcher, `namespace_value` S, `length`
// its symbol_flag:: bits; once done, `value` is undefined and
// `namespace_value` null, so a done iterator holds nothing. Its prototype
// is %RegExpStringIteratorPrototype% (a fixed realm row inheriting
// %IteratorPrototype%). `next` on one the job did not allocate faults
// NativeBoundary before it runs anything, as the Map and Set iterators'
// does (decision 3); a done one answers {undefined, true} without a frame.
// Otherwise its frame holds the matcher as receiver, S as input and the
// iterator as callee, and runs RegExpExec once: null, or any result without
// "g", ends the iterator; with "g" an empty match advances the matcher's
// lastIndex first.
//
// Work. Each operation has the Pants engine's allowance, max_replace_work()
// units (its MAX_WORK): one an exec, and one and a string's units for each
// element @@match or @@split appends (its pieces and captures, whatever
// they are; sym_append). Past it, the catchable RangeError "RegExp symbol
// work limit exceeded" (QuickJS has no such limit and keeps going), unless
// the job's remaining work runs out first (WorkLimit); the units are also
// charged to the job's work.

// Whether built-in b is one of these (regexp_object.goose regexp_native).
inline fn string_symbol(b: Builtin) -> bool { return b == Builtin.StringPrototypeMatch || b == Builtin.StringPrototypeMatchAll || b == Builtin.StringPrototypeSearch; }
fn symbols_native(b: Builtin) -> bool {
    return string_symbol(b) || b == Builtin.RegExpPrototypeMatch || b == Builtin.RegExpPrototypeMatchAll || b == Builtin.RegExpPrototypeSearch || b == Builtin.RegExpPrototypeSplit || b == Builtin.RegExpStringIteratorPrototypeNext;
}
// String method b's well-known symbol and the realm's method under it.
fn dispatch_symbol(b: Builtin) -> Value {
    let s = if b == Builtin.StringPrototypeMatch { well_known::Match } else if b == Builtin.StringPrototypeMatchAll { well_known::MatchAll } else { well_known::Search };
    return Value.Symbol { s: s as u32 };
}
fn dispatch_method(b: Builtin) -> Builtin { return if b == Builtin.StringPrototypeMatch { Builtin.RegExpPrototypeMatch } else if b == Builtin.StringPrototypeMatchAll { Builtin.RegExpPrototypeMatchAll } else { Builtin.RegExpPrototypeSearch }; }
// Whether v is a RegExp String Iterator.
fn regexp_string_cursor(heap: const Heap&, v: Value) -> bool { return v.is_object() && v.id() < heap.nodes.len && node(heap, v.id()).kind == node_kind::Iterator && node(heap, v.id()).function == iteration::RegExpString; }

// Start built-in b (symbols_native) with `receiver` and `args`, its operand
// base `base` (native_start's contract).
fn start_symbols(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let o = form_value(heap, receiver); let first = form_value(heap, arg(args, 0)); let second = form_value(heap, arg(args, 1));
    if string_symbol(b) { start_dispatch(job, b, o, first, base); return; }
    if b == Builtin.RegExpStringIteratorPrototypeNext { start_next(job, o, base); return; }
    let at = push_symbol(job, b, o, first, second, base);
    if at >= 0 { symbols_run(job, at, b); }
}
// One step of the frame at the top (regexp_object.goose regexp_step).
fn symbols_step(job: Job&, b: Builtin) { symbols_run(job, job.frames.len - 1, b); }
// Run the frame at `at` while it stays at the top and its values arrive
// without JavaScript, at most replace_chain() phases, until one ends the step.
fn symbols_run(job: Job&, at: i64, b: Builtin) {
    var n: i64 = 0;
    while job.heap.fault == Fault.None && job.frames.len - 1 == at && n < replace_chain() {
        let done = if string_symbol(b) { dispatch_phase(job, at, b) } else { symbol_phase(job, at, b) };
        if done { return; }
        n += 1;
    }
}

// ---- String.prototype.match, matchAll and search
fn start_dispatch(job: Job&, b: Builtin, o: Value, regexp: Value, base: i64) {
    let heap .= job.heap;
    if o.tag() <= value_tag::Null { type_error(job, if b == Builtin.StringPrototypeMatch { "String.prototype.match called on null or undefined" } else if b == Builtin.StringPrototypeMatchAll { "String.prototype.matchAll called on null or undefined" } else { "String.prototype.search called on null or undefined" }); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: dispatch_pc::Method, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: o, input: regexp, native: Native.RegExp { which: b } });
    let at = job.frames.len - 1;
    if regexp.tag() <= value_tag::Null { job.frames[at].pc = dispatch_pc::Text; push_operand(job, o); }
    else if b == Builtin.StringPrototypeMatchAll && regexp.is_object() { job.frames[at].pc = dispatch_pc::Match; start_is_regexp(job, b, regexp); }
    else { get_any(job, b, regexp, dispatch_symbol(b)); }
    symbols_run(job, at, b);
}
// The method's TypeError when it is no function.
fn dispatch_not_callable(job: Job&, b: Builtin) { type_error(job, if b == Builtin.StringPrototypeMatch { "Symbol.match is not a function" } else if b == Builtin.StringPrototypeMatchAll { "Symbol.matchAll is not a function" } else { "Symbol.search is not a function" }); }
// One phase of the String frame at `at`: true when the step ends.
fn dispatch_phase(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap; let f = job.frames[at];
    if job.operands.len != f.operand_base + 1 { heap.fault = Fault.InvalidState; return true; }
    match f.pc {
        dispatch_pc::Match => {
            let m = job.operands.pop();
            if is_regexp_answer(heap, f.input, m) { job.frames[at].pc = dispatch_pc::Flags; get_named(job, b, f.input, "flags"); }
            else { job.frames[at].pc = dispatch_pc::Method; get_any(job, b, f.input, dispatch_symbol(b)); }
        },
        dispatch_pc::Flags => {
            if form_value(heap, job.operands[job.operands.len - 1]).tag() <= value_tag::Null { type_error(job, "RegExp flags are undefined or null"); return true; }
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            if !has_unit(heap, s, 103) { type_error(job, "matchAll requires a global RegExp"); return true; }
            job.frames[at].pc = dispatch_pc::Method;
            get_any(job, b, f.input, dispatch_symbol(b));
        },
        dispatch_pc::Method => {
            let m = form_value(heap, job.operands.pop());
            if m.tag() <= value_tag::Null { job.frames[at].pc = dispatch_pc::Text; push_operand(job, f.receiver); return false; }
            if !is_callable(heap, m) { dispatch_not_callable(job, b); return true; }
            if f.input.is_object() && builtin_is(heap, m, dispatch_method(b)) { symbol_takeover(job, at, dispatch_method(b), f.input, f.receiver, Value.Undefined); return true; }
            job.frames[at].pc = dispatch_pc::Delegate;
            call_accessor(job, m, f.input, [f.receiver]);
        },
        dispatch_pc::Delegate => { finish_native(job, f, job.operands.pop()); return true; },
        dispatch_pc::Text => {
            let ready, s = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            job.frames[at].receiver = s; job.frames[at].pc = dispatch_pc::Created;
            let flags = if b == Builtin.StringPrototypeMatchAll { key_named(heap, "g") } else { Value.Undefined };
            if heap.fault != Fault.None || !push_operand(job, object(regexp_row())) || !push_operand(job, f.input) || !push_operand(job, flags) { return true; }
            construct(job, 2);
        },
        dispatch_pc::Created => {
            let rx = job.operands.pop();
            if !rx.is_object() || !f.receiver.is_string() { heap.fault = Fault.InvalidState; return true; }
            job.frames[at].callee = rx; job.frames[at].pc = dispatch_pc::Invoke;
            get_key(job, b, rx, dispatch_symbol(b));
        },
        // Invoke's Call: a value that is no function is its TypeError.
        dispatch_pc::Invoke => {
            let m = form_value(heap, job.operands.pop());
            if !f.callee.is_object() || !f.receiver.is_string() { heap.fault = Fault.InvalidState; return true; }
            if !is_callable(heap, m) { dispatch_not_callable(job, b); return true; }
            if builtin_is(heap, m, dispatch_method(b)) { symbol_takeover(job, at, dispatch_method(b), f.callee, f.receiver, Value.Undefined); return true; }
            job.frames[at].pc = dispatch_pc::Delegate;
            call_accessor(job, m, f.callee, [f.receiver]);
        },
        _ => { heap.fault = Fault.InvalidState; return true; },
    }
    return false;
}
// The realm's symbol method `which` answers for the frame at `at` (the top):
// its frame, called on rx with `string` (and split's `limit`), replaces it
// at the same operand base and runs from the next step.
fn symbol_takeover(job: Job&, at: i64, which: Builtin, rx: Value, string: Value, limit: Value) {
    let base = job.frames[at].operand_base;
    job.frames.pop(); job.operands.resize(base);
    push_symbol(job, which, rx, string, limit, base);
}

// ---- The symbol methods
// The frame of symbol method b on rx with the argument `string` (and
// split's `limit`), operand base `base`, awaiting the argument's ToString:
// its row, or -1 with the fault.
fn push_symbol(job: Job&, b: Builtin, rx: Value, string: Value, limit: Value, base: i64) -> i64 {
    if !rx.is_object() {
        type_error(job, match b { RegExpPrototypeMatch => "RegExp.prototype[Symbol.match] called on a non-object", RegExpPrototypeMatchAll => "RegExp.prototype[Symbol.matchAll] called on a non-object", RegExpPrototypeSearch => "RegExp.prototype[Symbol.search] called on a non-object", _ => "RegExp.prototype[Symbol.split] called on a non-object" });
        return -1;
    }
    let at = symbol_frame(job, b, rx, string, limit, base);
    if at < 0 || !push_operand(job, string) { return -1; }
    return at;
}
// A symbol frame in phase Input with its registers, awaiting nothing yet:
// its row, or -1 with the fault.
fn symbol_frame(job: Job&, b: Builtin, rx: Value, input: Value, callee: Value, base: i64) -> i64 {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return -1; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: sym_pc::Input, local_base: job.locals.len, operand_base: base, callee: callee, receiver: rx, input: input, output: number(0), native: Native.RegExp { which: b } });
    for r in sym_registers() { if !push_operand(job, if r >= sym_reg::P { number(0) } else { Value.Undefined }) { return -1; } }
    return job.frames.len - 1;
}
// Register r of the frame at `at`, a value.
inline fn sym_get(job: const Job&, at: i64, r: i64) -> Value { return job.operands[job.frames[at].operand_base + r]; }
inline fn sym_put(job: Job&, at: i64, r: i64, v: Value) { job.operands[job.frames[at].operand_base + r] = v; }
// RegExpExec's first read: Get(R, "exec"), after spending the exec's unit.
fn sym_ask_exec(job: Job&, at: i64, b: Builtin) {
    if !replace_spend(job, at, 1) { return; }
    job.frames[at].pc = sym_pc::Exec;
    get_named(job, b, job.frames[at].receiver, "exec");
}
// Set(o, "lastIndex", v, true) for the frame at the top, its phase `pc`
// taking the setter's return: true when it now runs above the frame;
// false when written, or with the fault.
fn put_index(job: Job&, at: i64, b: Builtin, o: Value, v: Value, pc: i64) -> bool {
    job.frames[at].pc = pc;
    return put_last_index_value(job, b, o, v);
}
// Append v to the frame's result (CreateDataPropertyOrThrow on the new
// array it alone holds), its unit and a string's units spent first, as
// the Pants engine's State::push spends every element @@match and @@split
// append: false with the fault.
fn sym_append(job: Job&, at: i64, v: Value) -> bool {
    let heap .= job.heap; let a = sym_get(job, at, sym_reg::Out);
    let n = job.frames[at].argument_count;
    if !a.is_object() || node(heap, a.id()).kind != node_kind::Array || n < 0 || n >= 4294967295 { heap.fault = Fault.InvalidState; return false; }
    if !replace_spend(job, at, 1 + str_len(heap, v)) || !set_element(heap, a, n, v) { return false; }
    job.frames[at].argument_count = n + 1;
    return true;
}
// Whether the frame's result has reached split's limit, a number in callee.
inline fn split_full(job: const Job&, at: i64) -> bool { return (job.frames[at].argument_count as f64) >= job.frames[at].callee.num(); }
// The answer of the frame at `at`.
fn sym_answer(job: Job&, at: i64, v: Value) { if job.heap.fault == Fault.None { finish_native(job, job.frames[at], v); } }
// One phase of the symbol frame at `at`: true when the step ends.
fn symbol_phase(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap; let f = job.frames[at];
    let top = f.operand_base + sym_registers();
    if !f.output.is_number() || !f.receiver.is_object() || f.index < 0 || f.index > (symbol_flag::Global | symbol_flag::Unicode) || f.argument_count < 0 || job.operands.len < top { heap.fault = Fault.InvalidState; return true; }
    if f.pc > sym_pc::Input && !f.input.is_string() { heap.fault = Fault.InvalidState; return true; }
    if f.pc == sym_pc::SplitLoop { if job.operands.len != top { heap.fault = Fault.InvalidState; return true; } return split_loop(job, at, b); }
    if job.operands.len != top + 1 { heap.fault = Fault.InvalidState; return true; }
    let rx = f.receiver; let s = f.input;
    match f.pc {
        sym_pc::Input => {
            let ready, text = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            job.frames[at].input = text;
            if b == Builtin.RegExpPrototypeMatch { job.frames[at].pc = sym_pc::Flags; get_named(job, b, rx, "flags"); }
            else if b == Builtin.RegExpPrototypeSearch { job.frames[at].pc = sym_pc::SearchSaved; ask_last_index(job, b, rx); }
            else if b == Builtin.RegExpPrototypeMatchAll || b == Builtin.RegExpPrototypeSplit { job.frames[at].pc = sym_pc::Constructor; get_named(job, b, rx, "constructor"); }
            else { heap.fault = Fault.InvalidState; return true; }
        },
        // SpeciesConstructor(rx, %RegExp%).
        sym_pc::Constructor => {
            let c = form_value(heap, job.operands.pop());
            if c.is_undefined() { sym_put(job, at, sym_reg::Out, object(regexp_row())); sym_ask_flags(job, at, b); return false; }
            if !c.is_object() { type_error(job, "RegExp constructor is not an object"); return true; }
            job.frames[at].pc = sym_pc::Species;
            get_key(job, b, c, Value.Symbol { s: well_known::Species });
        },
        sym_pc::Species => {
            let c = form_value(heap, job.operands.pop());
            if c.tag() <= value_tag::Null { sym_put(job, at, sym_reg::Out, object(regexp_row())); }
            else if !is_constructor(heap, c) { type_error(job, "RegExp species is not a constructor"); return true; }
            else { sym_put(job, at, sym_reg::Out, c); }
            sym_ask_flags(job, at, b);
        },
        sym_pc::Flags => {
            let ready, flags = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            let bits = (if has_unit(heap, flags, 103) { symbol_flag::Global } else { 0 }) | (if has_unit(heap, flags, 117) || has_unit(heap, flags, 118) { symbol_flag::Unicode } else { 0 });
            job.frames[at].index = bits;
            if b == Builtin.RegExpPrototypeMatch {
                if bits & symbol_flag::Global == 0 { sym_ask_exec(job, at, b); return false; }
                let a = new_array(heap, []);
                if heap.fault != Fault.None { return true; }
                sym_put(job, at, sym_reg::Out, a);
                if !put_index(job, at, b, rx, number(0), sym_pc::Reset) && heap.fault == Fault.None { sym_ask_exec(job, at, b); }
                return false;
            }
            // Construct(C, [rx, flags]), split's flags with "y".
            let c = sym_get(job, at, sym_reg::Out);
            if !c.is_object() { heap.fault = Fault.InvalidState; return true; }
            var given = flags;
            if b == Builtin.RegExpPrototypeSplit && !has_unit(heap, flags, 121) {
                if !charge(heap, str_len(heap, flags) + 1) { return true; }
                let y = key_named(heap, "y");
                if heap.fault != Fault.None { return true; }
                given = concat_pieces(job, [flags, y]);
                if heap.fault != Fault.None { return true; }
            }
            sym_put(job, at, sym_reg::Out, Value.Undefined);
            job.frames[at].pc = sym_pc::Created;
            if !push_operand(job, c) || !push_operand(job, rx) || !push_operand(job, given) { return true; }
            construct(job, 2);
        },
        sym_pc::Created => {
            let m = job.operands.pop();
            if !m.is_object() { heap.fault = Fault.InvalidState; return true; }
            if b == Builtin.RegExpPrototypeMatchAll { job.frames[at].callee = m; job.frames[at].pc = sym_pc::CopyIndex; ask_last_index(job, b, rx); return false; }
            let a = new_array(heap, []);
            if heap.fault != Fault.None { return true; }
            sym_put(job, at, sym_reg::Out, a);
            job.frames[at].receiver = m; job.frames[at].pc = sym_pc::SplitLimit;
            push_operand(job, f.callee);
        },
        sym_pc::CopyIndex => {
            let last, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            if !f.callee.is_object() { heap.fault = Fault.InvalidState; return true; }
            if !put_index(job, at, b, f.callee, number(last), sym_pc::CopySet) && heap.fault == Fault.None { sym_iterator(job, at); return true; }
        },
        sym_pc::CopySet => { job.operands.pop(); sym_iterator(job, at); return true; },
        sym_pc::Reset, sym_pc::SplitSet, sym_pc::SearchZero => { job.operands.pop(); sym_ask_exec(job, at, b); },
        sym_pc::Exec => {
            let e = form_value(heap, job.operands.pop());
            if is_callable(heap, e) && !builtin_exec_function(heap, e) {
                job.frames[at].pc = sym_pc::ExecResult;
                call_accessor(job, e, rx, [s]);
                return false;
            }
            if !is_regexp_object(heap, rx) { type_error(job, "RegExp exec method called on an object that is not a RegExp"); return true; }
            job.frames[at].pc = sym_pc::LastIndex;
            ask_last_index(job, b, rx);
        },
        sym_pc::LastIndex => {
            let last, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            if !is_regexp_object(heap, rx) { heap.fault = Fault.InvalidState; return true; }
            let r = builtin_exec(job, b, rx, s, last);
            if heap.fault != Fault.None { return true; }
            job.frames[at].pc = sym_pc::ExecResult;
            push_operand(job, r);
            return true;
        },
        sym_pc::ExecResult => {
            let r = form_value(heap, job.operands.pop());
            if !r.is_null() && !r.is_object() { type_error(job, "RegExp exec method returned something other than an object or null"); return true; }
            return exec_result(job, at, b, r);
        },
        sym_pc::MatchText => {
            let ready, text = regexp_text(job, b);
            if !ready || heap.fault != Fault.None { return true; }
            if b == Builtin.RegExpPrototypeMatch && !sym_append(job, at, text) { return true; }
            if str_len(heap, text) > 0 {
                if b == Builtin.RegExpStringIteratorPrototypeNext { next_answer(job, at, sym_get(job, at, sym_reg::Result), false); return true; }
                sym_ask_exec(job, at, b);
                return false;
            }
            job.frames[at].pc = sym_pc::Advance;
            ask_last_index(job, b, rx);
        },
        sym_pc::Advance => {
            let last, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            let next = advance_string_index(heap, s, last, f.index & symbol_flag::Unicode != 0);
            if put_index(job, at, b, rx, number(next), sym_pc::AdvanceSet) || heap.fault != Fault.None { return true; }
            return after_advance(job, at, b);
        },
        sym_pc::AdvanceSet => { job.operands.pop(); return after_advance(job, at, b); },
        sym_pc::SearchSaved => {
            let previous = job.operands.pop();
            sym_put(job, at, sym_reg::Out, previous);
            if same_value(heap, previous, number(0)) { sym_ask_exec(job, at, b); return false; }
            if !put_index(job, at, b, rx, number(0), sym_pc::SearchZero) && heap.fault == Fault.None { sym_ask_exec(job, at, b); }
        },
        sym_pc::SearchCurrent => {
            let current = job.operands.pop(); let previous = sym_get(job, at, sym_reg::Out);
            if same_value(heap, current, previous) { search_answer(job, at, b); return false; }
            if !put_index(job, at, b, rx, previous, sym_pc::SearchRestore) && heap.fault == Fault.None { search_answer(job, at, b); }
        },
        sym_pc::SearchRestore => { job.operands.pop(); search_answer(job, at, b); },
        sym_pc::SearchIndex => { sym_answer(job, at, job.operands.pop()); return true; },
        sym_pc::SplitLimit => {
            var lim: f64 = 4294967295;
            if f.callee.is_undefined() { job.operands.pop(); }
            else {
                let n, done = number_value(job, b);
                if !done || heap.fault != Fault.None { return true; }
                lim = to_uint32(n);
            }
            job.frames[at].callee = number(lim);
            if lim == 0 { sym_answer(job, at, sym_get(job, at, sym_reg::Out)); return true; }
            if str_len(heap, s) == 0 { sym_ask_exec(job, at, b); return false; }
            sym_put(job, at, sym_reg::P, number(0)); sym_put(job, at, sym_reg::Q, number(0));
            job.frames[at].pc = sym_pc::SplitLoop;
        },
        sym_pc::SplitEnd => {
            let last, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            let p = sym_number(job, at, sym_reg::P); let q = sym_number(job, at, sym_reg::Q);
            let size = str_len(heap, s);
            if heap.fault != Fault.None || p < 0 || p > q || q >= size { heap.fault = Fault.InvalidState; return true; }
            let e = min(last, size);
            if e == p {
                let next: f64 = advance_string_index(heap, s, q as f64, f.index & symbol_flag::Unicode != 0); sym_put(job, at, sym_reg::Q, number(next));
                job.frames[at].pc = sym_pc::SplitLoop;
                return false;
            }
            let piece = substring(heap, s, p, q);
            if heap.fault != Fault.None || !sym_append(job, at, piece) { return true; }
            if split_full(job, at) { sym_answer(job, at, sym_get(job, at, sym_reg::Out)); return true; }
            sym_put(job, at, sym_reg::P, number(e));
            job.frames[at].pc = sym_pc::SplitLength;
            get_named(job, b, sym_get(job, at, sym_reg::Result), "length");
            return true;
        },
        sym_pc::SplitLength => {
            let n, done = length_value(job, b);
            if !done || heap.fault != Fault.None { return true; }
            sym_put(job, at, sym_reg::Captures, number(if n >= 1 { n - 1 } else { 0 }));
            sym_put(job, at, sym_reg::Capture, number(1));
            return split_capture(job, at, b);
        },
        sym_pc::SplitCapture => {
            let v = job.operands.pop();
            let i = sym_number(job, at, sym_reg::Capture);
            if heap.fault != Fault.None || !sym_append(job, at, v) { return true; }
            if split_full(job, at) { sym_answer(job, at, sym_get(job, at, sym_reg::Out)); return true; }
            sym_put(job, at, sym_reg::Capture, number(i + 1));
            split_capture(job, at, b);
            return true;
        },
        _ => { heap.fault = Fault.InvalidState; return true; },
    }
    return false;
}
// Register r of the frame at `at` as a number, -1 (with InvalidState) when
// it holds none.
fn sym_number(job: Job&, at: i64, r: i64) -> i64 {
    let v = sym_get(job, at, r);
    if !v.is_number() || !(v.num() >= 0) { job.heap.fault = Fault.InvalidState; return -1; }
    return v.num() as i64;
}
fn sym_ask_flags(job: Job&, at: i64, b: Builtin) { job.frames[at].pc = sym_pc::Flags; get_named(job, b, job.frames[at].receiver, "flags"); }
// RegExpExec's result r (null or an object) for built-in b's frame at `at`.
fn exec_result(job: Job&, at: i64, b: Builtin, r: Value) -> bool {
    let heap .= job.heap; let f = job.frames[at];
    let global = f.index & symbol_flag::Global != 0;
    match b {
        RegExpPrototypeMatch => {
            if r.is_null() { sym_answer(job, at, if global && f.argument_count > 0 { sym_get(job, at, sym_reg::Out) } else { Value.Null }); return true; }
            if !global { sym_answer(job, at, r); return true; }
            sym_ask_text(job, at, b, r);
        },
        RegExpPrototypeSearch => {
            sym_put(job, at, sym_reg::Result, r);
            job.frames[at].pc = sym_pc::SearchCurrent;
            ask_last_index(job, b, f.receiver);
        },
        RegExpPrototypeSplit => {
            let size = str_len(heap, f.input);
            if size == 0 {
                if r.is_null() && !sym_append(job, at, f.input) { return true; }
                sym_answer(job, at, sym_get(job, at, sym_reg::Out));
                return true;
            }
            if r.is_null() {
                let q = sym_number(job, at, sym_reg::Q);
                if heap.fault != Fault.None || q >= size { heap.fault = Fault.InvalidState; return true; }
                let next: f64 = advance_string_index(heap, f.input, q as f64, f.index & symbol_flag::Unicode != 0); sym_put(job, at, sym_reg::Q, number(next));
                job.frames[at].pc = sym_pc::SplitLoop;
                return false;
            }
            sym_put(job, at, sym_reg::Result, r);
            job.frames[at].pc = sym_pc::SplitEnd;
            ask_last_index(job, b, f.receiver);
        },
        RegExpStringIteratorPrototypeNext => {
            if r.is_null() { next_answer(job, at, Value.Undefined, true); return true; }
            if !global { next_answer(job, at, r, false); return true; }
            sym_put(job, at, sym_reg::Result, r);
            sym_ask_text(job, at, b, r);
        },
        _ => { heap.fault = Fault.InvalidState; return true; },
    }
    return false;
}
// Get(r, "0"), then its ToString (MatchText).
fn sym_ask_text(job: Job&, at: i64, b: Builtin, r: Value) {
    let k = index_key(job.heap, 0);
    if job.heap.fault != Fault.None { return; }
    job.frames[at].pc = sym_pc::MatchText;
    get_key(job, b, r, k);
}
// After an empty match's advance: @@match's next exec, or next's answer.
fn after_advance(job: Job&, at: i64, b: Builtin) -> bool {
    if b == Builtin.RegExpStringIteratorPrototypeNext { next_answer(job, at, sym_get(job, at, sym_reg::Result), false); return true; }
    sym_ask_exec(job, at, b);
    return false;
}
// @@search's answer: -1 for no result, else Get(result, "index").
fn search_answer(job: Job&, at: i64, b: Builtin) {
    let r = sym_get(job, at, sym_reg::Result);
    if r.is_null() { sym_answer(job, at, number(-1)); return; }
    if !r.is_object() { job.heap.fault = Fault.InvalidState; return; }
    job.frames[at].pc = sym_pc::SearchIndex;
    get_named(job, b, r, "index");
}
// @@split's next position q: Set(splitter, "lastIndex", q) and RegExpExec,
// or past the end the last piece and the answer.
fn split_loop(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap; let f = job.frames[at];
    let p = sym_number(job, at, sym_reg::P); let q = sym_number(job, at, sym_reg::Q);
    let size = str_len(heap, f.input);
    if heap.fault != Fault.None || p > q || q > size || !f.callee.is_number() { heap.fault = Fault.InvalidState; return true; }
    if q < size {
        if !put_index(job, at, b, f.receiver, number(q), sym_pc::SplitSet) && heap.fault == Fault.None { sym_ask_exec(job, at, b); }
        return false;
    }
    let piece = substring(heap, f.input, p, size);
    if heap.fault != Fault.None || !sym_append(job, at, piece) { return true; }
    sym_answer(job, at, sym_get(job, at, sym_reg::Out));
    return true;
}
// Capture i of the result in use, or after the last the next position (p).
fn split_capture(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap;
    let i = sym_number(job, at, sym_reg::Capture); let n = sym_get(job, at, sym_reg::Captures);
    if heap.fault != Fault.None || !n.is_number() { heap.fault = Fault.InvalidState; return true; }
    if (i as f64) > n.num() {
        sym_put(job, at, sym_reg::Q, sym_get(job, at, sym_reg::P));
        job.frames[at].pc = sym_pc::SplitLoop;
        return false;
    }
    let k = index_key(heap, i);
    if heap.fault != Fault.None { return true; }
    job.frames[at].pc = sym_pc::SplitCapture;
    get_key(job, b, sym_get(job, at, sym_reg::Result), k);
    return false;
}
// @@matchAll's answer: a new RegExp String Iterator over the matcher (callee) and S.
fn sym_iterator(job: Job&, at: i64) {
    let heap .= job.heap; let f = job.frames[at];
    if !f.callee.is_object() || !f.input.is_string() { heap.fault = Fault.InvalidState; return; }
    let it = allocate(heap, Node { kind: node_kind::Iterator, function: iteration::RegExpString, length: f.index as u32, value: f.callee, namespace_value: f.input, proto: object(regexp_string_iterator_prototype_row()) });
    sym_answer(job, at, it);
}

// ---- %RegExpStringIteratorPrototype%.next
fn start_next(job: Job&, c: Value, base: i64) {
    let heap .= job.heap; let b = Builtin.RegExpStringIteratorPrototypeNext;
    if !regexp_string_cursor(heap, c) { type_error(job, "next requires a RegExp String Iterator"); return; }
    let n = node(heap, c.id());
    if n.value.is_undefined() {
        let r = iter_result(job, Value.Undefined, true);
        if heap.fault == Fault.None { job.operands.resize(base); deliver(job, r); }
        return;
    }
    if !writable(job, c.id()) { image_write(job, Value.Undefined, "an iterator's position"); return; }
    if !n.value.is_object() || !n.namespace_value.is_string() || n.length > (symbol_flag::Global | symbol_flag::Unicode) { heap.fault = Fault.InvalidState; return; }
    let at = symbol_frame(job, b, n.value, n.namespace_value, c, base);
    if at < 0 { return; }
    job.frames[at].index = n.length;
    sym_ask_exec(job, at, b);
    symbols_run(job, at, b);
}
// next's answer {v, done}; done, or a match without "g", ends the iterator.
fn next_answer(job: Job&, at: i64, v: Value, done: bool) {
    let heap .= job.heap; let f = job.frames[at];
    if !regexp_string_cursor(heap, f.callee) { heap.fault = Fault.InvalidState; return; }
    if done || f.index & symbol_flag::Global == 0 {
        if !writable(job, f.callee.id()) { image_write(job, Value.Undefined, "an iterator's position"); return; }
        node_mut(heap, f.callee.id()).value = Value.Undefined; node_mut(heap, f.callee.id()).namespace_value = Value.Null;
    }
    let r = iter_result(job, v, done);
    sym_answer(job, at, r);
}
