import std;
import .source_vm;
import .realm;
import .function_calls;
import .proxy;

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
// A proxy's [[PreventExtensions]] and [[IsExtensible]] fail by name
// (light-cljs-891). Light-cljs runtime values (forms) fail by name too: they
// have no own properties and take none here, while the V8 build's runtime
// (src/js_runtime.js makeVector, for one) makes them ordinary extensible
// objects, so no answer would agree with both.

// The kinds with [[Extensible]]: every JavaScript object kind with own
// properties (snapshot.goose named).
fn integrity_kind(kind: i64) -> bool {
    return kind == node_kind::Object || kind == node_kind::Array || kind == node_kind::Closure || kind == node_kind::Builtin || kind == node_kind::Error || kind == node_kind::Arguments || kind == node_kind::Generator || kind == node_kind::Promise || kind == node_kind::Wrapper || kind == node_kind::Iterator || kind == node_kind::BoundFunction;
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
    if !writable(job, o) { heap.fault = Fault.NativeBoundary; return false; }
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
    // A row the job may not write changes nothing if it is at that level already.
    if !writable(job, o) {
        if test_integrity(job, b, object, frozen) || heap.fault != Fault.None { return heap.fault == Fault.None; }
        heap.fault = Fault.NativeBoundary; return false;
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
// unless a proxy is involved, and a proxy fails by name).
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
