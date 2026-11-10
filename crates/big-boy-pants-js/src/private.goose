import std;
import .source_vm;
import .snapshot;
import .realm;
import .export_schema;
import .function_calls;

namespace source;

// Private class elements (light-cljs-889; docs/javascript-coverage.md,
// decision 2, "As landed by light-cljs-889"; the Pants engine's
// vm/private.rs and dispatch.rs private_step, bbp-291; QuickJS's
// JS_DefinePrivateField, JS_GetPrivateField, JS_SetPrivateField and
// js_operator_private_in).
//
// A private name (`#x`) is a symbol row (light-cljs-885, node_kind::JsSymbol):
// NewPrivateName makes a fresh unregistered one whose description is the
// name's source text, each time a class body is evaluated, and the Rust
// lowering keeps it in an ordinary const binding, which methods, arrows and
// direct eval code capture like any other (so eval code resolves `#x`
// through light-cljs-858's eval sites). JavaScript never holds the symbol as
// a value: only the private instructions below take it.
//
// An object's private elements are not properties. They are records of a
// node of their own, node_kind::PrivateElements, which the object's
// `metadata` names (a JavaScript object has no Clojure metadata, so the field
// is otherwise Undefined there; a form's metadata stays a map). Its records
// are dictionary-mode records in heap.properties (keys, key_count,
// key_capacity, grown by dict_append): the key is the private name, the
// value the field's value or the method's function, or an Accessor naming
// the getter/setter pair, and attrs is attr_writable() for a field and 0
// for a method or accessor. Nothing else links to the node.
//
// So no key path changes: own_keys, key lists, for-in, descriptors,
// integrity levels, JSON and every proxy trap read shapes, records and
// element runs, never `metadata`, and an object without private elements
// costs nothing anywhere. Freezing or preventing extensions of an object
// leaves its private elements writable and extensible, and a proxy has its
// own, never its target's. The rows are plain u32 links (decision 2): the
// worker reset truncates them with the job, in-job reclamation marks and
// moves `metadata` and record runs (reclaim.goose trace_node and relink, via
// snapshot::record_run), saved continuations keep every row they reach, and
// snapshot validation checks the node, its records and its one owner.
//
// The brand check: Get, Put and Define look only at the receiver's own
// records, never a prototype, and a missing name is a TypeError. Private
// methods and accessors are defined on each instance before its first field
// (the Rust lowering's order), so a method's record is its brand.

// Whether node kind `kind` can hold private elements: every JavaScript
// object with own properties (snapshot::named) and a proxy. A form (a
// light-cljs runtime value) cannot: its `metadata` is its Clojure metadata.
fn private_holder(kind: i64) -> bool { return kind == node_kind::Proxy || snapshot::named(kind); }
// The PrivateElements row of object v, or -1 when it has none (or is not a
// holder).
fn private_row(heap: const Heap&, v: Value) -> i64 {
    if !v.is_object() { return -1; }
    let m = node(heap, v.id()).metadata;
    if !m.is_object() || !private_holder(node(heap, v.id()).kind) { return -1; }
    return if node(heap, m.id()).kind == node_kind::PrivateElements { m.id() } else { -1 };
}
// The record of private name `name` among v's private elements: its index
// and the record, or -1. One work unit a record visited.
fn private_find(heap: Heap&, v: Value, name: Value) -> i64, Property {
    let r = private_row(heap, v);
    if r < 0 { return -1, Property {}; }
    let w = key_word(name);
    for i in node(heap, r).key_count {
        if !charge(heap) { return -1, Property {}; }
        let p = dict_at(heap, r, i);
        if key_word(p.key) == w { return i, p; }
    }
    return -1, Property {};
}
// The messages are the Pants engine's (vm/private.rs).
fn private_error(job: Job&) { type_error(job, "invalid private member receiver or operation"); }
// A private name operand: a symbol row NewPrivateName made.
fn private_name(job: Job&, name: Value) -> bool {
    if symbol_node(name) < 0 { job.heap.fault = Fault.InvalidState; return false; }
    return valid(job.heap, name);
}

// NewPrivateName (183): → a fresh private name, `description` its source
// text (a name constant).
fn new_private_name(job: Job&, description: Value) {
    let s = new_symbol(job.heap, description, false);
    if job.heap.fault == Fault.None { job.operands.push(s); }
}
// GetPrivate (184): receiver name → value. A getter runs as a frame entered
// from the read with the receiver as `this`, its return value the result
// (decision 4); a setter-only element is a TypeError.
fn get_private(job: Job&) {
    let heap .= job.heap;
    let name = job.operands.pop(); let object = job.operands.pop();
    if !private_name(job, name) || !valid(heap, object) { return; }
    let i, p = private_find(heap, object, name);
    if heap.fault != Fault.None { return; }
    if i < 0 { private_error(job); return; }
    if !is_accessor(p.value) { job.operands.push(p.value); return; }
    let getter = pair(heap, p.value).get;
    if !getter.is_object() { type_error(job, "private accessor has no getter"); return; }
    if node(heap, getter.id()).kind == node_kind::Closure { enter(job, getter, object, []); } else { call_accessor(job, getter, object, []); }
}
// PutPrivate (185): receiver name value → value. A setter runs in a Setter
// native frame with the receiver as `this`, the assignment's value its
// result; a method, a getter-only element or a missing name is a TypeError.
// A field of a row the job did not allocate faults NativeBoundary before it
// changes, as every write does (decision 3).
fn put_private(job: Job&) {
    let heap .= job.heap;
    let value = job.operands.pop(); let name = job.operands.pop(); let object = job.operands.pop();
    if !private_name(job, name) || !valid(heap, object) || !valid(heap, value) { return; }
    let i, p = private_find(heap, object, name);
    if heap.fault != Fault.None { return; }
    if i < 0 { private_error(job); return; }
    if is_accessor(p.value) {
        let setter = pair(heap, p.value).set;
        if !setter.is_object() { private_error(job); return; }
        call_setter(job, setter, object, value, value);
        return;
    }
    if p.attrs & attr_writable() == 0 { private_error(job); return; }
    if !writable(job, object.id()) { image_write(job, Value.Undefined, "a private field"); return; }
    var q = p; q.value = value; set_dict_at(heap, private_row(heap, object), i, q);
    job.operands.push(value);
}
// PrivateIn (190): name receiver → whether the receiver has the private
// element; a primitive receiver is a TypeError. Nothing runs: a proxy's
// traps are never consulted.
fn private_in(job: Job&) {
    let heap .= job.heap;
    let object = job.operands.pop(); let name = job.operands.pop();
    if !private_name(job, name) || !valid(heap, object) { return; }
    if !javascript_object(heap, object) { type_error(job, "private brand check on a non-object"); return; }
    let i, p = private_find(heap, object, name);
    if heap.fault == Fault.None { job.operands.push(boolean(i >= 0)); }
}
// DefinePrivateField, DefinePrivateMethod, DefinePrivateGetter and
// DefinePrivateSetter (186-189; `op` the opcode): receiver name value →
// receiver. A field is writable and a method read-only; a getter and a
// setter of one name share a record, each half defined once. Any second
// definition is a TypeError, so the constructor-return trick adds a field to
// a foreign object once. The receiver's extensibility does not matter. A
// receiver that cannot hold private elements (a primitive, a form) is a
// TypeError; a row the job did not allocate faults NativeBoundary.
fn define_private(job: Job&, op: i64) {
    let heap .= job.heap;
    let value = job.operands.pop(); let name = job.operands.pop(); let object = job.operands[job.operands.len - 1];
    if !private_name(job, name) || !valid(heap, object) || !valid(heap, value) { return; }
    if !object.is_object() || !private_holder(node(heap, object.id()).kind) { private_error(job); return; }
    if !writable(job, object.id()) { image_write(job, Value.Undefined, "a private field"); return; }
    let getter = op == opcode::DefinePrivateGetter; let setter = op == opcode::DefinePrivateSetter;
    let i, p = private_find(heap, object, name);
    if heap.fault != Fault.None { return; }
    if i >= 0 {
        // The other half of an accessor: a new pair (pairs are never
        // written in place).
        let old = pair(heap, p.value);
        if is_accessor(p.value) && (getter && old.get.is_undefined() || setter && old.set.is_undefined()) {
            let a = add_accessor(heap, if getter { Pair { get: value, set: old.set } } else { Pair { get: old.get, set: value } });
            if a < 0 { return; }
            var q = p; q.value = Value.Accessor { a: a as u32 }; set_dict_at(heap, private_row(heap, object), i, q);
            return;
        }
        type_error(job, "private member already initialized");
        return;
    }
    var record = Property { key: name, value: value, attrs: if op == opcode::DefinePrivateField { attr_writable() } else { 0 } };
    if getter || setter {
        let a = add_accessor(heap, if getter { Pair { get: value } } else { Pair { set: value } });
        if a < 0 { return; }
        record.value = Value.Accessor { a: a as u32 };
    }
    var r = private_row(heap, object);
    if r < 0 {
        let row = allocate(heap, Node { kind: node_kind::PrivateElements });
        if !row.is_object() { return; }
        r = row.id(); node_mut(heap, object.id()).metadata = row;
    }
    dict_append(heap, r, record);
}
