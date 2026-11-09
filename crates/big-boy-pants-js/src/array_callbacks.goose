import std;
import math;
import .source_vm;
import .realm;
import .function_calls;
import .runtime_compile;
import .descriptors;
import .prototypes;
import .object_statics;

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
//   pc 2      awaits element k from its getter (or a proxy's [[Get]])
//   pc 3      awaits the callback's result for index k
//   pc 4      awaits a proxy's answer to HasProperty(O, k) (light-cljs-890)
//   pc 5      (map) awaits O's `constructor` from its getter or a proxy's
//             [[Get]], for ArraySpeciesCreate
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
// `constructor` getter, and a host-effect or native-frame
// built-in as the callback.
//
// The template for areas porting a built-in that runs user code: a Builtin
// variant in realm.goose (BUILTINS, REALM_FUNCTIONS), a start function the
// dispatch loop's `call` reaches through native_start, and a step function
// native_step dispatches by `match` on its Native variant.

// Whether built-in b runs as a native frame (native_start).
fn native_builtin(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeMap || b == Builtin.ArrayPrototypeForEach || b == Builtin.Swap || b == Builtin.SwapVals || generator_method(b) || b == Builtin.FunctionPrototypeHasInstance || array_method(b) || iterator_native(b) || forwarding(b) || b == Builtin.FunctionPrototypeBind || b == Builtin.Eval || b == Builtin.Function || b == Builtin.IsInstance || descriptor_native(b) || prototype_native(b) || statics_native(b); }
// Start built-in b, called with `receiver` and `args`, as a native frame
// whose operand base is `base`, where the call's operands began: the
// dispatch loop's `call` cuts the operands to it and the finished frame
// pushes its result there.
fn native_start(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    // swap! and swap-vals! (cells.goose).
    if b == Builtin.Swap || b == Builtin.SwapVals { var copied: Value[>..] = []; copied.append(args); start_swap(job, b, copied, base); return; }
    // Function.prototype.call and apply, Reflect.apply and Reflect.construct
    // (light-cljs-851; function_calls.goose).
    if forwarding(b) { start_forward(job, b, receiver, args, base); return; }
    // Function.prototype.bind (light-cljs-852; function_calls.goose).
    if b == Builtin.FunctionPrototypeBind { start_bind(job, receiver, args, base); return; }
    // The global eval called indirectly (light-cljs-856; runtime_compile.goose).
    if b == Builtin.Eval { start_indirect_eval(job, args, base); return; }
    // The Function constructor called (light-cljs-857; runtime_compile.goose).
    if b == Builtin.Function { start_dynamic_function(job, args, base, Value.Undefined); return; }
    // A generator's next, return and throw (continuations.goose).
    if generator_method(b) { start_resume(job, b, receiver, arg(args, 0), base); return; }
    // Function.prototype[@@hasInstance] (light-cljs-834; source_vm.goose).
    if b == Builtin.FunctionPrototypeHasInstance { start_function_has_instance(job, receiver, arg(args, 0), base); return; }
    // rt.core.isInstance (light-cljs-875; source_vm.goose).
    if b == Builtin.IsInstance { start_is_instance(job, arg(args, 0), arg(args, 1), base); return; }
    // Object.defineProperty, Object.defineProperties and
    // Reflect.defineProperty (light-cljs-881; descriptors.goose).
    if descriptor_native(b) { start_descriptor(job, b, args, base); return; }
    // Object.create, whose properties argument runs
    // Object.defineProperties's Descriptor frame, and the [[GetPrototypeOf]]
    // readers, whose proxy runs its Proxy frame (light-cljs-883;
    // prototypes.goose).
    if prototype_native(b) { start_prototype(job, b, receiver, args, base); return; }
    // The Object statics that expose own keys, Reflect.ownKeys and has
    // (light-cljs-884; object_statics.goose).
    if statics_native(b) { start_statics(job, b, receiver, args, base); return; }
    // %ArrayIteratorPrototype%.next and String.prototype[@@iterator]
    // (light-cljs-844; iterators.goose).
    if iterator_native(b) { start_iterator(job, b, receiver, base); return; }
    let callback = arg(args, 0); let this_arg = arg(args, 1);
    // push, join and toString (light-cljs-842; array_methods.goose).
    if array_method(b) { let r = start_array_method(job, b, receiver, args, base); if r >= 0 { job.operands.resize(base); job.operands.push(number(r)); } return; }
    job.operands.resize(base);
    start_array_callbacks(job, b, receiver, callback, this_arg, base);
}
// Steps 1 and 2: ToObject(this) and Get(O, "length"), whose value the
// frame's pc 0 takes; a getter runs above the frame.
fn start_array_callbacks(job: Job&, b: Builtin, receiver: Value, callback: Value, this_arg: Value, base: i64) {
    let heap .= job.heap;
    if receiver.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); return; }
    if !receiver.is_object() { unsupported_step(job, b, "primitive wrapper objects"); return; }
    let k = key_named(heap, "length");
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, receiver, k);
    if heap.fault != Fault.None { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: callback, receiver: this_arg, input: receiver, native: Native.ArrayCallbacks { which: b } });
    if getter.is_object() { run_getter(job, b, getter, receiver); } else { job.operands.push(v); }
}
// Run getter `g` with receiver o on the general call path (call_accessor,
// light-cljs-881); its value arrives at the frame's current phase.
fn run_getter(job: Job&, b: Builtin, g: Value, o: Value) { call_accessor(job, g, o, []); }
// One step of the frame at the top (see above), dispatched on its phase.
fn array_callbacks_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match frame.pc {
        0 => {
            // A scalar form (a primitive with its provenance) is its primitive.
            let v = form_value(heap, job.operands.pop());
            if v.is_object() {
                // ToNumber of an object: its ToPrimitive with hint number
                // (@@toPrimitive, then valueOf and toString) and ToNumber in
                // a Convert frame above this one (light-cljs-828), which
                // leaves the number where the length was; this phase then
                // runs again and takes it.
                if !convertible(heap, v) { unsupported_step(job, b, "ToNumber of a light-cljs runtime value"); return; }
                job.operands.push(v); start_convert(job, Hint.Number, true, 1, 1); return;
            }
            let n = numeric(heap, v);
            if heap.fault != Fault.None { return; }
            // ToLength.
            let len: f64 = if !(n > 0) { 0 } else { min(trunc(n), 9007199254740991.0) };
            if !callable(heap, frame.callee) { type_error(job, "not a function"); return; }
            job.frames[at].argument_count = len as i64; job.frames[at].pc = 1;
            if b == Builtin.ArrayPrototypeMap {
                // ArraySpeciesCreate: an array's (IsArray: a proxy's final
                // target's) `constructor`, its getter or a proxy's [[Get]]
                // above the frame, whose value arrives at pc 5.
                if is_array(heap, frame.input) {
                    let k = key_named(heap, "constructor");
                    if heap.fault != Fault.None { return; }
                    let c, getter = lookup(heap, frame.input, k);
                    if heap.fault != Fault.None { return; }
                    if getter.is_object() { job.frames[at].pc = 5; run_getter(job, b, getter, frame.input); return; }
                    if !species_constructor(job, b, c) { return; }
                }
                let a = array_species_create(job, b, frame.input, len);
                if heap.fault != Fault.None { return; }
                job.frames[at].output = a;
            }
            array_visit(job, at, b);
        },
        // A proxy's answer to HasProperty(O, k) (light-cljs-890).
        4 => {
            job.frames[at].pc = 1;
            if truthy(heap, job.operands.pop()) { array_get(job, at, b); } else { job.frames[at].index += 1; }
        },
        // The `constructor` from a getter or a proxy's [[Get]].
        5 => {
            if !species_constructor(job, b, job.operands.pop()) { return; }
            let a = array_species_create(job, b, frame.input, frame.argument_count as f64);
            if heap.fault != Fault.None { return; }
            job.frames[at].output = a; job.frames[at].pc = 1;
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
// IsCallable: a closure, a built-in function or a bound function (a class constructor is
// callable; calling it throws TypeError when its frame is entered).
inline fn callable(heap: const Heap&, f: Value) -> bool { return f.is_object() && (node(heap, f.id()).kind == node_kind::Closure || node(heap, f.id()).kind == node_kind::Builtin || node(heap, f.id()).kind == node_kind::BoundFunction); }
// IsArray(v): an array, or a proxy whose final target is one.
fn is_array(heap: const Heap&, v: Value) -> bool {
    var t = v;
    while t.is_object() && node(heap, t.id()).kind == node_kind::Proxy { t = node(heap, t.id()).value; }
    return t.is_object() && node(heap, t.id()).kind == node_kind::Array;
}
// ArraySpeciesCreate's check of an array's `constructor` c (map's pc 0 or
// 5 read it): undefined or the realm's Array make a plain array; any other
// constructor object is the species step, not implemented.
fn species_constructor(job: Job&, b: Builtin, c: Value) -> bool {
    if c.is_object() { if c.id() != array_constructor_row() { unsupported_step(job, b, "species constructors"); return false; } }
    else if !c.is_undefined() { type_error(job, "array constructor is not an object"); return false; }
    return true;
}
// ArraySpeciesCreate(O, len) once O's `constructor` passed
// species_constructor (or O is no array): a new array of length len.
fn array_species_create(job: Job&, b: Builtin, o: Value, len: f64) -> Value {
    let heap .= job.heap;
    // ArrayCreate(len).
    if len > 4294967295.0 { job.detail.clear(); for c in "invalid array length" { job.detail.push(c); } heap.fault = Fault.RangeError; return Value.Undefined; }
    let a = allocate(heap, Node { kind: node_kind::Array });
    if heap.fault != Fault.None || !array_length(heap, a, number(len)) { return Value.Undefined; }
    // A dense receiver whose element run holds len slots gets a result run of
    // len holes at once, so map's stores never copy a growing run. Any other
    // length (a generic array-like may claim 2^32 - 1) grows as stores come.
    if len > 0 && node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized && len <= node(heap, o.id()).count && !element_room(heap, a.id(), (len as i64) - 1) { return Value.Undefined; }
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
    if node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized {
        let v, found = own_element(heap, o, k);
        if found { array_call(job, at, b, v); return; }
    }
    let key = index_key(heap, k);
    if heap.fault != Fault.None { return; }
    let has = has_property(heap, o, key);
    // A proxy's [[HasProperty]] answers at pc 4 (light-cljs-890).
    if proxy_pending(heap, proxy_method::HasProperty) { job.frames[at].pc = 4; proxy_take(job, proxy_method::HasProperty); return; }
    if heap.fault != Fault.None { return; }
    if !has { job.frames[at].index += 1; return; }
    array_get(job, at, b);
}
// Get(O, k) of the present index k: its value to the callback now or, from
// a getter or a proxy's [[Get]], at pc 2.
fn array_get(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap;
    let o = job.frames[at].input;
    let key = index_key(heap, job.frames[at].index);
    if heap.fault != Fault.None { return; }
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
    // A bound function runs in a Forward frame of its own (light-cljs-852).
    if is_bound(heap, f) { start_bound(job, f, this_arg, args, job.operands.len); return; }
    if node(heap, f.id()).kind == node_kind::Builtin {
        let cb = builtin_of(heap, f.id());
        // A delay forced as the callback runs in a frame of its own (cells.goose).
        if forces(heap, cb, args) { start_force(job, cb, v, job.operands.len); return; }
        // A forwarding built-in (light-cljs-851) runs in a frame of its own.
        if forwarding(cb) { native_start(job, cb, this_arg, args, job.operands.len); return; }
        // So do the Object statics (light-cljs-884; object_statics.goose).
        if statics_native(cb) { native_start(job, cb, this_arg, args, job.operands.len); return; }
        // So do the error built-ins when a step runs JavaScript (light-cljs-870).
        if conditional_native(cb) { start_conditional_native(job, cb, this_arg, args, job.operands.len); return; }
        if effect_builtin(cb) || cb == Builtin.Mapv || cb == Builtin.MapSeq || native_builtin(cb) { unsupported_step(job, b, "a host-effect or native-frame built-in callback"); return; }
        let r = call_builtin(job, node(heap, f.id()).function, this_arg, args);
        if heap.fault == Fault.None { job.operands.push(r); }
        return;
    }
    push_frame(job, f, this_arg, args, false, job.operands.len);
}
