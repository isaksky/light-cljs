import std;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .cells;

namespace source;

// Proxy objects and their internal methods (light-cljs-845, -890 and -891;
// decisions 1, 2, 4 and 5 of docs/javascript-coverage.md). Semantics from
// the Pants engine's rust/src/vm/proxy.rs and vm/keys.rs, QuickJS's
// js_proxy_* functions, and the specification (ECMAScript 10.5 Proxy
// exotic objects, 28.2.1.1 Proxy(target, handler), 28.2.2.1
// Proxy.revocable, 7.3.18 CreateListFromArrayLike, 6.2.6.4
// FromPropertyDescriptor, 6.2.6.5 ToPropertyDescriptor, 6.2.6.6
// CompletePropertyDescriptor, 10.1.6.2 IsCompatiblePropertyDescriptor,
// 10.1.6.3 ValidateAndApplyPropertyDescriptor).
//
// A proxy is a node of kind node_kind::Proxy whose internal slots hold its
// target (`value`) and handler (`namespace_value`), both objects, the
// target always an earlier row (decision 2); it has no own properties and
// no prototype of its own (`proto` Null). `new Proxy(target, handler)`
// makes one, a job row; Proxy called without `new`, or with a target or
// handler that is no object, is a TypeError. It is callable when its final
// target is, and a constructor when that is one (`typeof` "function",
// Object.prototype.toString's "Function"; Array.isArray and its "Array"
// see through it). Proxy.revocable makes one with a revoke function (a
// job row of Builtin.ProxyRevoke whose `value` is the proxy, null once it
// revoked it); revoking sets the proxy's revoked flag (its node's
// `initialized`; decision 2: proxy node state, which snapshots, the reset,
// reclamation and saved continuations carry as any field, and snapshot
// validation accepts), after which every internal method, Array.isArray
// and toString's tag are a TypeError; target and handler stay, so `typeof`
// does not change and an operation already past its handler read keeps it.
//
// Every internal method runs as a Proxy native frame (Native.Proxy, its
// proxy_method the payload): the trap read (a handler getter runs above
// the frame), the trap call, every observable read of a trap's result (the
// result's `length` and elements, a descriptor's fields), a forwarded
// getter or setter, and a proxy target's [[GetOwnProperty]],
// [[IsExtensible]] or [[GetPrototypeOf]] for a trap's invariants run as
// frames above it (decision 4), and its result arrives at its operand base
// for the frame below. A proxy without the trap forwards to its target: a
// proxy target restarts the frame on it (pc 0, one step a hop), as does a
// proxy that [[Get]], [[Set]] or [[HasProperty]] meets on the target's
// prototype chain; any other target answers through its own internal
// method, a getter or setter running above the frame; a forwarded [[Call]]
// or [[Construct]] becomes a Forward frame (function_calls.goose) that
// runs the target as any forwarded call. A proxy target's
// [[OwnPropertyKeys]] and each of its keys' [[GetOwnProperty]] for the
// ownKeys trap's invariants run as frames above it too (light-cljs-897).
//
//   input     the proxy whose method runs (its target after a forward)
//   output    the property key ([[GetOwnProperty]], the four property
//             methods, [[DefineOwnProperty]]); [[Call]]'s and
//             [[Construct]]'s argument array (CreateArrayFromList)
//   receiver  [[Get]]'s and [[Set]]'s receiver; [[Call]]'s this value,
//             [[Construct]]'s newTarget; for the two enumeration methods
//             the trap's result object while its fields are read;
//             [[GetPrototypeOf]]'s trap result and [[DefineOwnProperty]]'s
//             target descriptor while a proxy target answers; the object
//             answer::Object gives back
//   callee    [[Set]]'s value, [[SetPrototypeOf]]'s new prototype,
//             [[DefineOwnProperty]]'s descriptor (partial); for the
//             enumeration methods the trap while it is called, then the key
//             list or descriptor the frame builds; [[Get]]'s trap result
//             while the target's descriptor is read; [[IsExtensible]]'s and
//             [[PreventExtensions]]'s trap result while a proxy target
//             answers
//   index, argument_count  CreateListFromArrayLike's index and length;
//             ToPropertyDescriptor's next field and the target's
//             extensibility; for the methods with a boolean answer, index
//             the answer mode (answer::); for the four property methods,
//             argument_count the hops a prototype chain led back to a
//             proxy (proxy_restart)
//   pc 0  restart on `input` (after a forward)
//   pc 1  awaits the trap from a getter on the handler
//   pc 2  awaits the trap's result
//   pc 3  ([[OwnPropertyKeys]]) awaits the result's `length` value, from a
//         getter or converted by a Convert frame
//   pc 4  ([[OwnPropertyKeys]]) awaits element `index` from its getter
//   pc 5  ([[GetOwnProperty]]) awaits descriptor field `index` from its
//         getter
//   pc 6  ([[Get]], [[Set]], [[HasProperty]], [[Delete]],
//         [[DefineOwnProperty]], [[GetOwnProperty]]) awaits a proxy
//         target's descriptor for the trap result's invariants
//         ([[GetOwnProperty]]: the trap's result kept as the receiver,
//         light-cljs-895)
//   pc 7  ([[Get]], [[Set]]) awaits a forwarded getter's or setter's result
//   pc 8  ([[GetOwnProperty]]) awaits a proxy trap result's answer whether
//         it has descriptor field `index` (its [[HasProperty]])
//   pc 9  ([[Set]]) awaits both conversions of an array receiver's new
//         `length` (proxy_length); ([[DefineOwnProperty]]) of an array
//         target's new `length` given as an object, the target the one
//         its forward resolved (light-cljs-898; descriptors.goose
//         length_definition)
//   pc 10 ([[Set]]) awaits its proxy receiver's [[GetOwnProperty]]
//   pc 11 ([[Set]]) awaits its proxy receiver's [[DefineOwnProperty]]
//         (proxy_receiver_define)
//   pc 12 awaits a proxy target's [[IsExtensible]] for the invariants
//         ([[GetOwnProperty]]: the trap's result the receiver and, an
//         object, the descriptor to build the callee, light-cljs-895)
//   pc 13 ([[GetPrototypeOf]], [[SetPrototypeOf]]) awaits a non-extensible
//         proxy target's [[GetPrototypeOf]]
//   pc 14 ([[OwnPropertyKeys]]) awaits a proxy target's [[OwnPropertyKeys]]
//         for the invariants, the target's extensibility (0 or 1) in
//         argument_count (light-cljs-897)
//   pc 15 ([[OwnPropertyKeys]]) awaits that target's [[GetOwnProperty]] of
//         its key `index`; its key list is the frame's receiver, each
//         read key's row marked configurable or not in its attributes
//
// [[OwnPropertyKeys]] gives a key list (node_kind::KeyList, source_vm.goose's
// "Key enumeration") of its keys, symbols included, attributes clear.
// [[GetOwnProperty]] gives undefined or a *descriptor*: a key list whose
// records are the descriptor's fields, keyed by their names (`value`,
// `writable`, `get`, `set`, `enumerable`, `configurable`), complete once
// delivered; while the frame builds it, its namespace_value is the target's
// own descriptor (or Null), read before the trap result's fields as the
// specification orders it. [[DefineOwnProperty]] takes such a descriptor,
// partial, and gives its trap an ordinary object of its fields
// (from_descriptor). [[GetPrototypeOf]] gives an object or null, [[Get]],
// [[Call]] and [[Construct]] a value, the others a boolean that the frame's
// answer mode may turn into a TypeError, its object or undefined (the
// built-ins' results). Key lists, descriptors and argument arrays are job
// rows only frames hold, truncated with the job (decision 3).
//
// How an operation reaches a proxy (source_vm.goose): own_entry of a proxy
// faults UnsupportedOperand naming [[GetOwnProperty]], on the cold fault
// path only, so ordinary objects pay nothing new. lookup turns that into a
// *proxy hit*: the proxy comes back as the "getter", Heap.proxy_at holding
// it and Heap.proxy_key the key, and the getter paths (get_property, and
// call_accessor, which run_getter takes) start the [[Get]] frame with the
// getter's receiver, so every reader that runs getters runs [[Get]] traps
// too. has_property, assign (with inherited_assign and receiver_set) and
// delete_property keep the fault, relabelled [[HasProperty]], [[Set]] or
// [[Delete]] with the proxy and key recorded (proxy_relabel); receiver_set
// records a proxy receiver's [[DefineOwnProperty]] the same way; a caller
// that supports the method takes it (proxy_pending, take_proxy) and starts
// the frame. Object spread and object rest (CopyDataProperties; OwnKeys and
// IsEnumerable) run a proxy source's [[OwnPropertyKeys]], [[GetOwnProperty]]
// and [[Get]] above a CopyData frame, and Object.getOwnPropertyDescriptor
// and Reflect.getOwnPropertyDescriptor, conditional natives, its
// [[GetOwnProperty]] above a Descriptor frame (light-cljs-895). The two
// fields live only from the operation to its caller within one step;
// fail() and the job reset clear them, and in-job
// reclamation keeps and moves what they hold (reclaim.goose). A call or
// `new` reaches [[Call]] or [[Construct]] where it would push a closure's
// frame (push_frame, construct, call_accessor; proxy_call), so ordinary
// calls test nothing new. Object.isExtensible, preventExtensions,
// setPrototypeOf and their Reflect counterparts, the `__proto__` setter and
// isPrototypeOf are conditional natives (proxy_conditional), instanceof's
// walk and isPrototypeOf's run a proxy's [[GetPrototypeOf]] above a
// HasInstance frame (source_vm.goose prototype_walk), and the Descriptor
// frame (descriptors.goose) runs a proxy target's [[DefineOwnProperty]].
// Object.seal, freeze, isSealed and isFrozen of a proxy run its
// [[PreventExtensions]] or [[IsExtensible]], [[OwnPropertyKeys]],
// [[GetOwnProperty]] and [[DefineOwnProperty]] above a Statics frame
// (light-cljs-897, integrity.goose start_integrity).
//
// Still a named UnsupportedOperand (decision 1), each where no frame can
// start or for another pellet: Annex B's __defineGetter__ family on a
// proxy; Function.prototype.bind of a proxy (light-cljs-861); the plain
// built-ins run to completion (call_builtin) with a proxy, as a callback that cannot
// start a frame calls them; a revoke function called that way; a
// host-effect built-in or a delay as a trap.

// A key list's `function`: Pending while the keys of the proxy it walks
// (its `value`) are not listed yet (for-in, source_vm.goose).
let key_list::Listed = 0;
let key_list::Pending = 1;
// A proxy's internal methods, a Proxy frame's payload; Heap.proxy_method
// names the one a caller is asked to run (proxy_pending).
let proxy_method::GetPrototypeOf = 0;
let proxy_method::OwnPropertyKeys = 1;
let proxy_method::GetOwnProperty = 2;
let proxy_method::Get = 3;
let proxy_method::Set = 4;
let proxy_method::HasProperty = 5;
let proxy_method::Delete = 6;
let proxy_method::DefineOwnProperty = 7;
let proxy_method::Call = 8;
let proxy_method::Construct = 9;
let proxy_method::IsExtensible = 10;
let proxy_method::PreventExtensions = 11;
let proxy_method::SetPrototypeOf = 12;
let PROXY_METHODS: Name[13] = [
    Name { text: "[[GetPrototypeOf]]" }, Name { text: "[[OwnPropertyKeys]]" }, Name { text: "[[GetOwnProperty]]" }, Name { text: "[[Get]]" }, Name { text: "[[Set]]" },
    Name { text: "[[HasProperty]]" }, Name { text: "[[Delete]]" }, Name { text: "[[DefineOwnProperty]]" }, Name { text: "[[Call]]" }, Name { text: "[[Construct]]" },
    Name { text: "[[IsExtensible]]" }, Name { text: "[[PreventExtensions]]" }, Name { text: "[[SetPrototypeOf]]" },
];
// The trap of each method.
let TRAP_NAMES: Name[13] = [
    Name { text: "getPrototypeOf" }, Name { text: "ownKeys" }, Name { text: "getOwnPropertyDescriptor" }, Name { text: "get" }, Name { text: "set" }, Name { text: "has" },
    Name { text: "deleteProperty" }, Name { text: "defineProperty" }, Name { text: "apply" }, Name { text: "construct" }, Name { text: "isExtensible" },
    Name { text: "preventExtensions" }, Name { text: "setPrototypeOf" },
];
// A Proxy frame's phases beyond the enumeration methods' (see above).
let proxy_pc::Descriptor = 6;
let proxy_pc::Accessor = 7;
let proxy_pc::Length = 9;
let proxy_pc::ReceiverOwn = 10;
let proxy_pc::ReceiverDefine = 11;
let proxy_pc::TargetExtensible = 12;
let proxy_pc::TargetPrototype = 13;
let proxy_pc::TargetKeys = 14;
let proxy_pc::TargetKey = 15;
// What a frame of a method with a boolean answer delivers (its `index`):
// the boolean; the boolean, a false answer a TypeError (strict code, a
// built-in's Set(O, P, V, true)); the frame's `receiver` for true
// (Object.preventExtensions and setPrototypeOf give their object), false a
// TypeError; or undefined for true (the `__proto__` setter), false a
// TypeError.
let answer::Boolean = 0;
let answer::Throws = 1;
let answer::Object = 2;
let answer::Nothing = 3;
// A descriptor's fields, in ToPropertyDescriptor's reading order.
let field::Enumerable = 0;
let field::Configurable = 1;
let field::Value = 2;
let field::Writable = 3;
let field::Get = 4;
let field::Set = 5;
let FIELD_NAMES: Name[6] = [Name { text: "enumerable" }, Name { text: "configurable" }, Name { text: "value" }, Name { text: "writable" }, Name { text: "get" }, Name { text: "set" }];

inline fn is_proxy(heap: const Heap&, v: Value) -> bool { return v.is_object() && node(heap, v.id()).kind == node_kind::Proxy; }
// IsCallable of proxy v: its final target's (a target is an earlier row,
// so the walk ends).
fn proxy_callable(heap: const Heap&, v: Value) -> bool {
    var t = v;
    while is_proxy(heap, t) { t = node(heap, t.id()).value; }
    return t.is_object() && (node(heap, t.id()).kind == node_kind::Closure || node(heap, t.id()).kind == node_kind::Builtin || node(heap, t.id()).kind == node_kind::BoundFunction);
}
// An internal method of a proxy this VM does not implement: UnsupportedOperand,
// naming it for the report (Heap.proxy_method, fail).
fn proxy_fault(heap: Heap&, method: i64) { if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; heap.proxy_method = method; } }
// An operation that reached [[GetOwnProperty]] of proxy `at` (own_entry's
// fault) for key k is `method` ([[HasProperty]] of `in`, [[Set]] of an
// assignment ...): the fault names it, and the proxy and key are recorded
// for a caller that runs it (proxy_pending).
fn proxy_relabel(heap: Heap&, at: Value, method: i64, k: Value) {
    if heap.fault == Fault.UnsupportedOperand && heap.proxy_method == proxy_method::GetOwnProperty && is_proxy(heap, at) { heap.proxy_method = method; heap.proxy_at = at; heap.proxy_key = k; }
}
// Internal method `method` of proxy p for key k, reached directly (assign's
// or delete_property's proxy arm): its fault, recorded as proxy_relabel's.
fn proxy_reach(heap: Heap&, p: Value, method: i64, k: Value) { if heap.fault == Fault.None { proxy_fault(heap, method); heap.proxy_at = p; heap.proxy_key = k; } }
// lookup reached proxy `at` for key k (own_entry's fault): a proxy hit, the
// fault cleared and the proxy recorded, which lookup gives as the getter.
// False for any other fault.
fn proxy_hit(heap: Heap&, at: Value, k: Value) -> bool {
    if heap.fault != Fault.UnsupportedOperand || heap.proxy_method != proxy_method::GetOwnProperty || !is_proxy(heap, at) { return false; }
    heap.fault = Fault.None; heap.proxy_method = -1; heap.proxy_at = at; heap.proxy_key = k;
    return true;
}
// Whether getter g that lookup gave is its proxy hit.
inline fn is_hit(heap: const Heap&, g: Value) -> bool { return g.is_object() && heap.proxy_at.is_object() && g.id() == heap.proxy_at.id() && node(heap, g.id()).kind == node_kind::Proxy; }
// Whether the fault is internal method `method` of a recorded proxy, which
// the caller can run (take_proxy).
fn proxy_pending(heap: const Heap&, method: i64) -> bool { return heap.fault == Fault.UnsupportedOperand && heap.proxy_method == method && heap.proxy_at.is_object() && is_proxy(heap, heap.proxy_at); }
// Take the recorded proxy and key, clearing its fault (if any) and record.
fn take_proxy(heap: Heap&) -> Value, Value {
    let p = heap.proxy_at; let k = heap.proxy_key;
    if heap.fault == Fault.UnsupportedOperand && heap.proxy_method >= 0 { heap.fault = Fault.None; heap.proxy_method = -1; }
    heap.proxy_at = Value.Undefined; heap.proxy_key = Value.Undefined;
    return p, k;
}
fn proxy_detail(job: Job&) {
    job.detail.clear();
    for c in "Proxy: " { job.detail.push(c); }
    for c in PROXY_METHODS[job.heap.proxy_method].text { job.detail.push(c); }
}
// IsExtensible(target) for a trap's invariants (light-cljs-882,
// integrity.goose) of a target that is no proxy (target_extensible runs a
// proxy target's); a proxy's fails by name here, as does a form's.
fn extensible(heap: Heap&, target: Value) -> bool { return target.is_object() && is_extensible(heap, target); }

// ---- The Proxy constructor
// new Proxy(target, handler) (ProxyCreate): new.target is Proxy itself.
fn construct_proxy(job: Job&, args: const Value[:]) -> Value {
    let target = arg(args, 0); let handler = arg(args, 1);
    if !target.is_object() || !handler.is_object() { return type_error(job, "Proxy: target and handler must be objects"); }
    return allocate(job.heap, Node { kind: node_kind::Proxy, value: target, namespace_value: handler, proto: Value.Null });
}

// ---- Descriptors
// A new, empty descriptor with room for every field.
fn new_descriptor(heap: Heap&) -> Value {
    let at = add_properties(heap, FIELD_NAMES.len);
    if at < 0 { return Value.Undefined; }
    return allocate(heap, Node { kind: node_kind::KeyList, keys: at as u32, key_capacity: FIELD_NAMES.len as u32 });
}
// Field f of descriptor d, and whether d has it.
fn field_of(heap: Heap&, d: Value, f: i64) -> Value, bool {
    let k = key_named(heap, FIELD_NAMES[f].text);
    if heap.fault != Fault.None { return Value.Undefined, false; }
    let n .= node(heap, d.id());
    for i in n.key_count { let r .= property_row(heap, n.keys + i); if key_word(r.key) == key_word(k) { return r.value, true; } }
    return Value.Undefined, false;
}
fn has_field(heap: Heap&, d: Value, f: i64) -> bool { let v, found = field_of(heap, d, f); return found; }
fn flag_field(heap: Heap&, d: Value, f: i64) -> bool { let v, found = field_of(heap, d, f); return found && v.flag(); }
// Set field f of descriptor d to v.
fn set_field(heap: Heap&, d: Value, f: i64, v: Value) -> bool {
    let k = key_named(heap, FIELD_NAMES[f].text);
    if heap.fault != Fault.None { return false; }
    let o: i64 = d.id(); let keys: i64 = node(heap, o).keys; let count: i64 = node(heap, o).key_count;
    for i in count { if key_word(property_row(heap, keys + i).key) == key_word(k) { property_mut(heap, keys + i).value = v; return true; } }
    if count >= node(heap, o).key_capacity { heap.fault = Fault.InvalidState; return false; }
    set_property_row(heap, keys + count, Property { key: k, value: v });
    node_mut(heap, o).key_count = (count + 1) as u32;
    return true;
}
// A non-proxy target's own property k as a complete descriptor
// (OrdinaryGetOwnProperty), or undefined. An ordinary function's lazy
// `prototype` is made first, as Object.getOwnPropertyDescriptor makes it.
fn target_descriptor(job: Job&, target: Value, k: Value) -> Value {
    let heap .= job.heap;
    if lazy_prototype(heap, target.id()) && key_is(heap, k, "prototype") { prepare_function(job, target, k, false); }
    if heap.fault != Fault.None { return Value.Undefined; }
    let e = own_entry(heap, target, k);
    if heap.fault != Fault.None || !e.found { return Value.Undefined; }
    if is_pending(e.value) { pending_fault(heap, k); return Value.Undefined; }
    let d = new_descriptor(heap);
    if heap.fault != Fault.None { return Value.Undefined; }
    if is_accessor(e.value) {
        let fns = pair(heap, e.value);
        if !set_field(heap, d, field::Get, fns.get) || !set_field(heap, d, field::Set, fns.set) { return Value.Undefined; }
    } else if !set_field(heap, d, field::Value, e.value) || !set_field(heap, d, field::Writable, boolean(e.attrs & attr_writable() != 0)) { return Value.Undefined; }
    if !set_field(heap, d, field::Enumerable, boolean(e.attrs & attr_enumerable() != 0)) || !set_field(heap, d, field::Configurable, boolean(e.attrs & attr_configurable() != 0)) { return Value.Undefined; }
    return d;
}
// IsCompatiblePropertyDescriptor(extensible, d, current): d complete or
// partial ([[DefineOwnProperty]]'s); `current` a complete descriptor, or
// Null (or undefined) when the target lacks the property.
fn compatible(heap: Heap&, extensible: bool, d: Value, current: Value) -> bool {
    if !current.is_object() { return extensible; }
    if flag_field(heap, current, field::Configurable) { return true; }
    if flag_field(heap, d, field::Configurable) { return false; }
    if has_field(heap, d, field::Enumerable) && flag_field(heap, d, field::Enumerable) != flag_field(heap, current, field::Enumerable) { return false; }
    let accessor = has_field(heap, current, field::Get);
    let d_accessor = has_field(heap, d, field::Get) || has_field(heap, d, field::Set);
    let d_data = has_field(heap, d, field::Value) || has_field(heap, d, field::Writable);
    if (d_accessor || d_data) && d_accessor != accessor { return false; }
    if accessor {
        let a, has_get = field_of(heap, d, field::Get); let b, y = field_of(heap, current, field::Get);
        let c, has_set = field_of(heap, d, field::Set); let e, w = field_of(heap, current, field::Set);
        return (!has_get || same_value(heap, a, b)) && (!has_set || same_value(heap, c, e));
    }
    if flag_field(heap, current, field::Writable) { return true; }
    if flag_field(heap, d, field::Writable) { return false; }
    let v, has_value = field_of(heap, d, field::Value); let u, y = field_of(heap, current, field::Value);
    return !has_value || same_value(heap, v, u);
}

// ---- The Proxy frame
// Start internal method `method` of proxy p (key k for all but the
// enumeration methods; [[Get]]'s and [[Set]]'s receiver, [[Set]]'s value,
// and for [[Set]] and [[Delete]] whether a false result throws) in a Proxy
// frame above the top one; its result arrives on the operands. The frame
// reads its trap at its first step (pc 0), so a getter path that starts it
// (call_accessor) does not re-enter one.
fn proxy_start(job: Job&, method: i64, p: Value, k: Value, receiver: Value = Value.Undefined, value: Value = Value.Undefined, throws: bool = false) {
    proxy_push(job, method, p, k, receiver, value, if throws { answer::Throws } else { answer::Boolean }, job.operands.len);
}
// proxy_start with the frame's answer mode (answer::) and operand base.
fn proxy_push(job: Job&, method: i64, p: Value, k: Value, receiver: Value, value: Value, mode: i64, base: i64) -> bool {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return false; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: value, receiver: receiver, input: p, output: k, index: mode, native: Native.Proxy { method: method as u8 } });
    return true;
}
// [[Call]] (10.5.12) of callable proxy p with this `receiver`, or with
// `constructs` [[Construct]] (10.5.13) with newTarget `receiver`, and
// `args`: a Proxy frame whose result arrives at operand base `base`. The
// arguments become an array first (CreateArrayFromList, which nothing
// observes), the frame's `output`. A proxy of a function that is no
// constructor is not one either: `new` of it is a TypeError. Returns
// false on a fault.
fn proxy_call(job: Job&, p: Value, receiver: Value, args: const Value[:], base: i64, constructs: bool) -> bool {
    let heap .= job.heap;
    if !is_proxy(heap, p) { heap.fault = Fault.InvalidState; return false; }
    if constructs && !is_constructor(heap, p) { type_error(job, "not a constructor"); return false; }
    if !constructs && !proxy_callable(heap, p) { type_error(job, "not a function"); return false; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return false; }
    if !charge(heap, args.len) { return false; }
    let list = allocate(heap, Node { kind: node_kind::Array });
    if heap.fault != Fault.None { return false; }
    for v, i in args { if !set_element(heap, list, i, v) { return false; } }
    return proxy_push(job, if constructs { proxy_method::Construct } else { proxy_method::Call }, p, list, receiver, Value.Undefined, answer::Boolean, base);
}
// [[Get]] of the proxy hit p (lookup's) for `receiver`: its recorded key.
fn proxy_get(job: Job&, p: Value, receiver: Value) {
    let x, k = take_proxy(job.heap);
    if !is_proxy(job.heap, p) || !(k.is_string() || k.is_symbol()) { job.heap.fault = Fault.InvalidState; return; }
    proxy_start(job, proxy_method::Get, p, k, receiver);
}
// [[Set]], [[HasProperty]] or [[Delete]] of the recorded proxy
// (proxy_pending): its frame, the fault cleared.
fn proxy_take(job: Job&, method: i64, receiver: Value = Value.Undefined, value: Value = Value.Undefined, throws: bool = false) {
    let p, k = take_proxy(job.heap);
    proxy_start(job, method, p, k, receiver, value, throws);
}
// An assignment's [[Set]] of the recorded proxy with `receiver`: a Setter
// frame (source_vm.goose call_setter's) that drops the boolean and
// delivers the assignment's `result`, the Proxy frame above it, a false
// answer a TypeError in strict code.
fn proxy_assign(job: Job&, receiver: Value, value: Value, result: Value, strict: bool) {
    let p, k = take_proxy(job.heap);
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: setter_pc::Setter, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: receiver, output: result, native: Native.Setter });
    proxy_start(job, proxy_method::Set, p, k, receiver, value, strict);
}
// A super assignment whose receiver_set reached its proxy receiver
// (proxy_pending DefineOwnProperty, light-cljs-891): as proxy_assign, a
// Setter frame delivering `value` under a [[Set]] frame at its receiver
// phases (proxy_set_receiver).
fn proxy_assign_receiver(job: Job&, k: Value, value: Value, strict: bool) {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: setter_pc::Setter, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: job.heap.proxy_at, output: value, native: Native.Setter });
    proxy_set_receiver(job, k, value, strict);
}
// GetMethod(handler, trap name) of the frame's proxy.
fn proxy_trap(job: Job&, at: i64, method: i64) {
    let heap .= job.heap;
    let handler = node(heap, job.frames[at].input.id()).namespace_value;
    // Every internal method of a revoked proxy (Proxy.revocable) is a
    // TypeError.
    if revoked(heap, job.frames[at].input) { type_error(job, "revoked proxy"); return; }
    let k = key_named(heap, TRAP_NAMES[method].text);
    if heap.fault != Fault.None { return; }
    let v, getter = lookup(heap, handler, k);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { job.frames[at].pc = 1; run_getter(job, Builtin.Proxy, getter, handler); return; }
    proxy_with_trap(job, at, method, v);
}
// With the trap: undefined or null forwards to the target; anything else
// must be callable, and is called with the handler as `this`.
fn proxy_with_trap(job: Job&, at: i64, method: i64, trap: Value) {
    let heap .= job.heap; let p = job.frames[at].input;
    if trap.tag() <= value_tag::Null { proxy_forward(job, at, method); return; }
    if !is_callable(heap, trap) { type_error(job, "Proxy: trap is not a function"); return; }
    if method <= proxy_method::GetOwnProperty { job.frames[at].callee = trap; }
    var f = trap; var this_value = node(heap, p.id()).namespace_value;
    // The arguments all[first..count]: the target, then the key (or for
    // [[Call]] the this value, for [[Construct]] the argument array), then
    // [[Set]]'s value, [[DefineOwnProperty]]'s descriptor object, [[Call]]'s
    // argument array or [[Construct]]'s newTarget, then [[Set]]'s receiver;
    // [[Get]]'s receiver third.
    let frame = job.frames[at];
    var third = frame.receiver;
    if method == proxy_method::Set { third = frame.callee; }
    else if method == proxy_method::DefineOwnProperty { third = from_descriptor(job, frame.callee); if heap.fault != Fault.None { return; } }
    else if method == proxy_method::Call { third = frame.output; }
    let second = if method == proxy_method::Call { frame.receiver } else if method == proxy_method::SetPrototypeOf { frame.callee } else { frame.output };
    job.frames[at].pc = 2;
    let all:   Value[4] = [node(heap, p.id()).value, second, third, frame.receiver];
    let count: i64      = trap_arity(method);
    var first: i64 = 0;
    // Call(trap, handler, args). Function.prototype.call as the trap calls
    // its receiver with its first argument as `this`, as the dispatch loop's
    // call does; a native-frame built-in runs in its own frame above this
    // one, a plain one completes here; a proxy runs its [[Call]].
    loop {
        if is_proxy(heap, f) { proxy_call(job, f, this_value, all[first..count], job.operands.len, false); return; }
        if !callable(heap, f) { type_error(job, "Proxy: trap is not a function"); return; }
        if node(heap, f.id()).kind == node_kind::Closure { push_frame(job, f, this_value, all[first..count], false, job.operands.len); return; }
        if is_bound(heap, f) { start_bound(job, f, this_value, all[first..count], job.operands.len); return; }
        if builtin_of(heap, f.id()) != Builtin.FunctionPrototypeCall { break; }
        if !charge(heap) { return; }
        let next_this = if first < count { all[first] } else { Value.Undefined };
        if first < count { first += 1; }
        f = this_value; this_value = next_this;
    }
    let c = builtin_of(heap, f.id());
    if effect_builtin(c) || c == Builtin.Mapv || c == Builtin.MapSeq || forces(heap, c, all[first..count]) { unsupported_step(job, Builtin.Proxy, "a host-effect built-in or a delay as a trap"); return; }
    if native_builtin(c) { native_start(job, c, this_value, all[first..count], job.operands.len); return; }
    if conditional_native(c) { start_conditional_native(job, c, this_value, all[first..count], job.operands.len); return; }
    // A built-in that converts its receiver or arguments runs in a Coerce
    // frame (light-cljs-900).
    if coerces(heap, f, this_value, all[first..count]) { start_coerce(job, f, this_value, all[first..count], job.operands.len); return; }
    let r = call_plain(job, f, this_value, all[first..count]);
    if heap.fault == Fault.None { push_operand(job, r); }
}
// The method of the target: a proxy target's in this frame, from pc 0 at
// its next step; any other target's, now.
fn proxy_forward(job: Job&, at: i64, method: i64) {
    let heap .= job.heap; let frame = job.frames[at];
    let target = node(heap, frame.input.id()).value;
    if is_proxy(heap, target) { job.frames[at].input = target; job.frames[at].pc = 0; return; }
    if method == proxy_method::GetPrototypeOf { finish_native(job, frame, prototype(heap, target.id())); return; }
    if method == proxy_method::Call || method == proxy_method::Construct { proxy_forward_call(job, at, method, target); return; }
    if !listable(job, target) { return; }
    if method == proxy_method::IsExtensible { let ok = is_extensible(heap, target); if heap.fault == Fault.None { proxy_answer(job, at, method, ok, false); } return; }
    if method == proxy_method::PreventExtensions { let ok = prevent_extensions(job, Builtin.Proxy, target); if heap.fault == Fault.None { proxy_answer(job, at, method, ok, false); } return; }
    if method == proxy_method::SetPrototypeOf { let ok = set_prototype_of(job, Builtin.Proxy, target, frame.callee); if heap.fault == Fault.None { proxy_answer(job, at, method, ok, false); } return; }
    if method == proxy_method::DefineOwnProperty {
        let d = descriptor_of(heap, frame.callee);
        if heap.fault != Fault.None { return; }
        // An array target's object length converts above the frame first
        // (pc Length, light-cljs-898): the trap was read, so a trap the
        // conversion installs is not called, and the target stays the
        // proxy's.
        let converts = length_definition(heap, target, frame.output, d);
        if heap.fault != Fault.None { return; }
        if converts { convert_length(job, at, proxy_pc::Length, d.value); return; }
        forward_definition(job, at, target, d);
        return;
    }
    if method >= proxy_method::Get { proxy_forward_property(job, at, method, target); return; }
    if method == proxy_method::OwnPropertyKeys {
        var keys: OwnKey[>..] = [];
        if !own_property_keys(heap, target, keys) { return; }
        let list = key_list(heap, Value.Undefined, keys, true);
        if heap.fault == Fault.None { finish_native(job, frame, list); }
        return;
    }
    let d = target_descriptor(job, target, frame.output);
    if heap.fault == Fault.None { finish_native(job, frame, d); }
}
// A trapless [[DefineOwnProperty]]'s definition d on `target`, no proxy:
// its answer.
fn forward_definition(job: Job&, at: i64, target: Value, d: PropertyDescriptor) {
    let heap .= job.heap; let k = job.frames[at].output;
    let ok = define_descriptor(job, target, k, d);
    if heap.fault != Fault.None { return; }
    if ok && d.has_value && !accessor_descriptor(d) && k.is_string() { record(job, target, k, d.value); }
    proxy_answer(job, at, proxy_method::DefineOwnProperty, ok, false);
}
// One step of the Proxy frame at the top (see above).
fn proxy_step(job: Job&, frame: Frame, method: i64) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match frame.pc {
        0 => { proxy_trap(job, at, method); },
        1 => { let trap = job.operands.pop(); proxy_with_trap(job, at, method, trap); },
        2 => {
            let r = job.operands.pop();
            if method == proxy_method::OwnPropertyKeys { own_keys_result(job, at, r); }
            else if method == proxy_method::GetOwnProperty { descriptor_result(job, at, r); }
            else if method == proxy_method::GetPrototypeOf { prototype_result(job, at, r); }
            else if method == proxy_method::Call { finish_native(job, frame, r); }
            else if method == proxy_method::Construct {
                if !r.is_object() { type_error(job, "Proxy: the construct trap result is not an object"); return; }
                finish_native(job, frame, r);
            }
            else if method == proxy_method::IsExtensible || method == proxy_method::PreventExtensions { extensible_result(job, at, method, truthy(heap, r)); }
            else if method == proxy_method::SetPrototypeOf { set_prototype_result(job, at, truthy(heap, r)); }
            else { property_result(job, at, method, r); }
        },
        3 => {
            let len, done = length_value(job, Builtin.Proxy);
            if !done { return; }
            job.frames[at].argument_count = len as i64;
            own_keys_elements(job, at);
        },
        4 => { if own_keys_element(job, at, job.operands.pop()) { own_keys_elements(job, at); } },
        5 => { if descriptor_field(job, at, job.operands.pop(), Builtin.Proxy) && descriptor_fields(job, at, Builtin.Proxy, 5) { descriptor_done(job, at); } },
        6 => { let d = job.operands.pop(); if method == proxy_method::GetOwnProperty { descriptor_target(job, at, d); } else { property_invariants(job, at, method, d); } },
        // A forwarded getter's value is [[Get]]'s; a setter's completion
        // makes [[Set]] true.
        7 => { let v = job.operands.pop(); finish_native(job, frame, if method == proxy_method::Get { v } else { boolean(true) }); },
        // ([[GetOwnProperty]]) a descriptor field's presence, from a proxy
        // trap result's [[HasProperty]].
        8 => { if descriptor_present(job, at, Builtin.Proxy, 5, truthy(heap, job.operands.pop())) && descriptor_fields(job, at, Builtin.Proxy, 5) { descriptor_done(job, at); } },
        // ([[Set]]) both conversions of an array receiver's new `length`
        // arrived: the write, as Reflect.set's (reflect_length_step);
        // ([[DefineOwnProperty]]) of an array target's, the definition of
        // the number on that target (light-cljs-898).
        9 => {
            if method != proxy_method::DefineOwnProperty { proxy_length(job, at); return; }
            let d = converted_descriptor(job, frame.callee);
            if heap.fault == Fault.None { forward_definition(job, at, node(heap, frame.input.id()).value, d); }
        },
        // ([[Set]]) the proxy receiver's [[GetOwnProperty]], then its
        // [[DefineOwnProperty]]'s answer (proxy_receiver_define).
        10 => { receiver_descriptor(job, at, job.operands.pop()); },
        11 => { proxy_answer(job, at, method, truthy(heap, job.operands.pop()), false); },
        // A proxy target's [[IsExtensible]] for the invariants, and a
        // non-extensible proxy target's [[GetPrototypeOf]].
        12 => { target_extensible_known(job, at, method, truthy(heap, job.operands.pop())); },
        13 => { prototype_compare(job, at, job.operands.pop()); },
        // ([[OwnPropertyKeys]]) a proxy target's keys, then each key's
        // descriptor, for the invariants (light-cljs-897).
        14 => { own_keys_target_keys(job, at, job.operands.pop()); },
        15 => { own_keys_target_key(job, at, job.operands.pop()); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}

// ---- [[OwnPropertyKeys]] (10.5.11)
// CreateListFromArrayLike(r, « String, Symbol »): its `length` first.
fn own_keys_result(job: Job&, at: i64, r: Value) {
    let heap .= job.heap;
    if !r.is_object() { type_error(job, "Proxy: ownKeys trap result is not an object"); return; }
    let list = key_list(heap, Value.Undefined, [], true);
    if heap.fault != Fault.None { return; }
    job.frames[at].receiver = r; job.frames[at].callee = list; job.frames[at].index = 0; job.frames[at].pc = 3;
    get_named(job, Builtin.Proxy, r, "length");
}
// Elements from `index` up to the length: each read, a getter's above the
// frame (pc 4).
fn own_keys_elements(job: Job&, at: i64) {
    let heap .= job.heap;
    while job.frames[at].index < job.frames[at].argument_count {
        if !charge(heap) { return; }
        let r = job.frames[at].receiver; let i = job.frames[at].index;
        if node(heap, r.id()).kind == node_kind::Array {
            let v, found = own_element(heap, r, i);
            if heap.fault != Fault.None { return; }
            if found { if !own_keys_element(job, at, v) { return; } continue; }
        }
        let k = index_key(heap, i);
        if heap.fault != Fault.None { return; }
        let v, getter = lookup(heap, r, k);
        if heap.fault != Fault.None { return; }
        if getter.is_object() { job.frames[at].pc = 4; run_getter(job, Builtin.Proxy, getter, r); return; }
        if !own_keys_element(job, at, v) { return; }
    }
    own_keys_done(job, at);
}
// Element `index`: a string (as its interned key) or a symbol.
fn own_keys_element(job: Job&, at: i64, v: Value) -> bool {
    let heap .= job.heap;
    let x = form_value(heap, v);
    if heap.fault != Fault.None { return false; }
    if !x.is_string() && !x.is_symbol() { type_error(job, "Proxy: ownKeys trap result element is not a String or a Symbol"); return false; }
    let k = if x.is_string() { intern(heap, x) } else { x };
    if heap.fault != Fault.None || !list_append(heap, job.frames[at].callee.id(), [OwnKey { key: k }], true) { return false; }
    job.frames[at].index += 1;
    return true;
}
// Steps 8 to 25: no duplicates; every non-configurable key of the target
// present; with a non-extensible target, exactly its keys. The target's
// IsExtensible, [[OwnPropertyKeys]] and each of its keys' [[GetOwnProperty]]
// (steps 9, 10 and 15) are read in that order: a proxy target's each run
// as a Proxy frame above this one (pc TargetExtensible, TargetKeys and
// TargetKey), any other target's are read now (own_property_keys gives
// each key's attributes, unobservably).
fn own_keys_done(job: Job&, at: i64) {
    let heap .= job.heap;
    var words: i64[>..] = [];
    if !trap_words(heap, job.frames[at].callee, words) { return; }
    for i in 1..words.len { if words[i] == words[i - 1] { type_error(job, "Proxy: duplicate key in the ownKeys trap result"); return; } }
    let target = node(heap, job.frames[at].input.id()).value;
    // The trap's result array is read whole.
    if is_proxy(heap, target) { job.frames[at].receiver = Value.Undefined; }
    let ready, ext = target_extensible(job, at);
    if !ready { return; }
    var keys: OwnKey[>..] = [];
    if !listable(job, target) || !own_property_keys(heap, target, keys) || !charge(heap, keys.len) { return; }
    own_keys_check(job, at, ext, keys, words);
}
// The sorted key words of key list `list` (the trap's keys).
fn trap_words(heap: Heap&, list: Value, words: i64[>..]&) -> bool {
    let l: i64 = list.id(); let n: i64 = node(heap, l).key_count;
    if !charge(heap, n) { return false; }
    for i in n { words.push(key_word(property_row(heap, node(heap, l).keys + i).key)); }
    sort(words);
    return true;
}
// A proxy target's IsExtensible answered `ext` (pc TargetExtensible): its
// keys next.
fn own_keys_target(job: Job&, at: i64, ext: bool) {
    let heap .= job.heap; let target = node(heap, job.frames[at].input.id()).value;
    if !is_proxy(heap, target) { heap.fault = Fault.InvalidState; return; }
    job.frames[at].argument_count = if ext { 1 } else { 0 }; job.frames[at].pc = proxy_pc::TargetKeys;
    proxy_start(job, proxy_method::OwnPropertyKeys, target, Value.Undefined);
}
// A proxy target's key list arrived (pc TargetKeys), kept as the frame's
// receiver; or (pc TargetKey) descriptor d of its key `index`: a key
// without one, or with a configurable one, is a configurable key of the
// target (its row's attributes). Then the next key's [[GetOwnProperty]],
// or with every key read, the checks.
fn own_keys_target_keys(job: Job&, at: i64, v: Value) {
    let heap .= job.heap;
    if !v.is_object() || node(heap, v.id()).kind != node_kind::KeyList { heap.fault = Fault.InvalidState; return; }
    job.frames[at].receiver = v; job.frames[at].index = 0;
    own_keys_target_next(job, at);
}
fn own_keys_target_key(job: Job&, at: i64, d: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if !d.is_undefined() && (!d.is_object() || node(heap, d.id()).kind != node_kind::KeyList) { heap.fault = Fault.InvalidState; return; }
    let fixed = d.is_object() && !flag_field(heap, d, field::Configurable);
    if heap.fault != Fault.None { return; }
    property_mut(heap, node(heap, frame.receiver.id()).keys + frame.index).attrs = if fixed { 0 } else { attr_configurable() };
    job.frames[at].index += 1;
    own_keys_target_next(job, at);
}
fn own_keys_target_next(job: Job&, at: i64) {
    let heap .= job.heap; let frame = job.frames[at];
    let list: i64 = frame.receiver.id(); let i = frame.index;
    let target = node(heap, frame.input.id()).value;
    if i < node(heap, list).key_count {
        if !charge(heap) { return; }
        job.frames[at].pc = proxy_pc::TargetKey;
        proxy_start(job, proxy_method::GetOwnProperty, target, property_row(heap, node(heap, list).keys + i).key);
        return;
    }
    let n: i64 = node(heap, list).key_count;
    if !charge(heap, n) { return; }
    var keys: OwnKey[>..] = [];
    for j in n { let r = property_row(heap, node(heap, list).keys + j); keys.push(OwnKey { key: r.key, attrs: r.attrs }); }
    var words: i64[>..] = [];
    if trap_words(heap, frame.callee, words) { own_keys_check(job, at, frame.argument_count != 0, keys, words); }
}
// The checks of the trap's keys (sorted `words`) against the target's
// `keys` (each configurable or not) and its extensibility `ext`.
fn own_keys_check(job: Job&, at: i64, ext: bool, keys: const OwnKey[:], words: const i64[:]) {
    let frame = job.frames[at];
    let n = words.len;
    var seen: bool[>..] = []; seen.resize(n, false);
    // Non-configurable target keys first, then (non-extensible) the rest.
    for pass in 2 {
        if pass == 1 && ext { break; }
        for e in keys {
            if (e.attrs & attr_configurable() == 0) != (pass == 0) { continue; }
            let i = sorted_find(words, key_word(e.key));
            if i < 0 { type_error(job, if pass == 0 { "Proxy: the ownKeys trap result omits a non-configurable key of the target" } else { "Proxy: the ownKeys trap result omits a key of a non-extensible target" }); return; }
            seen[i] = true;
        }
    }
    if !ext { for s in seen { if !s { type_error(job, "Proxy: the ownKeys trap result adds a key to a non-extensible target"); return; } } }
    finish_native(job, frame, frame.callee);
}
// The position of w in sorted `words`, or -1.
fn sorted_find(words: const i64[:], w: i64) -> i64 {
    var lo: i64 = 0; var hi: i64 = words.len;
    while lo < hi { let mid = (lo + hi) / 2; if words[mid] < w { lo = mid + 1; } else { hi = mid; } }
    return if lo < words.len && words[lo] == w { lo } else { -1 };
}

// ---- [[GetOwnProperty]] (10.5.5)
// The trap's result r: undefined, or an object read by ToPropertyDescriptor
// after the target's own descriptor and extensibility. r is kept as the
// frame's receiver from here on; a proxy target's [[GetOwnProperty]] runs
// as a Proxy frame above this one (pc Descriptor, light-cljs-895), any
// other target's is read now.
fn descriptor_result(job: Job&, at: i64, r: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if !r.is_object() && !r.is_undefined() { type_error(job, "Proxy: getOwnPropertyDescriptor trap result is neither an object nor undefined"); return; }
    let target = node(heap, frame.input.id()).value;
    job.frames[at].receiver = r;
    if is_proxy(heap, target) { job.frames[at].pc = proxy_pc::Descriptor; proxy_start(job, proxy_method::GetOwnProperty, target, frame.output); return; }
    if !listable(job, target) { return; }
    let td = target_descriptor(job, target, frame.output);
    if heap.fault == Fault.None { descriptor_target(job, at, td); }
}
// The target's own descriptor td (undefined, or a complete descriptor)
// arrived: a property reported missing must be configurable, then the
// target's IsExtensible (a proxy target's above this frame, pc
// TargetExtensible) decides; for a trap result that is an object, the
// descriptor to build (the callee) keeps td as its namespace_value
// meanwhile.
fn descriptor_target(job: Job&, at: i64, td: Value) {
    let heap .= job.heap; let r = job.frames[at].receiver;
    if !td.is_undefined() && (!td.is_object() || node(heap, td.id()).kind != node_kind::KeyList) { heap.fault = Fault.InvalidState; return; }
    if r.is_undefined() {
        if td.is_undefined() { finish_native(job, job.frames[at], Value.Undefined); return; }
        let fixed = !flag_field(heap, td, field::Configurable);
        if heap.fault != Fault.None { return; }
        if fixed { type_error(job, "Proxy: a non-configurable property of the target is reported missing"); return; }
        job.frames[at].callee = Value.Undefined;
    } else {
        let d = new_descriptor(heap);
        if heap.fault != Fault.None { return; }
        node_mut(heap, d.id()).namespace_value = if td.is_object() { td } else { Value.Null };
        job.frames[at].callee = d;
    }
    let ready, ext = target_extensible(job, at);
    if ready { descriptor_known(job, at, ext); }
}
// The target's extensibility `ext` is known: a property of a
// non-extensible target cannot be reported missing; a trap result that is
// an object is read by ToPropertyDescriptor.
fn descriptor_known(job: Job&, at: i64, ext: bool) {
    if job.frames[at].receiver.is_undefined() {
        if !ext { type_error(job, "Proxy: a property of a non-extensible target is reported missing"); return; }
        finish_native(job, job.frames[at], Value.Undefined);
        return;
    }
    let d = job.frames[at].callee;
    if !d.is_object() || node(job.heap, d.id()).kind != node_kind::KeyList { job.heap.fault = Fault.InvalidState; return; }
    job.frames[at].index = 0; job.frames[at].argument_count = if ext { 1 } else { 0 };
    if descriptor_fields(job, at, Builtin.Proxy, 5) { descriptor_done(job, at); }
}
// ToPropertyDescriptor's reads from field `index` on into the descriptor
// `callee` of the frame at `at`, for built-in b (Proxy, or the Descriptor
// frame's, descriptors.goose): HasProperty, then Get, a getter's above the
// frame, whose value arrives at `phase`, or a proxy's [[HasProperty]]
// (light-cljs-890), whose answer arrives at descriptor_presence(phase)
// (descriptor_present). True when every field is read; false while a
// getter or trap runs, or on a fault.
fn descriptor_fields(job: Job&, at: i64, b: Builtin, phase: i64) -> bool {
    let heap .= job.heap;
    while job.frames[at].index < FIELD_NAMES.len {
        let r = job.frames[at].receiver;
        let k = key_named(heap, FIELD_NAMES[job.frames[at].index].text);
        if heap.fault != Fault.None { return false; }
        let has = has_property(heap, r, k);
        if proxy_pending(heap, proxy_method::HasProperty) { job.frames[at].pc = descriptor_presence(phase); proxy_take(job, proxy_method::HasProperty); return false; }
        if heap.fault != Fault.None || !descriptor_present(job, at, b, phase, has) { return false; }
    }
    return true;
}
// The phase at which a descriptor reader whose getters answer at `phase`
// awaits a field's presence: the Proxy frame's 8 after its 5, the
// Descriptor frame's 3 after its 2.
inline fn descriptor_presence(phase: i64) -> i64 { return if phase == 5 { 8 } else { phase + 1 }; }
// Field `index` is present (`has`) or not: absent, the next field; present,
// its Get, a getter's above the frame (at `phase`). False while a getter
// runs, or on a fault.
fn descriptor_present(job: Job&, at: i64, b: Builtin, phase: i64, has: bool) -> bool {
    let heap .= job.heap;
    if job.frames[at].index >= FIELD_NAMES.len { heap.fault = Fault.InvalidState; return false; }
    if !has { job.frames[at].index += 1; return true; }
    let r = job.frames[at].receiver;
    let k = key_named(heap, FIELD_NAMES[job.frames[at].index].text);
    if heap.fault != Fault.None { return false; }
    let v, getter = lookup(heap, r, k);
    if heap.fault != Fault.None { return false; }
    if getter.is_object() { job.frames[at].pc = phase; run_getter(job, b, getter, r); return false; }
    return descriptor_field(job, at, v, b);
}
// Field `index` read as v: a flag is its ToBoolean; a getter or setter
// must be callable or undefined.
fn descriptor_field(job: Job&, at: i64, v: Value, b: Builtin) -> bool {
    let heap .= job.heap; let f = job.frames[at].index;
    var x = v;
    if f == field::Enumerable || f == field::Configurable || f == field::Writable { x = boolean(truthy(heap, v)); }
    else if (f == field::Get || f == field::Set) && !v.is_undefined() && !proxy_callable(heap, v) {
        let proxy = b == Builtin.Proxy;
        type_error(job, if f == field::Get { if proxy { "Proxy: a descriptor's getter is not a function" } else { "Getter must be a function" } } else if proxy { "Proxy: a descriptor's setter is not a function" } else { "Setter must be a function" });
        return false;
    }
    if !set_field(heap, job.frames[at].callee, f, x) { return false; }
    job.frames[at].index += 1;
    return true;
}
// ToPropertyDescriptor's last check (step 15): a descriptor read for
// built-in b has accessor or data fields, not both.
fn descriptor_valid(job: Job&, d: Value, b: Builtin) -> bool {
    let heap .= job.heap;
    let accessor = has_field(heap, d, field::Get) || has_field(heap, d, field::Set);
    if accessor && (has_field(heap, d, field::Value) || has_field(heap, d, field::Writable)) {
        type_error(job, if b == Builtin.Proxy { "Proxy: a descriptor with both accessor and data fields" } else { "Invalid property descriptor. Cannot both specify accessors and a value or writable attribute" });
        return false;
    }
    return heap.fault == Fault.None;
}
// ToPropertyDescriptor's last check, CompletePropertyDescriptor and the
// invariants (steps 16 to 18).
fn descriptor_done(job: Job&, at: i64) {
    let heap .= job.heap; let frame = job.frames[at]; let d = frame.callee;
    if !descriptor_valid(job, d, Builtin.Proxy) { return; }
    let accessor = has_field(heap, d, field::Get) || has_field(heap, d, field::Set);
    // Absent fields take their defaults: undefined, or false for a flag.
    for f in FIELD_NAMES.len {
        let other = if accessor { f == field::Value || f == field::Writable } else { f == field::Get || f == field::Set };
        if other || has_field(heap, d, f) { continue; }
        if !set_field(heap, d, f, if f == field::Value || f == field::Get || f == field::Set { Value.Undefined } else { boolean(false) }) { return; }
    }
    let td = node(heap, d.id()).namespace_value;
    if !compatible(heap, frame.argument_count != 0, d, td) { if heap.fault == Fault.None { type_error(job, "Proxy: the getOwnPropertyDescriptor trap result is incompatible with the target's property"); } return; }
    if !flag_field(heap, d, field::Configurable) {
        if !td.is_object() || flag_field(heap, td, field::Configurable) { type_error(job, "Proxy: a property reported non-configurable is configurable or missing on the target"); return; }
        if !accessor && !flag_field(heap, d, field::Writable) && flag_field(heap, td, field::Writable) { type_error(job, "Proxy: a property reported non-configurable and non-writable is writable on the target"); return; }
    }
    if heap.fault != Fault.None { return; }
    node_mut(heap, d.id()).namespace_value = Value.Null;
    finish_native(job, frame, d);
}

// ---- [[GetPrototypeOf]] (10.5.1)
// The trap's result r (kept as the frame's receiver): a non-extensible
// target's prototype must be r.
fn prototype_result(job: Job&, at: i64, r: Value) {
    if !r.is_object() && !r.is_null() { type_error(job, "Proxy: getPrototypeOf trap result is neither an object nor null"); return; }
    job.frames[at].receiver = r;
    let ready, ext = target_extensible(job, at);
    if ready { prototype_known(job, at, ext); }
}
fn prototype_known(job: Job&, at: i64, ext: bool) {
    if ext { finish_native(job, job.frames[at], job.frames[at].receiver); return; }
    target_prototype(job, at);
}

// ---- The target's state for a trap's invariants (light-cljs-891)
// IsExtensible(target) of the frame at `at`: true and the answer for a
// target that is no proxy; a proxy target's [[IsExtensible]] runs as a
// Proxy frame above this one, its answer arriving at pc TargetExtensible
// (target_extensible_known), and false comes back.
fn target_extensible(job: Job&, at: i64) -> bool, bool {
    let heap .= job.heap; let target = node(heap, job.frames[at].input.id()).value;
    if is_proxy(heap, target) { job.frames[at].pc = proxy_pc::TargetExtensible; proxy_start(job, proxy_method::IsExtensible, target, Value.Undefined); return false, false; }
    let ext = extensible(heap, target);
    return heap.fault == Fault.None, ext;
}
// The target's IsExtensible arrived: the method's check goes on.
fn target_extensible_known(job: Job&, at: i64, method: i64, ext: bool) {
    if method == proxy_method::HasProperty || method == proxy_method::Delete { presence_known(job, at, method, ext); }
    else if method == proxy_method::GetPrototypeOf { prototype_known(job, at, ext); }
    else if method == proxy_method::IsExtensible || method == proxy_method::PreventExtensions { extensible_known(job, at, method, ext); }
    else if method == proxy_method::SetPrototypeOf { set_prototype_known(job, at, ext); }
    else if method == proxy_method::DefineOwnProperty { define_known(job, at, ext); }
    else if method == proxy_method::OwnPropertyKeys { own_keys_target(job, at, ext); }
    else if method == proxy_method::GetOwnProperty { descriptor_known(job, at, ext); }
    else { job.heap.fault = Fault.InvalidState; }
}
// A non-extensible target's [[GetPrototypeOf]]: a proxy target's runs as a
// Proxy frame above this one, its answer arriving at pc TargetPrototype.
fn target_prototype(job: Job&, at: i64) {
    let heap .= job.heap; let target = node(heap, job.frames[at].input.id()).value;
    if is_proxy(heap, target) { job.frames[at].pc = proxy_pc::TargetPrototype; proxy_start(job, proxy_method::GetPrototypeOf, target, Value.Undefined); return; }
    prototype_compare(job, at, prototype(heap, target.id()));
}
// The non-extensible target's prototype q must be [[GetPrototypeOf]]'s
// trap result (the receiver), or [[SetPrototypeOf]]'s new prototype.
fn prototype_compare(job: Job&, at: i64, q: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    let method: i64 = match frame.native { Proxy p => p.method, _ => -1 };
    if method == proxy_method::GetPrototypeOf {
        if !same_value(heap, frame.receiver, q) { type_error(job, "Proxy: the getPrototypeOf trap result differs from a non-extensible target's prototype"); return; }
        finish_native(job, frame, frame.receiver);
        return;
    }
    if method != proxy_method::SetPrototypeOf { heap.fault = Fault.InvalidState; return; }
    if !same_value(heap, frame.callee, q) { type_error(job, "Proxy: the setPrototypeOf trap changed a non-extensible target's prototype"); return; }
    proxy_answer(job, at, method, true, true);
}

// ---- [[Get]], [[Set]], [[HasProperty]] and [[Delete]] (10.5.8, 10.5.9,
// 10.5.7, 10.5.10)
// Without the trap: the method of `target`, no proxy, with the frame's key
// (and receiver, and [[Set]]'s value): OrdinaryGet, OrdinarySet (receiver_set,
// shared with super assignments and Reflect.set), OrdinaryHasProperty or the
// kind's [[Delete]]. A proxy on the target's prototype chain restarts the
// frame on it, with the same key and receiver (its [[Get]], [[Set]] or
// [[HasProperty]]); a getter or setter runs above the frame (pc Accessor).
// A proxy receiver of a forwarded [[Set]] is defined on through its
// [[GetOwnProperty]] and [[DefineOwnProperty]] (proxy_receiver_define).
fn proxy_forward_property(job: Job&, at: i64, method: i64, target: Value) {
    let heap .= job.heap; let frame = job.frames[at]; let k = frame.output;
    if method == proxy_method::Get {
        prepare_function(job, target, k, false);
        if heap.fault != Fault.None { return; }
        let v, getter = lookup(heap, target, k);
        if heap.fault != Fault.None { return; }
        if is_hit(heap, getter) { let p, x = take_proxy(heap); proxy_restart(job, at, p); return; }
        if !getter.is_object() { finish_native(job, frame, v); return; }
        job.frames[at].pc = proxy_pc::Accessor;
        call_accessor(job, getter, frame.receiver, []);
        return;
    }
    if method == proxy_method::HasProperty {
        let found = has_property(heap, target, k);
        if proxy_pending(heap, proxy_method::HasProperty) { let p, x = take_proxy(heap); proxy_restart(job, at, p); return; }
        if heap.fault == Fault.None { finish_native(job, frame, boolean(found)); }
        return;
    }
    if method == proxy_method::Delete {
        if !writable(job, target.id()) { image_write(job, k); return; }
        // A lazy closure's `length` or `name` is made before it goes (delete_op).
        if key_is(heap, k, "length") || key_is(heap, k, "name") { prepare_function(job, target, k, true); }
        let ok = delete_property(heap, target, k);
        if heap.fault == Fault.None { proxy_answer(job, at, method, ok, false); }
        return;
    }
    let setter, ok = receiver_set(job, target, k, frame.callee, frame.receiver);
    if proxy_pending(heap, proxy_method::Set) { let p, x = take_proxy(heap); proxy_restart(job, at, p); return; }
    if proxy_pending(heap, proxy_method::DefineOwnProperty) { take_proxy(heap); proxy_receiver_define(job, at); return; }
    if heap.fault != Fault.None { return; }
    // An array receiver's `length` set to an object converts it twice
    // (start_length's conversions) above the frame, at pc Length.
    if setter.is_hole() {
        job.frames[at].pc = proxy_pc::Length;
        if push_operand(job, frame.callee) && push_operand(job, frame.callee) { start_convert(job, Hint.Number, true, 2, 2); }
        return;
    }
    if setter.is_object() {
        job.frames[at].pc = proxy_pc::Accessor;
        call_accessor(job, setter, frame.receiver, [frame.callee]);
        return;
    }
    proxy_answer(job, at, method, ok, false);
}
// A forwarded [[Set]] of an array receiver's `length` once both
// conversions are numbers (converted_length): unchanged on a read-only
// length that keeps its value, else written to the receiver.
fn proxy_length(job: Job&, at: i64) {
    let heap .= job.heap; let frame = job.frames[at];
    let n = converted_length(job);
    if heap.fault != Fault.None { return; }
    let r = frame.receiver; let k = frame.output;
    if !r.is_object() || node(heap, r.id()).kind != node_kind::Array { heap.fault = Fault.InvalidState; return; }
    if length_kept(heap, r, n) { proxy_answer(job, at, proxy_method::Set, true, false); return; }
    let setter, ok = write(heap, r, k, n);
    if heap.fault != Fault.None { return; }
    if setter.is_object() { heap.fault = Fault.InvalidState; return; }
    if ok { record(job, r, k, n); }
    proxy_answer(job, at, proxy_method::Set, ok, false);
}
// The frame goes on with proxy p's method (pc 0, its next step).
// A prototype chain can lead back to the proxy (a target whose prototype
// is the proxy): the frame counts these hops in argument_count, and past
// the frame limit fails StackLimit, as nesting the hops would
// (proxy_forwarding_cycles_are_bounded).
fn proxy_restart(job: Job&, at: i64, p: Value) {
    if !is_proxy(job.heap, p) { job.heap.fault = Fault.InvalidState; return; }
    if job.frames[at].argument_count >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames[at].argument_count += 1;
    job.frames[at].input = p; job.frames[at].pc = 0;
}
// [[Set]]'s or [[Delete]]'s answer `ok`, from the trap (`trap`) or the
// target: false is a TypeError when the frame's caller throws on it
// (strict code, Set(O, P, V, true)).
fn proxy_answer(job: Job&, at: i64, method: i64, ok: bool, trap: bool) {
    let mode = job.frames[at].index;
    if !ok && mode != answer::Boolean {
        if method == proxy_method::Delete { type_error(job, if trap { "Proxy: the deleteProperty trap returned false" } else { "could not delete property" }); }
        else if method == proxy_method::Set { type_error(job, if trap { "Proxy: the set trap returned false" } else { "Proxy: the target refused the assignment" }); }
        else if method == proxy_method::PreventExtensions { type_error(job, if trap { "Proxy: the preventExtensions trap returned false" } else { "proxy preventExtensions handler returned false" }); }
        else if method == proxy_method::SetPrototypeOf { type_error(job, if trap { "Proxy: the setPrototypeOf trap returned false" } else { "circular prototype chain or non-extensible object" }); }
        else { type_error(job, if trap { "Proxy: the defineProperty trap returned false" } else { "cannot define the property" }); }
        return;
    }
    let frame = job.frames[at];
    finish_native(job, frame, if mode == answer::Object { frame.receiver } else if mode == answer::Nothing { Value.Undefined } else { boolean(ok) });
}
// The trap's result r: [[Get]]'s value, the others' ToBoolean. A false
// [[Set]] or [[Delete]] and a true [[HasProperty]] answer now; otherwise
// the target's own descriptor decides the invariants: a proxy target's
// [[GetOwnProperty]] runs as a Proxy frame above this one (pc Descriptor),
// any other target's is read now.
fn property_result(job: Job&, at: i64, method: i64, r: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if method == proxy_method::Get { job.frames[at].callee = r; }
    else {
        let b = truthy(heap, r);
        if method == proxy_method::HasProperty { if b { finish_native(job, frame, boolean(true)); return; } }
        else if !b { proxy_answer(job, at, method, false, true); return; }
    }
    let target = node(heap, frame.input.id()).value;
    if is_proxy(heap, target) { job.frames[at].pc = proxy_pc::Descriptor; proxy_start(job, proxy_method::GetOwnProperty, target, frame.output); return; }
    if !listable(job, target) { return; }
    let d = target_descriptor(job, target, frame.output);
    if heap.fault == Fault.None { property_invariants(job, at, method, d); }
}
// The invariants against the target's own descriptor d (undefined, or a
// complete descriptor): [[Get]]'s result and [[Set]]'s value must be the
// value of a non-configurable, non-writable data property, and a
// non-configurable accessor without the getter (setter) gives undefined
// (refuses any [[Set]]); a property [[HasProperty]] reports missing or
// [[Delete]] reports deleted must be configurable, on an extensible target.
fn property_invariants(job: Job&, at: i64, method: i64, d: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if !d.is_undefined() && (!d.is_object() || node(heap, d.id()).kind != node_kind::KeyList) { heap.fault = Fault.InvalidState; return; }
    let found = d.is_object();
    let fixed = found && !flag_field(heap, d, field::Configurable);
    if heap.fault != Fault.None { return; }
    if method == proxy_method::DefineOwnProperty { define_invariants(job, at, d, found, fixed); return; }
    if method == proxy_method::Get || method == proxy_method::Set {
        let v = frame.callee;
        if fixed {
            let accessor = has_field(heap, d, field::Get) || has_field(heap, d, field::Set);
            if !accessor && !flag_field(heap, d, field::Writable) {
                let x, y = field_of(heap, d, field::Value);
                if heap.fault == Fault.None && !same_value(heap, v, x) { type_error(job, if method == proxy_method::Get { "Proxy: the get trap result differs from the target's non-writable, non-configurable property" } else { "Proxy: the set trap changed the target's non-writable, non-configurable property" }); return; }
            }
            if accessor {
                let half, y = field_of(heap, d, if method == proxy_method::Get { field::Get } else { field::Set });
                if heap.fault == Fault.None && half.is_undefined() && (method == proxy_method::Set || !v.is_undefined()) { type_error(job, if method == proxy_method::Get { "Proxy: the get trap result is not undefined for the target's non-configurable accessor without a getter" } else { "Proxy: the set trap succeeded for the target's non-configurable accessor without a setter" }); return; }
            }
        }
        if heap.fault == Fault.None { finish_native(job, frame, if method == proxy_method::Get { v } else { boolean(true) }); }
        return;
    }
    let has = method == proxy_method::HasProperty;
    if found {
        if fixed { type_error(job, if has { "Proxy: a non-configurable property of the target is reported missing" } else { "Proxy: a non-configurable property of the target is reported deleted" }); return; }
        let ready, ext = target_extensible(job, at);
        if ready { presence_known(job, at, method, ext); }
        return;
    }
    finish_native(job, frame, boolean(!has));
}
// A property the target has, reported missing or deleted: the target must
// be extensible.
fn presence_known(job: Job&, at: i64, method: i64, ext: bool) {
    let has = method == proxy_method::HasProperty;
    if !ext { type_error(job, if has { "Proxy: a property of a non-extensible target is reported missing" } else { "Proxy: a property of a non-extensible target is reported deleted" }); return; }
    finish_native(job, job.frames[at], boolean(!has));
}

// ---- [[DefineOwnProperty]] (10.5.6)
// The trap answered true for descriptor `callee` (partial): a property the
// target lacks needs an extensible target and may not be reported
// non-configurable; one it has must be compatible, reported
// non-configurable only when it is, and reported non-writable only when a
// non-configurable data property is.
// The target's descriptor d is kept as the frame's receiver meanwhile.
fn define_invariants(job: Job&, at: i64, d: Value, found: bool, fixed: bool) {
    job.frames[at].receiver = d;
    let ready, ext = target_extensible(job, at);
    if ready { define_known(job, at, ext); }
}
fn define_known(job: Job&, at: i64, ext: bool) {
    let heap .= job.heap; let frame = job.frames[at]; let desc = frame.callee; let d = frame.receiver;
    if !d.is_undefined() && (!d.is_object() || node(heap, d.id()).kind != node_kind::KeyList) { heap.fault = Fault.InvalidState; return; }
    let found = d.is_object();
    let fixed = found && !flag_field(heap, d, field::Configurable);
    let c, has_c = field_of(heap, desc, field::Configurable);
    if heap.fault != Fault.None { return; }
    let config_false = has_c && !c.flag();
    if !found {
        if !ext { type_error(job, "Proxy: the defineProperty trap added a property to a non-extensible target"); return; }
        if config_false { type_error(job, "Proxy: the defineProperty trap reported a property the target lacks non-configurable"); return; }
        proxy_answer(job, at, proxy_method::DefineOwnProperty, true, true);
        return;
    }
    if !compatible(heap, ext, desc, d) { if heap.fault == Fault.None { type_error(job, "Proxy: the defineProperty trap's descriptor is incompatible with the target's property"); } return; }
    if config_false && !fixed { type_error(job, "Proxy: the defineProperty trap reported a configurable property of the target non-configurable"); return; }
    if fixed && has_field(heap, d, field::Value) && flag_field(heap, d, field::Writable) && has_field(heap, desc, field::Writable) && !flag_field(heap, desc, field::Writable) { type_error(job, "Proxy: the defineProperty trap reported a writable, non-configurable property of the target non-writable"); return; }
    if heap.fault == Fault.None { proxy_answer(job, at, proxy_method::DefineOwnProperty, true, true); }
}
// FromPropertyDescriptor(d): an ordinary object with d's fields, in the
// specification's order, the descriptor a defineProperty trap receives.
let FROM_ORDER: i64[6] = [field::Value, field::Writable, field::Get, field::Set, field::Enumerable, field::Configurable];
fn from_descriptor(job: Job&, d: Value) -> Value {
    let heap .= job.heap;
    let o = allocate(heap, Node { kind: node_kind::Object });
    if heap.fault != Fault.None { return Value.Undefined; }
    for f in FROM_ORDER {
        let v, found = field_of(heap, d, f);
        if heap.fault != Fault.None { return Value.Undefined; }
        if !found { continue; }
        let k = key_named(heap, FIELD_NAMES[f].text);
        if heap.fault != Fault.None { return Value.Undefined; }
        if !add_own(heap, o.id(), k, v, attrs_data()) { if heap.fault == Fault.None { memory(heap); } return Value.Undefined; }
    }
    return o;
}
// A trapless [[Set]] reached its proxy receiver (OrdinarySetWithOwnDescriptor
// steps 2.c to 2.e): the receiver's [[GetOwnProperty]] (pc ReceiverOwn),
// then its [[DefineOwnProperty]] of {[[Value]]: V} on an existing writable
// data property or CreateDataProperty's complete descriptor on none (pc
// ReceiverDefine), each a Proxy frame above this [[Set]] frame, whose answer
// is the definition's. Also started for Reflect.set and super assignments
// with a proxy receiver (proxy_set_receiver).
fn proxy_receiver_define(job: Job&, at: i64) {
    let frame = job.frames[at];
    if !is_proxy(job.heap, frame.receiver) { job.heap.fault = Fault.InvalidState; return; }
    job.frames[at].pc = proxy_pc::ReceiverOwn;
    proxy_start(job, proxy_method::GetOwnProperty, frame.receiver, frame.output);
}
fn receiver_descriptor(job: Job&, at: i64, d: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if !d.is_undefined() && (!d.is_object() || node(heap, d.id()).kind != node_kind::KeyList) { heap.fault = Fault.InvalidState; return; }
    if d.is_object() && (has_field(heap, d, field::Get) || has_field(heap, d, field::Set) || !flag_field(heap, d, field::Writable)) {
        if heap.fault == Fault.None { proxy_answer(job, at, proxy_method::Set, false, false); }
        return;
    }
    let desc = new_descriptor(heap);
    if heap.fault != Fault.None || !set_field(heap, desc, field::Value, frame.callee) { return; }
    if !d.is_object() && !(set_field(heap, desc, field::Writable, boolean(true)) && set_field(heap, desc, field::Enumerable, boolean(true)) && set_field(heap, desc, field::Configurable, boolean(true))) { return; }
    job.frames[at].pc = proxy_pc::ReceiverDefine;
    proxy_push(job, proxy_method::DefineOwnProperty, frame.receiver, frame.output, Value.Undefined, desc, answer::Boolean, job.operands.len);
}
// A built-in's or super assignment's [[Set]] whose receiver_set reached a
// proxy receiver (proxy_pending DefineOwnProperty): a [[Set]] frame on that
// receiver for key k and value v, at its receiver phases, whose answer
// arrives on the operands (`throws`: a false one is a TypeError).
fn proxy_set_receiver(job: Job&, k: Value, v: Value, throws: bool) {
    let r, x = take_proxy(job.heap);
    if !proxy_push(job, proxy_method::Set, r, k, r, v, if throws { answer::Throws } else { answer::Boolean }, job.operands.len) { return; }
    proxy_receiver_define(job, job.frames.len - 1);
}

// ---- [[Call]] and [[Construct]] (10.5.12, 10.5.13)
// The arguments of each method's trap, after the target.
fn trap_arity(method: i64) -> i64 {
    if method == proxy_method::Set { return 4; }
    if method == proxy_method::Get || method == proxy_method::DefineOwnProperty || method == proxy_method::Call || method == proxy_method::Construct { return 3; }
    if method == proxy_method::GetPrototypeOf || method == proxy_method::OwnPropertyKeys || method == proxy_method::IsExtensible || method == proxy_method::PreventExtensions { return 1; }
    return 2;
}
// Without the trap: the call or construction of the target (no proxy)
// with the frame's arguments replaces the frame, in a Forward frame
// (start_bound) as a forwarded call runs it; a construction keeps the
// frame's newTarget (the proxy itself after `new p`), whose `prototype`
// the new object then takes through the proxy's [[Get]].
fn proxy_forward_call(job: Job&, at: i64, method: i64, target: Value) {
    let heap .= job.heap; let frame = job.frames[at]; let list = frame.output;
    if !list.is_object() || node(heap, list.id()).kind != node_kind::Array || at != job.frames.len - 1 { heap.fault = Fault.InvalidState; return; }
    let n: i64 = node(heap, list.id()).length;
    if !charge(heap, n) { return; }
    var args: Value[>..] = [];
    for i in n { let v, found = own_element(heap, list, i); if heap.fault != Fault.None { return; } args.push(v); }
    job.frames.pop();
    job.operands.resize(frame.operand_base);
    start_bound(job, target, frame.receiver, args[..], frame.operand_base, method == proxy_method::Construct);
}

// ---- [[IsExtensible]], [[PreventExtensions]] and [[SetPrototypeOf]]
// (10.5.3, 10.5.4, 10.5.2)
// The trap's ToBoolean result b: [[IsExtensible]]'s must be the target's;
// a true [[PreventExtensions]] needs a non-extensible target.
// The result is kept as the frame's callee meanwhile.
fn extensible_result(job: Job&, at: i64, method: i64, b: bool) {
    if method == proxy_method::PreventExtensions && !b { proxy_answer(job, at, method, false, true); return; }
    job.frames[at].callee = boolean(b);
    let ready, ext = target_extensible(job, at);
    if ready { extensible_known(job, at, method, ext); }
}
fn extensible_known(job: Job&, at: i64, method: i64, ext: bool) {
    let b = job.frames[at].callee.flag();
    if method == proxy_method::IsExtensible && ext != b { type_error(job, "Proxy: the isExtensible trap result differs from the target's"); return; }
    if method == proxy_method::PreventExtensions && ext { type_error(job, "Proxy: the preventExtensions trap returned true for an extensible target"); return; }
    proxy_answer(job, at, method, b, true);
}
// A true [[SetPrototypeOf]] of a non-extensible target needs the target's
// prototype to be the new one (`callee`).
fn set_prototype_result(job: Job&, at: i64, b: bool) {
    if !b { proxy_answer(job, at, proxy_method::SetPrototypeOf, false, true); return; }
    let ready, ext = target_extensible(job, at);
    if ready { set_prototype_known(job, at, ext); }
}
fn set_prototype_known(job: Job&, at: i64, ext: bool) {
    if ext { proxy_answer(job, at, proxy_method::SetPrototypeOf, true, true); return; }
    target_prototype(job, at);
}

// ---- The built-ins that reach these methods
// Object.isExtensible, preventExtensions and setPrototypeOf, their Reflect
// counterparts, the `__proto__` setter and Object.prototype.isPrototypeOf
// run where a frame can start (conditional_native): a proxy target's
// method runs as its Proxy frame at the call's operand base, the frame's
// answer mode giving the built-in's result; isPrototypeOf walks with
// prototype_walk (source_vm.goose); anything else is the plain built-in.
fn proxy_conditional(b: Builtin) -> bool {
    return match b { ObjectIsExtensible, ReflectIsExtensible, ObjectPreventExtensions, ReflectPreventExtensions, ObjectSetPrototypeOf, ReflectSetPrototypeOf, ObjectPrototypeProtoSetter, ObjectPrototypeIsPrototypeOf => true, _ => false };
}
fn start_proxy_conditional(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    if b == Builtin.ObjectPrototypeIsPrototypeOf { start_is_prototype_of(job, receiver, arg(args, 0), base); return; }
    let setter = b == Builtin.ObjectPrototypeProtoSetter;
    let target = if setter { receiver } else { arg(args, 0) };
    let value = if setter { arg(args, 0) } else { arg(args, 1) };
    var r = Value.Undefined;
    if !is_proxy(heap, target) { r = call_builtin(job, code(b), receiver, args); }
    if heap.fault != Fault.None { return; }
    if !is_proxy(heap, target) { job.operands.resize(base); deliver(job, r); return; }
    var method = proxy_method::IsExtensible; var mode = answer::Boolean;
    if b == Builtin.ObjectPreventExtensions || b == Builtin.ReflectPreventExtensions {
        method = proxy_method::PreventExtensions;
        if b == Builtin.ObjectPreventExtensions { mode = answer::Object; }
    } else if b == Builtin.ObjectSetPrototypeOf || b == Builtin.ReflectSetPrototypeOf || setter {
        if !prototype_value(value) {
            if setter { job.operands.resize(base); deliver(job, Value.Undefined); return; }
            type_error(job, "Object prototype may only be an Object or null"); return;
        }
        method = proxy_method::SetPrototypeOf;
        mode = if setter { answer::Nothing } else if b == Builtin.ObjectSetPrototypeOf { answer::Object } else { answer::Boolean };
    }
    job.operands.resize(base);
    proxy_push(job, method, target, Value.Undefined, target, value, mode, base);
}

// ---- Proxy.revocable (28.2.2.1)
// Proxy.revocable(target, handler): a new proxy and its revoke function
// (Builtin.ProxyRevoke, a job row whose `value` is the proxy until it
// revokes it), in a new object's `proxy` and `revoke`.
fn proxy_revocable(job: Job&, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let p = construct_proxy(job, args);
    if heap.fault != Fault.None { return Value.Undefined; }
    let revoke = allocate(heap, Node { kind: node_kind::Builtin, function: code(Builtin.ProxyRevoke), value: p });
    let o = allocate(heap, Node { kind: node_kind::Object });
    if heap.fault != Fault.None { return Value.Undefined; }
    let kp = key_named(heap, "proxy"); let kr = key_named(heap, "revoke");
    if heap.fault != Fault.None { return Value.Undefined; }
    if !add_own(heap, o.id(), kp, p, attrs_data()) || !add_own(heap, o.id(), kr, revoke, attrs_data()) { if heap.fault == Fault.None { memory(heap); } return Value.Undefined; }
    return o;
}
// A revoke function f called: its proxy is marked revoked (its node's
// `initialized`, the revoked flag; decision 2: proxy node state, which
// snapshots, the reset, reclamation and continuations carry as any field),
// and f forgets the proxy. Target and handler stay: `typeof` of the proxy
// does not change, and an operation that read its handler before the
// revocation (a trap getter that revokes) still calls its trap with that
// handler. A second call does nothing.
fn proxy_revoke(job: Job&, f: Value) -> Value {
    let heap .= job.heap;
    if !f.is_object() || node(heap, f.id()).kind != node_kind::Builtin { heap.fault = Fault.InvalidState; return Value.Undefined; }
    let p = node(heap, f.id()).value;
    if !p.is_object() { return Value.Undefined; }
    if !is_proxy(heap, p) { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if !writable(job, f.id()) || !writable(job, p.id()) { image_write(job, Value.Undefined, "a proxy's revocation"); return Value.Undefined; }
    node_mut(heap, p.id()).initialized = true;
    node_mut(heap, f.id()).value = Value.Null;
    return Value.Undefined;
}
// Call plain built-in function f (a Builtin row) with `receiver` and
// `args` to completion: call_builtin, except that a revoke function reads
// its own row. Every caller that knows the function object calls through
// here.
fn call_plain(job: Job&, f: Value, receiver: Value, args: const Value[:]) -> Value {
    if builtin_of(job.heap, f.id()) == Builtin.ProxyRevoke { return proxy_revoke(job, f); }
    return call_builtin(job, node(job.heap, f.id()).function, receiver, args);
}
// Whether proxy p is revoked.
inline fn revoked(heap: const Heap&, p: Value) -> bool { return node(heap, p.id()).initialized; }
// IsArray(v) (7.2.2): an array, or a proxy whose target is one; a revoked
// proxy on the way is a TypeError (false, with the fault).
fn is_array_value(job: Job&, v: Value) -> bool {
    let heap .= job.heap; var t = v;
    while is_proxy(heap, t) {
        if revoked(heap, t) { type_error(job, "revoked proxy"); return false; }
        t = node(heap, t.id()).value;
    }
    return t.is_object() && node(heap, t.id()).kind == node_kind::Array;
}

// ---- for-in over a proxy (source_vm.goose's for_in_walk)
// Wait in a ForIn frame (pushed at the operands of ForInNext's results, or
// the one at `at`) at `phase` for internal method `method` of proxy p.
fn for_in_proxy(job: Job&, at: i64, o: i64, phase: i64, method: i64, p: Value, k: Value) {
    if at >= 0 { job.frames[at].pc = phase; }
    else {
        if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
        job.frames.push(Frame { function: -1, pc: phase, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: Value.Undefined, input: object(o), native: Native.ForIn });
    }
    proxy_start(job, method, p, k);
}
// One step of the ForIn frame at the top: pc 1 takes the walked proxy's
// keys (every string key a candidate, as the Pants engine's for_in_set_keys
// marks them), pc 2 the descriptor of the key just reached (undefined: the
// key is skipped and hides no name further up; otherwise visited when
// enumerable), pc 3 the walk's next object.
fn for_in_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1; let o: i64 = frame.input.id();
    let v = job.operands.pop();
    // The key list's state at the frame's phase (a restored image's rows
    // are only checked one by one, so the walk checks its own cursor): it
    // walks a proxy whose keys are pending (pc 1), whose key at `length - 1`
    // was just reached (pc 2), or whose keys are used up (pc 3).
    let n .= node(heap, o);
    let walking = is_proxy(heap, n.value) && (if frame.pc == 1 { n.function == key_list::Pending } else if frame.pc == 2 { n.function == key_list::Listed && n.start < n.length && n.length <= n.key_count } else { n.function == key_list::Listed && n.length == n.key_count });
    if !walking { heap.fault = Fault.InvalidState; return; }
    match frame.pc {
        1 => {
            if !v.is_object() || node(heap, v.id()).kind != node_kind::KeyList { heap.fault = Fault.InvalidState; return; }
            var keys: OwnKey[>..] = [];
            for i in node(heap, v.id()).key_count { keys.push(OwnKey { key: property_row(heap, node(heap, v.id()).keys + i).key, attrs: attr_enumerable() }); }
            if !list_append(heap, o, keys, false) { return; }
            node_mut(heap, o).function = key_list::Listed;
        },
        2 => {
            let row: i64 = node(heap, o).keys + node(heap, o).length - 1;
            if v.is_undefined() { property_mut(heap, row).value = Value.Null; }
            else if flag_field(heap, v, field::Enumerable) { for_in_yield(job, at, property_row(heap, row).key, true); return; }
            if heap.fault != Fault.None { return; }
        },
        3 => { if !for_in_enter(job, at, o, v) { return; } },
        _ => { heap.fault = Fault.InvalidState; return; },
    }
    for_in_walk(job, at, o);
}
