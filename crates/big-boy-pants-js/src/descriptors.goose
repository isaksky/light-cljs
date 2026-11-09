import std;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .function_calls;
import .proxy;

namespace source;

// Property descriptors (light-cljs-881; decisions 2, 3, 4 and 5 of
// docs/javascript-coverage.md). Semantics from the specification
// (ECMAScript 6.2.6 the Property Descriptor type, 6.2.6.5
// ToPropertyDescriptor, 10.1.6.3 ValidateAndApplyPropertyDescriptor,
// 10.4.2.4 ArraySetLength, 20.1.2.3 Object.defineProperties, 20.1.2.4
// Object.defineProperty, 28.1.3 Reflect.defineProperty, 28.1.4
// Reflect.deleteProperty, 28.1.7 Reflect.getOwnPropertyDescriptor, B.2.2.2
// to B.2.2.5 __defineGetter__ and its siblings), the Pants engine's
// rust/src/vm/descriptor.rs and object_statics.rs, and QuickJS's
// js_object_defineProperty and JS_DefineProperty.
//
// Attributes stay where decision 2 put them: in shapes and dictionary
// records, with an array's two flags in its node (array_flag); there is no
// descriptor table. A descriptor while it is read is the key list
// proxy.goose's [[GetOwnProperty]] builds (new_descriptor, `field::` names);
// define_descriptor takes it as a PropertyDescriptor value, which never outlives
// the call.
//
// Object.defineProperty, Reflect.defineProperty and Object.defineProperties
// run as a Descriptor native frame (Native.Descriptor, its Builtin the
// payload), since ToPropertyDescriptor reads its fields through
// [[HasProperty]] and [[Get]] and a getter runs JavaScript (decision 4):
//   input     the target object O
//   output    the property key (defineProperty), or a key list of the
//             properties object's own keys (defineProperties), the object
//             in its `value` and, once read, each key's descriptor (or
//             Null: not enumerable, or gone) in its record's value
//   receiver  the descriptor object whose fields are read
//   callee    the descriptor being built (proxy.goose new_descriptor)
//   index     ToPropertyDescriptor's next field (FIELD_NAMES order)
//   argument_count  defineProperties's next key
//   pc 0      (defineProperty) awaits the key's ToPrimitive from a Convert
//             frame (an object key), the output still undefined
//   pc 1      (defineProperties) awaits Get(properties, key) from its getter
//   pc 2      awaits field `index` from its getter
// defineProperties reads every descriptor before the first definition
// (object_statics.rs object_statics_define_properties_conversion_before_writes).
// Then the definitions run in key order as plain Goose: none of them runs
// JavaScript. A definition refused is a TypeError for Object's methods and
// false for Reflect.defineProperty.
//
// ValidateAndApplyPropertyDescriptor (define_descriptor) covers every
// JavaScript object kind through the property model's [[GetOwnProperty]]
// (own_entry) and [[DefineOwnProperty]] (define_property with
// half::Complete): ordinary objects and their elements, arguments objects
// (light-cljs-860's mapping), arrays (`length` by ArraySetLength, elements
// with attributes by dictionary mode), closures (their lazy `length`,
// `name` and `prototype` made first), built-ins, bound functions, String
// wrappers (exotic indices and `length`, which change by nothing), errors,
// generators, promises, iterators, frozen template arrays and
// %ThrowTypeError% (not extensible). A write into a row the job did not
// allocate (the realm, a restored definition, an installed publication)
// faults NativeBoundary before anything changes (decision 3). A proxy's
// [[DefineOwnProperty]] fails by name (light-cljs-891).
//
// The other methods run no JavaScript and are plain built-ins:
// Reflect.getOwnPropertyDescriptor and Reflect.deleteProperty require an
// object target; Annex B's __defineGetter__ and __defineSetter__ define an
// enumerable, configurable accessor half, and __lookupGetter__ and
// __lookupSetter__ walk the prototype chain to the first own property of
// the key (QuickJS-only: the Pants engine lacks them).

// A complete or partial property descriptor (6.2.6): which fields it has
// and their values; a flag field's value is its ToBoolean.
struct PropertyDescriptor {
    value:     Value = Value.Undefined, get: Value = Value.Undefined, set: Value = Value.Undefined,
    has_value: bool  = false, has_writable: bool = false, has_get: bool = false, has_set: bool = false, has_enumerable: bool = false, has_configurable: bool = false,
    writable:  bool  = false, enumerable: bool = false, configurable: bool = false,
}
inline fn accessor_descriptor(d: PropertyDescriptor) -> bool { return d.has_get || d.has_set; }
inline fn data_descriptor(d: PropertyDescriptor) -> bool { return d.has_value || d.has_writable; }
// The built-ins that run as a Descriptor frame (native_start).
inline fn descriptor_native(b: Builtin) -> bool { return b == Builtin.ObjectDefineProperty || b == Builtin.ObjectDefineProperties || b == Builtin.ReflectDefineProperty; }

// The PropertyDescriptor of descriptor key list d (proxy.goose's fields).
fn descriptor_of(heap: Heap&, d: Value) -> PropertyDescriptor {
    var r = PropertyDescriptor {};
    let value, has_value = field_of(heap, d, field::Value); r.value = value; r.has_value = has_value;
    let get, has_get = field_of(heap, d, field::Get); r.get = get; r.has_get = has_get;
    let set, has_set = field_of(heap, d, field::Set); r.set = set; r.has_set = has_set;
    let w, has_w = field_of(heap, d, field::Writable); r.writable = has_w && w.flag(); r.has_writable = has_w;
    let e, has_e = field_of(heap, d, field::Enumerable); r.enumerable = has_e && e.flag(); r.has_enumerable = has_e;
    let c, has_c = field_of(heap, d, field::Configurable); r.configurable = has_c && c.flag(); r.has_configurable = has_c;
    return r;
}
inline fn flag_attrs(writable: bool, enumerable: bool, configurable: bool) -> u8 {
    return (if writable { attr_writable() } else { 0 }) | (if enumerable { attr_enumerable() } else { 0 }) | (if configurable { attr_configurable() } else { 0 });
}

// ---- ValidateAndApplyPropertyDescriptor (10.1.6.3)
// [[DefineOwnProperty]](k, d) of `object` (k interned): whether it was
// accepted. A definition that changes nothing is accepted without a write.
fn define_descriptor(job: Job&, object: Value, k: Value, d: PropertyDescriptor) -> bool {
    let heap .= job.heap; let o: i64 = object.id();
    if node(heap, o).kind == node_kind::Proxy { proxy_fault(heap, proxy_method::DefineOwnProperty); return false; }
    if !writable(job, o) { heap.fault = Fault.NativeBoundary; return false; }
    prepare_function(job, object, k, true);
    if heap.fault != Fault.None { return false; }
    let current = own_entry(heap, object, k);
    if heap.fault != Fault.None { return false; }
    if current.found && is_pending(current.value) { pending_fault(heap, k); return false; }
    if node(heap, o).kind == node_kind::Array && key_is(heap, k, "length") { return define_length(job, object, k, current, d); }
    let accessor = accessor_descriptor(d);
    if !current.found {
        if sealed(heap, o) { return false; }
        if accessor {
            let a = add_accessor(heap, Pair { get: d.get, set: d.set });
            if a < 0 { return false; }
            return define_property(heap, object, k, Value.Accessor { a: a as u32 }, flag_attrs(false, d.enumerable, d.configurable), half::Complete);
        }
        return define_property(heap, object, k, d.value, flag_attrs(d.writable, d.enumerable, d.configurable), half::Complete);
    }
    let was_accessor = is_accessor(current.value);
    let fns = pair(heap, current.value);
    let was_enumerable = current.attrs & attr_enumerable() != 0;
    let was_writable = current.attrs & attr_writable() != 0;
    if current.attrs & attr_configurable() == 0 {
        if d.has_configurable && d.configurable { return false; }
        if d.has_enumerable && d.enumerable != was_enumerable { return false; }
        if accessor && !was_accessor || data_descriptor(d) && was_accessor { return false; }
        if was_accessor {
            if d.has_get && !same_value(heap, d.get, fns.get) || d.has_set && !same_value(heap, d.set, fns.set) { return false; }
        } else if !was_writable {
            if d.has_writable && d.writable { return false; }
            if d.has_value && !same_value(heap, d.value, current.value) { return false; }
        }
    }
    let enumerable = if d.has_enumerable { d.enumerable } else { was_enumerable };
    let configurable = if d.has_configurable { d.configurable } else { current.attrs & attr_configurable() != 0 };
    if accessor || was_accessor && !data_descriptor(d) {
        let get = if d.has_get { d.get } else if was_accessor { fns.get } else { Value.Undefined };
        let set = if d.has_set { d.set } else if was_accessor { fns.set } else { Value.Undefined };
        let attrs = flag_attrs(false, enumerable, configurable);
        if was_accessor && attrs == current.attrs && same_bits(get, fns.get) && same_bits(set, fns.set) { return true; }
        let a = add_accessor(heap, Pair { get: get, set: set });
        if a < 0 { return false; }
        return define_property(heap, object, k, Value.Accessor { a: a as u32 }, attrs, half::Complete);
    }
    let value = if d.has_value { d.value } else if was_accessor { Value.Undefined } else { current.value };
    let w = if d.has_writable { d.writable } else if was_accessor { false } else { was_writable };
    let attrs = flag_attrs(w, enumerable, configurable);
    if !was_accessor && attrs == current.attrs && (!d.has_value || same_value(heap, value, current.value)) { return true; }
    return define_property(heap, object, k, value, attrs, half::Complete);
}
// An array's `length` (ArraySetLength, 10.4.2.4, with the validation of a
// non-configurable, non-enumerable data property): a new value must be
// ToUint32 of itself (a RangeError otherwise, before any validation); a
// non-writable length takes no other value and does not become writable.
fn define_length(job: Job&, object: Value, k: Value, current: Own, d: PropertyDescriptor) -> bool {
    let heap .= job.heap;
    var length: f64 = current.value.num();
    if d.has_value {
        let n = primitive_number(job, Builtin.ObjectDefineProperty, d.value);
        if heap.fault != Fault.None { return false; }
        let u = to_uint32(n) as f64;
        if u != n { job.detail.clear(); for c in "invalid array length" { job.detail.push(c); } heap.fault = Fault.RangeError; return false; }
        length = u;
    }
    let was_writable = current.attrs & attr_writable() != 0;
    if accessor_descriptor(d) || d.has_configurable && d.configurable || d.has_enumerable && d.enumerable { return false; }
    if !was_writable && (d.has_writable && d.writable || length != current.value.num()) { return false; }
    let w = if d.has_writable { d.writable } else { was_writable };
    if w == was_writable && length == current.value.num() { return true; }
    return define_property(heap, object, k, number(length), if w { attr_writable() } else { 0 }, half::Complete);
}

// ---- The Descriptor frame
// Start built-in b (descriptor_native) called with `args`, its operand
// base `base` (native_start).
fn start_descriptor(job: Job&, b: Builtin, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let target = arg(args, 0);
    if !target.is_object() { type_error(job, if b == Builtin.ReflectDefineProperty { "Reflect.defineProperty: the target is not an object" } else { "Object.defineProperty called on non-object" }); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    if b == Builtin.ObjectDefineProperties {
        let props = to_object(job, arg(args, 1));
        if heap.fault != Fault.None { return; }
        var keys: OwnKey[>..] = [];
        if !listable(job, props) || !own_property_keys(heap, props, keys) { return; }
        let list = key_list(heap, props, keys, true);
        if heap.fault != Fault.None { return; }
        job.operands.resize(base);
        job.frames.push(Frame { function: -1, pc: 1, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: Value.Undefined, input: target, output: list, native: Native.Descriptor { which: b } });
        visit_properties(job, job.frames.len - 1, b);
        return;
    }
    let kv = arg(args, 1); let attributes = arg(args, 2);
    // An object key's ToPropertyKey (ToPrimitive with hint string) runs in
    // a Convert frame above this one, which leaves the primitive for pc 0.
    let converts = kv.is_object() && convertible(heap, kv);
    let k = if converts { Value.Undefined } else { property_key(job, b, kv) };
    if heap.fault != Fault.None { return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: if converts { 0 } else { 2 }, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: attributes, input: target, output: k, native: Native.Descriptor { which: b } });
    let at = job.frames.len - 1;
    if converts { job.operands.push(kv); start_convert(job, Hint.String, false, 1, 1); return; }
    if begin_descriptor(job, at, b, attributes) { define_one(job, at, b); }
}
// ToPropertyDescriptor(v) for the frame at `at`: a TypeError unless v is an
// object; then its fields from the first (descriptor_fields). True when
// every field was read without a getter.
fn begin_descriptor(job: Job&, at: i64, b: Builtin, v: Value) -> bool {
    let heap .= job.heap;
    if !v.is_object() { type_error(job, "Property description must be an object"); return false; }
    let d = new_descriptor(heap);
    if heap.fault != Fault.None { return false; }
    job.frames[at].receiver = v; job.frames[at].callee = d; job.frames[at].index = 0; job.frames[at].pc = 2;
    return descriptor_fields(job, at, b, 2) && descriptor_valid(job, d, b);
}
// Object.defineProperty and Reflect.defineProperty, the descriptor read.
fn define_one(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap; let frame = job.frames[at];
    let d = descriptor_of(heap, frame.callee);
    if heap.fault != Fault.None { return; }
    let ok = define_descriptor(job, frame.input, frame.output, d);
    if heap.fault != Fault.None { return; }
    if b == Builtin.ReflectDefineProperty { finish_native(job, frame, boolean(ok)); return; }
    if !ok { named_error(job, Fault.TypeError, "cannot redefine property '", frame.output, "'"); return; }
    if d.has_value && !accessor_descriptor(d) { record(job, frame.input, frame.output, d.value); }
    finish_native(job, frame, frame.input);
}
// Object.defineProperties: from key `argument_count` on, each own key's
// enumerability ([[GetOwnProperty]]), then Get of it (a getter's above the
// frame, pc 1) and ToPropertyDescriptor (a field getter's above the frame,
// pc 2); with every descriptor read, the definitions.
fn visit_properties(job: Job&, at: i64, b: Builtin) {
    let heap .= job.heap;
    let list: i64 = job.frames[at].output.id();
    loop {
        let i = job.frames[at].argument_count;
        if i >= node(heap, list).key_count { define_all(job, at); return; }
        if !charge(heap) { return; }
        let props = node(heap, list).value; let k = property_row(heap, node(heap, list).keys + i).key;
        let own = enumerable_entry(heap, props, k);
        if heap.fault != Fault.None { return; }
        if !own.found || own.attrs & attr_enumerable() == 0 { property_mut(heap, node(heap, list).keys + i).value = Value.Null; job.frames[at].argument_count += 1; continue; }
        prepare_function(job, props, k, false);
        if heap.fault != Fault.None { return; }
        let v, getter = lookup(heap, props, k);
        if heap.fault != Fault.None { return; }
        if getter.is_object() { job.frames[at].pc = 1; run_getter(job, b, getter, props); return; }
        if !begin_descriptor(job, at, b, v) || !keep_descriptor(job, at) { return; }
    }
}
// The descriptor just read is key `argument_count`'s.
fn keep_descriptor(job: Job&, at: i64) -> bool {
    let heap .= job.heap; let list: i64 = job.frames[at].output.id(); let i = job.frames[at].argument_count;
    if i >= node(heap, list).key_count { heap.fault = Fault.InvalidState; return false; }
    property_mut(heap, node(heap, list).keys + i).value = job.frames[at].callee;
    job.frames[at].argument_count += 1; job.frames[at].callee = Value.Undefined; job.frames[at].receiver = Value.Undefined; job.frames[at].pc = 1;
    return true;
}
// Every descriptor read: DefinePropertyOrThrow of each in key order.
fn define_all(job: Job&, at: i64) {
    let heap .= job.heap; let frame = job.frames[at]; let list: i64 = frame.output.id();
    for i in node(heap, list).key_count {
        let r = property_row(heap, node(heap, list).keys + i);
        if !r.value.is_object() { continue; }
        let d = descriptor_of(heap, r.value);
        if heap.fault != Fault.None { return; }
        let ok = define_descriptor(job, frame.input, r.key, d);
        if heap.fault != Fault.None { return; }
        if !ok { named_error(job, Fault.TypeError, "cannot redefine property '", r.key, "'"); return; }
        if d.has_value && !accessor_descriptor(d) && r.key.is_string() { record(job, frame.input, r.key, d.value); }
    }
    finish_native(job, frame, frame.input);
}
// One step of the Descriptor frame at the top (see above).
fn descriptor_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let v = job.operands.pop();
    match frame.pc {
        0 => {
            let k = property_key(job, b, v);
            if heap.fault != Fault.None { return; }
            job.frames[at].output = k;
            if begin_descriptor(job, at, b, frame.receiver) { define_one(job, at, b); }
        },
        1 => { if begin_descriptor(job, at, b, v) && keep_descriptor(job, at) { visit_properties(job, at, b); } },
        2 => {
            if !descriptor_field(job, at, v, b) || !descriptor_fields(job, at, b, 2) || !descriptor_valid(job, job.frames[at].callee, b) { return; }
            if b == Builtin.ObjectDefineProperties { if keep_descriptor(job, at) { visit_properties(job, at, b); } } else { define_one(job, at, b); }
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}

// ---- Plain built-ins
// Reflect.getOwnPropertyDescriptor(target, key): Object's, for an object.
fn reflect_own_descriptor(job: Job&, b: Builtin, target: Value, k: Value) -> Value {
    if !target.is_object() { return type_error(job, "Reflect.getOwnPropertyDescriptor: the target is not an object"); }
    return own_descriptor(job, b, target, k);
}
// Reflect.deleteProperty(target, key): [[Delete]], false for a
// non-configurable property.
fn reflect_delete(job: Job&, b: Builtin, target: Value, kv: Value) -> Value {
    let heap .= job.heap;
    if !target.is_object() { return type_error(job, "Reflect.deleteProperty: the target is not an object"); }
    let k = property_key(job, b, kv);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !writable(job, target.id()) { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    // A lazy closure's `length` or `name` is made before it goes, as `delete` makes it.
    if k.is_string() && (key_is(heap, k, "length") || key_is(heap, k, "name")) { prepare_function(job, target, k, true); }
    if heap.fault != Fault.None { return Value.Undefined; }
    let ok = delete_property(heap, target, k);
    return if heap.fault == Fault.None { boolean(ok) } else { Value.Undefined };
}
// Object.prototype.__defineGetter__ and __defineSetter__ (B.2.2.2, B.2.2.3):
// an enumerable, configurable accessor half of ToObject(this).
fn define_accessor_half(job: Job&, b: Builtin, receiver: Value, kv: Value, f: Value) -> Value {
    let heap .= job.heap;
    let o = to_object(job, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !is_callable(heap, f) { return type_error(job, if b == Builtin.ObjectPrototypeDefineGetter { "invalid getter" } else { "invalid setter" }); }
    let k = property_key(job, b, kv);
    if heap.fault != Fault.None { return Value.Undefined; }
    var d = PropertyDescriptor { has_enumerable: true, enumerable: true, has_configurable: true, configurable: true };
    if b == Builtin.ObjectPrototypeDefineGetter { d.has_get = true; d.get = f; } else { d.has_set = true; d.set = f; }
    let ok = define_descriptor(job, o, k, d);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !ok { named_error(job, Fault.TypeError, "cannot redefine property '", k, "'"); }
    return Value.Undefined;
}
// Object.prototype.__lookupGetter__ and __lookupSetter__ (B.2.2.4,
// B.2.2.5): the first own property of the key on ToObject(this)'s chain,
// its getter or setter, or undefined for a data property or none.
fn lookup_accessor_half(job: Job&, b: Builtin, receiver: Value, kv: Value) -> Value {
    let heap .= job.heap;
    var at = to_object(job, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    let k = property_key(job, b, kv);
    if heap.fault != Fault.None { return Value.Undefined; }
    loop {
        if node(heap, at.id()).kind == node_kind::Proxy { proxy_fault(heap, proxy_method::GetOwnProperty); return Value.Undefined; }
        let own = enumerable_entry(heap, at, k);
        if heap.fault != Fault.None { return Value.Undefined; }
        if own.found {
            if is_pending(own.value) { pending_fault(heap, k); return Value.Undefined; }
            if !is_accessor(own.value) { return Value.Undefined; }
            let fns = pair(heap, own.value);
            return if b == Builtin.ObjectPrototypeLookupGetter { fns.get } else { fns.set };
        }
        let proto = prototype(heap, at.id());
        if !proto.is_object() || !charge(heap) { return Value.Undefined; }
        at = proto;
    }
}
