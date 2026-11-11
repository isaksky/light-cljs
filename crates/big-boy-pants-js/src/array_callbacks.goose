import std;
import math;
import .source_vm;
import .realm;
import .function_calls;
import .runtime_compile;
import .descriptors;
import .prototypes;
import .object_statics;
import .regexp_object;

namespace source;

// The Array.prototype methods that call a callback: map and forEach
// (light-cljs-789), the reference built-ins that run user code (decision 5,
// rule 1 of docs/javascript-coverage.md), and filter, some, every, reduce,
// reduceRight, flatMap, find, findIndex, findLast and findLastIndex
// (light-cljs-908). Ported from the Pants engine's
// rust/src/vm/native/array_callbacks.rs, whose resumable State is this
// design, with the specification's steps (ECMAScript 23.1.3): ToObject(this),
// LengthOfArrayLike, the IsCallable check, for map, filter and flatMap
// ArraySpeciesCreate, then for each index below the length HasProperty, Get,
// the callback and what the method does with its result.
//
// The operation is a native frame (Native.ArrayCallbacks, its Builtin the
// payload) with its whole state in the frame's fields and a few scratch
// operands; it never calls the interpreter. Every step that can run
// JavaScript (the `length` getter, the length's valueOf or toString, an
// element getter, a proxy trap, a `constructor` or @@species getter, the
// species constructor, the callback) enters that function's frame above
// this one, sets the phase (`pc`) that consumes its result and returns to
// the dispatch loop; the callee's Return pushes the result on the frame's
// operands. An exception unwinds through the frame like any frame's.
//   callee    the callback          receiver  thisArg
//   input     O, the receiver       argument_count  len, LengthOfArrayLike(O)
// The other fields, the scratch operands and the phases are listed with the
// step function below. A step visits at most one index, so the budget, the
// instruction limit and a suspension see a boundary between callbacks, holes
// included.
//
// The fast path: an own data element of a dense array answers HasProperty
// and Get with no user code. It is re-read on every visit, because a
// callback may delete elements, install accessors, change the prototype or
// shorten the array; anything else (a hole, a dictionary array, an ordinary
// or arguments object, a string's wrapper, a proxy) takes the
// specification's general steps.
//
// Not implemented yet, each a named UnsupportedOperand (decision 1), never a
// TypeError JavaScript could catch: a BigInt receiver (light-cljs-771), a
// length that is a light-cljs runtime value, and a host-effect built-in as
// the callback.
//
// The template for areas porting a built-in that runs user code: a Builtin
// variant in realm.goose (BUILTINS, REALM_FUNCTIONS), a start function the
// dispatch loop's `call` reaches through native_start, and a step function
// native_step dispatches by `match` on its Native variant.

// Whether built-in b runs as a native frame (native_start).
fn native_builtin(b: Builtin) -> bool { return array_callback(b) || array_search(b) || b == Builtin.Swap || b == Builtin.SwapVals || generator_method(b) || b == Builtin.FunctionPrototypeHasInstance || array_method(b) || iterator_native(b) || forwarding(b) || b == Builtin.FunctionPrototypeBind || b == Builtin.Eval || b == Builtin.Function || b == Builtin.IsInstance || descriptor_native(b) || prototype_native(b) || statics_native(b) || b == Builtin.MathSumPrecise || regexp_native(b); }
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
    // Math.sumPrecise (light-cljs-901; math_object.goose).
    if b == Builtin.MathSumPrecise { start_sum_precise(job, arg(args, 0), base); return; }
    // RegExp called, and RegExp.prototype.flags and toString (light-cljs-932;
    // regexp_object.goose).
    if regexp_native(b) { start_regexp_native(job, b, receiver, args, base); return; }
    // %ArrayIteratorPrototype%.next and String.prototype[@@iterator]
    // (light-cljs-844; iterators.goose).
    if iterator_native(b) { start_iterator(job, b, receiver, base); return; }
    // The search methods (light-cljs-908; array_search.goose).
    if array_search(b) { start_array_search(job, b, receiver, args, base); return; }
    // The callback methods (light-cljs-789, -908).
    if array_callback(b) { start_array_callbacks(job, b, receiver, args, base); return; }
    // push, join and toString (light-cljs-842; array_methods.goose), last:
    // a push the fast path finished cuts the operands `args` views.
    let r = start_array_method(job, b, receiver, args, base);
    if r >= 0 { job.operands.resize(base); job.operands.push(number(r)); }
}
// The callback methods (light-cljs-908): map and forEach above, filter,
// some, every, reduce, reduceRight, flatMap, find, findIndex, findLast and
// findLastIndex. One frame runs them all (ECMAScript 23.1.3.8 to .28; the
// Pants engine's array_callbacks.rs and array_search.rs; QuickJS's
// js_array_every, js_array_reduce, js_array_find and js_array_flatten):
//   output    map, filter, flatMap: the result A; reduce, reduceRight: the
//             accumulator; find and its siblings: the element being tested
//   index     k, forward from 0 or, for reduceRight, findLast and
//             findLastIndex, backward from len - 1
// and scratch operands at the operand base (callbacks_slot::), below the
// values its steps await:
//   reduce, reduceRight   Seeded, whether the accumulator holds a value
//                         (the initial value, else the first present element)
//   filter                Kept, the element the callback is judging; To, the
//                         next result index
//   flatMap               Inner, the mapped array being flattened; To;
//                         InnerIndex and InnerLength, its next index and
//                         LengthOfArrayLike
// Phases (callbacks_pc::), each awaiting one value from a frame above:
//   Length       Get(O, "length") (then its ToNumber in a Convert frame)
//   Visit        visits index k
//   Element      element k from its getter or a proxy's [[Get]]
//   Result       the callback's result for index k
//   Has          a proxy's answer to HasProperty(O, k) (light-cljs-890)
//   Constructor  ArraySpeciesCreate: O's `constructor` from its getter
//   Species      the constructor's @@species from its getter
//   Created      Construct(species, [length])'s object, the result A
//   Stored       a proxy result's [[DefineOwnProperty]] answer
//                (CreateDataPropertyOrThrow; a false one throws there)
//   InnerLength, InnerVisit, InnerHas, InnerElement, InnerStored
//                flatMap's walk of a mapped array, as Length, Visit, Has,
//                Element and Stored are the receiver's
// find and its siblings read every index (no HasProperty); the others skip
// an absent one.
let callbacks_pc::Length = 0;
let callbacks_pc::Visit = 1;
let callbacks_pc::Element = 2;
let callbacks_pc::Result = 3;
let callbacks_pc::Has = 4;
let callbacks_pc::Constructor = 5;
let callbacks_pc::Species = 6;
let callbacks_pc::Created = 7;
let callbacks_pc::Stored = 8;
let callbacks_pc::InnerLength = 9;
let callbacks_pc::InnerVisit = 10;
let callbacks_pc::InnerHas = 11;
let callbacks_pc::InnerElement = 12;
let callbacks_pc::InnerStored = 13;
let callbacks_slot::Seeded = 0;
let callbacks_slot::Kept = 0;
let callbacks_slot::Inner = 0;
let callbacks_slot::To = 1;
let callbacks_slot::InnerIndex = 2;
let callbacks_slot::InnerLength = 3;

// Whether b is one of the callback methods.
fn array_callback(b: Builtin) -> bool {
    return match b {
        ArrayPrototypeMap, ArrayPrototypeForEach, ArrayPrototypeFilter, ArrayPrototypeReduce, ArrayPrototypeReduceRight, ArrayPrototypeSome, ArrayPrototypeEvery, ArrayPrototypeFlatMap, ArrayPrototypeFind, ArrayPrototypeFindIndex, ArrayPrototypeFindLast, ArrayPrototypeFindLastIndex => true,
        _ => false,
    };
}
inline fn callbacks_backward(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeReduceRight || b == Builtin.ArrayPrototypeFindLast || b == Builtin.ArrayPrototypeFindLastIndex; }
inline fn callbacks_finder(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeFind || b == Builtin.ArrayPrototypeFindIndex || b == Builtin.ArrayPrototypeFindLast || b == Builtin.ArrayPrototypeFindLastIndex; }
inline fn callbacks_reducer(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeReduce || b == Builtin.ArrayPrototypeReduceRight; }
// The methods whose result ArraySpeciesCreate makes.
inline fn callbacks_species(b: Builtin) -> bool { return b == Builtin.ArrayPrototypeMap || b == Builtin.ArrayPrototypeFilter || b == Builtin.ArrayPrototypeFlatMap; }
// The scratch operands b's frame keeps at its operand base.
inline fn callbacks_scratch(b: Builtin) -> i64 { return if b == Builtin.ArrayPrototypeFlatMap { 4 } else if b == Builtin.ArrayPrototypeFilter { 2 } else if callbacks_reducer(b) { 1 } else { 0 }; }
inline fn callbacks_advance(job: Job&, at: i64, b: Builtin) { job.frames[at].index += if callbacks_backward(b) { -1 } else { 1 }; }

// Steps 1 and 2: ToObject(this) and Get(O, "length"), whose value the
// frame's pc Length takes; a getter runs above the frame. `args` views the
// operands above `base`, which are cut to it here.
fn start_array_callbacks(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let callback = arg(args, 0); let second = arg(args, 1); let seeded = args.len > 1;
    job.operands.resize(base);
    // ToObject(this): undefined and null are a TypeError, a primitive its
    // wrapper object (a string's has its indices and length).
    let o = to_object(job, receiver);
    if heap.fault != Fault.None { return; }
    let k = key_named(heap, "length");
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, o, k);
    if heap.fault != Fault.None { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    if callbacks_scratch(b) + 1 > heap.limits.stack - job.locals.len - base { heap.fault = Fault.StackLimit; return; }
    // reduce and reduceRight take an initial value where the others take
    // thisArg; their callback's `this` is undefined.
    let reduces = callbacks_reducer(b);
    job.frames.push(Frame { function: -1, pc: callbacks_pc::Length, local_base: job.locals.len, operand_base: base, callee: callback, receiver: if reduces { Value.Undefined } else { second }, input: o, output: if reduces && seeded { second } else { Value.Undefined }, native: Native.ArrayCallbacks { which: b } });
    if reduces { job.operands.push(boolean(seeded)); }
    else { for i in callbacks_scratch(b) { job.operands.push(if i == callbacks_slot::Kept { Value.Undefined } else { number(0) }); } }
    if getter.is_object() { run_getter(job, b, getter, o); } else { job.operands.push(v); }
}
// Run getter `g` with receiver o on the general call path (call_accessor,
// light-cljs-881); its value arrives at the frame's current phase.
fn run_getter(job: Job&, b: Builtin, g: Value, o: Value) { call_accessor(job, g, o, []); }
// One step of the frame at the top (see above), dispatched on its phase.
fn array_callbacks_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1; let base = frame.operand_base;
    if job.operands.len < base + callbacks_scratch(b) || frame.pc >= callbacks_pc::InnerLength && b != Builtin.ArrayPrototypeFlatMap || frame.pc >= callbacks_pc::Constructor && frame.pc <= callbacks_pc::Stored && !callbacks_species(b) { heap.fault = Fault.InvalidState; return; }
    match frame.pc {
        callbacks_pc::Length => {
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
            if !is_callable(heap, frame.callee) { type_error(job, "not a function"); return; }
            job.frames[at].argument_count = len as i64; job.frames[at].index = if callbacks_backward(b) { (len as i64) - 1 } else { 0 }; job.frames[at].pc = callbacks_pc::Visit;
            if callbacks_species(b) && !species_begin(job, at, b) { return; }
            array_visit(job, at, b);
        },
        callbacks_pc::Element => { let v = job.operands.pop(); job.frames[at].pc = callbacks_pc::Visit; array_element(job, at, b, v); },
        callbacks_pc::Result => {
            let r = job.operands.pop();
            job.frames[at].pc = callbacks_pc::Visit;
            match b {
                // CreateDataPropertyOrThrow(A, k, r).
                ArrayPrototypeMap => { if !array_store(job, at, frame.output, frame.index, r, callbacks_pc::Stored) { return; } },
                ArrayPrototypeFilter => {
                    if truthy(heap, r) {
                        let to = job.operands[base + callbacks_slot::To].num();
                        job.operands[base + callbacks_slot::To] = number(to + 1);
                        if !array_store(job, at, frame.output, to as i64, job.operands[base + callbacks_slot::Kept], callbacks_pc::Stored) { return; }
                    }
                },
                ArrayPrototypeSome => { if truthy(heap, r) { finish_native(job, frame, boolean(true)); return; } },
                ArrayPrototypeEvery => { if !truthy(heap, r) { finish_native(job, frame, boolean(false)); return; } },
                ArrayPrototypeFind, ArrayPrototypeFindLast => { if truthy(heap, r) { finish_native(job, frame, frame.output); return; } },
                ArrayPrototypeFindIndex, ArrayPrototypeFindLastIndex => { if truthy(heap, r) { finish_native(job, frame, number(frame.index)); return; } },
                ArrayPrototypeReduce, ArrayPrototypeReduceRight => { job.frames[at].output = r; },
                ArrayPrototypeFlatMap => { flatten_begin(job, at, r); return; },
                _ => {},
            }
            callbacks_advance(job, at, b);
            array_visit(job, at, b);
        },
        // A proxy's answer to HasProperty(O, k) (light-cljs-890).
        callbacks_pc::Has => {
            job.frames[at].pc = callbacks_pc::Visit;
            if truthy(heap, job.operands.pop()) { array_get(job, at, b); } else { callbacks_advance(job, at, b); }
        },
        // ArraySpeciesCreate's reads and construction.
        callbacks_pc::Constructor => { job.frames[at].pc = callbacks_pc::Visit; if species_constructor(job, at, b, job.operands.pop()) { array_visit(job, at, b); } },
        callbacks_pc::Species => { job.frames[at].pc = callbacks_pc::Visit; if species_value(job, at, b, job.operands.pop()) { array_visit(job, at, b); } },
        callbacks_pc::Created => { job.frames[at].output = job.operands.pop(); job.frames[at].pc = callbacks_pc::Visit; },
        callbacks_pc::Stored => { job.operands.pop(); job.frames[at].pc = callbacks_pc::Visit; callbacks_advance(job, at, b); },
        callbacks_pc::InnerLength => {
            let len, done = length_value(job, b);
            if !done { return; }
            job.operands[base + callbacks_slot::InnerLength] = number(len); job.frames[at].pc = callbacks_pc::InnerVisit;
        },
        callbacks_pc::InnerVisit => { flatten_visit(job, at); },
        callbacks_pc::InnerHas => {
            job.frames[at].pc = callbacks_pc::InnerVisit;
            if truthy(heap, job.operands.pop()) { flatten_get(job, at); } else { job.operands[base + callbacks_slot::InnerIndex] = number(job.operands[base + callbacks_slot::InnerIndex].num() + 1); }
        },
        callbacks_pc::InnerElement => { let v = job.operands.pop(); flatten_element(job, at, v); },
        callbacks_pc::InnerStored => { job.operands.pop(); job.frames[at].pc = callbacks_pc::InnerVisit; },
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

// ---- ArraySpeciesCreate (ECMAScript 10.4.3.x ArraySpeciesCreate; QuickJS's
// JS_ArraySpeciesCreate; light-cljs-908)
// ArraySpeciesCreate(O, n) for the frame at `at`, n the length for map and
// 0 for filter and flatMap, its result the frame's output. True when the
// result is made; false with a fault, or with a step above the frame (a
// `constructor` or @@species getter, a proxy's [[Get]], the species
// constructor's call), whose value arrives at pc Constructor, Species or
// Created. A non-array O (IsArray: a proxy's final target decides, a
// revoked one a TypeError) makes a plain array.
fn species_begin(job: Job&, at: i64, b: Builtin) -> bool {
    let heap .= job.heap; let o = job.frames[at].input;
    let array = is_array_value(job, o);
    if heap.fault != Fault.None { return false; }
    if !array { return species_plain(job, at, b); }
    let k = key_named(heap, "constructor");
    if heap.fault != Fault.None { return false; }
    let c, getter = lookup(heap, o, k);
    if heap.fault != Fault.None { return false; }
    if getter.is_object() { job.frames[at].pc = callbacks_pc::Constructor; run_getter(job, b, getter, o); return false; }
    return species_constructor(job, at, b, c);
}
// O's `constructor` c: undefined makes a plain array, an object gives its
// @@species, and any other value is no constructor, a TypeError. The
// realm's Array answers at once while its @@species getter is the realm's
// own (intrinsic_species), which returns it: Construct(Array, [n]) is
// ArrayCreate(n).
fn species_constructor(job: Job&, at: i64, b: Builtin, c: Value) -> bool {
    let heap .= job.heap;
    if c.is_undefined() || c.is_object() && c.id() == array_constructor_row() && intrinsic_species(heap) { return species_plain(job, at, b); }
    if !c.is_object() { type_error(job, "array constructor is not an object"); return false; }
    let s, getter = lookup(heap, c, Value.Symbol { s: well_known::Species });
    if heap.fault != Fault.None { return false; }
    if getter.is_object() { job.frames[at].pc = callbacks_pc::Species; run_getter(job, b, getter, c); return false; }
    return species_value(job, at, b, s);
}
// Whether the realm's Array still has its own @@species getter (a job that
// owns the realm's rows, as the standalone runner's does, may redefine it).
fn intrinsic_species(heap: Heap&) -> bool {
    let p = find_own(heap, array_constructor_row(), Value.Symbol { s: well_known::Species });
    if !p.found || !is_accessor(p.value) { return false; }
    let g = pair(heap, p.value).get;
    return g.is_object() && node(heap, g.id()).kind == node_kind::Builtin && builtin_of(heap, g.id()) == Builtin.ArraySpecies;
}
// The constructor's @@species s: undefined and null make a plain array, a
// constructor is constructed with the length, anything else a TypeError.
fn species_value(job: Job&, at: i64, b: Builtin, s: Value) -> bool {
    let heap .= job.heap;
    if s.tag() <= value_tag::Null || s.is_object() && s.id() == array_constructor_row() { return species_plain(job, at, b); }
    if !is_constructor(heap, s) { type_error(job, "species is not a constructor"); return false; }
    if !push_operand(job, s) || !push_operand(job, number(species_length(job, at, b))) { return false; }
    job.frames[at].pc = callbacks_pc::Created;
    construct(job, 1);
    return false;
}
inline fn species_length(job: const Job&, at: i64, b: Builtin) -> f64 { return if b == Builtin.ArrayPrototypeMap { job.frames[at].argument_count } else { 0.0 }; }
fn species_plain(job: Job&, at: i64, b: Builtin) -> bool {
    let a = array_species_create(job, b, job.frames[at].input, species_length(job, at, b));
    if job.heap.fault != Fault.None { return false; }
    job.frames[at].output = a;
    return true;
}
// ArrayCreate(len), the plain result of ArraySpeciesCreate(O, len).
fn array_species_create(job: Job&, b: Builtin, o: Value, len: f64) -> Value {
    let heap .= job.heap;
    if len > 4294967295.0 { job.detail.clear(); for c in "invalid array length" { job.detail.push(c); } heap.fault = Fault.RangeError; return Value.Undefined; }
    let a = allocate(heap, Node { kind: node_kind::Array });
    if heap.fault != Fault.None || !array_length(heap, a, number(len)) { return Value.Undefined; }
    // A dense receiver whose element run holds len slots gets a result run of
    // len holes at once, so map's stores never copy a growing run. Any other
    // length (a generic array-like may claim 2^32 - 1) grows as stores come.
    if len > 0 && node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized && len <= node(heap, o.id()).count && !element_room(heap, a.id(), (len as i64) - 1) { return Value.Undefined; }
    return a;
}
// CreateDataPropertyOrThrow(a, i, v) for the frame at `at`: true when
// stored; false with a fault (a refused definition is a TypeError), or with
// a proxy's [[DefineOwnProperty]] above the frame, whose answer arrives at
// pc `wait` (a false one throws in its frame). A dense array the job made
// with the realm's prototype (a plain result always is one) stores in its
// run.
fn array_store(job: Job&, at: i64, a: Value, i: i64, v: Value, wait: i64) -> bool {
    let heap .= job.heap;
    if dense(job, a) && writable(job, a.id()) && !node(heap, a.id()).initialized { return set_element(heap, a, i, v); }
    let k = index_key(heap, i);
    if heap.fault != Fault.None { return false; }
    if is_proxy(heap, a) {
        let d = new_descriptor(heap);
        if heap.fault != Fault.None || !(set_field(heap, d, field::Value, v) && set_field(heap, d, field::Writable, boolean(true)) && set_field(heap, d, field::Enumerable, boolean(true)) && set_field(heap, d, field::Configurable, boolean(true))) { return false; }
        job.frames[at].pc = wait;
        proxy_push(job, proxy_method::DefineOwnProperty, a, k, Value.Undefined, d, answer::Throws, job.operands.len);
        return false;
    }
    return define_entry(job, a, k, PropertyDescriptor { value: v, has_value: true, writable: true, has_writable: true, enumerable: true, has_enumerable: true, configurable: true, has_configurable: true });
}

// ---- The walk
// Visit index k of the frame at `at`: finish past either end; otherwise
// HasProperty(O, k) (not for find and its siblings), skipping an absent
// index in this step, then Get(O, k), whose value goes to the callback now
// or, from a getter, at pc Element.
fn array_visit(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap;
    let k = job.frames[at].index; let o = job.frames[at].input;
    if k < 0 || k >= job.frames[at].argument_count { array_finish(job, at, b); return; }
    if node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized {
        let v, found = own_element(heap, o, k);
        if found { array_element(job, at, b, v); return; }
    }
    if callbacks_finder(b) { array_get(job, at, b); return; }
    let key = index_key(heap, k);
    if heap.fault != Fault.None { return; }
    let has = has_property(heap, o, key);
    // A proxy's [[HasProperty]] answers at pc Has (light-cljs-890).
    if proxy_pending(heap, proxy_method::HasProperty) { job.frames[at].pc = callbacks_pc::Has; proxy_take(job, proxy_method::HasProperty); return; }
    if heap.fault != Fault.None { return; }
    if !has { callbacks_advance(job, at, b); return; }
    array_get(job, at, b);
}
// The walk passed its last index: the method's result. reduce and
// reduceRight without a value by then (no initial value, no present
// element) are a TypeError.
fn array_finish(job: Job&, at: i64, b: Builtin) {
    let frame = job.frames[at];
    if callbacks_reducer(b) && !job.operands[frame.operand_base + callbacks_slot::Seeded].flag() { type_error(job, "reduce of empty array with no initial value"); return; }
    let r: Value = match b {
        ArrayPrototypeMap, ArrayPrototypeFilter, ArrayPrototypeFlatMap, ArrayPrototypeReduce, ArrayPrototypeReduceRight => frame.output,
        ArrayPrototypeSome => boolean(false),
        ArrayPrototypeEvery => boolean(true),
        ArrayPrototypeFindIndex, ArrayPrototypeFindLastIndex => number(-1),
        _ => Value.Undefined,
    };
    finish_native(job, frame, r);
}
// Get(O, k) of index k: its value to the callback now or, from a getter or
// a proxy's [[Get]], at pc Element.
fn array_get(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap;
    let o = job.frames[at].input;
    let key = index_key(heap, job.frames[at].index);
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, o, key);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { job.frames[at].pc = callbacks_pc::Element; run_getter(job, b, getter, o); return; }
    array_element(job, at, b, v);
}
// Element v of index k: the callback's call, its result arriving at pc
// Result; or, for reduce and reduceRight without a value yet, the
// accumulator.
fn array_element(job: Job&, at: i64, b: Builtin, v: Value) {
    let frame = job.frames[at]; let base = frame.operand_base;
    if callbacks_reducer(b) {
        if !job.operands[base + callbacks_slot::Seeded].flag() {
            job.operands[base + callbacks_slot::Seeded] = boolean(true);
            job.frames[at].output = v; job.frames[at].pc = callbacks_pc::Visit;
            callbacks_advance(job, at, b);
            return;
        }
        let args: Value[4] = [frame.output, v, number(frame.index), frame.input];
        array_call(job, at, b, args[..]);
        return;
    }
    if b == Builtin.ArrayPrototypeFilter { job.operands[base + callbacks_slot::Kept] = v; } else if callbacks_finder(b) { job.frames[at].output = v; }
    let args: Value[3] = [v, number(frame.index), frame.input];
    array_call(job, at, b, args[..]);
}
// Call the frame's callback with thisArg and `args`; its result arrives at
// pc Result. A plain built-in callback runs here and pushes its result; one
// that requests a host effect fails by name.
fn array_call(job: Job&, at: i64, b: Builtin, args: const Value[:]) {
    let heap .= job.heap;
    job.frames[at].pc = callbacks_pc::Result;
    let f = job.frames[at].callee; let this_arg = job.frames[at].receiver;
    // A bound function runs in a Forward frame of its own (light-cljs-852).
    if is_bound(heap, f) { start_bound(job, f, this_arg, args, job.operands.len); return; }
    // A callable proxy's [[Call]] runs as its Proxy frame (light-cljs-891).
    if is_proxy(heap, f) { proxy_call(job, f, this_arg, args, job.operands.len, false); return; }
    if node(heap, f.id()).kind == node_kind::Builtin {
        let cb = builtin_of(heap, f.id());
        // A built-in that converts its receiver or arguments runs in a
        // Coerce frame (light-cljs-900).
        if coerces(heap, f, this_arg, args) { start_coerce(job, f, this_arg, args, job.operands.len); return; }
        // A delay forced as the callback runs in a frame of its own (cells.goose).
        if forces(heap, cb, args) { start_force(job, cb, args[0], job.operands.len); return; }
        // A forwarding built-in (light-cljs-851) runs in a frame of its own.
        if forwarding(cb) { native_start(job, cb, this_arg, args, job.operands.len); return; }
        // So do the Object statics (light-cljs-884; object_statics.goose)
        // and Math.sumPrecise (light-cljs-901; math_object.goose).
        if statics_native(cb) || cb == Builtin.MathSumPrecise || regexp_native(cb) { native_start(job, cb, this_arg, args, job.operands.len); return; }
        // So do the error built-ins when a step runs JavaScript (light-cljs-870).
        if conditional_native(cb) { start_conditional_native(job, cb, this_arg, args, job.operands.len); return; }
        if effect_builtin(cb) || cb == Builtin.Mapv || cb == Builtin.MapSeq { unsupported_step(job, b, "a host-effect built-in callback"); return; }
        // Any other native-frame built-in runs through a Forward frame, as
        // an accessor's does (call_accessor).
        if native_builtin(cb) { start_bound(job, f, this_arg, args, job.operands.len); return; }
        // A revoke function reads its own row (light-cljs-891; proxy.goose).
        let r = call_plain(job, f, this_arg, args);
        if heap.fault == Fault.None { job.operands.push(r); }
        return;
    }
    push_frame(job, f, this_arg, args, false, job.operands.len);
}

// ---- flatMap's FlattenIntoArray at depth 1 (ECMAScript 23.1.3.13.1)
// The mapped value r of index k: an array (IsArray) is walked, each
// present element stored at the next result index; any other value is
// stored itself.
fn flatten_begin(job: Job&, at: i64, r: Value) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeFlatMap; let base = job.frames[at].operand_base;
    let flat = is_array_value(job, r);
    if heap.fault != Fault.None { return; }
    if !flat {
        if flatten_store(job, at, r, callbacks_pc::Stored) { callbacks_advance(job, at, b); }
        return;
    }
    job.operands[base + callbacks_slot::Inner] = r; job.operands[base + callbacks_slot::InnerIndex] = number(0);
    job.frames[at].pc = callbacks_pc::InnerLength;
    get_named(job, b, r, "length");
}
// CreateDataPropertyOrThrow(A, to, v), then to + 1: array_store's answer.
fn flatten_store(job: Job&, at: i64, v: Value, wait: i64) -> bool {
    let base = job.frames[at].operand_base;
    let to = job.operands[base + callbacks_slot::To].num();
    if to >= max_safe_integer() { type_error(job, "array-like length exceeds 2^53 - 1"); return false; }
    job.operands[base + callbacks_slot::To] = number(to + 1);
    return array_store(job, at, job.frames[at].output, to as i64, v, wait);
}
// Visit index j of the mapped array: back to the receiver's walk past its
// length; otherwise HasProperty, skipping an absent index in this step,
// then Get.
fn flatten_visit(job: Job&, at: i64) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeFlatMap; let base = job.frames[at].operand_base;
    let e = job.operands[base + callbacks_slot::Inner]; let j = job.operands[base + callbacks_slot::InnerIndex].num() as i64;
    if j >= job.operands[base + callbacks_slot::InnerLength].num() as i64 {
        job.operands[base + callbacks_slot::Inner] = Value.Undefined; job.frames[at].pc = callbacks_pc::Visit;
        callbacks_advance(job, at, b);
        return;
    }
    if node(heap, e.id()).kind == node_kind::Array && !node(heap, e.id()).initialized {
        let v, found = own_element(heap, e, j);
        if found { flatten_element(job, at, v); return; }
    }
    let key = index_key(heap, j);
    if heap.fault != Fault.None { return; }
    let has = has_property(heap, e, key);
    if proxy_pending(heap, proxy_method::HasProperty) { job.frames[at].pc = callbacks_pc::InnerHas; proxy_take(job, proxy_method::HasProperty); return; }
    if heap.fault != Fault.None { return; }
    if !has { job.operands[base + callbacks_slot::InnerIndex] = number(j + 1); return; }
    flatten_get(job, at);
}
fn flatten_get(job: Job&, at: i64) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeFlatMap; let base = job.frames[at].operand_base;
    let e = job.operands[base + callbacks_slot::Inner];
    let key = index_key(heap, job.operands[base + callbacks_slot::InnerIndex].num() as i64);
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, e, key);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { job.frames[at].pc = callbacks_pc::InnerElement; run_getter(job, b, getter, e); return; }
    flatten_element(job, at, v);
}
// Element v of the mapped array's index j: stored, and on to j + 1.
fn flatten_element(job: Job&, at: i64, v: Value) {
    let base = job.frames[at].operand_base;
    job.operands[base + callbacks_slot::InnerIndex] = number(job.operands[base + callbacks_slot::InnerIndex].num() + 1);
    job.frames[at].pc = callbacks_pc::InnerVisit;
    flatten_store(job, at, v, callbacks_pc::InnerStored);
}
