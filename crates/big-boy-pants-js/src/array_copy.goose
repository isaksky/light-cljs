import std;
import math;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .array_mutators;
import .array_sort;
import .function_calls;
import .object_statics;
import .coerce;
import .proxy;

namespace source;

// The Array methods that copy (light-cljs-911; decision 5 of
// docs/javascript-coverage.md): Array.prototype.slice, concat, flat,
// toReversed and with, and the statics Array.of and Array.from. Semantics
// from the Pants engine's rust/src/vm/native/array_copy.rs and array_from.rs
// and QuickJS's js_array_slice, js_array_concat, js_array_flatten,
// js_array_toReversed, js_array_with, js_array_of and js_array_from, with
// the specification's steps (ECMAScript 23.1.3 and 23.1.2).
//
// All run in array_mutators.goose's frame (Native.ArrayMethod) as units
// over its memoized primitives, so every HasProperty, Get, call,
// conversion, definition and Set is a step of its own. slice, concat and
// flat make their result with ArraySpeciesCreate (array_callbacks.goose's
// steps) and fill it with CreateDataPropertyOrThrow; toReversed and with
// make a plain array; Array.of and Array.from construct `this` when it is
// a constructor. concat, Array.of and Array.from read no receiver length:
// their frames start at pc Run. Each method's use of the frame:
//   slice     P the next index, Q the end, R the next result index, S the
//             count asked of ArraySpeciesCreate
//   concat    P the item (0 the receiver, then the arguments), Q the next
//             index within a spread item, S its length, R the next result
//             index
//   flat      callee a plain array the job allocates, the walk's stack:
//             a source, its next index and its length a level; R the next
//             result index, S the depth
//   toReversed, with   P the next index; with's Q its index
//   Array.of  receiver the constructor; P the next item
//   Array.from  receiver the constructor, callee the iterator record,
//             input the array-like once it is one; P the next index, Held
//             the item being mapped, or the exception that closes the
//             iterator
// Array.from over an iterable follows light-cljs-884's Object.fromEntries:
// while an item is mapped and defined the frame holds a Catch handler at
// pc Caught, so an abrupt completion there closes the iterator (its
// `return` read and called under a handler at pc Closed, whatever it does
// dropped) and is rethrown; a failing `next`, `done` or `value` closes
// nothing.
//
// The fast paths copy a dense array's present elements into a dense result
// in one step (array_mutators.goose's mutator_dense and its bound).
//
// Not implemented yet, each a named UnsupportedOperand (decision 1): a
// BigInt length, index or depth (light-cljs-771) and a light-cljs runtime
// value to convert or iterate. Array.fromAsync is the generators-async
// area's (light-cljs-768).
let mutate_stage::Step = 13;
let mutate_stage::Map = 14;
let mutate_stage::Close = 15;
let mutate_stage::Like = 16;

// Whether b is one of these built-ins.
fn array_copier(b: Builtin) -> bool {
    return match b {
        ArrayPrototypeSlice, ArrayPrototypeConcat, ArrayPrototypeFlat, ArrayPrototypeToReversed, ArrayPrototypeWith, ArrayOf, ArrayFrom => true,
        _ => false,
    };
}
// The copiers whose frame reads no receiver length first.
inline fn copier_static(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeConcat || b == Builtin.ArrayOf || b == Builtin.ArrayFrom; }
// The copiers whose result ArraySpeciesCreate makes.
inline fn copier_species(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeSlice || b == Builtin.ArrayPrototypeConcat || b == Builtin.ArrayPrototypeFlat || b == Builtin.ArrayPrototypeSplice; }
fn range_error(job: Job&, message: const u8[:]) { job.detail.clear(); for c in message { job.detail.push(c); } job.heap.fault = Fault.RangeError; }
// Argument i of the frame m walks, undefined when not passed.
inline fn copier_arg(job: const Job&, m: const Micro&, i: i64) -> Value { return if i < job.frames[m.at].index { job.operands[m.base + mutate_slot::Args + i] } else { Value.Undefined }; }

// ---- More primitives
// Get(o, key) for any key; a primitive o reads through its prototype.
fn micro_get_key(job: Job&, m: Micro&, b: Builtin, o: Value, key: Value) -> Value {
    let heap .= job.heap;
    let replayed, memo = micro_replay(job, m);
    if replayed { return memo; }
    if heap.fault != Fault.None { m.stop = true; return Value.Undefined; }
    let v, getter = read(heap, o, key);
    if heap.fault != Fault.None { m.stop = true; return Value.Undefined; }
    if getter.is_object() { micro_wait(job, m); run_getter(job, b, getter, o); return Value.Undefined; }
    micro_record(job, m, v);
    return v;
}
fn micro_get_named(job: Job&, m: Micro&, b: Builtin, o: Value, name: const u8[:]) -> Value {
    let key = key_named(job.heap, name);
    return micro_get_key(job, m, b, o, key);
}
// Construct(f, args): the new object arrives at pc Await.
fn micro_construct(job: Job&, m: Micro&, f: Value, args: const Value[:]) -> Value {
    let replayed, memo = micro_replay(job, m);
    if replayed { return memo; }
    if !push_operand(job, f) { m.stop = true; return Value.Undefined; }
    for v in args { if !push_operand(job, v) { m.stop = true; return Value.Undefined; } }
    micro_wait(job, m);
    construct(job, args.len);
    return Value.Undefined;
}
// GetIteratorFromMethod(v, method): the iterator record arrives at pc Await.
fn micro_iterator(job: Job&, m: Micro&, v: Value, method: Value) -> Value {
    let replayed, memo = micro_replay(job, m);
    if replayed { return memo; }
    micro_wait(job, m);
    get_iterator_from_method(job, v, method);
    return Value.Undefined;
}
// IsArray(v), remembered: a later primitive of the unit may revoke the
// proxy it looked through, and the unit runs again after each one.
fn micro_is_array(job: Job&, m: Micro&, v: Value) -> bool {
    let replayed, memo = micro_replay(job, m);
    if replayed { return memo.flag(); }
    let r = is_array_value(job, v);
    if job.heap.fault != Fault.None { m.stop = true; return false; }
    micro_record(job, m, boolean(r), false);
    return r;
}
// LengthOfArrayLike(o): Get(o, "length"), then ToLength.
fn micro_length(job: Job&, m: Micro&, b: Builtin, o: Value) -> i64 {
    let v = micro_get_named(job, m, b, o, "length");
    if m.stop { return 0; }
    let n = micro_to_number(job, m, b, v);
    if m.stop { return 0; }
    return to_length(n) as i64;
}
// The frame's Catch handler at `pc`, held while a guarded unit runs: pushed
// once, whichever step of the unit this is.
fn copier_guard(job: Job&, m: const Micro&, pc: i64) {
    let n = job.handlers.len;
    if n > 0 && job.handlers[n - 1].frame == m.at && job.handlers[n - 1].pc == pc && job.handlers[n - 1].kind == HandlerKind.Catch { return; }
    close_guard(job, m.at, pc);
}
// A plain result: ArrayCreate(len), a RangeError past 2^32 - 1.
fn copier_plain(job: Job&, m: Micro&, b: Builtin, len: i64) -> Value {
    let length: f64 = len;
    let a = array_species_create(job, b, job.frames[m.at].input, length);
    if job.heap.fault != Fault.None { m.stop = true; }
    return a;
}
// Whether result a takes the fast path's direct stores: a dense array the
// job made, with the realm's prototype.
inline fn copier_dense(job: Job&, a: Value) -> bool { return dense(job, a) && writable(job, a.id()) && !node(job.heap, a.id()).initialized; }
// The frame's species phases finished with the result in its output: the
// units go on at stage Collect (array_mutator_step).

// Start a copier that reads no receiver length (copier_static): the
// mutators' frame at pc Run, `receiver` kept for the units.
fn start_array_static(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    // Array.from checks its map function before anything is read.
    if b == Builtin.ArrayFrom && !arg(args, 1).is_undefined() && !is_callable(heap, arg(args, 1)) { type_error(job, "the map function must be a function or undefined"); return; }
    var input = object(array_constructor_row());
    if b == Builtin.ArrayPrototypeConcat {
        input = to_object(job, receiver);
        if heap.fault != Fault.None { return; }
    }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    if mutate_slot::Args + args.len + 2 > heap.limits.stack - job.locals.len - base { heap.fault = Fault.StackLimit; return; }
    var copied: Value[>..] = []; copied.append(args);
    job.operands.resize(base);
    for i in mutate_slot::Args { job.operands.push(if i >= mutate_slot::Memo && i < mutate_slot::Stage || i == mutate_slot::Held { Value.Undefined } else { number(0) }); }
    job.operands.append(copied);
    job.frames.push(Frame { function: -1, pc: mutate_pc::Run, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: receiver, input: input, index: copied.len, native: Native.ArrayMethod { which: b } });
}
// One unit of copier b for the frame m walks.
fn copy_unit(job: Job&, m: Micro&, b: Builtin) {
    match b {
        ArrayPrototypeSlice => { slice_unit(job, m); },
        ArrayPrototypeConcat => { concat_unit(job, m); },
        ArrayPrototypeFlat => { flat_unit(job, m); },
        ArrayPrototypeToReversed, ArrayPrototypeWith => { replace_unit(job, m, b); },
        ArrayOf => { of_unit(job, m); },
        ArrayFrom => { from_unit(job, m); },
        _ => { job.heap.fault = Fault.InvalidState; },
    }
}
// An exception arrived at the frame's handler (Array.from): at pc Caught
// it is kept and the iterator closed; at pc Closed it is `return`'s own,
// dropped for the one kept.
fn copier_caught(job: Job&, at: i64, b: Builtin, closed: bool) {
    let heap .= job.heap; let base = job.frames[at].operand_base;
    if b != Builtin.ArrayFrom { heap.fault = Fault.InvalidState; return; }
    let e = job.operands.pop();
    if closed { raise(job, job.operands[base + mutate_slot::Held], job.frames[at]); return; }
    job.operands[base + mutate_slot::Held] = e;
    mutate_put(job, base, mutate_slot::Done, 0); mutate_put(job, base, mutate_slot::Stage, mutate_stage::Close);
    job.frames[at].pc = mutate_pc::Run;
}

// ---- slice(start, end) (23.1.3.28)
fn slice_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeSlice;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let from = micro_number(job, m, b, copier_arg(job, m, 0));
        if m.stop { return; }
        let end = copier_arg(job, m, 1);
        var to: f64 = len;
        if !end.is_undefined() { to = micro_number(job, m, b, end); if m.stop { return; } }
        let k = relative_index(from, len); let last = relative_index(to, len);
        micro_unit_done(job, m);
        mutate_put(job, m.base, mutate_slot::P, k); mutate_put(job, m.base, mutate_slot::Q, last); mutate_put(job, m.base, mutate_slot::R, 0); mutate_put(job, m.base, mutate_slot::S, max(last - k, 0));
        if species_begin(job, m.at, b) { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Collect); } else { m.stop = true; }
        return;
    }
    let k = mutate_number(job, m.base, mutate_slot::P); let last = mutate_number(job, m.base, mutate_slot::Q); let n = mutate_number(job, m.base, mutate_slot::R);
    let a = job.frames[m.at].output;
    if stage == mutate_stage::Collect {
        if n == 0 && k < last && mutate_number(job, m.base, mutate_slot::Done) == 0 && !(a == o) && copier_dense(job, a) {
            let fast = mutator_dense(job, o, len);
            if heap.fault != Fault.None { m.stop = true; return; }
            if fast {
                for i in k..last { if !set_element(heap, a, i - k, sort_value(heap, o, i)) { m.stop = true; return; } }
                mutate_put(job, m.base, mutate_slot::P, last); mutate_put(job, m.base, mutate_slot::R, last - k);
                mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish);
                return;
            }
        }
        if k >= last { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish); return; }
        let present = micro_has(job, m, o, k);
        if m.stop { return; }
        if present {
            let v = micro_get(job, m, b, o, k);
            if m.stop { return; }
            micro_create(job, m, a, n, v);
            if m.stop { return; }
        }
        mutate_put(job, m.base, mutate_slot::P, k + 1); mutate_put(job, m.base, mutate_slot::R, n + 1);
        micro_unit_done(job, m);
        return;
    }
    micro_set_length(job, m, b, a, n);
    if m.stop { return; }
    micro_finish(job, m, a);
}

// ---- concat(...items) (23.1.3.2)
fn concat_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeConcat;
    let o = job.frames[m.at].input; let count = job.frames[m.at].index;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        if species_begin(job, m.at, b) { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Collect); } else { m.stop = true; }
        return;
    }
    let i = mutate_number(job, m.base, mutate_slot::P); let n = mutate_number(job, m.base, mutate_slot::R);
    let a = job.frames[m.at].output;
    if stage == mutate_stage::Finish {
        micro_set_length(job, m, b, a, n);
        if m.stop { return; }
        micro_finish(job, m, a);
        return;
    }
    if i > count { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish); return; }
    let e = if i == 0 { o } else { job.operands[m.base + mutate_slot::Args + i - 1] };
    if stage == mutate_stage::Collect {
        // IsConcatSpreadable(E): an object's @@isConcatSpreadable when it is
        // not undefined, otherwise IsArray(E).
        var spread = false;
        let item = form_value(heap, e);
        if item.is_object() && convertible(heap, item) {
            let s = micro_get_key(job, m, b, item, Value.Symbol { s: well_known::IsConcatSpreadable });
            if m.stop { return; }
            if s.is_undefined() {
                spread = micro_is_array(job, m, item);
                if m.stop { return; }
            } else { spread = truthy(heap, form_value(heap, s)); }
        }
        if !spread {
            if n >= max_safe_integer() { type_error(job, "array-like length exceeds 2^53 - 1"); m.stop = true; return; }
            micro_create(job, m, a, n, e);
            if m.stop { return; }
            mutate_put(job, m.base, mutate_slot::P, i + 1); mutate_put(job, m.base, mutate_slot::R, n + 1);
            micro_unit_done(job, m);
            return;
        }
        let length = micro_length(job, m, b, item);
        if m.stop { return; }
        if length > max_safe_integer() - n { type_error(job, "array-like length exceeds 2^53 - 1"); m.stop = true; return; }
        micro_unit_done(job, m);
        mutate_put(job, m.base, mutate_slot::Q, 0); mutate_put(job, m.base, mutate_slot::S, length);
        mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Items);
        // A dense item into a dense result: its elements at once.
        if !(a == item) && copier_dense(job, a) {
            let fast = mutator_dense(job, item, length);
            if heap.fault != Fault.None { m.stop = true; return; }
            if fast {
                for j in length { if !set_element(heap, a, n + j, sort_value(heap, item, j)) { m.stop = true; return; } }
                mutate_put(job, m.base, mutate_slot::Q, length); mutate_put(job, m.base, mutate_slot::R, n + length);
            }
        }
        return;
    }
    // Items: element Q of the spread item.
    let k = mutate_number(job, m.base, mutate_slot::Q);
    if k >= mutate_number(job, m.base, mutate_slot::S) { mutate_put(job, m.base, mutate_slot::P, i + 1); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Collect); return; }
    let item = form_value(heap, e);
    let present = micro_has(job, m, item, k);
    if m.stop { return; }
    if present {
        let v = micro_get(job, m, b, item, k);
        if m.stop { return; }
        micro_create(job, m, a, n, v);
        if m.stop { return; }
    }
    mutate_put(job, m.base, mutate_slot::Q, k + 1); mutate_put(job, m.base, mutate_slot::R, n + 1);
    micro_unit_done(job, m);
}

// ---- flat(depth) (23.1.3.13): FlattenIntoArray as a walk over a stack of
// levels the job allocates, never Goose recursion; a level is the frame
// limit's to refuse (the catchable stack overflow), as a nested call is.
fn flat_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeFlat;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let d = copier_arg(job, m, 0);
        var depth: f64 = 1;
        if !d.is_undefined() { depth = micro_number(job, m, b, d); if m.stop { return; } }
        micro_unit_done(job, m);
        let stack = allocate(heap, Node { kind: node_kind::Array });
        if heap.fault != Fault.None || !set_element(heap, stack, 0, o) || !set_element(heap, stack, 1, number(0)) || !set_element(heap, stack, 2, number(len)) { m.stop = true; return; }
        job.frames[m.at].callee = stack;
        mutate_put(job, m.base, mutate_slot::R, 0); job.operands[m.base + mutate_slot::S] = number(max(min(depth, max_safe_integer()), 0.0));
        if species_begin(job, m.at, b) { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Collect); } else { m.stop = true; }
        return;
    }
    let stack = job.frames[m.at].callee; let a = job.frames[m.at].output;
    if !stack.is_object() || node(heap, stack.id()).kind != node_kind::Array { heap.fault = Fault.InvalidState; m.stop = true; return; }
    let top: i64 = node(heap, stack.id()).length;
    if top < 3 { micro_finish(job, m, a); return; }
    let source = sort_value(heap, stack, top - 3); let k = sort_value(heap, stack, top - 2).num() as i64; let length = sort_value(heap, stack, top - 1).num() as i64;
    if !source.is_object() { heap.fault = Fault.InvalidState; m.stop = true; return; }
    // The level is walked: back to the one below.
    if k >= length { if !array_length(heap, stack, number(top - 3)) { m.stop = true; } return; }
    let n = mutate_number(job, m.base, mutate_slot::R); let level = top / 3;
    let present = micro_has(job, m, source, k);
    if m.stop { return; }
    var deeper = false; var element = Value.Undefined; var inner: i64 = 0;
    if present {
        element = micro_get(job, m, b, source, k);
        if m.stop { return; }
        if level <= mutate_number(job, m.base, mutate_slot::S) {
            deeper = micro_is_array(job, m, element);
            if m.stop { return; }
        }
        if deeper {
            inner = micro_length(job, m, b, element);
            if m.stop { return; }
        } else {
            if n >= max_safe_integer() { type_error(job, "array-like length exceeds 2^53 - 1"); m.stop = true; return; }
            micro_create(job, m, a, n, element);
            if m.stop { return; }
            mutate_put(job, m.base, mutate_slot::R, n + 1);
        }
    }
    if !set_element(heap, stack, top - 2, number(k + 1)) { m.stop = true; return; }
    if deeper {
        if level >= heap.limits.frames - job.frames.len { heap.fault = Fault.StackLimit; m.stop = true; return; }
        if !set_element(heap, stack, top, element) || !set_element(heap, stack, top + 1, number(0)) || !set_element(heap, stack, top + 2, number(inner)) { m.stop = true; return; }
    }
    micro_unit_done(job, m);
}

// ---- toReversed() and with(index, value) (23.1.3.33, 23.1.3.39): a plain
// array of the same length, read back to front, or with one index replaced.
fn replace_unit(job: Job&, m: Micro&, b: Builtin) {
    let heap .= job.heap; let reversing = b == Builtin.ArrayPrototypeToReversed;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        var actual: i64 = -1;
        if !reversing {
            let relative = micro_number(job, m, b, copier_arg(job, m, 0));
            if m.stop { return; }
            let position: f64 = if relative >= 0 { relative } else { len + relative };
            if !(position >= 0) || position >= len { range_error(job, "invalid index"); m.stop = true; return; }
            actual = position as i64;
            micro_unit_done(job, m);
        }
        let a = copier_plain(job, m, b, len);
        if m.stop { return; }
        job.frames[m.at].output = a;
        mutate_put(job, m.base, mutate_slot::P, 0); mutate_put(job, m.base, mutate_slot::Q, actual);
        mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Copy);
        let fast = mutator_dense(job, o, len);
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            for i in len { if !set_element(heap, a, i, if reversing { sort_value(heap, o, len - 1 - i) } else if i == actual { copier_arg(job, m, 1) } else { sort_value(heap, o, i) }) { m.stop = true; return; } }
            micro_finish(job, m, a);
        }
        return;
    }
    let i = mutate_number(job, m.base, mutate_slot::P); let a = job.frames[m.at].output;
    if i >= len { micro_finish(job, m, a); return; }
    var v = Value.Undefined;
    if !reversing && i == mutate_number(job, m.base, mutate_slot::Q) { v = copier_arg(job, m, 1); }
    else {
        v = micro_get(job, m, b, o, if reversing { len - 1 - i } else { i });
        if m.stop { return; }
    }
    if !set_element(heap, a, i, v) { m.stop = true; return; }
    mutate_put(job, m.base, mutate_slot::P, i + 1);
    micro_unit_done(job, m);
}

// ---- Array.of(...items) (23.1.2.3)
fn of_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayOf;
    let c = job.frames[m.at].receiver; let count = job.frames[m.at].index;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        var a = Value.Undefined;
        if is_constructor(heap, c) && !(c.is_object() && c.id() == array_constructor_row()) {
            let length: Value[1] = [number(count)];
            a = micro_construct(job, m, c, length[..]);
            if m.stop { return; }
            micro_unit_done(job, m);
        } else {
            a = copier_plain(job, m, b, count);
            if m.stop { return; }
        }
        job.frames[m.at].output = a;
        mutate_put(job, m.base, mutate_slot::P, 0); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Items);
        return;
    }
    let a = job.frames[m.at].output; let k = mutate_number(job, m.base, mutate_slot::P);
    if stage == mutate_stage::Items {
        if k >= count { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish); return; }
        micro_create(job, m, a, k, job.operands[m.base + mutate_slot::Args + k]);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::P, k + 1);
        micro_unit_done(job, m);
        return;
    }
    micro_set_length(job, m, b, a, count);
    if m.stop { return; }
    micro_finish(job, m, a);
}

// ---- Array.from(items, mapfn, thisArg) (23.1.2.1)
fn from_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayFrom;
    let c = job.frames[m.at].receiver;
    let items = copier_arg(job, m, 0); let mapfn = copier_arg(job, m, 1); let this_arg = copier_arg(job, m, 2);
    let constructs = is_constructor(heap, c) && !(c.is_object() && c.id() == array_constructor_row());
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        // GetMethod(items, @@iterator): undefined and null are no object to
        // read; an undefined or null method makes items an array-like.
        let source = form_value(heap, items);
        if source.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); m.stop = true; return; }
        let method = micro_get_key(job, m, b, source, Value.Symbol { s: well_known::Iterator });
        if m.stop { return; }
        if method.tag() <= value_tag::Null {
            let like = to_object(job, source);
            if heap.fault != Fault.None { m.stop = true; return; }
            micro_unit_done(job, m);
            job.frames[m.at].input = like;
            mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Like);
            return;
        }
        if !is_callable(heap, method) { type_error(job, "value is not iterable"); m.stop = true; return; }
        var a = Value.Undefined;
        if constructs { a = micro_construct(job, m, c, []); if m.stop { return; } }
        let record = micro_iterator(job, m, source, method);
        if m.stop { return; }
        if !constructs { a = copier_plain(job, m, b, 0); if m.stop { return; } }
        micro_unit_done(job, m);
        job.frames[m.at].output = a; job.frames[m.at].callee = record;
        mutate_put(job, m.base, mutate_slot::P, 0); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Step);
        return;
    }
    let k = mutate_number(job, m.base, mutate_slot::P);
    if stage == mutate_stage::Like {
        // The array-like's length, then the result.
        let like = job.frames[m.at].input;
        let length = micro_length(job, m, b, like);
        if m.stop { return; }
        var a = Value.Undefined;
        if constructs {
            let argument: Value[1] = [number(length)];
            a = micro_construct(job, m, c, argument[..]);
            if m.stop { return; }
        } else {
            a = copier_plain(job, m, b, length);
            if m.stop { return; }
        }
        micro_unit_done(job, m);
        job.frames[m.at].output = a; job.frames[m.at].argument_count = length;
        mutate_put(job, m.base, mutate_slot::P, 0); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Items);
        return;
    }
    let a = job.frames[m.at].output;
    if stage == mutate_stage::Items {
        // An array-like's index k: Get, the map function, the definition.
        let like = job.frames[m.at].input; let length = job.frames[m.at].argument_count;
        if k >= length { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish); return; }
        var v = micro_get(job, m, b, like, k);
        if m.stop { return; }
        if !mapfn.is_undefined() {
            let pair: Value[2] = [v, number(k)];
            v = micro_call(job, m, mapfn, this_arg, pair[..]);
            if m.stop { return; }
        }
        micro_create(job, m, a, k, v);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::P, k + 1);
        micro_unit_done(job, m);
        return;
    }
    if stage == mutate_stage::Finish {
        micro_set_length(job, m, b, a, k);
        if m.stop { return; }
        micro_finish(job, m, a);
        return;
    }
    let record = job.frames[m.at].callee;
    if !record.is_object() { heap.fault = Fault.InvalidState; m.stop = true; return; }
    let it = record_field(job, record, "iterator");
    if heap.fault != Fault.None { m.stop = true; return; }
    if stage == mutate_stage::Step {
        // IteratorStepValue: next(), its `done`, its `value`.
        let next = record_field(job, record, "next");
        if heap.fault != Fault.None { m.stop = true; return; }
        if !is_callable(heap, next) { type_error(job, "iterator next is not a function"); m.stop = true; return; }
        let r = micro_call(job, m, next, it, []);
        if m.stop { return; }
        if !r.is_object() { type_error(job, "iterator result is not an object"); m.stop = true; return; }
        let done = micro_get_named(job, m, b, r, "done");
        if m.stop { return; }
        if truthy(heap, form_value(heap, done)) {
            micro_unit_done(job, m);
            mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish);
            return;
        }
        let v = micro_get_named(job, m, b, r, "value");
        if m.stop { return; }
        micro_unit_done(job, m);
        job.operands[m.base + mutate_slot::Held] = v;
        mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Map);
        return;
    }
    if stage == mutate_stage::Map {
        // The item mapped and defined, an abrupt completion closing the iterator.
        copier_guard(job, m, mutate_pc::Caught);
        var v = job.operands[m.base + mutate_slot::Held];
        if !mapfn.is_undefined() {
            let pair: Value[2] = [v, number(k)];
            v = micro_call(job, m, mapfn, this_arg, pair[..]);
            if m.stop { return; }
        }
        micro_create(job, m, a, k, v);
        if m.stop { return; }
        close_unguard(job, m.at, mutate_pc::Caught);
        micro_unit_done(job, m);
        job.operands[m.base + mutate_slot::Held] = Value.Undefined;
        mutate_put(job, m.base, mutate_slot::P, k + 1); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Step);
        return;
    }
    // Close: IteratorClose for the exception in Held, which is rethrown
    // whatever `return` does.
    copier_guard(job, m, mutate_pc::Closed);
    let method = micro_get_named(job, m, b, it, "return");
    if m.stop { return; }
    if method.tag() > value_tag::Null && is_callable(heap, form_value(heap, method)) {
        micro_call(job, m, form_value(heap, method), it, []);
        if m.stop { return; }
    }
    close_unguard(job, m.at, mutate_pc::Closed);
    m.stop = true;
    raise(job, job.operands[m.base + mutate_slot::Held], job.frames[m.at]);
}
