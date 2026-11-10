import std;
import .source_vm;
import .realm;
import .function_calls;
import .proxy;
import .object_statics;

namespace source;

// [[Extensible]] and integrity levels (light-cljs-882; decisions 2, 3 and 5
// of docs/javascript-coverage.md). Semantics from the specification
// (ECMAScript 7.3.15 SetIntegrityLevel, 7.3.16 TestIntegrityLevel, 10.1.3
// OrdinaryPreventExtensions, 10.1.4 OrdinaryIsExtensible, 10.4.4.2 a
// mapped arguments object's [[DefineOwnProperty]], 20.1.2.6
// Object.freeze, 20.1.2.15 to 20.1.2.17 Object.isExtensible, isFrozen and
// isSealed, 20.1.2.19 Object.preventExtensions, 20.1.2.22 Object.seal,
// 28.1.10 Reflect.isExtensible, 28.1.12 Reflect.preventExtensions), the
// Pants engine's rust/src/vm/integrity.rs and QuickJS's
// js_object_seal and JS_PreventExtensions.
//
// The state is the node's: [[Extensible]] cleared is Node.non_extensible
// (decision 2: state on the object row; no table, no link), which every
// kind with own properties carries (integrity_kind) and which snapshot
// validation, plain_bytes, in-job reclamation and the worker reset carry
// as any node field. A non-extensible object refuses a new property in
// [[Set]] (assign: a TypeError in strict code, nothing in sloppy code) and
// [[DefineOwnProperty]] (define_property, define_descriptor: a TypeError
// from Object.defineProperty, false from Reflect.defineProperty), an array
// a new element (push throws through its [[Set]]) while a shorter `length`
// still works, and [[SetPrototypeOf]] another prototype (set_prototype).
// Only the job that allocated a node sets it (decision 3: a realm row,
// restored definition or installed publication faults NativeBoundary
// before anything changes, unless the call would change nothing); the realm
// builds %ThrowTypeError% non-extensible, as QuickJS seals it.
//
// SetIntegrityLevel (set_integrity) makes the object non-extensible, then
// clears `configurable` from every own property and, frozen, `writable`
// from every data property, by kind and without running JavaScript:
//   - shape keys get a new chain of transitions with the same keys
//     (restrict_shape), so values keep their slots and objects sealed alike
//     share shapes; dictionary records change in place;
//   - an ordinary or arguments object's elements, whose attributes are all
//     set by definition (light-cljs-804), become shape keys or records;
//     freezing a mapped arguments object first unmaps each mapped index
//     with its current value (10.4.4.2: a definition that makes it
//     non-writable), and sealing keeps the mapping, its cells naming the
//     key their property moved to;
//   - a dense array keeps its run: array_flag::Sealed or Frozen says what
//     its elements' attributes are now (element_attrs), so it stays dense
//     and costs a record nothing an element; freezing also makes its
//     `length` read-only. A dictionary array's records change in place
//     (Attributed). A template object is such a frozen array (light-cljs-862);
//   - a closure's lazy `length`, `name` and `prototype` are made first
//     (prepare_function), as the Pants engine's function record holds them
//     before it stops taking properties; a String wrapper's exotic `length`
//     and indices are already neither writable nor configurable.
// TestIntegrityLevel (test_integrity) reads the attributes own_property_keys
// lists. Objects that never stop being extensible pay one byte test where
// a new property is added (and none on the dense-array fast paths, which
// already test the node).
//
// A proxy's [[PreventExtensions]] and [[IsExtensible]] run as its Proxy
// frame where Object.isExtensible, preventExtensions and their Reflect
// counterparts can start one (light-cljs-891, proxy.goose
// proxy_conditional). Object.seal, freeze, isSealed and isFrozen of a proxy
// run the specification's steps through its internal methods in a Statics
// frame where one can start (light-cljs-897, "Integrity levels of a proxy"
// below); the plain built-ins here, run to completion where no frame can
// start, fail by name on a proxy. Light-cljs runtime values (forms) fail by
// name too: they have no own properties and take none here, while the V8
// build's runtime (src/js_runtime.js makeVector, for one) makes them
// ordinary extensible objects, so no answer would agree with both.

// The kinds with [[Extensible]]: every JavaScript object kind with own
// properties (snapshot.goose named).
fn integrity_kind(kind: i64) -> bool {
    return kind == node_kind::Object || kind == node_kind::Array || kind == node_kind::Closure || kind == node_kind::Builtin || kind == node_kind::Error || kind == node_kind::Arguments || kind == node_kind::Generator || kind == node_kind::Promise || kind == node_kind::Wrapper || kind == node_kind::Iterator || kind == node_kind::RegExp || kind == node_kind::BoundFunction;
}
// IsExtensible(object) for Goose callers (proxy invariants): a proxy's fails
// by name, as does a form's.
fn is_extensible(heap: Heap&, object: Value) -> bool {
    let kind = node(heap, object.id()).kind;
    if kind == node_kind::Proxy { proxy_fault(heap, proxy_method::IsExtensible); return false; }
    if !integrity_kind(kind) { if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; } return false; }
    return !node(heap, object.id()).non_extensible;
}
// Fail built-in b by name for an object without [[Extensible]] here: a
// proxy (`method`, its internal method) or a form. True when it has one.
fn integrity_target(job: Job&, b: Builtin, object: Value, method: i64) -> bool {
    let heap .= job.heap; let kind = node(heap, object.id()).kind;
    if kind == node_kind::Proxy { proxy_fault(heap, method); return false; }
    if !integrity_kind(kind) { unsupported_step(job, b, "a light-cljs runtime value"); return false; }
    // An async generator's `prototype` is not made yet (source_vm.goose
    // unsupported_prototype), so it cannot stop being extensible (and so is
    // never sealed or frozen).
    if method == proxy_method::PreventExtensions && kind == node_kind::Closure && unsupported_prototype(heap, object.id()) { unsupported_step(job, b, "an async generator function"); return false; }
    return true;
}
// [[PreventExtensions]] (OrdinaryPreventExtensions): true, or false with a
// fault. A row the job did not allocate faults NativeBoundary unless it is
// already non-extensible.
fn prevent_extensions(job: Job&, b: Builtin, object: Value) -> bool {
    let heap .= job.heap; let o: i64 = object.id();
    if !integrity_target(job, b, object, proxy_method::PreventExtensions) { return false; }
    if node(heap, o).non_extensible { return true; }
    if !writable(job, o) { image_write(job, Value.Undefined, "extensibility"); return false; }
    // A lazy closure's properties come first (the Pants engine's function.rs).
    prepare_function(job, object, Value.Undefined, true);
    if heap.fault != Fault.None { return false; }
    node_mut(heap, o).non_extensible = true;
    return true;
}
// TestIntegrityLevel(object, sealed or frozen).
fn test_integrity(job: Job&, b: Builtin, object: Value, frozen: bool) -> bool {
    let heap .= job.heap;
    if !integrity_target(job, b, object, proxy_method::IsExtensible) || !node(heap, object.id()).non_extensible { return false; }
    var keys: OwnKey[>..] = [];
    if !own_property_keys(heap, object, keys) { return false; }
    for e in keys { if e.attrs & attr_configurable() != 0 || frozen && e.attrs & attr_writable() != 0 { return false; } }
    return true;
}
// SetIntegrityLevel(object, sealed or frozen): true, or false with a fault
// (a memory or work limit leaves a valid object partly restricted, as a
// failing DefinePropertyOrThrow would).
fn set_integrity(job: Job&, b: Builtin, object: Value, frozen: bool) -> bool {
    let heap .= job.heap; let o: i64 = object.id();
    if !integrity_target(job, b, object, proxy_method::PreventExtensions) { return false; }
    // A shareable regex value is sealed already, and its `lastIndex` stays
    // writable (light-cljs-935; the Pants engine's integrity.rs).
    if shared_regexp_node(node(heap, o)) {
        if frozen { type_error(job, shared_writable_message()); return false; }
        return true;
    }
    // A row the job may not write changes nothing if it is at that level already.
    if !writable(job, o) {
        if test_integrity(job, b, object, frozen) || heap.fault != Fault.None { return heap.fault == Fault.None; }
        image_write(job, Value.Undefined, "integrity level"); return false;
    }
    if !prevent_extensions(job, b, object) { return false; }
    let kind = node(heap, o).kind;
    if element_kind(kind) && node(heap, o).count > 0 {
        // Freezing unmaps every mapped index first, keeping its value.
        if frozen {
            let n = mapped_count(heap, o);
            for i in n {
                let c = mapped_cell_of(heap, o, i);
                if c < 0 { continue; }
                let v = mapped_value(heap, o, i, c);
                if heap.fault != Fault.None { return false; }
                unmap(heap, o, i, c, v);
            }
        }
        let count: i64 = node(heap, o).count;
        if !charge(heap, count) { return false; }
        for i in count {
            let v = slot(heap, node(heap, o).start + i);
            if v.is_hole() { continue; }
            let k = index_key(heap, i);
            if heap.fault != Fault.None || !add_own(heap, o, k, v, restricted(attrs_data(), false, frozen)) { if heap.fault == Fault.None { memory(heap); } return false; }
            set_slot(heap, node(heap, o).start + i, hole());
            // A mapped index stays mapped to the key its property moved to.
            let c = mapped_cell_of(heap, o, i);
            if c >= 0 { node_mut(heap, c).namespace_value = k; }
        }
    }
    if node(heap, o).initialized {
        let count: i64 = node(heap, o).key_count;
        if !charge(heap, count) { return false; }
        for i in count { var p = dict_at(heap, o, i); p.attrs = restricted(p.attrs, is_accessor(p.value), frozen); set_dict_at(heap, o, i, p); }
        if kind == node_kind::Array { node_mut(heap, o).function = node(heap, o).function | array_flag::Attributed; }
    } else {
        if node(heap, o).key_count > 0 && !restrict_shape(heap, o, frozen) { if heap.fault == Fault.None { memory(heap); } return false; }
        if kind == node_kind::Array { node_mut(heap, o).function = node(heap, o).function | array_flag::Sealed | (if frozen { array_flag::Frozen } else { 0 }); }
    }
    if kind == node_kind::Array && frozen { node_mut(heap, o).function = node(heap, o).function | array_flag::ReadOnlyLength; }
    return true;
}
// The number of indices arguments object o's map lists (0 for an object
// without one).
fn mapped_count(heap: const Heap&, o: i64) -> i64 {
    let n .= node(heap, o);
    return if n.kind == node_kind::Arguments && n.namespace_value.is_object() { node(heap, n.namespace_value.id()).count } else { 0 };
}

// ---- The built-ins (decision 5: plain functions, since no JavaScript runs
// unless a proxy is involved: proxy.goose's proxy_conditional and
// start_integrity below start a proxy's frames first, and a proxy that
// reaches these fails by name).
fn integrity_builtin(job: Job&, b: Builtin, target: Value) -> Value {
    let heap .= job.heap;
    if !target.is_object() {
        // Object's methods take any value: a primitive is not extensible
        // and is sealed and frozen already, and comes back unchanged.
        return match b {
            ObjectIsExtensible => boolean(false),
            ObjectIsSealed, ObjectIsFrozen => boolean(true),
            ReflectIsExtensible => type_error(job, "Reflect.isExtensible: the target is not an object"),
            ReflectPreventExtensions => type_error(job, "Reflect.preventExtensions: the target is not an object"),
            _ => target,
        };
    }
    if !valid(heap, target) { return Value.Undefined; }
    var ok = false;
    match b {
        ObjectIsExtensible, ReflectIsExtensible => { if integrity_target(job, b, target, proxy_method::IsExtensible) { ok = !node(heap, target.id()).non_extensible; } },
        ObjectPreventExtensions, ReflectPreventExtensions => { ok = prevent_extensions(job, b, target); },
        ObjectSeal => { ok = set_integrity(job, b, target, false); },
        ObjectFreeze => { ok = set_integrity(job, b, target, true); },
        ObjectIsSealed => { ok = test_integrity(job, b, target, false); },
        ObjectIsFrozen => { ok = test_integrity(job, b, target, true); },
        _ => { heap.fault = Fault.InvalidState; },
    }
    if heap.fault != Fault.None { return Value.Undefined; }
    return match b { ObjectPreventExtensions, ObjectSeal, ObjectFreeze => target, _ => boolean(ok) };
}

// ---- Integrity levels of a proxy (light-cljs-897)
// Object.seal, freeze, isSealed and isFrozen are conditional natives
// (realm.goose conditional_native): of a proxy, SetIntegrityLevel (7.3.15)
// and TestIntegrityLevel (7.3.16) run in a Statics frame
// (object_statics.goose; Native.Statics, the built-in its payload) whose
// `input` is the proxy, each of the proxy's internal methods a Proxy frame
// above it whose answer arrives at the Statics frame's pc (decision 4):
//
//   statics_pc::Prevented   [[PreventExtensions]]'s boolean (seal, freeze;
//                           false is a TypeError)
//   statics_pc::Extensible  [[IsExtensible]]'s boolean (isSealed, isFrozen;
//                           true answers false)
//   statics_pc::List        [[OwnPropertyKeys]]'s key list, then the frame's
//                           `callee`, `index` the next key
//   statics_pc::Own         [[GetOwnProperty]] of key index - 1 (freeze,
//                           isSealed, isFrozen)
//   statics_pc::Defined     [[DefineOwnProperty]] of key index - 1 with
//                           {configurable: false}, and for a data property
//                           being frozen also {writable: false}
//                           (DefinePropertyOrThrow: false is a TypeError)
//
// Seal defines every key without reading it; freeze reads each key's
// descriptor first and skips a key without one; the tests answer false at
// the first configurable property (or, frozen, writable data property) and
// true after the last key. Any other argument is the plain built-in, with
// no frame. The descriptors the frame defines with are job rows a Proxy
// frame holds, truncated with the job (decision 3); the frame keeps no
// state outside its fields (decision 2).
fn integrity_walks(b: Builtin) -> bool { return match b { ObjectSeal, ObjectFreeze, ObjectIsSealed, ObjectIsFrozen => true, _ => false }; }
inline fn integrity_tests(b: Builtin) -> bool { return b == Builtin.ObjectIsSealed || b == Builtin.ObjectIsFrozen; }
// Start built-in b (integrity_walks) with `args` at operand base `base`
// (native_start's contract).
fn start_integrity(job: Job&, b: Builtin, args: const Value[:], base: i64) {
    let heap .= job.heap; let o = arg(args, 0);
    if !is_proxy(heap, o) { let r = integrity_builtin(job, b, o); statics_plain(job, base, r); return; }
    let tests = integrity_tests(b);
    statics_frame(job, b, base, if tests { statics_pc::Extensible } else { statics_pc::Prevented }, o, Value.Undefined);
    proxy_push(job, if tests { proxy_method::IsExtensible } else { proxy_method::PreventExtensions }, o, Value.Undefined, Value.Undefined, Value.Undefined, answer::Boolean, job.operands.len);
}
// One step of an integrity built-in's Statics frame (statics_step).
fn integrity_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let v = job.operands.pop();
    if !is_proxy(heap, frame.input) { heap.fault = Fault.InvalidState; return; }
    match frame.pc {
        statics_pc::Prevented => {
            if !truthy(heap, v) { type_error(job, "proxy preventExtensions handler returned false"); return; }
            integrity_keys(job, at);
        },
        statics_pc::Extensible => {
            if truthy(heap, v) { finish_native(job, frame, boolean(false)); return; }
            integrity_keys(job, at);
        },
        statics_pc::List => {
            if !v.is_object() || node(heap, v.id()).kind != node_kind::KeyList { heap.fault = Fault.InvalidState; return; }
            job.frames[at].callee = v; job.frames[at].index = 0;
            integrity_next(job, at, b);
        },
        statics_pc::Own => { integrity_descriptor(job, at, b, v); },
        statics_pc::Defined => {
            if !truthy(heap, v) { named_error(job, Fault.TypeError, "cannot redefine property '", integrity_key(heap, frame), "'"); return; }
            integrity_next(job, at, b);
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// The proxy's [[OwnPropertyKeys]], its list arriving at pc List.
fn integrity_keys(job: Job&, at: i64) {
    job.frames[at].pc = statics_pc::List;
    push_proxy(job, proxy_method::OwnPropertyKeys, job.frames[at].input, Value.Undefined);
}
// The key the frame is at (index - 1).
fn integrity_key(heap: const Heap&, frame: Frame) -> Value { return property_row(heap, node(heap, frame.callee.id()).keys + frame.index - 1).key; }
// The next key: seal defines it, the others read its descriptor; with none
// left, the answer (the proxy, or true for the tests).
fn integrity_next(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap; let frame = job.frames[at];
    let i = frame.index;
    if i >= node(heap, frame.callee.id()).key_count { finish_native(job, frame, if integrity_tests(b) { boolean(true) } else { frame.input }); return; }
    if !charge(heap) { return; }
    job.frames[at].index = i + 1;
    let k = property_row(heap, node(heap, frame.callee.id()).keys + i).key;
    if b == Builtin.ObjectSeal { integrity_define(job, at, k, false); return; }
    job.frames[at].pc = statics_pc::Own;
    push_proxy(job, proxy_method::GetOwnProperty, frame.input, k);
}
// [[GetOwnProperty]]'s answer d (undefined, or a complete descriptor) for
// the key the frame is at.
fn integrity_descriptor(job: Job&, at: i64, b: Builtin, d: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if d.is_undefined() { integrity_next(job, at, b); return; }
    if !d.is_object() || node(heap, d.id()).kind != node_kind::KeyList { heap.fault = Fault.InvalidState; return; }
    let accessor = has_field(heap, d, field::Get) || has_field(heap, d, field::Set);
    if integrity_tests(b) {
        let open = flag_field(heap, d, field::Configurable) || b == Builtin.ObjectIsFrozen && !accessor && flag_field(heap, d, field::Writable);
        if heap.fault != Fault.None { return; }
        if open { finish_native(job, frame, boolean(false)); return; }
        integrity_next(job, at, b);
        return;
    }
    if heap.fault == Fault.None { integrity_define(job, at, integrity_key(heap, frame), !accessor); }
}
// DefinePropertyOrThrow(proxy, k, {configurable: false}), with
// `read_only` also {writable: false}: the proxy's [[DefineOwnProperty]],
// its answer arriving at pc Defined.
fn integrity_define(job: Job&, at: i64, k: Value, read_only: bool) {
    let heap .= job.heap;
    let d = new_descriptor(heap);
    if heap.fault != Fault.None || !set_field(heap, d, field::Configurable, boolean(false)) { return; }
    if read_only && !set_field(heap, d, field::Writable, boolean(false)) { return; }
    job.frames[at].pc = statics_pc::Defined;
    proxy_push(job, proxy_method::DefineOwnProperty, job.frames[at].input, k, Value.Undefined, d, answer::Boolean, job.operands.len);
}
