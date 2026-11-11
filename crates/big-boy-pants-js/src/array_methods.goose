import std;
import math;
import .source_vm;
import .realm;
import .array_callbacks;
import .function_calls;
import .regexp_object;

namespace source;

// Array.prototype.push, join and toString (light-cljs-842): built-ins that
// run user code (decision 5, rule 1 of docs/javascript-coverage.md, after
// light-cljs-789's template in array_callbacks.goose). Semantics from the
// Pants engine's rust/src/vm/native.rs (NativeState::Push, ArrayJoin,
// ArrayToString) and QuickJS's js_array_push, js_array_join and
// js_array_toString, with the specification's steps (ECMAScript 23.1.3.23,
// 23.1.3.18 and 23.1.3.36).
//
// Each is a native frame (Native.ArrayMethod, its Builtin the payload) with
// its state in the frame's fields and its scratch on the operand stack above
// its operand base; it never calls the interpreter. A step that can run
// JavaScript (a `length` getter, ToLength's valueOf, an element getter,
// ToString of an object element or separator, an index or `length` setter,
// a `join` getter, the join call) enters that function's frame, or a Convert
// frame (light-cljs-828), above this one, sets the phase (`pc`) that
// consumes its result and returns to the dispatch loop. A step writes or
// reads at most one element, so every budget cut, limit and suspension
// falls between them.
//
// push(...items)    input O = ToObject(this); the items at
//                   operands[operand_base, operand_base + argument_count);
//                   output len, ToLength(Get(O, "length")), as a number;
//                   index i, the next item.
//   pc 0  awaits the length's value (a getter's result, or converted by a
//         Convert frame with hint number)
//   pc 1  Set(O, len + i, item i, true), then Set(O, "length", len + n, true)
//   pc 2  awaits an index setter's return (dropped)
//   pc 3  awaits the `length` setter's return (dropped); the result is len + n
// A dense array (node_kind::Array in element-run mode) the job may write, with the
// realm's prototype chain, appends to its run at the call: Array.prototype
// and Object.prototype are realm rows a job cannot give an index accessor
// (no definition built-in reaches them yet, light-cljs-766), so Set of a new
// index there is the run's store, as an assignment's (assign) is.
//
// join(separator)   input O = ToObject(this); receiver the separator, then
//                   its ToString; argument_count len; index k, the next
//                   element; output the code units collected, as a number;
//                   the pieces, separators and element strings, none empty,
//                   at operands[operand_base..].
//   pc 0  awaits the length's value
//   pc 1  converts the separator (undefined gives ",")
//   pc 2  awaits the separator's ToString from a Convert frame
//   pc 3  visits element k: the separator before it, then Get(O, k)
//   pc 4  awaits element k from its getter
//   pc 5  awaits element k's ToString from a Convert frame
// At k = len the pieces become one new string (a job row), each piece's
// code units charged to the work limit as it was collected. A cycle
// (`a[1] = a; a.join()`) recurses through toString and join frames until
// the frame limit raises the catchable stack overflow (InternalError).
//
// toString()        input O = ToObject(this).
//   pc 1  awaits Get(O, "join") (a getter's result): a callable one is
//         called with O, any other value gives %Object.prototype.toString%
//         of O (object_tag)
//   pc 2  awaits the join call's result
//
// Not implemented yet, each a named UnsupportedOperand (decision 1): a
// BigInt length, separator or element, a light-cljs
// runtime value to convert, and as toString's `join` a host-effect built-in
// or a native-frame built-in other than these and map and forEach.

// Whether b is one of these built-ins.
inline fn array_method(b: Builtin) -> bool { return b == Builtin.ArrayPrototypePush || b == Builtin.ArrayPrototypeJoin || b == Builtin.ArrayPrototypeToString; }
// Whether b is an Array.prototype built-in a native frame may start above
// itself as a method it calls (a ToPrimitive method, toString's join).
fn array_native(b: Builtin) -> bool { return array_method(b) || array_callback(b) || array_search(b); }
// 2^53 - 1, ToLength's bound.
inline fn max_safe_integer() -> f64 { return 9007199254740991.0; }

// Start built-in b (array_method) called with `receiver` and `args`, its
// operand base `base` (native_start). A push the fast path finished returns
// its result, for native_start to deliver at `base` (`args` views the
// operands); otherwise -1, with the frame started or a fault.
fn start_array_method(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) -> i64 {
    let heap .= job.heap;
    // ToObject(this): undefined and null are a TypeError.
    let o = to_object(job, receiver);
    if heap.fault != Fault.None { return -1; }
    if b == Builtin.ArrayPrototypePush {
        let r = push_fast(job, o, args);
        if r != -1 { return if r >= 0 { r } else { -1 }; }
    }
    // `args` views the operands, which are cut to the base below.
    var items: Value[>..] = [];
    if b == Builtin.ArrayPrototypePush { items.append(args); }
    let separator = if b == Builtin.ArrayPrototypeJoin { arg(args, 0) } else { Value.Undefined };
    job.operands.resize(base); job.operands.append(items);
    if !push_method_frame(job, b, o, separator, base) { return -1; }
    // toString's frame starts at pc 1 with Get(O, "join").
    if b == Builtin.ArrayPrototypeToString { get_named(job, b, o, "join"); return -1; }
    let at = job.frames.len - 1;
    job.frames[at].argument_count = items.len; job.frames[at].pc = 0;
    if b == Builtin.ArrayPrototypeJoin { job.frames[at].output = number(0); }
    get_named(job, b, o, "length");
    return -1;
}
fn push_method_frame(job: Job&, b: Builtin, o: Value, receiver: Value, base: i64) -> bool {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return false; }
    job.frames.push(Frame { function: -1, pc: 1, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: receiver, input: o, native: Native.ArrayMethod { which: b } });
    return true;
}
// Get(o, name) for the frame at the top: the value goes on the operands for
// its current phase, or a getter runs above it and its result arrives there.
fn get_named(job: Job&, b: Builtin, o: Value, name: const u8[:]) {
    let heap .= job.heap;
    let k = key_named(heap, name);
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, o, k);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { run_getter(job, b, getter, o); } else { push_operand(job, v); }
}
fn push_operand(job: Job&, v: Value) -> bool {
    if job.operands.len >= job.heap.limits.stack - job.locals.len { job.heap.fault = Fault.StackLimit; return false; }
    job.operands.push(v); return true;
}
// One step of the frame at the top, dispatched on its built-in.
fn array_method_step(job: Job&, frame: Frame, b: Builtin) {
    match b {
        ArrayPrototypePush => { push_step(job, frame); },
        ArrayPrototypeJoin => { join_step(job, frame); },
        ArrayPrototypeToString => { to_string_step(job, frame); },
        // The search methods (light-cljs-908; array_search.goose).
        _ => { if array_search(b) { array_search_step(job, frame, b); } else { job.heap.fault = Fault.InvalidState; } },
    }
}
// ToLength of the length value at the operand top for the frame at `at`, or
// false when it is an object, whose ToNumber (ToPrimitive with hint number)
// a Convert frame above runs first, leaving the number where the value was;
// the phase then runs again and takes it.
fn length_value(job: Job&, b: Builtin) -> f64, bool {
    let heap .= job.heap;
    let v = form_value(heap, job.operands.pop());
    if v.is_object() {
        if !convertible(heap, v) { unsupported_step(job, b, "ToNumber of a light-cljs runtime value"); return 0, false; }
        job.operands.push(v); start_convert(job, Hint.Number, true, 1, 1); return 0, false;
    }
    let n = primitive_number(job, b, v);
    if heap.fault != Fault.None { return 0, false; }
    return if !(n > 0) { 0.0 } else { min(trunc(n), max_safe_integer()) }, true;
}

// ---- push
// The dense-array fast path (see above): every item stored in the run (its
// growth and its 1,048,576-element limit, MemoryLimit, as an assignment's),
// the new length the result. -1 leaves everything to the general steps; -2
// is a fault.
fn push_fast(job: Job&, o: Value, items: const Value[:]) -> i64 {
    let heap .= job.heap;
    if !dense(job, o) || !writable(job, o.id()) || node(heap, o.id()).initialized || !node(heap, o.id()).proto.is_hole() { return -1; }
    let len: i64 = node(heap, o.id()).length;
    if len + items.len > 4294967295 { return -1; }
    for v, i in items { if !set_element(heap, o, len + i, v) { return -2; } }
    return len + items.len;
}
fn push_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypePush; let at = job.frames.len - 1;
    let n = frame.argument_count;
    match frame.pc {
        0 => {
            let len, done = length_value(job, b);
            if !done { return; }
            // Before any write (QuickJS's "Array loo long").
            if n > max_safe_integer() - len { type_error(job, "array-like length exceeds 2^53 - 1"); return; }
            job.frames[at].output = number(len); job.frames[at].pc = 1;
        },
        1 => {
            let len = frame.output.num();
            if frame.index < n {
                let k = index_key(heap, (len as i64) + frame.index);
                if heap.fault != Fault.None { return; }
                job.frames[at].pc = 2;
                if !set_or_throw(job, b, frame.input, k, job.operands[frame.operand_base + frame.index]) && heap.fault == Fault.None { job.frames[at].index += 1; job.frames[at].pc = 1; }
                return;
            }
            let k = key_named(heap, "length");
            if heap.fault != Fault.None { return; }
            job.frames[at].pc = 3;
            if !set_or_throw(job, b, frame.input, k, number(len + n)) && heap.fault == Fault.None { finish_native(job, frame, number(len + n)); }
        },
        2 => { job.operands.pop(); job.frames[at].index += 1; job.frames[at].pc = 1; },
        _ => { job.operands.pop(); finish_native(job, frame, number(frame.output.num() + n)); },
    }
}
// Set(o, k, v, true) for built-in b: true when o's setter for k now runs
// above the frame (its return value arrives at the frame's phase); false
// when the value was written, or with the fault: a refused write is a
// TypeError, a row the job did not allocate NativeBoundary before it
// changes (decision 3).
fn set_or_throw(job: Job&, b: Builtin, o: Value, k: Value, v: Value) -> bool {
    let heap .= job.heap;
    // A shareable regex value's own or published (light-cljs-935): assign
    // changes none of its rows, and its refusal is worded below.
    if !writable(job, o.id()) && !shared_regexp(heap, o.id()) { image_write(job, k); return false; }
    prepare_function(job, o, k, true);
    if heap.fault != Fault.None { return false; }
    let setter, ok = write(heap, o, k, v);
    shared_regexp_wording(job, o);
    // A proxy's [[Set]] (light-cljs-890), its answer arriving as a setter's
    // would, a false one a TypeError.
    if proxy_pending(heap, proxy_method::Set) { proxy_take(job, proxy_method::Set, o, v, true); return heap.fault == Fault.None; }
    if heap.fault != Fault.None { return false; }
    if setter.is_object() {
        call_accessor(job, setter, o, [v]);
        return heap.fault == Fault.None;
    }
    if !ok { named_error(job, Fault.TypeError, "cannot set property '", k, "'"); return false; }
    record(job, o, k, v);
    return false;
}

// ---- join
fn join_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeJoin; let at = job.frames.len - 1;
    match frame.pc {
        0 => {
            let len, done = length_value(job, b);
            if !done { return; }
            job.frames[at].argument_count = len as i64; job.frames[at].pc = 1;
        },
        1 => {
            let s = form_value(heap, frame.receiver);
            if s.is_undefined() { job.frames[at].receiver = key_named(heap, ","); job.frames[at].pc = 3; return; }
            if s.is_object() {
                if !convertible(heap, s) { unsupported_step(job, b, "ToString of a light-cljs runtime value"); return; }
                job.frames[at].pc = 2;
                if push_operand(job, s) { start_convert(job, Hint.String, true, 1, 1); }
                return;
            }
            if s.is_bigint() { unsupported_step(job, b, "a BigInt separator"); return; }
            let t = primitive_string(job, b, s);
            if heap.fault != Fault.None { return; }
            job.frames[at].receiver = t; job.frames[at].pc = 3;
        },
        2 => { job.frames[at].receiver = job.operands.pop(); job.frames[at].pc = 3; },
        3 => { join_visit(job, at); },
        4 => { join_element(job, at, job.operands.pop()); },
        _ => { if join_piece(job, at, job.operands.pop()) { job.frames[at].index += 1; job.frames[at].pc = 3; } },
    }
}
// Element k of the frame at `at`: finish at the length; otherwise the
// separator before it (k > 0), then Get(O, k), read from a dense array's run
// when it holds the element and otherwise through the prototype chain, a
// getter's value arriving at pc 4.
fn join_visit(job: Job&, at: i64) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeJoin;
    let k = job.frames[at].index; let o = job.frames[at].input;
    if k >= job.frames[at].argument_count { join_finish(job, at); return; }
    if k > 0 && !join_piece(job, at, job.frames[at].receiver) { return; }
    if node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized {
        let v, found = own_element(heap, o, k);
        if found { join_element(job, at, v); return; }
    }
    let key = index_key(heap, k);
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, o, key);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { job.frames[at].pc = 4; run_getter(job, b, getter, o); return; }
    join_element(job, at, v);
}
// Element value v: undefined and null add nothing, a primitive its
// ToString, an object its ToString in a Convert frame (pc 5).
fn join_element(job: Job&, at: i64, value: Value) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeJoin;
    let v = form_value(heap, value);
    var s = Value.Undefined;
    if v.tag() <= value_tag::Null {}
    else if v.is_object() {
        if !convertible(heap, v) { unsupported_step(job, b, "ToString of a light-cljs runtime value"); return; }
        job.frames[at].pc = 5;
        if push_operand(job, v) { start_convert(job, Hint.String, true, 1, 1); }
        return;
    } else if v.is_bigint() { unsupported_step(job, b, "a BigInt element"); return; }
    else {
        s = primitive_string(job, b, v);
        if heap.fault != Fault.None { return; }
    }
    if s.is_string() && !join_piece(job, at, s) { return; }
    job.frames[at].index += 1; job.frames[at].pc = 3;
}
// Collect string s, unless empty, charging its code units to the work
// limit and checking them against the unit limit.
fn join_piece(job: Job&, at: i64, s: Value) -> bool {
    let heap .= job.heap;
    let n = str_len(heap, s);
    if n == 0 { return true; }
    let units = job.frames[at].output.num() + n;
    if !units_fit(heap, units as i64) { return memory(heap); }
    if !charge(heap, n) || !push_operand(job, s) { return false; }
    job.frames[at].output = number(units);
    return true;
}
// The pieces as one string: none is "", one is itself, more a new string in
// its canonical encoding (charged as they were collected).
fn join_finish(job: Job&, at: i64) {
    let heap .= job.heap; let frame = job.frames[at];
    let count = job.operands.len - frame.operand_base;
    var r = Value.Undefined;
    if count == 0 { r = key_named(heap, ""); }
    else if count == 1 { r = job.operands[frame.operand_base]; }
    else {
        let units = frame.output.num() as i64;
        if !units_fit(heap, units) || heap.strings.len - heap.image.strings >= heap.limits.units { memory(heap); return; }
        var wide = false;
        for p in job.operands[frame.operand_base..] { if header(heap, p.sid()).wide { wide = true; } }
        if wide {
            var out: u16[>..] = [];
            for p in job.operands[frame.operand_base..] { append_utf16(heap, p, out); }
            r = import_text(heap, out);
        } else {
            var out: u8[>..] = [];
            for p in job.operands[frame.operand_base..] { out.append(narrow_run(heap, header(heap, p.sid()))); }
            r = string_ref(heap, add_latin1(heap, out), out.len, false);
        }
    }
    if heap.fault != Fault.None { return; }
    finish_native(job, frame, r);
}

// ---- toString
fn to_string_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeToString; let at = job.frames.len - 1;
    if frame.pc == 2 { finish_native(job, frame, job.operands.pop()); return; }
    let f = job.operands.pop();
    // %Object.prototype.toString%, whose @@toStringTag getter runs in a
    // Statics frame above this one (light-cljs-884); its result arrives at pc 2.
    if !callable(heap, f) { job.frames[at].pc = 2; start_conditional_native(job, Builtin.ObjectPrototypeToString, frame.input, [], job.operands.len); return; }
    job.frames[at].pc = 2;
    if node(heap, f.id()).kind == node_kind::Closure { push_frame(job, f, frame.input, [], false, job.operands.len); return; }
    if is_bound(heap, f) { start_bound(job, f, frame.input, [], job.operands.len); return; }
    let c = builtin_of(heap, f.id());
    // A built-in that converts its receiver runs in a Coerce frame (light-cljs-900).
    if coerces(heap, f, frame.input, []) { start_coerce(job, f, frame.input, [], job.operands.len); return; }
    if array_native(c) || forwarding(c) || regexp_native(c) { native_start(job, c, frame.input, [], job.operands.len); return; }
    if conditional_native(c) { start_conditional_native(job, c, frame.input, [], job.operands.len); return; }
    if effect_builtin(c) || native_builtin(c) || c == Builtin.Mapv || c == Builtin.MapSeq || c == Builtin.FunctionPrototypeCall || forces(heap, c, []) { unsupported_step(job, b, "a host-effect or native-frame built-in as join"); return; }
    let r = call_plain(job, f, frame.input, []);
    if heap.fault == Fault.None { job.operands.push(r); }
}
