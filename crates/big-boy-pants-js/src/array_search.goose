import std;
import math;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .function_calls;
import .coerce;
import .proxy;

namespace source;

// Array.prototype.indexOf, lastIndexOf, includes and at (light-cljs-908):
// the search methods, which call no callback (decision 5 of
// docs/javascript-coverage.md, on light-cljs-842's template in
// array_methods.goose). Semantics from the Pants engine's
// rust/src/vm/native/array_search.rs and QuickJS's js_array_indexOf,
// js_array_lastIndexOf, js_array_includes and js_array_at, with the
// specification's steps (ECMAScript 23.1.3.17, .20, .16 and .1):
// ToObject(this), LengthOfArrayLike, then, unless the length is 0 (at
// converts regardless), ToIntegerOrInfinity of fromIndex (at: of its index),
// then the walk. indexOf and lastIndexOf skip an absent index (HasProperty)
// and compare with strict equality; includes reads every index and compares
// with SameValueZero; at reads one.
//
// Each is a native frame (Native.ArrayMethod, its Builtin the payload):
//   input O; receiver the search element; callee fromIndex (at: the index);
//   output whether fromIndex was passed (lastIndexOf starts at len - 1
//   without one); argument_count len; index k, the next index.
//   pc Length     awaits Get(O, "length") (a getter's result, or converted
//                 by a Convert frame with hint number)
//   pc FromIndex  awaits fromIndex's ToNumber (a Convert frame's for an
//                 object) and places k
//   pc Visit      visits from index k
//   pc Has        awaits a proxy's answer to HasProperty(O, k)
//   pc Element    awaits element k from its getter or a proxy's [[Get]]
// A step that runs no JavaScript visits up to search_batch() indices, each
// a work unit, so a search of a long array crosses budget cuts and the
// instruction limit between batches; anything that runs JavaScript (a
// getter, a proxy trap, a conversion) is a step boundary.
let search_pc::Length = 0;
let search_pc::FromIndex = 1;
let search_pc::Visit = 2;
let search_pc::Has = 3;
let search_pc::Element = 4;
inline fn search_batch() -> i64 { return 1024; }

// Whether b is one of these built-ins.
inline fn array_search(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeIndexOf || b == Builtin.ArrayPrototypeLastIndexOf || b == Builtin.ArrayPrototypeIncludes || b == Builtin.ArrayPrototypeAt; }
// The answer of a search that found nothing.
inline fn search_miss(b: Builtin) -> Value { return if b == Builtin.ArrayPrototypeIncludes { boolean(false) } else if b == Builtin.ArrayPrototypeAt { Value.Undefined } else { number(-1) }; }

// Start built-in b (array_search) called with `receiver` and `args`, its
// operand base `base` (native_start).
fn start_array_search(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let at_index = b == Builtin.ArrayPrototypeAt;
    let target = if at_index { Value.Undefined } else { arg(args, 0) };
    let from = if at_index { arg(args, 0) } else { arg(args, 1) };
    let passed = at_index || args.len > 1;
    let o = to_object(job, receiver);
    if heap.fault != Fault.None { return; }
    job.operands.resize(base);
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: search_pc::Length, local_base: job.locals.len, operand_base: base, callee: from, receiver: target, input: o, output: boolean(passed), native: Native.ArrayMethod { which: b } });
    get_named(job, b, o, "length");
}
// One step of the frame at the top (see above).
fn array_search_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match frame.pc {
        search_pc::Length => {
            let len, done = length_value(job, b);
            if !done { return; }
            job.frames[at].argument_count = len as i64;
            if len == 0 && b != Builtin.ArrayPrototypeAt { finish_native(job, frame, search_miss(b)); return; }
            job.frames[at].pc = search_pc::FromIndex;
            let f = form_value(heap, frame.callee);
            if f.is_object() {
                if !convertible(heap, f) { unsupported_step(job, b, "ToNumber of a light-cljs runtime value"); return; }
                if push_operand(job, f) { start_convert(job, Hint.Number, true, 1, 1); }
                return;
            }
            push_operand(job, f);
        },
        search_pc::FromIndex => {
            let v = job.operands.pop();
            let n = to_integer_or_infinity(primitive_number(job, b, v));
            if heap.fault != Fault.None { return; }
            let len:      f64 = frame.argument_count;
            let infinity: f64 = 1.0 / 0.0;
            var k: f64 = 0;
            if b == Builtin.ArrayPrototypeLastIndexOf {
                k = if !frame.output.flag() { len - 1 } else if n >= 0 { min(n, len - 1) } else { len + n };
            } else if b == Builtin.ArrayPrototypeAt {
                k = if n >= 0 { n } else { len + n };
            } else {
                k = if n >= 0 { n } else { max(len + n, 0.0) };
            }
            // Past either end (an infinite fromIndex too): nothing to visit.
            if !(k >= 0) || k >= len || k == infinity { finish_native(job, frame, search_miss(b)); return; }
            job.frames[at].index = k as i64; job.frames[at].pc = search_pc::Visit;
        },
        search_pc::Visit => { search_visit(job, at, b); },
        search_pc::Has => {
            job.frames[at].pc = search_pc::Visit;
            if truthy(heap, job.operands.pop()) { search_get(job, at, b); } else { search_advance(job, at, b); }
        },
        _ => { let v = job.operands.pop(); job.frames[at].pc = search_pc::Visit; search_element(job, at, b, v); },
    }
}
inline fn search_advance(job: Job&, at: i64, b: Builtin) { job.frames[at].index += if b == Builtin.ArrayPrototypeLastIndexOf { -1 } else { 1 }; }
// Visit from index k of the frame at `at` until the search ends, a step
// above the frame must answer, or the batch is spent.
fn search_visit(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap; let o = job.frames[at].input; let len = job.frames[at].argument_count;
    var visited: i64 = 0;
    loop {
        let k = job.frames[at].index;
        if k < 0 || k >= len { finish_native(job, job.frames[at], search_miss(b)); return; }
        if visited == search_batch() { return; }
        // The step charged the first index's work unit (native_step).
        if visited > 0 && !charge(heap) { return; }
        visited += 1;
        if node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized {
            let v, found = own_element(heap, o, k);
            if found { if search_element(job, at, b, v) { return; } continue; }
        }
        if b == Builtin.ArrayPrototypeIndexOf || b == Builtin.ArrayPrototypeLastIndexOf {
            let key = index_key(heap, k);
            if heap.fault != Fault.None { return; }
            let has = has_property(heap, o, key);
            // A proxy's [[HasProperty]] answers at pc Has (light-cljs-890).
            if proxy_pending(heap, proxy_method::HasProperty) { job.frames[at].pc = search_pc::Has; proxy_take(job, proxy_method::HasProperty); return; }
            if heap.fault != Fault.None { return; }
            if !has { search_advance(job, at, b); continue; }
        }
        if search_get(job, at, b) { return; }
    }
}
// Get(O, k): true when the frame finished, faulted or awaits the value at
// pc Element; false when the search goes on at the next index.
fn search_get(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap; let o = job.frames[at].input;
    let key = index_key(heap, job.frames[at].index);
    if heap.fault != Fault.None { return true; }
    let v, getter = lookup(heap, o, key);
    if heap.fault != Fault.None { return true; }
    if getter.is_object() { job.frames[at].pc = search_pc::Element; run_getter(job, b, getter, o); return true; }
    return search_element(job, at, b, v);
}
// Element v of index k: true when it ends the search (the frame finished);
// otherwise the index moved on.
fn search_element(job: Job&, at: i64, b: Builtin, v: Value) -> bool {
    let heap .= job.heap; let frame = job.frames[at];
    if b == Builtin.ArrayPrototypeAt { finish_native(job, frame, v); return true; }
    let x = form_value(heap, frame.receiver); let y = form_value(heap, v);
    // includes compares with SameValueZero: strict equality, and NaN is NaN.
    let hit = equal(heap, x, y) || b == Builtin.ArrayPrototypeIncludes && x.is_number() && y.is_number() && x.num() != x.num() && y.num() != y.num();
    if heap.fault != Fault.None { return true; }
    if hit { finish_native(job, frame, if b == Builtin.ArrayPrototypeIncludes { boolean(true) } else { number(frame.index) }); return true; }
    search_advance(job, at, b);
    return false;
}
