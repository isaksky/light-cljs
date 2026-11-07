import std;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .continuations;

namespace source;

// The built-in array and string iterators (light-cljs-844; decisions 2, 5
// and 6 of docs/javascript-coverage.md). Semantics from the Pants engine's
// rust/src/vm/native/iterator.rs (Data { source, index, kind, done },
// State::next, State::string) and QuickJS's js_create_array_iterator,
// js_array_iterator_next and js_string_iterator_next, with the
// specification's steps (ECMAScript 23.1.5 Array Iterator Objects, 22.1.5
// String Iterator Objects, 23.1.3.38 values, 23.1.3.19 keys, 23.1.3.5
// entries, 22.1.3.36 String.prototype[@@iterator]).
//
// A cursor is a node of kind node_kind::Iterator, whose state is its internal
// slots, never JavaScript properties (decision 2): `value` the source (an
// object for an array iterator, a string for a string iterator) and
// Undefined once the cursor is done, so a done cursor stays done and holds
// nothing; `function` its iteration (iteration::Values, Keys, Entries or
// CodePoints); `length` the next index (a code unit index for a string).
// Its own properties are an ordinary object's, its prototype
// %ArrayIteratorPrototype% or %StringIteratorPrototype% (fixed realm rows
// inheriting %IteratorPrototype%, decision 6). Cursors and their results
// are job rows; a cursor the job did not allocate faults NativeBoundary
// when `next` would advance it (decision 3).
//
// Array.prototype.values, keys and entries (values is also
// Array.prototype[@@iterator]) and %StringIteratorPrototype%.next run no
// JavaScript and complete in call_builtin. %ArrayIteratorPrototype%.next
// and String.prototype[@@iterator] are native frames (Native.BuiltinIterator,
// their Builtin the payload) when user code runs, and otherwise complete at
// the call (native_start delivers the result at the call's operand base):
//
// %ArrayIteratorPrototype%.next()   input the cursor; receiver its source
//                                   and index its index, both read when the
//                                   call starts (the Pants engine's
//                                   State::next), so a `next` call a length
//                                   getter makes does not move this one's.
//   pc 0  awaits LengthOfArrayLike(source): a `length` getter's result, or
//         its value converted by a Convert frame with hint number; an
//         array's own data `length` is read without the frame, on every
//         call (the length is live)
//   pc 1  awaits element `index` from its getter
// At or past the length the cursor is done; otherwise its index advances
// before Get(source, index) (so a throwing getter still consumed the
// index). `keys` gives the index, `entries` a new [index, value] array; a
// hole reads through the prototype chain and so gives undefined.
//
// String.prototype[@@iterator]()    RequireObjectCoercible(this), then
//                                   ToString: an object's in a Convert frame
//   pc 0  awaits the string
//
// Not implemented yet, each a named UnsupportedOperand (decision 1): Map,
// Set and typed-array iterators (light-cljs-769), a BigInt receiver of
// String.prototype[@@iterator], a light-cljs runtime value as a source, and
// a built-in getter.

// A cursor's iteration, its node's `function`.
let iteration::Values = 0;
let iteration::Keys = 1;
let iteration::Entries = 2;
let iteration::CodePoints = 3;
// Whether b runs as a BuiltinIterator native frame.
inline fn iterator_native(b: Builtin) -> bool { return b == Builtin.ArrayIteratorPrototypeNext || b == Builtin.StringPrototypeIterator; }
// Whether v is a cursor of an array iteration (`strings` false) or a string one.
fn is_cursor(heap: const Heap&, v: Value, strings: bool) -> bool {
    if !v.is_object() || v.id() >= heap.nodes.len || node(heap, v.id()).kind != node_kind::Iterator { return false; }
    return (node(heap, v.id()).function == iteration::CodePoints) == strings;
}
// A new cursor over `source`, a job row.
fn new_cursor(job: Job&, source: Value, iteration: i64) -> Value {
    let proto = if iteration == iteration::CodePoints { string_iterator_prototype_row() } else { array_iterator_prototype_row() };
    return allocate(job.heap, Node { kind: node_kind::Iterator, function: iteration as u32, value: source, proto: object(proto) });
}

// ---- Array.prototype.values, keys and entries
// CreateArrayIterator(ToObject(this), kind): a primitive is its wrapper,
// undefined and null a TypeError. An arguments object iterates its
// elements. A cursor only reads its source, so a row the job may not write
// is a source too (decision 3); the compiler roots object, which answers
// only its own names, fails NativeBoundary.
fn array_iterator(job: Job&, b: Builtin, receiver: Value) -> Value {
    let heap .= job.heap;
    let v = form_value(heap, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    let o = to_object(job, v);
    if heap.fault != Fault.None { return Value.Undefined; }
    if job.root_receiver.is_object() && o == job.root_receiver { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    if !convertible(heap, o) && node(heap, o.id()).kind != node_kind::Arguments { return unsupported_step(job, b, "a light-cljs runtime value as the receiver"); }
    let iteration = if b == Builtin.ArrayPrototypeKeys { iteration::Keys } else if b == Builtin.ArrayPrototypeEntries { iteration::Entries } else { iteration::Values };
    return new_cursor(job, o, iteration);
}

// ---- The native-frame built-ins
// Start built-in b (iterator_native) called with `receiver`, its operand
// base `base` (native_start): the result now at `base`, or a frame above.
fn start_iterator(job: Job&, b: Builtin, receiver: Value, base: i64) {
    job.operands.resize(base);
    if b == Builtin.StringPrototypeIterator { start_string_iterator(job, receiver); return; }
    array_next(job, receiver);
}
fn push_iterator_frame(job: Job&, b: Builtin, cursor: Value, source: Value) -> i64 {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return -1; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: source, input: cursor, native: Native.BuiltinIterator { which: b } });
    return job.frames.len - 1;
}
// One step of the frame at the top (see above), dispatched on its built-in.
fn iterator_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if b == Builtin.StringPrototypeIterator {
        let s = string_cursor(job, b, form_value(heap, job.operands.pop()));
        if heap.fault == Fault.None { finish_native(job, frame, s); }
        return;
    }
    if b != Builtin.ArrayIteratorPrototypeNext { heap.fault = Fault.InvalidState; return; }
    match frame.pc {
        0 => {
            let len, done = length_value(job, b);
            if done { array_next_at(job, at, frame.input, frame.receiver, frame.index, len); }
        },
        1 => { array_element(job, at, frame.input, frame.index, job.operands.pop()); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}

// ---- %ArrayIteratorPrototype%.next
fn array_next(job: Job&, receiver: Value) {
    let heap .= job.heap; let b = Builtin.ArrayIteratorPrototypeNext;
    if !is_cursor(heap, receiver, false) { type_error(job, "not an Array Iterator"); return; }
    let source = node(heap, receiver.id()).value;
    if source.is_undefined() { next_result(job, -1, Value.Undefined, true); return; }
    if !writable(job, receiver.id()) { heap.fault = Fault.NativeBoundary; return; }
    let i: i64 = node(heap, receiver.id()).length;
    // An array's `length` is its own data property.
    if node(heap, source.id()).kind == node_kind::Array { let len: f64 = node(heap, source.id()).length; array_next_at(job, -1, receiver, source, i, len); return; }
    let at = push_iterator_frame(job, b, receiver, source);
    if at < 0 { return; }
    job.frames[at].index = i;
    get_named(job, b, source, "length");
}
// The step of the cursor at index i of `source` with
// LengthOfArrayLike(source) = len, for the frame at `at` (-1 while the call
// runs without one).
fn array_next_at(job: Job&, at: i64, cursor: Value, source: Value, i: i64, len: f64) {
    let heap .= job.heap; let b = Builtin.ArrayIteratorPrototypeNext; let c = cursor.id();
    if i >= len { node_mut(heap, c).value = Value.Undefined; next_result(job, at, Value.Undefined, true); return; }
    if i + 1 >= 4294967295 { unsupported_step(job, b, "an index past 2^32 - 2"); return; }
    node_mut(heap, c).length = (i + 1) as u32;
    if node(heap, c).function == iteration::Keys { next_result(job, at, number(i), false); return; }
    // Get(source, i): an array's own element from its run or records, any
    // other through the prototype chain.
    if node(heap, source.id()).kind == node_kind::Array {
        let v, found = own_element(heap, source, i);
        if heap.fault != Fault.None { return; }
        if found { array_element(job, at, cursor, i, v); return; }
    }
    let k = index_key(heap, i);
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, source, k);
    if heap.fault != Fault.None { return; }
    if !getter.is_object() { array_element(job, at, cursor, i, v); return; }
    let frame = if at >= 0 { at } else { push_iterator_frame(job, b, cursor, source) };
    if frame < 0 { return; }
    job.frames[frame].pc = 1; job.frames[frame].index = i;
    run_getter(job, b, getter, source);
}
// Element i's value v: the result's value, or with `entries` a new
// [i, v] array.
fn array_element(job: Job&, at: i64, cursor: Value, i: i64, v: Value) {
    let heap .= job.heap;
    if node(heap, cursor.id()).function != iteration::Entries { next_result(job, at, v, false); return; }
    let pair = allocate(heap, Node { kind: node_kind::Array });
    if heap.fault != Fault.None || !set_element(heap, pair, 0, number(i)) || !set_element(heap, pair, 1, v) { return; }
    next_result(job, at, pair, false);
}
// The result {value, done} (CreateIterResultObject): delivered by the
// frame at `at`, or pushed at the call.
fn next_result(job: Job&, at: i64, v: Value, done: bool) {
    let r = iter_result(job, v, done);
    if job.heap.fault != Fault.None { return; }
    if at >= 0 { finish_native(job, job.frames[at], r); } else { push_operand(job, r); }
}

// ---- String.prototype[@@iterator] and %StringIteratorPrototype%.next
fn start_string_iterator(job: Job&, receiver: Value) {
    let heap .= job.heap; let b = Builtin.StringPrototypeIterator;
    let v = form_value(heap, receiver);
    if heap.fault != Fault.None { return; }
    if v.tag() <= value_tag::Null { type_error(job, "String.prototype[Symbol.iterator] called on null or undefined"); return; }
    if v.is_object() {
        if !convertible(heap, v) { unsupported_step(job, b, "ToString of a light-cljs runtime value"); return; }
        if push_iterator_frame(job, b, Value.Undefined, Value.Undefined) >= 0 && push_operand(job, v) { start_convert(job, Hint.String, true, 1, 1); }
        return;
    }
    let s = string_cursor(job, b, v);
    if heap.fault == Fault.None { push_operand(job, s); }
}
// A cursor over ToString(v), v a primitive.
fn string_cursor(job: Job&, b: Builtin, v: Value) -> Value {
    if v.is_bigint() { return unsupported_step(job, b, "a BigInt receiver"); }
    let s = primitive_string(job, b, v);
    if job.heap.fault != Fault.None { return Value.Undefined; }
    return new_cursor(job, s, iteration::CodePoints);
}
// The next code point as a string: a surrogate pair is one string of two
// code units, a lone surrogate one of one.
fn string_iterator_next(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap;
    if !is_cursor(heap, receiver, true) { return type_error(job, "not a String Iterator"); }
    let c = receiver.id(); let s = node(heap, c).value;
    if s.is_undefined() { return iter_result(job, Value.Undefined, true); }
    if !writable(job, c) { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    let i: i64 = node(heap, c).length; let n = str_len(heap, s);
    if i >= n { node_mut(heap, c).value = Value.Undefined; return iter_result(job, Value.Undefined, true); }
    let u = unit(heap, s, i);
    let pair = u >= 0xD800 && u <= 0xDBFF && i + 1 < n && unit(heap, s, i + 1) >= 0xDC00 && unit(heap, s, i + 1) <= 0xDFFF;
    let piece = if pair { string(heap, [u, unit(heap, s, i + 1)]) } else { string(heap, [u]) };
    if heap.fault != Fault.None { return Value.Undefined; }
    node_mut(heap, c).length = (i + (if pair { 2 } else { 1 })) as u32;
    return iter_result(job, piece, false);
}
