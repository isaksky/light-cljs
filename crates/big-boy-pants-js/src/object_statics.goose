import std;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .function_calls;
import .proxy;

namespace source;

// The Object statics that expose own keys, with Reflect.ownKeys and
// Reflect.has (light-cljs-884; decisions 2, 3, 4 and 5 of
// docs/javascript-coverage.md). Semantics from the specification
// (ECMAScript 7.3.23 EnumerableOwnProperties, 7.4.11 IteratorClose, 20.1.2.1
// Object.assign, 20.1.2.5 Object.entries, 20.1.2.7 Object.fromEntries,
// 20.1.2.9 Object.getOwnPropertyDescriptors, 20.1.2.10 and 20.1.2.11
// Object.getOwnPropertyNames and getOwnPropertySymbols, 20.1.2.13
// Object.groupBy, 20.1.2.18 Object.keys, 20.1.2.24 Object.values, 20.1.3.4
// Object.prototype.propertyIsEnumerable, 20.1.3.5 toLocaleString, 20.1.3.6
// toString's @@toStringTag, 28.1.9 Reflect.has, 28.1.11 Reflect.ownKeys),
// the Pants engine's rust/src/vm/keys.rs, vm/native/object_statics.rs and
// copy_properties.rs, and QuickJS's js_object_keys, JS_GetOwnPropertyNames2,
// js_object_assign, js_object_fromEntries, js_object_groupBy and
// js_object_propertyIsEnumerable.
//
// Own keys come from source_vm.goose's own_property_keys ([[OwnPropertyKeys]]
// order for every kind) into a key list, light-cljs-804's job row
// (decision 2: no new table); results are new job rows (decision 3). The
// built-ins run as a Statics native frame (Native.Statics, its Builtin the
// payload), since a getter, a setter, a callback, an iterator, a key's
// ToPrimitive or a proxy trap runs JavaScript (decision 4); a call that
// meets none of them pushes and pops the frame in one step, outside the
// frame limit, so it behaves as a plain function (decision 5). The frame:
//
//   input     the object whose keys are walked (ToObject of the argument;
//             Object.assign's current source); for an iteration the item
//             being processed, and while the iterator closes the exception
//   callee    the key list of `input`'s own keys (Undefined while a
//             proxy's [[OwnPropertyKeys]] runs); for an iteration the
//             iterator record (source_vm.goose get_iterator)
//   output    the result: the array or object being built, or
//             Object.assign's target
//   receiver  Object.assign's sources (an array row), Object.groupBy's
//             callback, Object.fromEntries's entry key while its value is
//             read
//   index     the next key of the list; Object.groupBy's item count
//   argument_count  Object.assign's next source
//   pc        the value awaited (statics_pc::)
//
// EnumerableOwnProperties (keys, values, entries) and Object.assign re-read
// each key's [[GetOwnProperty]] just before it is used, so a getter that
// deletes a later key or makes it non-enumerable is observed; values,
// entries and assign then [[Get]] it, a getter running above the frame.
// Object.assign [[Set]]s each value on its target: a setter runs above the
// frame, a refusal is a TypeError, and a row the job did not allocate
// faults NativeBoundary before it changes. Object.getOwnPropertyDescriptors
// reads descriptors, never values through getters.
//
// Object.fromEntries and Object.groupBy (QuickJS-only) iterate: GetIterator
// (get_iterator) delivers the record, each step calls `next` and reads
// `done` and `value` (getters above the frame). While an item is processed
// (fromEntries's entry reads and key conversion, groupBy's callback and key
// conversion) the frame holds a Catch handler at statics_pc::Close, so an
// abrupt completion there, its own TypeError included, closes the iterator
// (IteratorClose: `return`'s exception is dropped, the original rethrown);
// an exception from `next`, `done` or `value` does not.
//
// A proxy's [[OwnPropertyKeys]] and [[GetOwnProperty]] run as
// light-cljs-845's Proxy frame above this one; its [[Get]], [[Set]] and
// [[HasProperty]] fail by name (light-cljs-890), as do a light-cljs runtime
// value as the object, and a Symbol or BigInt wrapper (ToObject).
//
// Object.prototype.toString, Object.hasOwn and
// Object.prototype.hasOwnProperty stay plain built-ins (realm.goose); where
// they can start a frame (conditional_native) a @@toStringTag getter or a
// key object's ToPrimitive runs in this frame too.

// The value a Statics frame awaits (its pc).
let statics_pc::Walk = 0;
let statics_pc::List = 1;
let statics_pc::Own = 2;
let statics_pc::Value = 3;
let statics_pc::Set = 4;
let statics_pc::Record = 5;
let statics_pc::Step = 6;
let statics_pc::Done = 7;
let statics_pc::Item = 8;
let statics_pc::First = 9;
let statics_pc::Second = 10;
let statics_pc::Key = 11;
let statics_pc::Group = 12;
let statics_pc::Close = 13;
let statics_pc::Return = 14;
let statics_pc::Closed = 15;
let statics_pc::Tag = 16;
let statics_pc::Method = 17;
let statics_pc::Result = 18;
let statics_pc::Iterable = 19;

// The built-ins that always start a Statics frame (native_start).
fn statics_native(b: Builtin) -> bool {
    return match b {
        ObjectKeys, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectGetOwnPropertySymbols, ObjectGetOwnPropertyDescriptors, ObjectAssign, ObjectFromEntries, ObjectGroupBy,
        ObjectPrototypePropertyIsEnumerable, ObjectPrototypeToLocaleString, ReflectOwnKeys, ReflectHas => true,
        _ => false,
    };
}
// The plain built-ins that start a Statics frame only when a step runs
// JavaScript (conditional_native).
inline fn statics_conditional(b: Builtin) -> bool { return b == Builtin.ObjectPrototypeToString || b == Builtin.ObjectHasOwn || b == Builtin.ObjectPrototypeHasOwnProperty; }
// The built-ins that walk a key list.
inline fn statics_walks(b: Builtin) -> bool { return b == Builtin.ObjectKeys || b == Builtin.ObjectValues || b == Builtin.ObjectEntries || b == Builtin.ObjectGetOwnPropertyDescriptors || b == Builtin.ObjectAssign; }
// The built-ins that list key names only.
inline fn statics_names(b: Builtin) -> bool { return b == Builtin.ObjectGetOwnPropertyNames || b == Builtin.ObjectGetOwnPropertySymbols || b == Builtin.ReflectOwnKeys; }
// The built-ins that iterate.
inline fn statics_iterates(b: Builtin) -> bool { return b == Builtin.ObjectFromEntries || b == Builtin.ObjectGroupBy; }
// The built-ins whose second step is a key's ToPropertyKey.
inline fn statics_keyed(b: Builtin) -> bool { return b == Builtin.ObjectHasOwn || b == Builtin.ObjectPrototypeHasOwnProperty || b == Builtin.ObjectPrototypePropertyIsEnumerable || b == Builtin.ReflectHas; }

// ---- Starting
// Start built-in b (statics_native or statics_conditional), called with
// `receiver` and `args`, its operand base `base` (native_start's contract).
fn start_statics(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    if statics_names(b) || statics_walks(b) && b != Builtin.ObjectAssign { start_keys(job, b, arg(args, 0), base); return; }
    if b == Builtin.ObjectAssign { start_assign(job, args, base); return; }
    if statics_iterates(b) { start_iteration(job, b, args, base); return; }
    if statics_keyed(b) { start_keyed(job, b, receiver, args, base); return; }
    if b == Builtin.ObjectPrototypeToString { start_tag(job, receiver, base); return; }
    if b == Builtin.ObjectPrototypeToLocaleString { start_locale(job, receiver, base); return; }
    heap.fault = Fault.InvalidState;
}
// The Statics frame of built-in b, pushed outside the frame limit (frames
// above it count): its row.
fn statics_frame(job: Job&, b: Builtin, base: i64, pc: i64, input: Value, output: Value) -> i64 {
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: pc, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: Value.Undefined, input: input, output: output, native: Native.Statics { which: b } });
    return job.frames.len - 1;
}
// Deliver result r of a built-in that started no frame.
fn statics_plain(job: Job&, base: i64, r: Value) { if job.heap.fault == Fault.None { job.operands.resize(base); deliver(job, r); } }
// Push a Proxy frame running `method` of proxy p (key k for
// [[GetOwnProperty]]); its result arrives at the frame below.
fn push_proxy(job: Job&, method: i64, p: Value, k: Value) {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: Value.Undefined, input: p, output: k, native: Native.Proxy { method: method as u8 } });
}
// A key list of object o's own keys (symbols included), or Undefined with a
// fault. A proxy's are not listed here.
fn own_key_list(job: Job&, o: Value) -> Value {
    let heap .= job.heap;
    if !listable(job, o) { return Value.Undefined; }
    var keys: OwnKey[>..] = [];
    if !own_property_keys(heap, o, keys) { return Value.Undefined; }
    return key_list(heap, o, keys, true);
}
// Append v to array a (a job row the frame built).
fn push_element(heap: Heap&, a: Value, v: Value) -> bool { return set_element(heap, a, node(heap, a.id()).length, v); }

// ---- Object.keys, values, entries, getOwnPropertyNames,
// getOwnPropertySymbols, getOwnPropertyDescriptors and Reflect.ownKeys
fn start_keys(job: Job&, b: Builtin, v: Value, base: i64) {
    let heap .= job.heap;
    if b == Builtin.ReflectOwnKeys && !v.is_object() { type_error(job, "Reflect.ownKeys: the target is not an object"); return; }
    let o = to_object(job, v);
    if heap.fault != Fault.None || !listable(job, o) { return; }
    let result = if b == Builtin.ObjectGetOwnPropertyDescriptors { allocate(heap, Node { kind: node_kind::Object }) } else { allocate(heap, Node { kind: node_kind::Array }) };
    if heap.fault != Fault.None { return; }
    if is_proxy(heap, o) {
        let at = statics_frame(job, b, base, statics_pc::List, o, result);
        push_proxy(job, proxy_method::OwnPropertyKeys, o, Value.Undefined);
        return;
    }
    let list = own_key_list(job, o);
    if heap.fault != Fault.None { return; }
    if statics_names(b) { statics_plain(job, base, names_of(job, b, list, result)); return; }
    let at = statics_frame(job, b, base, statics_pc::Walk, o, result);
    job.frames[at].callee = list;
    walk_keys(job, at, b);
}
// The names of key list `list` that built-in b lists, appended to array a.
fn names_of(job: Job&, b: Builtin, list: Value, a: Value) -> Value {
    let heap .= job.heap; let l: i64 = list.id();
    for i in node(heap, l).key_count {
        let k = property_row(heap, node(heap, l).keys + i).key;
        if b == Builtin.ObjectGetOwnPropertyNames && !k.is_string() || b == Builtin.ObjectGetOwnPropertySymbols && !k.is_symbol() { continue; }
        if !push_element(heap, a, k) { return Value.Undefined; }
    }
    return if heap.fault == Fault.None { a } else { Value.Undefined };
}
// The walk of the frame at `at` from its next key: each key's
// [[GetOwnProperty]], then the built-in's use of it. Returns when the frame
// awaits JavaScript, finished or failed.
fn walk_keys(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap;
    loop {
        if heap.fault != Fault.None { return; }
        let list = job.frames[at].callee;
        if !list.is_object() || node(heap, list.id()).kind != node_kind::KeyList { heap.fault = Fault.InvalidState; return; }
        let i = job.frames[at].index;
        if i >= node(heap, list.id()).key_count {
            if b == Builtin.ObjectAssign { if !next_source(job, at) { return; } continue; }
            finish_native(job, job.frames[at], job.frames[at].output); return;
        }
        if !charge(heap) { return; }
        let k = property_row(heap, node(heap, list.id()).keys + i).key;
        job.frames[at].index = i + 1;
        // EnumerableOwnProperties lists string keys only.
        if !k.is_string() && b != Builtin.ObjectAssign && b != Builtin.ObjectGetOwnPropertyDescriptors { continue; }
        let o = job.frames[at].input;
        if is_proxy(heap, o) { job.frames[at].pc = statics_pc::Own; push_proxy(job, proxy_method::GetOwnProperty, o, k); return; }
        if b == Builtin.ObjectGetOwnPropertyDescriptors {
            prepare_function(job, o, k, false);
            if heap.fault != Fault.None { return; }
            let p = own_entry(heap, o, k);
            if heap.fault != Fault.None { return; }
            if !p.found { continue; }
            if is_pending(p.value) { pending_fault(heap, k); return; }
            let d = descriptor_object(heap, p);
            if heap.fault != Fault.None || !define_property(heap, job.frames[at].output, k, d, attrs_data()) { return; }
            continue;
        }
        let e = enumerable_entry(heap, o, k);
        if heap.fault != Fault.None { return; }
        if !e.found || e.attrs & attr_enumerable() == 0 { continue; }
        if b == Builtin.ObjectKeys { if !push_element(heap, job.frames[at].output, k) { return; } continue; }
        if !get_value(job, at, b, o, k) { return; }
    }
}
// [[Get]] of key k of `o` for the frame at `at`: a getter runs above the
// frame (pc Value); a data value is used now (use_value). Whether the walk
// goes on.
fn get_value(job: Job&, at: i64, b: Builtin, o: Value, k: Value) -> bool {
    let heap .= job.heap;
    if is_proxy(heap, o) { proxy_fault(heap, proxy_method::Get); return false; }
    prepare_function(job, o, k, false);
    if heap.fault != Fault.None { return false; }
    let v, getter = lookup(heap, o, k);
    if heap.fault != Fault.None { return false; }
    if getter.is_object() { job.frames[at].pc = statics_pc::Value; run_getter(job, b, getter, o); return false; }
    return use_value(job, at, b, k, v);
}
// Value v of key k (the key index - 1): values and entries append it,
// Object.assign sets it on its target (a setter above the frame, pc Set).
// Whether the walk goes on.
fn use_value(job: Job&, at: i64, b: Builtin, k: Value, v: Value) -> bool {
    let heap .= job.heap; let out = job.frames[at].output;
    if b == Builtin.ObjectValues { return push_element(heap, out, v); }
    if b == Builtin.ObjectEntries {
        let pair = allocate(heap, Node { kind: node_kind::Array });
        return heap.fault == Fault.None && set_element(heap, pair, 0, k) && set_element(heap, pair, 1, v) && push_element(heap, out, pair);
    }
    if b != Builtin.ObjectAssign { heap.fault = Fault.InvalidState; return false; }
    // Set(to, key, value, true).
    if is_proxy(heap, out) { proxy_fault(heap, proxy_method::Set); return false; }
    if !writable(job, out.id()) { heap.fault = Fault.NativeBoundary; return false; }
    prepare_function(job, out, k, true);
    prepare_function(job, out, k, false);
    let setter, ok = write(heap, out, k, v);
    if heap.fault != Fault.None { property_fault(job, out, k, true, true); return false; }
    if setter.is_object() { job.frames[at].pc = statics_pc::Set; call_accessor(job, setter, out, [v]); return false; }
    if !ok { refused_write(job, out, k); if heap.fault == Fault.None { heap.fault = Fault.TypeError; } return false; }
    record(job, out, k, v);
    return true;
}
// Object.getOwnPropertyDescriptors and the enumerability checks over a
// proxy: descriptor d (undefined, or a descriptor key list) of key
// index - 1 arrived.
fn proxy_descriptor(job: Job&, at: i64, b: Builtin, d: Value) {
    let heap .= job.heap;
    let list = job.frames[at].callee;
    let k = property_row(heap, node(heap, list.id()).keys + job.frames[at].index - 1).key;
    if !d.is_undefined() && (!d.is_object() || node(heap, d.id()).kind != node_kind::KeyList) { heap.fault = Fault.InvalidState; return; }
    if b == Builtin.ObjectGetOwnPropertyDescriptors {
        if d.is_object() {
            let o = descriptor_fields_object(heap, d);
            if heap.fault != Fault.None || !define_property(heap, job.frames[at].output, k, o, attrs_data()) { return; }
        }
        walk_keys(job, at, b); return;
    }
    if !d.is_object() || !flag_field(heap, d, field::Enumerable) { if heap.fault == Fault.None { walk_keys(job, at, b); } return; }
    if b == Builtin.ObjectKeys { if push_element(heap, job.frames[at].output, k) { walk_keys(job, at, b); } return; }
    if get_value(job, at, b, job.frames[at].input, k) { walk_keys(job, at, b); }
}
// FromPropertyDescriptor of a complete descriptor key list d (a proxy's
// [[GetOwnProperty]] result): its fields in the specification's order.
fn descriptor_fields_object(heap: Heap&, d: Value) -> Value {
    let o = allocate(heap, Node { kind: node_kind::Object });
    if heap.fault != Fault.None { return Value.Undefined; }
    let order: i64[6] = [field::Value, field::Writable, field::Get, field::Set, field::Enumerable, field::Configurable];
    for f in order {
        let v, has = field_of(heap, d, f);
        if heap.fault != Fault.None { return Value.Undefined; }
        if has && !realm_value(heap, o.id(), FIELD_NAMES[f].text, v, attrs_data()) { return Value.Undefined; }
    }
    return o;
}

// ---- Object.assign
fn start_assign(job: Job&, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let target = to_object(job, arg(args, 0));
    if heap.fault != Fault.None { return; }
    if args.len <= 1 { statics_plain(job, base, target); return; }
    let sources = allocate(heap, Node { kind: node_kind::Array });
    if heap.fault != Fault.None { return; }
    for v, i in args[1..] { if !set_element(heap, sources, i, v) { return; } }
    let at = statics_frame(job, Builtin.ObjectAssign, base, statics_pc::Walk, Value.Undefined, target);
    job.frames[at].receiver = sources;
    if next_source(job, at) { walk_keys(job, at, Builtin.ObjectAssign); }
}
// Object.assign's next source: undefined and null are skipped; another's
// keys are listed (a proxy's by its frame, false), or with none left the
// target is delivered (false). True when the walk goes on.
fn next_source(job: Job&, at: i64) -> bool {
    let heap .= job.heap; let sources = job.frames[at].receiver;
    loop {
        let i = job.frames[at].argument_count;
        if i >= node(heap, sources.id()).length { finish_native(job, job.frames[at], job.frames[at].output); return false; }
        job.frames[at].argument_count = i + 1;
        let v, found = own_element(heap, sources, i);
        if heap.fault != Fault.None { return false; }
        if v.tag() <= value_tag::Null { continue; }
        let from = to_object(job, v);
        if heap.fault != Fault.None || !listable(job, from) { return false; }
        job.frames[at].input = from; job.frames[at].index = 0;
        if is_proxy(heap, from) { job.frames[at].callee = Value.Undefined; job.frames[at].pc = statics_pc::List; push_proxy(job, proxy_method::OwnPropertyKeys, from, Value.Undefined); return false; }
        let list = own_key_list(job, from);
        if heap.fault != Fault.None { return false; }
        job.frames[at].callee = list;
        return true;
    }
}

// ---- Object.fromEntries and Object.groupBy
fn start_iteration(job: Job&, b: Builtin, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let items = arg(args, 0); let callback = arg(args, 1);
    if items.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); return; }
    if b == Builtin.ObjectGroupBy && !is_callable(heap, callback) { type_error(job, "not a function"); return; }
    // groupBy's result has a null prototype (OrdinaryObjectCreate(null)).
    let result = allocate(heap, if b == Builtin.ObjectGroupBy { Node { kind: node_kind::Object, proto: Value.Null } } else { Node { kind: node_kind::Object } });
    if heap.fault != Fault.None { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    let at = statics_frame(job, b, base, statics_pc::Iterable, Value.Undefined, result);
    if b == Builtin.ObjectGroupBy { job.frames[at].receiver = callback; }
    // GetIterator runs from the frame's first step (pc Iterable), which
    // takes the iterable from its operands.
    push_operand(job, items);
}
// GetIterator(items): the record arrives on this frame's operands, now or
// from a GetIterator frame above; the next step (pc Record) takes it.
fn begin_iteration(job: Job&, at: i64, items: Value) {
    job.frames[at].pc = statics_pc::Record;
    get_iterator(job, items);
}
// The iterator record's field `name`.
fn record_field(job: Job&, record: Value, name: const u8[:]) -> Value {
    let heap .= job.heap;
    let k = key_named(heap, name);
    if heap.fault != Fault.None { return Value.Undefined; }
    let v, found = own_property(heap, record, k);
    if !found && heap.fault == Fault.None { heap.fault = Fault.InvalidState; }
    return v;
}
// IteratorStep: call `next`; its result arrives at pc Step.
fn iterate(job: Job&, at: i64) {
    let heap .= job.heap; let record = job.frames[at].callee;
    let it = record_field(job, record, "iterator");
    let next = record_field(job, record, "next");
    if heap.fault != Fault.None { return; }
    // IteratorNext's Call: a `next` that is not callable is a TypeError.
    if !is_callable(heap, next) { type_error(job, "iterator next is not a function"); return; }
    job.frames[at].pc = statics_pc::Step; job.frames[at].input = Value.Undefined;
    call_accessor(job, next, it, []);
}
// Read field `name` of object o for the frame at `at`: a getter runs above
// it, its value arriving at `pc` (false); otherwise true and the value.
fn read_field(job: Job&, at: i64, b: Builtin, o: Value, name: const u8[:], pc: i64) -> bool, Value {
    let heap .= job.heap;
    if is_proxy(heap, o) { proxy_fault(heap, proxy_method::Get); return false, Value.Undefined; }
    let k = key_named(heap, name);
    if heap.fault != Fault.None { return false, Value.Undefined; }
    let v, getter = read(heap, o, k);
    if heap.fault != Fault.None { return false, Value.Undefined; }
    if getter.is_object() { job.frames[at].pc = pc; run_getter(job, b, getter, o); return false, Value.Undefined; }
    return true, v;
}
// `next`'s result r arrived: it must be an object; then its `done`.
fn step_result(job: Job&, at: i64, b: Builtin, r: Value) {
    let heap .= job.heap;
    if !r.is_object() { type_error(job, "iterator result is not an object"); return; }
    job.frames[at].input = r;
    let ready, done = read_field(job, at, b, r, "done", statics_pc::Done);
    if ready { step_done(job, at, b, truthy(heap, done)); }
}
// IteratorComplete gave `done`: the result is delivered, or the item read.
fn step_done(job: Job&, at: i64, b: Builtin, done: bool) {
    if done { finish_native(job, job.frames[at], job.frames[at].output); return; }
    let ready, v = read_field(job, at, b, job.frames[at].input, "value", statics_pc::Item);
    if ready { take_item(job, at, b, v); }
}
// The frame's Catch handler, which closes the iterator on an abrupt
// completion while an item is processed (pc Close or Closed).
fn close_guard(job: Job&, at: i64, pc: i64) {
    job.handlers.push(Handler { frame: at, pc: pc, operands: job.operands.len, locals: job.locals.len, kind: HandlerKind.Catch });
}
fn close_unguard(job: Job&, at: i64, pc: i64) {
    let n = job.handlers.len;
    if n > 0 && job.handlers[n - 1].frame == at && job.handlers[n - 1].pc == pc && job.handlers[n - 1].kind == HandlerKind.Catch { job.handlers.pop(); }
}
// Item v arrived: fromEntries reads its entry's `0` and `1`; groupBy calls
// its callback with (v, k).
fn take_item(job: Job&, at: i64, b: Builtin, v: Value) {
    let heap .= job.heap;
    job.frames[at].input = v;
    close_guard(job, at, statics_pc::Close);
    if b == Builtin.ObjectGroupBy {
        let k = job.frames[at].index;
        job.frames[at].pc = statics_pc::Group;
        let f64k: f64 = k;
        call_accessor(job, job.frames[at].receiver, Value.Undefined, [v, number(f64k)]);
        return;
    }
    if !v.is_object() { type_error(job, "iterator value is not an entry object"); return; }
    let ready, k = read_field(job, at, b, v, "0", statics_pc::First);
    if ready { take_first(job, at, b, k); }
}
fn take_first(job: Job&, at: i64, b: Builtin, k: Value) {
    job.frames[at].receiver = k;
    let ready, v = read_field(job, at, b, job.frames[at].input, "1", statics_pc::Second);
    if ready { take_second(job, at, b, v); }
}
// The entry's key (receiver) and value v: the key's ToPropertyKey (an
// object's in a Convert frame above, pc Key), then the definition.
fn take_second(job: Job&, at: i64, b: Builtin, v: Value) {
    let heap .= job.heap;
    let k = job.frames[at].receiver;
    job.frames[at].input = v; job.frames[at].receiver = Value.Undefined;
    if convertible(heap, k) { job.frames[at].pc = statics_pc::Key; push_operand(job, k); start_convert(job, Hint.String, false, 1, 1); return; }
    take_key(job, at, b, k);
}
// The item's key kv (a primitive, or ToPrimitive's result): fromEntries
// defines it on its result, groupBy adds the item to its group; then the
// next step.
fn take_key(job: Job&, at: i64, b: Builtin, kv: Value) {
    let heap .= job.heap; let out = job.frames[at].output; let v = job.frames[at].input;
    let k = property_key(job, b, kv);
    if heap.fault != Fault.None { return; }
    if b == Builtin.ObjectFromEntries {
        if !define_property(heap, out, k, v, attrs_data()) { if heap.fault == Fault.None { heap.fault = Fault.TypeError; } return; }
        record(job, out, k, v);
    } else {
        let group, found = own_property(heap, out, k);
        if heap.fault != Fault.None { return; }
        var g = group;
        if !found {
            g = allocate(heap, Node { kind: node_kind::Array });
            if heap.fault != Fault.None || !define_property(heap, out, k, g, attrs_data()) { return; }
        }
        if !g.is_object() || !push_element(heap, g, v) { if heap.fault == Fault.None { heap.fault = Fault.InvalidState; } return; }
        job.frames[at].index += 1;
    }
    close_unguard(job, at, statics_pc::Close);
    iterate(job, at);
}
// IteratorClose after an abrupt completion: the exception e arrived at the
// frame's handler. `return` is read and called with a handler of its own
// (pc Closed); whatever it does, e is rethrown.
fn close_iterator(job: Job&, at: i64, b: Builtin, e: Value) {
    let heap .= job.heap;
    job.frames[at].input = e;
    let it = record_field(job, job.frames[at].callee, "iterator");
    if heap.fault != Fault.None { return; }
    close_guard(job, at, statics_pc::Closed);
    let ready, m = read_field(job, at, b, it, "return", statics_pc::Return);
    if heap.fault != Fault.None { return; }
    if ready { call_return(job, at, m); }
}
fn call_return(job: Job&, at: i64, m: Value) {
    let heap .= job.heap;
    if m.tag() <= value_tag::Null || !is_callable(heap, m) { rethrow(job, at); return; }
    let it = record_field(job, job.frames[at].callee, "iterator");
    if heap.fault != Fault.None { return; }
    job.frames[at].pc = statics_pc::Closed;
    call_accessor(job, m, it, []);
}
// Rethrow the exception that closed the iterator (IteratorClose's
// completion is the original throw).
fn rethrow(job: Job&, at: i64) {
    close_unguard(job, at, statics_pc::Closed);
    let frame = job.frames[at];
    raise(job, frame.input, frame);
}

// ---- Keyed built-ins: Object.hasOwn, Object.prototype.hasOwnProperty and
// propertyIsEnumerable, Reflect.has
fn start_keyed(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let own = b == Builtin.ObjectHasOwn || b == Builtin.ReflectHas;
    let o = if own { arg(args, 0) } else { receiver };
    let kv = arg(args, if own { 1 } else { 0 });
    // Object.hasOwn converts its object first, Reflect.has requires one;
    // hasOwnProperty and propertyIsEnumerable convert their key first.
    if b == Builtin.ReflectHas && !o.is_object() { type_error(job, "Reflect.has: the target is not an object"); return; }
    if b == Builtin.ObjectHasOwn && o.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); return; }
    if convertible(heap, kv) {
        let at = statics_frame(job, b, base, statics_pc::Key, o, Value.Undefined);
        push_operand(job, kv); start_convert(job, Hint.String, false, 1, 1);
        return;
    }
    let k = property_key(job, b, kv);
    if heap.fault != Fault.None { return; }
    // Object.hasOwn and hasOwnProperty without a conversion stay the plain
    // functions they are (no frame).
    if b == Builtin.ObjectHasOwn || b == Builtin.ObjectPrototypeHasOwnProperty {
        if o.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); return; }
        let found = has_own(job, b, o, k);
        statics_plain(job, base, boolean(found)); return;
    }
    let at = statics_frame(job, b, base, statics_pc::Key, o, Value.Undefined);
    keyed(job, at, b, k);
}
// The keyed built-in's answer for interned key k.
fn keyed(job: Job&, at: i64, b: Builtin, k: Value) {
    let heap .= job.heap; let o = job.frames[at].input;
    if b == Builtin.ReflectHas {
        let found = has_property(heap, o, k);
        if heap.fault == Fault.None { finish_native(job, job.frames[at], boolean(found)); }
        return;
    }
    if o.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); return; }
    if b != Builtin.ObjectPrototypePropertyIsEnumerable {
        let found = has_own(job, b, o, k);
        if heap.fault == Fault.None { finish_native(job, job.frames[at], boolean(found)); }
        return;
    }
    // propertyIsEnumerable: ToObject(this).[[GetOwnProperty]](k).
    if o.is_string() { let i = index(heap, k); if heap.fault == Fault.None { finish_native(job, job.frames[at], boolean(i >= 0 && i < str_len(heap, o))); } return; }
    if !o.is_object() { finish_native(job, job.frames[at], boolean(false)); return; }
    if is_proxy(heap, o) { job.frames[at].pc = statics_pc::Own; push_proxy(job, proxy_method::GetOwnProperty, o, k); return; }
    if !listable(job, o) { return; }
    let e = enumerable_entry(heap, o, k);
    if heap.fault == Fault.None { finish_native(job, job.frames[at], boolean(e.found && e.attrs & attr_enumerable() != 0)); }
}

// ---- Object.prototype.toString's @@toStringTag getter and toLocaleString
fn start_tag(job: Job&, receiver: Value, base: i64) {
    let heap .= job.heap;
    if receiver.tag() <= value_tag::Null { statics_plain(job, base, object_tag(job, receiver)); return; }
    let tag, getter = tag_lookup(job, receiver);
    if heap.fault != Fault.None { return; }
    if !getter.is_object() { statics_plain(job, base, object_tag_text(job, receiver, tag)); return; }
    // The getter's this is ToObject(this).
    let o = to_object(job, receiver);
    if heap.fault != Fault.None { return; }
    let at = statics_frame(job, Builtin.ObjectPrototypeToString, base, statics_pc::Tag, receiver, Value.Undefined);
    run_getter(job, Builtin.ObjectPrototypeToString, getter, o);
}
// Object.prototype.toLocaleString: Invoke(this, "toString").
fn start_locale(job: Job&, receiver: Value, base: i64) {
    let heap .= job.heap; let b = Builtin.ObjectPrototypeToLocaleString;
    if receiver.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); return; }
    let at = statics_frame(job, b, base, statics_pc::Method, receiver, Value.Undefined);
    let ready, m = read_field(job, at, b, receiver, "toString", statics_pc::Method);
    if ready { locale_call(job, at, m); }
}
fn locale_call(job: Job&, at: i64, m: Value) {
    let heap .= job.heap;
    if !is_callable(heap, m) { type_error(job, "toString is not a function"); return; }
    job.frames[at].pc = statics_pc::Result;
    call_accessor(job, m, job.frames[at].input, []);
}

// ---- The frame's step
fn statics_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let v = job.operands.pop();
    match frame.pc {
        statics_pc::List => {
            if !v.is_object() || node(heap, v.id()).kind != node_kind::KeyList { heap.fault = Fault.InvalidState; return; }
            if statics_names(b) { let r = names_of(job, b, v, frame.output); if heap.fault == Fault.None { finish_native(job, frame, r); } return; }
            job.frames[at].callee = v; job.frames[at].index = 0;
            walk_keys(job, at, b);
        },
        statics_pc::Own => {
            if statics_keyed(b) {
                let enumerable = v.is_object() && flag_field(heap, v, field::Enumerable);
                if heap.fault == Fault.None { finish_native(job, frame, boolean(enumerable)); }
                return;
            }
            proxy_descriptor(job, at, b, v);
        },
        statics_pc::Value => {
            let list = frame.callee;
            let k = property_row(heap, node(heap, list.id()).keys + frame.index - 1).key;
            if use_value(job, at, b, k, v) { walk_keys(job, at, b); }
        },
        // The setter's return value is dropped.
        statics_pc::Set => { walk_keys(job, at, b); },
        statics_pc::Iterable => { begin_iteration(job, at, v); },
        statics_pc::Record => {
            if !v.is_object() { heap.fault = Fault.InvalidState; return; }
            job.frames[at].callee = v;
            iterate(job, at);
        },
        statics_pc::Step => { step_result(job, at, b, v); },
        statics_pc::Done => { step_done(job, at, b, truthy(heap, v)); },
        statics_pc::Item => { take_item(job, at, b, v); },
        statics_pc::First => { take_first(job, at, b, v); },
        statics_pc::Second => { take_second(job, at, b, v); },
        statics_pc::Key => { if statics_keyed(b) { let k = property_key(job, b, v); if heap.fault == Fault.None { keyed(job, at, b, k); } } else { take_key(job, at, b, v); } },
        statics_pc::Group => {
            if convertible(heap, v) { job.frames[at].pc = statics_pc::Key; push_operand(job, v); start_convert(job, Hint.String, false, 1, 1); return; }
            take_key(job, at, b, v);
        },
        statics_pc::Close => { close_iterator(job, at, b, v); },
        statics_pc::Return => { call_return(job, at, v); },
        statics_pc::Closed => { rethrow(job, at); },
        statics_pc::Tag => { let r = object_tag_text(job, frame.input, v); if heap.fault == Fault.None { finish_native(job, frame, r); } },
        statics_pc::Method => { locale_call(job, at, v); },
        statics_pc::Result => { finish_native(job, frame, v); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
