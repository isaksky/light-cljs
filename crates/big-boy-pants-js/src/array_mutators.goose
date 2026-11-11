import std;
import math;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .array_search;
import .function_calls;
import .coerce;
import .proxy;
import .descriptors;
import .regexp_object;

namespace source;

// Array.prototype.pop, shift, unshift, splice, reverse, fill and copyWithin,
// which mutate their receiver, and toSpliced, which copies it
// (light-cljs-909; decision 5 of docs/javascript-coverage.md, on
// light-cljs-842's template in array_methods.goose). Semantics from the
// Pants engine's rust/src/vm/native/array_mutate.rs and QuickJS's
// js_array_pop, js_array_push, js_array_splice, js_array_toSpliced,
// js_array_reverse, js_array_fill and js_array_copyWithin, with the
// specification's steps (ECMAScript 23.1.3): ToObject(this),
// LengthOfArrayLike, ToIntegerOrInfinity of the relative indices, then Get,
// HasProperty, Set(..., true) and DeletePropertyOrThrow in the
// specification's order, and Set of `length` last.
//
// Each is a native frame (Native.ArrayMethod, its Builtin the payload):
//   input O; argument_count len; index the call's argument count; output
//   splice's and toSpliced's result A.
// with scratch operands at its operand base (mutate_slot::), then the
// call's arguments:
//   Done     how many primitives of the current unit have run
//   Memo..   the results of those primitives, at most mutate_memos()
//   Stage    the method's stage (mutate_stage::)
//   P Q R S  the stage's numbers (cursors, counts; each method says which)
//   Held     a value kept across units (shift's first element)
//
// Units and primitives. A method is a sequence of units: one element moved,
// one pair swapped, one item set, the argument conversions. A unit is
// straight-line code over primitives (micro_has, micro_get, micro_set,
// micro_delete, micro_create, micro_number), each of which may run
// JavaScript: a getter, a setter, a proxy trap, a valueOf. A primitive that
// runs records its result in the next Memo slot and ends the step (its
// JavaScript, if any, runs above the frame and its answer arrives at pc
// Await, which records it); the next step runs the unit again from its
// top, the primitives already done answering from their memos without
// running, until the unit's last primitive is done and the unit advances
// the stage's numbers and clears Done. So every Get, HasProperty, Set and
// Delete is its own native-frame step: a budget cut, limit, suspension or
// collection falls between two of them, and nothing runs twice.
//   pc Length   awaits Get(O, "length") (then its ToNumber in a Convert frame)
//   pc Run      runs the current unit
//   pc Await    awaits a primitive's answer
//   callbacks_pc::Constructor, Species, Created   splice's ArraySpeciesCreate
//               (array_callbacks.goose's steps, shared)
//
// The fast paths. A dense array the job made, with the realm's prototype
// and every element below its length present (mutator_dense), has no
// getter, setter, hole or inherited index for these steps to see once the
// arguments are converted, so the method edits its element run in one
// step, charged a work unit an element; an array longer than
// mutator_fast_limit() takes the units, so no step moves more elements than
// that. A fast path that would create an own element (unshift, a splice
// that grows, fill over holes) also needs a job that cannot write the
// realm's prototypes (mutator_grows). Removed elements are no longer
// referenced by the run, so in-job reclamation drops them. Any other
// receiver takes the units.
//
// Not implemented yet, each a named UnsupportedOperand (decision 1): a
// BigInt length or index (light-cljs-771) and a light-cljs runtime value to
// convert.
let mutate_pc::Length = 0;
let mutate_pc::Run = 1;
let mutate_pc::Await = 2;
let mutate_slot::Done = 0;
let mutate_slot::Memo = 1;
let mutate_slot::Stage = 7;
let mutate_slot::P = 8;
let mutate_slot::Q = 9;
let mutate_slot::R = 10;
let mutate_slot::S = 11;
let mutate_slot::Held = 12;
let mutate_slot::Args = 13;
inline fn mutate_memos() -> i64 { return 6; }
// Stages: Setup converts the arguments and takes the fast path; Move walks
// elements from one index to another; Items sets the call's items; Trim
// deletes what a shorter result leaves; Finish sets `length`. splice adds
// Species (ArraySpeciesCreate), Collect (the removed elements into A) and
// ResultLength (A's length), and moves down or up; fill has Fill, reverse
// Pairs and toSpliced Copy.
let mutate_stage::Setup = 0;
let mutate_stage::Move = 1;
let mutate_stage::Items = 2;
let mutate_stage::Trim = 3;
let mutate_stage::Finish = 4;
let mutate_stage::Collect = 5;
let mutate_stage::ResultLength = 6;
let mutate_stage::MoveUp = 7;
let mutate_stage::Fill = 8;
let mutate_stage::Pairs = 9;
let mutate_stage::Copy = 10;

// Whether b is one of these built-ins.
fn array_mutator(b: Builtin) -> bool {
    return match b {
        ArrayPrototypePop, ArrayPrototypeShift, ArrayPrototypeUnshift, ArrayPrototypeSplice, ArrayPrototypeReverse, ArrayPrototypeFill, ArrayPrototypeCopyWithin, ArrayPrototypeToSpliced => true,
        // sort and toSorted run in this frame too (light-cljs-910; array_sort.goose).
        ArrayPrototypeSort, ArrayPrototypeToSorted => true,
        _ => false,
    };
}
// A unit's walk over its primitives: the frame, its operand base, how many
// primitives this run of the unit has passed, and whether the step ends
// here (a primitive ran or waits, the frame finished, or a fault).
struct Micro { at: i64, base: i64, done: i64 = 0, stop: bool = false }
inline fn mutate_number(job: const Job&, base: i64, slot: i64) -> i64 { return job.operands[base + slot].num() as i64; }
inline fn mutate_put(job: Job&, base: i64, slot: i64, n: i64) { job.operands[base + slot] = number(n); }

// Start built-in b (array_mutator) called with `receiver` and `args`, its
// operand base `base` (native_start). `args` views the operands above the
// base, which become the frame's scratch and the copied arguments.
fn start_array_mutator(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    // sort and toSorted check their comparator before anything is read.
    if array_sorter(b) && !arg(args, 0).is_undefined() && !is_callable(heap, arg(args, 0)) { type_error(job, "the comparator must be a function or undefined"); return; }
    let o = to_object(job, receiver);
    if heap.fault != Fault.None { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    if mutate_slot::Args + args.len + 2 > heap.limits.stack - job.locals.len - base { heap.fault = Fault.StackLimit; return; }
    var copied: Value[>..] = []; copied.append(args);
    job.operands.resize(base);
    for i in mutate_slot::Args { job.operands.push(if i >= mutate_slot::Memo && i < mutate_slot::Stage || i == mutate_slot::Held { Value.Undefined } else { number(0) }); }
    job.operands.append(copied);
    job.frames.push(Frame { function: -1, pc: mutate_pc::Length, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: Value.Undefined, input: o, index: copied.len, native: Native.ArrayMethod { which: b } });
    get_named(job, b, o, "length");
}
// One step of the frame at the top (see above).
fn array_mutator_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1; let base = frame.operand_base;
    if frame.index < 0 || job.operands.len < base + mutate_slot::Args + frame.index { heap.fault = Fault.InvalidState; return; }
    // A restored frame's scratch (snapshot.goose checks its fields): the
    // memo count, a stage the method has, numbers where the units read
    // numbers, and the result splice and toSpliced hold past Setup.
    let done = mutate_number(job, base, mutate_slot::Done); let stage = mutate_number(job, base, mutate_slot::Stage);
    if done < 0 || done > mutate_memos() || !mutator_stage(b, stage) { heap.fault = Fault.InvalidState; return; }
    for i in mutate_slot::Stage..mutate_slot::Held { if !job.operands[base + i].is_number() { heap.fault = Fault.InvalidState; return; } }
    if (b == Builtin.ArrayPrototypeSplice && stage >= mutate_stage::Collect || b == Builtin.ArrayPrototypeToSpliced && stage == mutate_stage::Copy || b == Builtin.ArrayPrototypeToSorted && stage != mutate_stage::Setup) && !frame.output.is_object() { heap.fault = Fault.InvalidState; return; }
    match frame.pc {
        mutate_pc::Length => {
            let len, done = length_value(job, b);
            if !done { return; }
            job.frames[at].argument_count = len as i64; job.frames[at].pc = mutate_pc::Run;
        },
        mutate_pc::Await => {
            let v = job.operands.pop();
            let completed = mutate_number(job, base, mutate_slot::Done);
            if completed < 0 || completed >= mutate_memos() { heap.fault = Fault.InvalidState; return; }
            job.operands[base + mutate_slot::Memo + completed] = v; mutate_put(job, base, mutate_slot::Done, completed + 1);
            job.frames[at].pc = mutate_pc::Run;
        },
        // splice's ArraySpeciesCreate (array_callbacks.goose): its result
        // made, the removed elements are collected.
        callbacks_pc::Constructor => {
            if b != Builtin.ArrayPrototypeSplice { heap.fault = Fault.InvalidState; return; }
            job.frames[at].pc = mutate_pc::Run;
            if species_constructor(job, at, b, job.operands.pop()) { mutate_put(job, base, mutate_slot::Stage, mutate_stage::Collect); }
        },
        callbacks_pc::Species => {
            if b != Builtin.ArrayPrototypeSplice { heap.fault = Fault.InvalidState; return; }
            job.frames[at].pc = mutate_pc::Run;
            if species_value(job, at, b, job.operands.pop()) { mutate_put(job, base, mutate_slot::Stage, mutate_stage::Collect); }
        },
        callbacks_pc::Created => {
            if b != Builtin.ArrayPrototypeSplice { heap.fault = Fault.InvalidState; return; }
            job.frames[at].output = job.operands.pop(); job.frames[at].pc = mutate_pc::Run;
            mutate_put(job, base, mutate_slot::Stage, mutate_stage::Collect);
        },
        mutate_pc::Run => {
            var m = Micro { at: at, base: base };
            // Units run until a primitive ends the step; a unit without one
            // (a stage's end) goes straight on to the next.
            while !m.stop && heap.fault == Fault.None {
                m.done = 0;
                match b {
                    ArrayPrototypePop => { pop_unit(job, m); },
                    ArrayPrototypeShift => { shift_unit(job, m); },
                    ArrayPrototypeUnshift => { unshift_unit(job, m); },
                    ArrayPrototypeReverse => { reverse_unit(job, m); },
                    ArrayPrototypeFill => { fill_unit(job, m); },
                    ArrayPrototypeCopyWithin => { copy_within_unit(job, m); },
                    ArrayPrototypeSplice => { splice_unit(job, m); },
                    ArrayPrototypeToSpliced => { to_spliced_unit(job, m); },
                    ArrayPrototypeSort, ArrayPrototypeToSorted => { sort_unit(job, m, b); },
                    _ => { heap.fault = Fault.InvalidState; },
                }
            }
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}

// ---- Primitives
// The memo of the unit's next primitive, if it already ran.
fn micro_replay(job: Job&, m: Micro&) -> bool, Value {
    if m.done < mutate_number(job, m.base, mutate_slot::Done) {
        let v = job.operands[m.base + mutate_slot::Memo + m.done];
        m.done += 1;
        return true, v;
    }
    return false, Value.Undefined;
}
// Record the result of the primitive that just ran. `yields` ends the
// step (every primitive that reads or writes the object); a conversion of
// a primitive value goes on.
fn micro_record(job: Job&, m: Micro&, v: Value, yields: bool = true) {
    let completed = mutate_number(job, m.base, mutate_slot::Done);
    if completed >= mutate_memos() { job.heap.fault = Fault.InvalidState; m.stop = true; return; }
    job.operands[m.base + mutate_slot::Memo + completed] = v; mutate_put(job, m.base, mutate_slot::Done, completed + 1);
    if yields { m.stop = true; } else { m.done += 1; }
}
// The primitive's JavaScript runs above the frame; its answer arrives at pc Await.
inline fn micro_wait(job: Job&, m: Micro&) { job.frames[m.at].pc = mutate_pc::Await; m.stop = true; }
// The unit's last primitive is done.
inline fn micro_unit_done(job: Job&, m: Micro&) { mutate_put(job, m.base, mutate_slot::Done, 0); m.done = 0; }
// HasProperty(o, k).
fn micro_has(job: Job&, m: Micro&, o: Value, k: i64) -> bool {
    let heap .= job.heap;
    let replayed, memo = micro_replay(job, m);
    if replayed { return truthy(heap, memo); }
    if node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized {
        let _, found = own_element(heap, o, k);
        if found { micro_record(job, m, boolean(true)); return true; }
    }
    let key = index_key(heap, k);
    if heap.fault != Fault.None { m.stop = true; return false; }
    let has = has_property(heap, o, key);
    // A proxy's [[HasProperty]] (light-cljs-890).
    if proxy_pending(heap, proxy_method::HasProperty) { micro_wait(job, m); proxy_take(job, proxy_method::HasProperty); return false; }
    if heap.fault != Fault.None { m.stop = true; return false; }
    micro_record(job, m, boolean(has));
    return has;
}
// Get(o, k).
fn micro_get(job: Job&, m: Micro&, b: Builtin, o: Value, k: i64) -> Value {
    let heap .= job.heap;
    let replayed, memo = micro_replay(job, m);
    if replayed { return memo; }
    let key = index_key(heap, k);
    if heap.fault != Fault.None { m.stop = true; return Value.Undefined; }
    let v, getter = lookup(heap, o, key);
    if heap.fault != Fault.None { m.stop = true; return Value.Undefined; }
    if getter.is_object() { micro_wait(job, m); run_getter(job, b, getter, o); return Value.Undefined; }
    micro_record(job, m, v);
    return v;
}
// Set(o, key, v, true).
fn micro_set(job: Job&, m: Micro&, b: Builtin, o: Value, key: Value, v: Value) {
    let heap .= job.heap;
    let replayed, _ = micro_replay(job, m);
    if replayed { return; }
    if heap.fault != Fault.None { m.stop = true; return; }
    job.frames[m.at].pc = mutate_pc::Await;
    // A setter or a proxy's [[Set]] runs above the frame.
    if set_or_throw(job, b, o, key, v) { m.stop = true; return; }
    job.frames[m.at].pc = mutate_pc::Run;
    if heap.fault != Fault.None { m.stop = true; return; }
    micro_record(job, m, Value.Undefined);
}
fn micro_set_index(job: Job&, m: Micro&, b: Builtin, o: Value, k: i64, v: Value) {
    let key = index_key(job.heap, k);
    micro_set(job, m, b, o, key, v);
}
fn micro_set_length(job: Job&, m: Micro&, b: Builtin, o: Value, n: i64) {
    let key = key_named(job.heap, "length");
    micro_set(job, m, b, o, key, number(n));
}
// DeletePropertyOrThrow(o, k).
fn micro_delete(job: Job&, m: Micro&, b: Builtin, o: Value, k: i64) {
    let heap .= job.heap;
    let replayed, _ = micro_replay(job, m);
    if replayed { return; }
    let key = index_key(heap, k);
    if heap.fault != Fault.None { m.stop = true; return; }
    // A row the job did not allocate faults before it changes (decision 3).
    if !writable(job, o.id()) && !shared_regexp(heap, o.id()) { image_write(job, key); m.stop = true; return; }
    let ok = delete_property(heap, o, key);
    // A proxy's [[Delete]], a false answer a TypeError (light-cljs-890).
    if proxy_pending(heap, proxy_method::Delete) { micro_wait(job, m); proxy_take(job, proxy_method::Delete, Value.Undefined, Value.Undefined, true); return; }
    if heap.fault != Fault.None { m.stop = true; return; }
    if !ok { named_error(job, Fault.TypeError, "cannot delete property '", key, "'"); m.stop = true; return; }
    micro_record(job, m, Value.Undefined);
}
// CreateDataPropertyOrThrow(a, i, v) (array_callbacks.goose's array_store).
fn micro_create(job: Job&, m: Micro&, a: Value, i: i64, v: Value) {
    let replayed, _ = micro_replay(job, m);
    if replayed { return; }
    if array_store(job, m.at, a, i, v, mutate_pc::Await) { micro_record(job, m, Value.Undefined); } else { m.stop = true; }
}
// ToIntegerOrInfinity(v).
fn micro_number(job: Job&, m: Micro&, b: Builtin, v: Value) -> f64 { return to_integer_or_infinity(micro_to_number(job, m, b, v)); }
// Call(f, this_value, args) on the general call path (call_accessor): its
// result arrives at pc Await.
fn micro_call(job: Job&, m: Micro&, f: Value, this_value: Value, args: const Value[:]) -> Value {
    let replayed, memo = micro_replay(job, m);
    if replayed { return memo; }
    micro_wait(job, m);
    call_accessor(job, f, this_value, args);
    return Value.Undefined;
}
// ToString(v): an object's in a Convert frame above the frame, a
// primitive's here, without ending the step.
fn micro_string(job: Job&, m: Micro&, b: Builtin, v: Value) -> Value {
    let heap .= job.heap;
    let replayed, memo = micro_replay(job, m);
    if replayed {
        if memo.is_string() { return memo; }
        let s = primitive_string(job, b, memo);
        if heap.fault != Fault.None { m.stop = true; }
        return s;
    }
    let f = form_value(heap, v);
    if f.is_object() {
        if !convertible(heap, f) { unsupported_step(job, b, "ToString of a light-cljs runtime value"); m.stop = true; return Value.Undefined; }
        if push_operand(job, f) { micro_wait(job, m); start_convert(job, Hint.String, true, 1, 1); } else { m.stop = true; }
        return Value.Undefined;
    }
    let s = primitive_string(job, b, f);
    if heap.fault != Fault.None { m.stop = true; return Value.Undefined; }
    micro_record(job, m, s, false);
    return s;
}
// ToNumber(v): an object's in a Convert frame above the frame, a
// primitive's here, without ending the step.
fn micro_to_number(job: Job&, m: Micro&, b: Builtin, v: Value) -> f64 {
    let heap .= job.heap;
    let replayed, memo = micro_replay(job, m);
    if replayed {
        let n = primitive_number(job, b, memo);
        if heap.fault != Fault.None { m.stop = true; return 0; }
        return n;
    }
    let f = form_value(heap, v);
    if f.is_object() {
        if !convertible(heap, f) { unsupported_step(job, b, "ToNumber of a light-cljs runtime value"); m.stop = true; return 0; }
        if push_operand(job, f) { micro_wait(job, m); start_convert(job, Hint.Number, true, 1, 1); } else { m.stop = true; }
        return 0;
    }
    let n = primitive_number(job, b, f);
    if heap.fault != Fault.None { m.stop = true; return 0; }
    micro_record(job, m, number(n), false);
    return n;
}
// A relative index against len: from the end when negative, clamped to
// [0, len].
inline fn relative_index(n: f64, len: i64) -> i64 { return (if n < 0 { max(len + n, 0.0) } else { min(n, len as f64) }) as i64; }
// The frame's result: it pops, and the step ends.
fn micro_finish(job: Job&, m: Micro&, result: Value) { finish_native(job, job.frames[m.at], result); m.stop = true; }
// One element moved: HasProperty(from), then Get(from) and Set(to), or
// DeletePropertyOrThrow(to) for an absent one.
fn micro_move(job: Job&, m: Micro&, b: Builtin, o: Value, from: i64, to: i64) {
    let present = micro_has(job, m, o, from);
    if m.stop { return; }
    if present {
        let v = micro_get(job, m, b, o, from);
        if m.stop { return; }
        micro_set_index(job, m, b, o, to, v);
    } else {
        micro_delete(job, m, b, o, to);
    }
}
// Whether o is a dense array the job may write, with the realm's prototype,
// length len and elements 0 to len - 1 all present: the methods' fast path
// (see above). The scan is charged.
fn mutator_dense(job: Job&, o: Value, len: i64) -> bool {
    let heap .= job.heap;
    if len > mutator_fast_limit() || !dense(job, o) || !writable(job, o.id()) || node(heap, o.id()).initialized || node(heap, o.id()).length != len || node(heap, o.id()).count < len { return false; }
    if !charge(heap, len) { return false; }
    let start: i64 = node(heap, o.id()).start;
    for i in len { if slot(heap, start + i).is_hole() { return false; } }
    return true;
}
// The longest array a fast path edits in one step: a longer one takes the
// units, so a budget cut, cancellation or collection is never further away
// than this many element moves.
inline fn mutator_fast_limit() -> i64 { return 65536; }
// Whether o is a dense array the job may write with length len, holes or
// not: a fast path that only removes or overwrites present elements.
inline fn mutator_writable(job: Job&, o: Value, len: i64) -> bool {
    return len <= mutator_fast_limit() && dense(job, o) && writable(job, o.id()) && !node(job.heap, o.id()).initialized && node(job.heap, o.id()).length == len;
}
// Whether a fast path may create an own element of a dense array without
// asking its prototypes: Set of a new index consults the chain, where a
// setter or a read-only index on %Array.prototype% or %Object.prototype%
// would decide. A job that cannot write the realm's rows (decision 3: a
// compiler job's image) cannot have put one there; one that owns them (the
// standalone runner's) takes the units instead.
inline fn mutator_grows(job: const Job&) -> bool { return !writable(job, array_prototype_row()) && !writable(job, object_prototype_row()); }
// Whether `stage` is one of b's stages (a restored frame's is checked).
fn mutator_stage(b: Builtin, stage: i64) -> bool {
    return match b {
        ArrayPrototypePop => stage == mutate_stage::Setup,
        ArrayPrototypeShift => stage == mutate_stage::Setup || stage == mutate_stage::Move || stage == mutate_stage::Trim,
        ArrayPrototypeUnshift => stage == mutate_stage::Setup || stage == mutate_stage::Move || stage == mutate_stage::Items || stage == mutate_stage::Finish,
        ArrayPrototypeReverse => stage == mutate_stage::Setup || stage == mutate_stage::Pairs,
        ArrayPrototypeFill => stage == mutate_stage::Setup || stage == mutate_stage::Fill,
        ArrayPrototypeCopyWithin => stage == mutate_stage::Setup || stage == mutate_stage::Move,
        ArrayPrototypeSplice => stage >= mutate_stage::Setup && stage <= mutate_stage::MoveUp,
        ArrayPrototypeToSpliced => stage == mutate_stage::Setup || stage == mutate_stage::Copy,
        ArrayPrototypeSort, ArrayPrototypeToSorted => stage == mutate_stage::Setup || stage == mutate_stage::Collect || stage == mutate_stage::Sort || stage == mutate_stage::Write,
        _ => false,
    };
}

// ---- pop (23.1.3.22): one unit.
fn pop_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypePop;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count;
    if len == 0 {
        micro_set_length(job, m, b, o, 0);
        if m.stop { return; }
        micro_finish(job, m, Value.Undefined);
        return;
    }
    if mutate_number(job, m.base, mutate_slot::Done) == 0 && mutator_writable(job, o, len) {
        let v, found = own_element(heap, o, len - 1);
        if found {
            if array_length(heap, o, number(len - 1)) { micro_finish(job, m, v); }
            m.stop = true;
            return;
        }
    }
    let v = micro_get(job, m, b, o, len - 1);
    if m.stop { return; }
    micro_delete(job, m, b, o, len - 1);
    if m.stop { return; }
    micro_set_length(job, m, b, o, len - 1);
    if m.stop { return; }
    micro_finish(job, m, v);
}

// ---- shift (23.1.3.27): Held the first element, P the next index to move down.
fn shift_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeShift;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        if len == 0 {
            micro_set_length(job, m, b, o, 0);
            if m.stop { return; }
            micro_finish(job, m, Value.Undefined);
            return;
        }
        if mutate_number(job, m.base, mutate_slot::Done) == 0 {
            let fast = mutator_dense(job, o, len);
            if heap.fault != Fault.None { m.stop = true; return; }
            if fast {
                let start: i64 = node(heap, o.id()).start;
                let first = slot(heap, start);
                for i in 1..len { set_slot(heap, start + i - 1, slot(heap, start + i)); }
                if array_length(heap, o, number(len - 1)) { micro_finish(job, m, first); }
                m.stop = true;
                return;
            }
        }
        let first = micro_get(job, m, b, o, 0);
        if m.stop { return; }
        job.operands[m.base + mutate_slot::Held] = first;
        mutate_put(job, m.base, mutate_slot::P, 1); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Move);
        micro_unit_done(job, m);
        return;
    }
    if stage == mutate_stage::Move {
        let k = mutate_number(job, m.base, mutate_slot::P);
        if k >= len { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Trim); return; }
        micro_move(job, m, b, o, k, k - 1);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::P, k + 1);
        micro_unit_done(job, m);
        return;
    }
    micro_delete(job, m, b, o, len - 1);
    if m.stop { return; }
    micro_set_length(job, m, b, o, len - 1);
    if m.stop { return; }
    micro_finish(job, m, job.operands[m.base + mutate_slot::Held]);
}

// ---- unshift (23.1.3.37): P the index above the next element to move up,
// then the next item.
fn unshift_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeUnshift;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count; let count = job.frames[m.at].index;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        if count > 0 && count > max_safe_integer() - len { type_error(job, "array-like length exceeds 2^53 - 1"); m.stop = true; return; }
        if count == 0 { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish); return; }
        let fast = mutator_grows(job) && mutator_dense(job, o, len);
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            var i = len - 1;
            while i >= 0 {
                let v, _ = own_element(heap, o, i);
                if !set_element(heap, o, i + count, v) { m.stop = true; return; }
                i -= 1;
            }
            for j in count { if !set_element(heap, o, j, job.operands[m.base + mutate_slot::Args + j]) { m.stop = true; return; } }
            micro_finish(job, m, number(len + count));
            return;
        }
        mutate_put(job, m.base, mutate_slot::P, len); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Move);
        return;
    }
    if stage == mutate_stage::Move {
        let k = mutate_number(job, m.base, mutate_slot::P);
        if k <= 0 { mutate_put(job, m.base, mutate_slot::P, 0); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Items); return; }
        micro_move(job, m, b, o, k - 1, k + count - 1);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::P, k - 1);
        micro_unit_done(job, m);
        return;
    }
    if stage == mutate_stage::Items {
        let j = mutate_number(job, m.base, mutate_slot::P);
        if j >= count { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish); return; }
        micro_set_index(job, m, b, o, j, job.operands[m.base + mutate_slot::Args + j]);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::P, j + 1);
        micro_unit_done(job, m);
        return;
    }
    micro_set_length(job, m, b, o, len + count);
    if m.stop { return; }
    micro_finish(job, m, number(len + count));
}

// ---- reverse (23.1.3.26): P the lower index of the next pair.
fn reverse_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeReverse;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let fast = mutator_dense(job, o, len);
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            let start: i64 = node(heap, o.id()).start;
            for i in len / 2 { let low = slot(heap, start + i); set_slot(heap, start + i, slot(heap, start + len - 1 - i)); set_slot(heap, start + len - 1 - i, low); }
            micro_finish(job, m, o);
            return;
        }
        mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Pairs);
        return;
    }
    let lower = mutate_number(job, m.base, mutate_slot::P);
    if lower >= len / 2 { micro_finish(job, m, o); return; }
    let upper = len - lower - 1;
    let lower_exists = micro_has(job, m, o, lower);
    if m.stop { return; }
    var lower_value = Value.Undefined;
    if lower_exists { lower_value = micro_get(job, m, b, o, lower); if m.stop { return; } }
    let upper_exists = micro_has(job, m, o, upper);
    if m.stop { return; }
    var upper_value = Value.Undefined;
    if upper_exists { upper_value = micro_get(job, m, b, o, upper); if m.stop { return; } }
    if lower_exists && upper_exists {
        micro_set_index(job, m, b, o, lower, upper_value);
        if m.stop { return; }
        micro_set_index(job, m, b, o, upper, lower_value);
    } else if upper_exists {
        micro_set_index(job, m, b, o, lower, upper_value);
        if m.stop { return; }
        micro_delete(job, m, b, o, upper);
    } else if lower_exists {
        micro_delete(job, m, b, o, lower);
        if m.stop { return; }
        micro_set_index(job, m, b, o, upper, lower_value);
    }
    if m.stop { return; }
    mutate_put(job, m.base, mutate_slot::P, lower + 1);
    micro_unit_done(job, m);
}

// ---- fill(value, start, end) (23.1.3.7): P the next index, Q the end.
fn fill_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeFill;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count; let count = job.frames[m.at].index;
    let value = if count > 0 { job.operands[m.base + mutate_slot::Args] } else { Value.Undefined };
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let from = micro_number(job, m, b, if count > 1 { job.operands[m.base + mutate_slot::Args + 1] } else { Value.Undefined });
        if m.stop { return; }
        let end = if count > 2 { job.operands[m.base + mutate_slot::Args + 2] } else { Value.Undefined };
        var to: f64 = len;
        if !end.is_undefined() { to = micro_number(job, m, b, end); if m.stop { return; } }
        let k = relative_index(from, len); let last = relative_index(to, len);
        micro_unit_done(job, m);
        // Overwrites of present elements, or new ones where no prototype can object.
        let fast = if mutator_grows(job) { mutator_writable(job, o, len) } else { mutator_dense(job, o, len) };
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            for i in k..last { if !set_element(heap, o, i, value) { m.stop = true; return; } }
            micro_finish(job, m, o);
            return;
        }
        mutate_put(job, m.base, mutate_slot::P, k); mutate_put(job, m.base, mutate_slot::Q, last); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Fill);
        return;
    }
    let k = mutate_number(job, m.base, mutate_slot::P);
    if k >= mutate_number(job, m.base, mutate_slot::Q) { micro_finish(job, m, o); return; }
    micro_set_index(job, m, b, o, k, value);
    if m.stop { return; }
    mutate_put(job, m.base, mutate_slot::P, k + 1);
    micro_unit_done(job, m);
}

// ---- copyWithin(target, start, end) (23.1.3.4): P from, Q to, R the
// direction, S the elements left.
fn copy_within_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeCopyWithin;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count; let count = job.frames[m.at].index;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let target = micro_number(job, m, b, if count > 0 { job.operands[m.base + mutate_slot::Args] } else { Value.Undefined });
        if m.stop { return; }
        let start = micro_number(job, m, b, if count > 1 { job.operands[m.base + mutate_slot::Args + 1] } else { Value.Undefined });
        if m.stop { return; }
        let end = if count > 2 { job.operands[m.base + mutate_slot::Args + 2] } else { Value.Undefined };
        var limit: f64 = len;
        if !end.is_undefined() { limit = micro_number(job, m, b, end); if m.stop { return; } }
        var to = relative_index(target, len); var from = relative_index(start, len); let last = relative_index(limit, len);
        let n = min(last - from, len - to);
        micro_unit_done(job, m);
        if n <= 0 { micro_finish(job, m, o); return; }
        let fast = mutator_dense(job, o, len);
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            let run_start: i64 = node(heap, o.id()).start;
            var moved: Value[>..] = [];
            for i in n { moved.push(slot(heap, run_start + from + i)); }
            for i in n { set_slot(heap, run_start + to + i, moved[i]); }
            micro_finish(job, m, o);
            return;
        }
        var direction = 1;
        if from < to && to < from + n { direction = -1; from += n - 1; to += n - 1; }
        mutate_put(job, m.base, mutate_slot::P, from); mutate_put(job, m.base, mutate_slot::Q, to); mutate_put(job, m.base, mutate_slot::R, direction); mutate_put(job, m.base, mutate_slot::S, n);
        mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Move);
        return;
    }
    let left = mutate_number(job, m.base, mutate_slot::S);
    if left <= 0 { micro_finish(job, m, o); return; }
    let from = mutate_number(job, m.base, mutate_slot::P); let to = mutate_number(job, m.base, mutate_slot::Q); let direction = mutate_number(job, m.base, mutate_slot::R);
    micro_move(job, m, b, o, from, to);
    if m.stop { return; }
    mutate_put(job, m.base, mutate_slot::P, from + direction); mutate_put(job, m.base, mutate_slot::Q, to + direction); mutate_put(job, m.base, mutate_slot::S, left - 1);
    micro_unit_done(job, m);
}

// splice's and toSpliced's start and count: actualStart and
// actualDeleteCount (toSpliced's skip count) from the call's first two
// arguments, as their unit's first primitives. False when the step ends.
fn splice_range(job: Job&, m: Micro&, b: Builtin) -> bool, i64, i64 {
    let len = job.frames[m.at].argument_count; let count = job.frames[m.at].index;
    let relative = micro_number(job, m, b, if count > 0 { job.operands[m.base + mutate_slot::Args] } else { Value.Undefined });
    if m.stop { return false, 0, 0; }
    let start = relative_index(relative, len);
    var removed: i64 = 0;
    if count == 1 { removed = len - start; }
    else if count > 1 {
        let n = micro_number(job, m, b, job.operands[m.base + mutate_slot::Args + 1]);
        if m.stop { return false, 0, 0; }
        removed = min(max(n, 0.0), (len - start) as f64) as i64;
    }
    return true, start, removed;
}

// ---- splice(start, deleteCount, ...items) (23.1.3.31): P actualStart, Q
// actualDeleteCount, S the item count, R the stage's cursor.
fn splice_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeSplice;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count; let count = job.frames[m.at].index;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let ok, start, removed = splice_range(job, m, b);
        if !ok { return; }
        let items = max(count - 2, 0);
        if items - removed > max_safe_integer() - len { type_error(job, "array-like length exceeds 2^53 - 1"); m.stop = true; return; }
        micro_unit_done(job, m);
        mutate_put(job, m.base, mutate_slot::P, start); mutate_put(job, m.base, mutate_slot::Q, removed); mutate_put(job, m.base, mutate_slot::S, items); mutate_put(job, m.base, mutate_slot::R, 0);
        // ArraySpeciesCreate(O, actualDeleteCount): made now, or at pc
        // Constructor, Species or Created.
        if species_begin(job, m.at, b) { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Collect); } else { m.stop = true; }
        return;
    }
    let start = mutate_number(job, m.base, mutate_slot::P); let removed = mutate_number(job, m.base, mutate_slot::Q); let items = mutate_number(job, m.base, mutate_slot::S);
    let k = mutate_number(job, m.base, mutate_slot::R);
    let a = job.frames[m.at].output;
    if stage == mutate_stage::Collect {
        if k == 0 && mutate_number(job, m.base, mutate_slot::Done) == 0 && (items <= removed || mutator_grows(job)) && !(a == o) && mutator_writable(job, a, node(heap, a.id()).length) {
            let fast = mutator_dense(job, o, len);
            if heap.fault != Fault.None { m.stop = true; return; }
            if fast { splice_dense(job, m, o, a, start, removed, items); return; }
        }
        if k >= removed { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::ResultLength); return; }
        let present = micro_has(job, m, o, start + k);
        if m.stop { return; }
        if present {
            let v = micro_get(job, m, b, o, start + k);
            if m.stop { return; }
            micro_create(job, m, a, k, v);
            if m.stop { return; }
        }
        mutate_put(job, m.base, mutate_slot::R, k + 1);
        micro_unit_done(job, m);
        return;
    }
    if stage == mutate_stage::ResultLength {
        micro_set_length(job, m, b, a, removed);
        if m.stop { return; }
        micro_unit_done(job, m);
        if items < removed { mutate_put(job, m.base, mutate_slot::R, start); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Move); }
        else if items > removed { mutate_put(job, m.base, mutate_slot::R, len - removed); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::MoveUp); }
        else { mutate_put(job, m.base, mutate_slot::R, 0); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Items); }
        return;
    }
    // Fewer items than removed elements: the tail moves down, then the
    // indices past the new length are deleted from the top.
    if stage == mutate_stage::Move {
        if k >= len - removed { mutate_put(job, m.base, mutate_slot::R, len); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Trim); return; }
        micro_move(job, m, b, o, k + removed, k + items);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::R, k + 1);
        micro_unit_done(job, m);
        return;
    }
    if stage == mutate_stage::Trim {
        if k <= len - removed + items { mutate_put(job, m.base, mutate_slot::R, 0); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Items); return; }
        micro_delete(job, m, b, o, k - 1);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::R, k - 1);
        micro_unit_done(job, m);
        return;
    }
    // More items: the tail moves up, from the top.
    if stage == mutate_stage::MoveUp {
        if k <= start { mutate_put(job, m.base, mutate_slot::R, 0); mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Items); return; }
        micro_move(job, m, b, o, k + removed - 1, k + items - 1);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::R, k - 1);
        micro_unit_done(job, m);
        return;
    }
    if stage == mutate_stage::Items {
        if k >= items { mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Finish); return; }
        micro_set_index(job, m, b, o, start + k, job.operands[m.base + mutate_slot::Args + 2 + k]);
        if m.stop { return; }
        mutate_put(job, m.base, mutate_slot::R, k + 1);
        micro_unit_done(job, m);
        return;
    }
    micro_set_length(job, m, b, o, len - removed + items);
    if m.stop { return; }
    micro_finish(job, m, a);
}
// splice of a dense receiver into a dense result (mutator_dense,
// mutator_writable): the removed elements stored in A's run and its length
// set, then O's tail and the items written in place.
fn splice_dense(job: Job&, m: Micro&, o: Value, a: Value, start: i64, removed: i64, items: i64) {
    let heap .= job.heap; let len = job.frames[m.at].argument_count;
    m.stop = true;
    for i in removed {
        let v, _ = own_element(heap, o, start + i);
        if !set_element(heap, a, i, v) { return; }
    }
    if !array_length(heap, a, number(removed)) { if heap.fault == Fault.None { heap.fault = Fault.TypeError; } return; }
    var tail: Value[>..] = [];
    for i in (start + removed)..len { let v, _ = own_element(heap, o, i); tail.push(v); }
    for j in items { if !set_element(heap, o, start + j, job.operands[m.base + mutate_slot::Args + 2 + j]) { return; } }
    for v, t in tail { if !set_element(heap, o, start + items + t, v) { return; } }
    if !array_length(heap, o, number(len - removed + items)) { if heap.fault == Fault.None { heap.fault = Fault.TypeError; } return; }
    micro_finish(job, m, a);
}

// ---- toSpliced(start, skipCount, ...items) (23.1.3.35): P actualStart, Q
// the skip count, S the item count, R the next result index. The result is
// a new plain array this frame fills; holes read as undefined.
fn to_spliced_unit(job: Job&, m: Micro&) {
    let heap .= job.heap; let b = Builtin.ArrayPrototypeToSpliced;
    let o = job.frames[m.at].input; let len = job.frames[m.at].argument_count; let count = job.frames[m.at].index;
    let stage = mutate_number(job, m.base, mutate_slot::Stage);
    if stage == mutate_stage::Setup {
        let ok, start, skipped = splice_range(job, m, b);
        if !ok { return; }
        let items = max(count - 2, 0);
        if items - skipped > max_safe_integer() - len { type_error(job, "array-like length exceeds 2^53 - 1"); m.stop = true; return; }
        micro_unit_done(job, m);
        let length: f64 = len + items - skipped;
        let a = array_species_create(job, b, o, length);
        if heap.fault != Fault.None { m.stop = true; return; }
        job.frames[m.at].output = a;
        mutate_put(job, m.base, mutate_slot::P, start); mutate_put(job, m.base, mutate_slot::Q, skipped); mutate_put(job, m.base, mutate_slot::S, items); mutate_put(job, m.base, mutate_slot::R, 0);
        mutate_put(job, m.base, mutate_slot::Stage, mutate_stage::Copy);
        let fast = mutator_dense(job, o, len);
        if heap.fault != Fault.None { m.stop = true; return; }
        if fast {
            m.stop = true;
            for i in (len + items - skipped) {
                var v = Value.Undefined;
                if i < start { let e, _ = own_element(heap, o, i); v = e; }
                else if i < start + items { v = job.operands[m.base + mutate_slot::Args + 2 + i - start]; }
                else { let e, _ = own_element(heap, o, i - items + skipped); v = e; }
                if !set_element(heap, a, i, v) { return; }
            }
            micro_finish(job, m, a);
        }
        return;
    }
    let start = mutate_number(job, m.base, mutate_slot::P); let skipped = mutate_number(job, m.base, mutate_slot::Q); let items = mutate_number(job, m.base, mutate_slot::S);
    let i = mutate_number(job, m.base, mutate_slot::R);
    let a = job.frames[m.at].output;
    if i >= len + items - skipped { micro_finish(job, m, a); return; }
    var v = Value.Undefined;
    if i >= start && i < start + items { v = job.operands[m.base + mutate_slot::Args + 2 + i - start]; }
    else {
        v = micro_get(job, m, b, o, if i < start { i } else { i - items + skipped });
        if m.stop { return; }
    }
    if !set_element(heap, a, i, v) { m.stop = true; return; }
    mutate_put(job, m.base, mutate_slot::R, i + 1);
    micro_unit_done(job, m);
}
