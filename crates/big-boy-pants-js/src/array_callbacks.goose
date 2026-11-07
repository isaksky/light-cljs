import std;
import math;
import .source_vm;
import .realm;

namespace source;

// Array.prototype.map and Array.prototype.forEach (light-cljs-789): the
// reference built-ins that run user code (decision 5, rule 1 of
// docs/javascript-coverage.md). Ported from the Pants engine's
// rust/src/vm/native/array_callbacks.rs, whose resumable State is this
// design, with the specification's steps (ECMAScript 23.1.3.15 and .21):
// ToObject(this), LengthOfArrayLike, the IsCallable check, for map
// ArraySpeciesCreate, then for each index below the length HasProperty, Get,
// the callback with (value, index, object) and, for map,
// CreateDataPropertyOrThrow on the result.
//
// The operation is a native frame (Native.ArrayCallbacks, its Builtin the
// payload) with its whole state in the frame's fields; it never calls the
// interpreter. Every step that can run JavaScript (the `length` getter, the
// length's valueOf or toString, an element getter, the callback) enters
// that function's frame above this one, sets the phase (`pc`) that consumes
// its result and returns to the dispatch loop; the callee's Return pushes
// the result at the frame's operand base. An exception unwinds through the
// frame like any frame's.
//   callee    the callback          receiver  thisArg
//   input     O, the receiver       output    A, map's result (else undefined)
//   index     k, the next index     argument_count  len, LengthOfArrayLike(O)
//   pc 0      awaits the length's value (Get(O, "length"), then ToNumber)
//   pc 1      visits index k (scan)
//   pc 2      awaits element k from its getter
//   pc 3      awaits the callback's result for index k
// A step visits at most one index, so the budget, the instruction limit and
// a suspension see a boundary between callbacks, holes included.
//
// The fast path: an own data element of a dense array answers HasProperty
// and Get with no user code. It is re-read on every visit, because a
// callback may delete elements, install accessors, change the prototype or
// shorten the array; anything else (a hole, a dictionary array, an ordinary
// or arguments object) takes the specification's general steps.
//
// Not implemented yet, each a named UnsupportedOperand (decision 1), never a
// TypeError JavaScript could catch: a primitive receiver other than
// undefined and null (String wrapper objects), species (an Array receiver
// whose `constructor` is neither undefined nor the realm's Array), a
// `constructor` getter, a built-in getter, and a host-effect or native-frame
// built-in as the callback.
//
// The template for areas porting a built-in that runs user code: a Builtin
// variant in realm.goose (BUILTINS, REALM_FUNCTIONS), a start function the
// dispatch loop's `call` reaches through native_start, and a step function
// native_step dispatches by `match` on its Native variant.

// Whether built-in b runs as a native frame (native_start).
fn native_builtin(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeMap || b == Builtin.ArrayPrototypeForEach || b == Builtin.Swap || b == Builtin.SwapVals || generator_method(b); }
// Start built-in b, called with `receiver` and `args`, as a native frame
// whose operand base is `base`, where the call's operands began: the
// dispatch loop's `call` cuts the operands to it and the finished frame
// pushes its result there.
fn native_start(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    // swap! and swap-vals! (cells.goose).
    if b == Builtin.Swap || b == Builtin.SwapVals { var copied: Value[>..] = []; copied.append(args); start_swap(job, b, copied, base); return; }
    // A generator's next, return and throw (continuations.goose).
    if generator_method(b) { start_resume(job, b, receiver, arg(args, 0), base); return; }
    let callback = arg(args, 0); let this_arg = arg(args, 1);
    job.operands.resize(base);
    start_array_callbacks(job, b, receiver, callback, this_arg, base);
}
// Steps 1 and 2: ToObject(this) and Get(O, "length"), whose value the
// frame's pc 0 takes; a getter runs above the frame.
fn start_array_callbacks(job: Job&, b: Builtin, receiver: Value, callback: Value, this_arg: Value, base: i64) {
    let heap .= job.heap;
    if receiver.tag() <= value_tag::Null { type_error(job, "cannot convert undefined or null to object"); return; }
    if !receiver.is_object() { unsupported_step(job, b, "primitive wrapper objects"); return; }
    let k = key_named(heap, "length");
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, receiver, k);
    if heap.fault != Fault.None { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: callback, receiver: this_arg, input: receiver, native: Native.ArrayCallbacks { which: b } });
    if getter.is_object() { run_getter(job, b, getter, receiver); } else { job.operands.push(v); }
}
// Enter getter `g` with receiver o; its value arrives at the frame's
// current phase. Only an ordinary function can be a getter here.
fn run_getter(job: Job&, b: Builtin, g: Value, o: Value) {
    if node(job.heap, g.id()).kind != 2 { unsupported_step(job, b, "a built-in getter"); return; }
    push_frame(job, g, o, [], false, job.operands.len);
}
// One step of the frame at the top (see above), dispatched on its phase.
fn array_callbacks_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match frame.pc {
        0 => {
            // A scalar form (a primitive with its provenance) is its primitive.
            let v = form_value(heap, job.operands.pop());
            if v.is_object() {
                // ToNumber of an object: its ToPrimitive (valueOf, then
                // toString) in a native frame above this one, as `v - 0`;
                // the number comes back to this phase.
                // The ToPrimitive frame runs OrdinaryToPrimitive only, so a
                // length with a @@toPrimitive method (own or inherited, even
                // behind a getter) is a named unsupported step.
                if !ordinary(heap, v) { unsupported_step(job, b, "ToNumber of a non-ordinary length"); return; }
                let hook, hook_getter = lookup(heap, v, Value.Symbol { s: well_known::ToPrimitive });
                if heap.fault != Fault.None { return; }
                if hook_getter.is_object() || hook.tag() > value_tag::Null { unsupported_step(job, b, "a length's @@toPrimitive"); return; }
                to_primitive(job, 8, v, number(0)); return;
            }
            let n = numeric(heap, v);
            if heap.fault != Fault.None { return; }
            // ToLength.
            let len: f64 = if !(n > 0) { 0 } else { min(trunc(n), 9007199254740991.0) };
            if !callable(heap, frame.callee) { type_error(job, "not a function"); return; }
            if b == Builtin.ArrayPrototypeMap {
                let a = array_species_create(job, b, frame.input, len);
                if heap.fault != Fault.None { return; }
                job.frames[at].output = a;
            }
            job.frames[at].argument_count = len as i64; job.frames[at].pc = 1;
            array_visit(job, at, b);
        },
        2 => { let v = job.operands.pop(); array_call(job, at, b, v); },
        3 => {
            let r = job.operands.pop();
            // CreateDataPropertyOrThrow(A, k, r): A is the new array this
            // frame made, which no JavaScript has seen, so it always succeeds
            // or fails a VM limit.
            if b == Builtin.ArrayPrototypeMap && !set_element(heap, frame.output, frame.index, r) { return; }
            job.frames[at].index += 1; job.frames[at].pc = 1;
            array_visit(job, at, b);
        },
        _ => { array_visit(job, at, b); },
    }
}
// IsCallable: a closure or a built-in function (a class constructor is
// callable; calling it throws TypeError when its frame is entered).
inline fn callable(heap: const Heap&, f: Value) -> bool { return f.is_object() && (node(heap, f.id()).kind == 2 || node(heap, f.id()).kind == 4); }
// ArraySpeciesCreate(O, len) without species: a new array of length len
// when O is not an array or its `constructor` is undefined or the realm's
// Array; any other constructor object is the species step, not implemented.
fn array_species_create(job: Job&, b: Builtin, o: Value, len: f64) -> Value {
    let heap .= job.heap;
    if node(heap, o.id()).kind == 1 {
        let k = key_named(heap, "constructor");
        if heap.fault != Fault.None { return Value.Undefined; }
        let c, getter = lookup(heap, o, k);
        if heap.fault != Fault.None { return Value.Undefined; }
        if getter.is_object() { return unsupported_step(job, b, "a constructor getter"); }
        if c.is_object() { if c.id() != array_constructor_row() { return unsupported_step(job, b, "species constructors"); } }
        else if !c.is_undefined() { return type_error(job, "array constructor is not an object"); }
    }
    // ArrayCreate(len).
    if len > 4294967295.0 { job.detail.clear(); for c in "invalid array length" { job.detail.push(c); } heap.fault = Fault.RangeError; return Value.Undefined; }
    let a = allocate(heap, Node { kind: 1 });
    if heap.fault != Fault.None || !array_length(heap, a, number(len)) { return Value.Undefined; }
    // A dense receiver whose element run holds len slots gets a result run of
    // len holes at once, so map's stores never copy a growing run. Any other
    // length (a generic array-like may claim 2^32 - 1) grows as stores come.
    if len > 0 && node(heap, o.id()).kind == 1 && !node(heap, o.id()).initialized && len <= node(heap, o.id()).count && !element_room(heap, a.id(), (len as i64) - 1) { return Value.Undefined; }
    return a;
}
// Visit index k of the frame at `at`: finish at the length; otherwise
// HasProperty(O, k), skipping a hole in this step, then Get(O, k), whose
// value goes to the callback now or, from a getter, at pc 2.
fn array_visit(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap;
    let k = job.frames[at].index; let o = job.frames[at].input;
    if k >= job.frames[at].argument_count {
        finish_native(job, job.frames[at], if b == Builtin.ArrayPrototypeMap { job.frames[at].output } else { Value.Undefined });
        return;
    }
    if node(heap, o.id()).kind == 1 && !node(heap, o.id()).initialized {
        let v, found = own_element(heap, o, k);
        if found { array_call(job, at, b, v); return; }
    }
    let key = index_key(heap, k);
    if heap.fault != Fault.None { return; }
    if !has_property(heap, o, key) {
        if heap.fault == Fault.None { job.frames[at].index += 1; }
        return;
    }
    let v, getter = lookup(heap, o, key);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { job.frames[at].pc = 2; run_getter(job, b, getter, o); return; }
    array_call(job, at, b, v);
}
// Call the frame's callback with thisArg and (v, k, O); its result arrives
// at pc 3. A plain built-in callback runs here and pushes its result; one
// that requests a host effect or needs a native frame of its own fails by
// name.
fn array_call(job: Job&, at: i64, b: Builtin, v: Value) {
    let heap .= job.heap;
    job.frames[at].pc = 3;
    let f = job.frames[at].callee; let this_arg = job.frames[at].receiver;
    let args: Value[3] = [v, number(job.frames[at].index), job.frames[at].input];
    if node(heap, f.id()).kind == 4 {
        let cb = builtin_of(heap, f.id());
        // A delay forced as the callback runs in a frame of its own (cells.goose).
        if forces(heap, cb, args) { start_force(job, cb, v, job.operands.len); return; }
        if effect_builtin(cb) || cb == Builtin.Mapv || cb == Builtin.MapSeq || native_builtin(cb) { unsupported_step(job, b, "a host-effect or native-frame built-in callback"); return; }
        let r = call_builtin(job, node(heap, f.id()).function, this_arg, args);
        if heap.fault == Fault.None { job.operands.push(r); }
        return;
    }
    push_frame(job, f, this_arg, args, false, job.operands.len);
}
