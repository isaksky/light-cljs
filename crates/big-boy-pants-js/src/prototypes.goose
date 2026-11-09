import std;
import .source_vm;
import .realm;
import .descriptors;
import .integrity;
import .proxy;

namespace source;

// Prototypes (light-cljs-883; decisions 2, 3, 5 and 6 of
// docs/javascript-coverage.md). Semantics from the specification
// (ECMAScript 10.1.2.1 OrdinarySetPrototypeOf, 10.4.7 immutable prototype
// exotic objects, 20.1.2.2 Object.create, 20.1.2.12 Object.getPrototypeOf,
// 20.1.2.23 Object.setPrototypeOf, 20.1.3.3 Object.prototype.isPrototypeOf,
// 20.1.3.10 Object.prototype.__proto__, 28.1.8 Reflect.getPrototypeOf,
// 28.1.13 Reflect.setPrototypeOf), the Pants engine's
// rust/src/vm/prototype.rs and QuickJS's js_object_create,
// JS_SetPrototypeInternal, js_object___getPrototypeOf and
// js_object___setPrototypeOf.
//
// The prototype stays where decision 2 put it: the `proto` field of the
// object row, Empty for the kind's realm prototype (realm.goose prototype).
// Shapes hold no prototype, so a change invalidates nothing; the dense-array
// paths that read an array's run directly require Empty (source_vm.goose
// dense), so an array given another prototype takes the general path,
// where a hole inherits. [[SetPrototypeOf]] is source_vm.goose's
// set_prototype, which SetProto (a literal's `__proto__: v`) also uses.
//
// The built-ins are plain (decision 5) except where JavaScript can run:
// Object.getPrototypeOf, Reflect.getPrototypeOf and the `__proto__` getter
// of a proxy run its [[GetPrototypeOf]] as light-cljs-845's Proxy frame
// (start_prototype), and Object.create with a properties argument runs
// Object.defineProperties's Descriptor frame (descriptors.goose) on the new
// object. Object.setPrototypeOf, Reflect.setPrototypeOf and the
// `__proto__` setter of a proxy run its [[SetPrototypeOf]], and
// isPrototypeOf runs a [[GetPrototypeOf]] met on its walk, as Proxy frames
// where a frame can start (light-cljs-891, proxy.goose proxy_conditional);
// run to completion here, a proxy fails by name. A realm row never changes its
// prototype (NativeBoundary, decision 3). Object.prototype's `__proto__` is an accessor pair of the
// realm (proto_pair, decision 6), its getter and setter two built-ins; an
// object without Object.prototype on its chain has a plain `__proto__`, and
// an own `__proto__` data property shadows the accessor. Forms (light-cljs
// runtime values) fail by name, as Object.getPrototypeOf always did: they
// have no [[Prototype]] here, while the V8 build's runtime makes them
// ordinary objects.

// [[GetPrototypeOf]] of ToObject(v) for built-in b: a primitive's wrapper
// prototype, which needs no wrapper; undefined and null are a TypeError
// (`message`).
fn prototype_of_value(job: Job&, b: Builtin, v: Value, message: const u8[:]) -> Value {
    let heap .= job.heap;
    if v.tag() <= value_tag::Null { return type_error(job, message); }
    // ToObject's wrapper inherits its realm prototype; none is made.
    if !v.is_object() { let p = primitive_prototype(v); return if p.is_object() { p } else { unsupported_step(job, b, "Symbol and BigInt prototypes") }; }
    return prototype_of_object(job, b, v);
}
// [[GetPrototypeOf]] of object o for built-in b, run to completion. A
// proxy's fails by name here (its callers start its Proxy frame first
// where they can), as does a form's.
fn prototype_of_object(job: Job&, b: Builtin, o: Value) -> Value {
    let heap .= job.heap;
    if !valid(heap, o) { return Value.Undefined; }
    let kind = node(heap, o.id()).kind;
    if kind == node_kind::Proxy { proxy_fault(heap, proxy_method::GetPrototypeOf); return Value.Undefined; }
    if !integrity_kind(kind) { return unsupported_step(job, b, "a runtime form"); }
    return prototype(heap, o.id());
}
// Whether v may become a prototype: an object or null.
inline fn prototype_value(v: Value) -> bool { return v.is_object() || v.is_null(); }
// [[SetPrototypeOf]] of object o to proto for built-in b: whether it was
// accepted (false with no fault: refused). A form fails by name.
fn set_prototype_of(job: Job&, b: Builtin, o: Value, proto: Value) -> bool {
    let heap .= job.heap;
    if !valid(heap, o) || !valid(heap, proto) { return false; }
    let kind = node(heap, o.id()).kind;
    if kind != node_kind::Proxy && !integrity_kind(kind) { unsupported_step(job, b, "a light-cljs runtime value"); return false; }
    return set_prototype(job, o, proto);
}

// ---- The plain built-ins (decision 5).
fn prototype_builtin(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    match b {
        // QuickJS's js_object_getPrototypeOf words this "not an object".
        ObjectGetPrototypeOf => { return prototype_of_value(job, b, arg(args, 0), "not an object"); },
        ReflectGetPrototypeOf => {
            let target = arg(args, 0);
            if !target.is_object() { return type_error(job, "Reflect.getPrototypeOf: the target is not an object"); }
            return prototype_of_object(job, b, target);
        },
        // RequireObjectCoercible(O), then the prototype's type; a primitive
        // O comes back unchanged, a refusal is a TypeError.
        ObjectSetPrototypeOf => {
            let o = arg(args, 0); let proto = arg(args, 1);
            if o.tag() <= value_tag::Null { return type_error(job, "Object.setPrototypeOf called on null or undefined"); }
            if !prototype_value(proto) { return type_error(job, "Object prototype may only be an Object or null"); }
            if !o.is_object() { return o; }
            if set_prototype_of(job, b, o, proto) { return o; }
            if heap.fault == Fault.None { prototype_type_error(job, o.id(), proto); }
            return Value.Undefined;
        },
        ReflectSetPrototypeOf => {
            let target = arg(args, 0); let proto = arg(args, 1);
            if !target.is_object() { return type_error(job, "Reflect.setPrototypeOf: the target is not an object"); }
            if !prototype_value(proto) { return type_error(job, "Object prototype may only be an Object or null"); }
            let ok = set_prototype_of(job, b, target, proto);
            return if heap.fault == Fault.None { boolean(ok) } else { Value.Undefined };
        },
        // A value that is not an object has no prototype to look for, even
        // with an undefined this (step 1 before ToObject). A primitive this
        // becomes a new wrapper, on no chain, but the walk still runs: a
        // proxy on it fails by name here (where a frame can start,
        // source_vm.goose start_is_prototype_of runs its trap), as does a
        // form.
        ObjectPrototypeIsPrototypeOf => {
            var v = arg(args, 0);
            if !v.is_object() { return boolean(false); }
            if receiver.tag() <= value_tag::Null { return type_error(job, "cannot convert to object"); }
            if !valid(heap, v) || !valid(heap, receiver) { return Value.Undefined; }
            let target: i64 = if receiver.is_object() { receiver.id() } else { -1 };
            loop {
                v = prototype_of_object(job, b, v);
                if heap.fault != Fault.None { return Value.Undefined; }
                if !v.is_object() { return boolean(false); }
                if v.id() == target { return boolean(true); }
                if !charge(heap) { return Value.Undefined; }
            }
            return Value.Undefined;
        },
        // get __proto__: [[GetPrototypeOf]] of ToObject(this).
        ObjectPrototypeProtoGetter => { return prototype_of_value(job, b, receiver, "cannot convert to object"); },
        // set __proto__: RequireObjectCoercible(this); a prototype that is
        // neither an object nor null, or a primitive this, changes nothing;
        // a refusal is a TypeError, in sloppy code too.
        ObjectPrototypeProtoSetter => {
            let proto = arg(args, 0);
            if receiver.tag() <= value_tag::Null { return type_error(job, "not an object"); }
            if !prototype_value(proto) || !receiver.is_object() { return Value.Undefined; }
            if !set_prototype_of(job, b, receiver, proto) && heap.fault == Fault.None { prototype_type_error(job, receiver.id(), proto); }
            return Value.Undefined;
        },
        _ => { heap.fault = Fault.InvalidState; return Value.Undefined; },
    }
}

// The built-ins that run as a native frame (native_start): Object.create,
// and the readers of one [[GetPrototypeOf]] (Object.getPrototypeOf,
// Reflect.getPrototypeOf, the `__proto__` getter), whose proxy runs
// light-cljs-845's Proxy frame; its result is the call's.
inline fn prototype_native(b: Builtin) -> bool { return b == Builtin.ObjectCreate || b == Builtin.ObjectGetPrototypeOf || b == Builtin.ReflectGetPrototypeOf || b == Builtin.ObjectPrototypeProtoGetter; }
fn start_prototype(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    if b == Builtin.ObjectCreate { start_create(job, args, base); return; }
    let v = if b == Builtin.ObjectPrototypeProtoGetter { receiver } else { arg(args, 0) };
    // The Proxy frame starts at pc 0 (its trap read) on its first step, so
    // a native built-in as the trap starts from the dispatch loop, not
    // from inside this one.
    if is_proxy(heap, v) {
        if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
        job.operands.resize(base);
        job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: Value.Undefined, input: v, output: Value.Undefined, native: Native.Proxy { method: proxy_method::GetPrototypeOf } });
        return;
    }
    let r = prototype_builtin(job, b, receiver, args);
    if heap.fault == Fault.None { job.operands.resize(base); job.operands.push(r); }
}
// Object.create(proto, properties) as a native frame's start (native_start):
// a new ordinary object with prototype proto, the job's row, and with a
// properties argument other than undefined, Object.defineProperties(O,
// properties) as its Descriptor frame (descriptors.goose), which delivers O.
// QuickJS-only: the Pants engine refuses a properties argument.
fn start_create(job: Job&, args: const Value[:], base: i64) {
    let heap .= job.heap;
    let proto = arg(args, 0); let properties = arg(args, 1);
    if !prototype_value(proto) { type_error(job, "Object prototype may only be an Object or null"); return; }
    if !valid(heap, proto) || !valid(heap, properties) { return; }
    // Object.prototype is an ordinary object's Empty (prototype).
    let default = heap.global.is_object() && proto.is_object() && proto.id() == object_prototype_row();
    let o = allocate(heap, Node { kind: node_kind::Object, proto: if default { Value.Empty } else { proto } });
    if heap.fault != Fault.None { return; }
    if properties.is_undefined() { job.operands.resize(base); job.operands.push(o); return; }
    let forwarded: Value[2] = [o, properties];
    start_descriptor(job, Builtin.ObjectDefineProperties, forwarded, base);
}
