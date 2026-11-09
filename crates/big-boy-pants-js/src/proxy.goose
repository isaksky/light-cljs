import std;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .cells;

namespace source;

// Proxy objects and the three internal methods for-in uses (light-cljs-845;
// decisions 1, 2, 4 and 5 of docs/javascript-coverage.md). Semantics from
// the Pants engine's rust/src/vm/proxy.rs and vm/keys.rs, QuickJS's
// js_proxy_constructor, js_proxy_get_own_property_names and
// js_proxy_get_own_property, and the specification (ECMAScript 10.5.1
// [[GetPrototypeOf]], 10.5.5 [[GetOwnProperty]], 10.5.11
// [[OwnPropertyKeys]], 28.2.1.1 Proxy(target, handler), 7.3.18
// CreateListFromArrayLike, 6.2.6.5 ToPropertyDescriptor, 6.2.6.6
// CompletePropertyDescriptor, 10.1.6.3 ValidateAndApplyPropertyDescriptor).
//
// A proxy is a node of kind node_kind::Proxy whose internal slots hold its
// target (`value`) and handler (`namespace_value`), both objects, the
// target always an earlier row (decision 2); it has no own properties and
// no prototype of its own (`proto` Null). `new Proxy(target, handler)`
// makes one, a job row; Proxy called without `new`, or with a target or
// handler that is no object, is a TypeError. `typeof` gives "function" when
// its final target is callable. Proxy.revocable is light-cljs-766's.
//
// [[OwnPropertyKeys]], [[GetOwnProperty]] and [[GetPrototypeOf]] (for-in,
// light-cljs-845) and [[Get]], [[Set]], [[HasProperty]] and [[Delete]]
// (ECMAScript 10.5.8, 10.5.9, 10.5.7 and 10.5.10; light-cljs-890) run as a
// Proxy native frame (Native.Proxy, its proxy_method the payload): every
// trap getter, trap call and observable read of a trap's result (the
// result's `length` and elements, a descriptor's fields), a forwarded
// getter or setter, and a proxy target's [[GetOwnProperty]] for a trap's
// invariants runs as a frame above it (decision 4), and its result arrives
// at its operand base for the frame below. A proxy without the trap
// forwards to its target: a proxy target restarts the frame on it (pc 0,
// one step a hop), as does a proxy that [[Get]], [[Set]] or
// [[HasProperty]] meets on the target's prototype chain; any other target
// answers through its own internal method, a getter or setter running
// above the frame.
//
//   input     the proxy whose method runs (its target after a forward)
//   output    the property key (all but [[GetPrototypeOf]] and
//             [[OwnPropertyKeys]])
//   receiver  [[Get]]'s and [[Set]]'s receiver; for the two enumeration
//             methods the trap's result object while its fields are read
//   callee    [[Set]]'s value; for the enumeration methods the trap while
//             it is called, then the key list or descriptor the frame
//             builds; [[Get]]'s trap result while the target's descriptor
//             is read
//   index, argument_count  CreateListFromArrayLike's index and length;
//             ToPropertyDescriptor's next field and the target's
//             extensibility; for [[Set]] and [[Delete]], index 1 when a
//             false result throws (strict code, or a built-in's
//             Set(O, P, V, true)); for the four property methods,
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
//   pc 6  ([[Get]], [[Set]], [[HasProperty]], [[Delete]]) awaits a proxy
//         target's descriptor for the trap result's invariants
//   pc 7  ([[Get]], [[Set]]) awaits a forwarded getter's or setter's result
//   pc 8  ([[GetOwnProperty]]) awaits a proxy trap result's answer whether
//         it has descriptor field `index` (its [[HasProperty]])
//   pc 9  ([[Set]]) awaits both conversions of an array receiver's new
//         `length` (proxy_length)
//
// [[OwnPropertyKeys]] gives a key list (node_kind::KeyList, source_vm.goose's
// "Key enumeration") of its keys, symbols included, attributes clear.
// [[GetOwnProperty]] gives undefined or a *descriptor*: a key list whose
// records are the descriptor's fields, keyed by their names (`value`,
// `writable`, `get`, `set`, `enumerable`, `configurable`), complete once
// delivered; while the frame builds it, its namespace_value is the target's
// own descriptor (or Null), read before the trap result's fields as the
// specification orders it. [[GetPrototypeOf]] gives an object or null,
// [[Get]] the value, the other three a boolean. Key lists and descriptors
// are job rows only frames hold, truncated with the job (decision 3).
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
// [[Delete]] with the proxy and key recorded (proxy_relabel); a caller that
// supports the method takes it (proxy_pending, take_proxy) and starts the
// frame, and any other caller still fails by name. The two fields live
// only from the operation to its caller within one step; fail() and the
// job reset clear them, and in-job reclamation keeps and moves what they
// hold (reclaim.goose).
//
// Not implemented yet, each a named UnsupportedOperand (decision 1): a
// proxy's [[DefineOwnProperty]] (so `set` without a trap, which defines on
// the proxy receiver, fails by name), [[Call]], [[Construct]],
// [[IsExtensible]], [[PreventExtensions]] and [[SetPrototypeOf]]
// (light-cljs-891), and [[GetPrototypeOf]] where a built-in other than
// for-in reaches it (Object.getPrototypeOf), named in Job.detail as
// `Proxy: [[DefineOwnProperty]]` (Heap.proxy_method, fail); [[IsExtensible]]
// of a proxy target in a trap's invariants, and for getOwnPropertyDescriptor
// its [[GetOwnProperty]]; a host-effect built-in or a delay as a trap. A
// callable proxy counts as callable for `typeof`, for calls, which fail by
// name, and as a descriptor's getter or setter, but not where a built-in
// takes a callback (a TypeError there, as for any non-function).

// A key list's `function`: Pending while the keys of the proxy it walks
// (its `value`) are not listed yet (for-in, source_vm.goose).
let key_list::Listed = 0;
let key_list::Pending = 1;
// A proxy's internal methods: the seven a Proxy frame runs (its payload),
// then the ones that fail by name (Heap.proxy_method).
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
// The trap of each method a Proxy frame runs.
let TRAP_NAMES: Name[7] = [
    Name { text: "getPrototypeOf" }, Name { text: "ownKeys" }, Name { text: "getOwnPropertyDescriptor" }, Name { text: "get" }, Name { text: "set" }, Name { text: "has" },
    Name { text: "deleteProperty" },
];
// A Proxy frame's phases beyond the enumeration methods' (see above).
let proxy_pc::Descriptor = 6;
let proxy_pc::Accessor = 7;
let proxy_pc::Length = 9;
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
// A proxy call or construction: a callable proxy's fails by name, any other
// is a TypeError (not a function, not a constructor).
fn proxy_invoke(heap: Heap&, f: Value, method: i64) { if proxy_callable(heap, f) { proxy_fault(heap, method); } else if heap.fault == Fault.None { heap.fault = Fault.TypeError; } }
// IsExtensible(target) for a trap's invariants (light-cljs-882,
// integrity.goose): a proxy target's [[IsExtensible]] fails by name, as
// does a form's.
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
// IsCompatiblePropertyDescriptor(extensible, d, current): d is complete;
// `current` is a descriptor or Null.
fn compatible(heap: Heap&, extensible: bool, d: Value, current: Value) -> bool {
    if !current.is_object() { return extensible; }
    if flag_field(heap, current, field::Configurable) { return true; }
    if flag_field(heap, d, field::Configurable) || flag_field(heap, d, field::Enumerable) != flag_field(heap, current, field::Enumerable) { return false; }
    let accessor = has_field(heap, current, field::Get);
    if has_field(heap, d, field::Get) != accessor { return false; }
    if accessor {
        let a, x = field_of(heap, d, field::Get); let b, y = field_of(heap, current, field::Get);
        let c, z = field_of(heap, d, field::Set); let e, w = field_of(heap, current, field::Set);
        return same_value(heap, a, b) && same_value(heap, c, e);
    }
    if flag_field(heap, current, field::Writable) { return true; }
    if flag_field(heap, d, field::Writable) { return false; }
    let v, x = field_of(heap, d, field::Value); let u, y = field_of(heap, current, field::Value);
    return same_value(heap, v, u);
}

// ---- The Proxy frame
// Start internal method `method` of proxy p (key k for all but the
// enumeration methods; [[Get]]'s and [[Set]]'s receiver, [[Set]]'s value,
// and for [[Set]] and [[Delete]] whether a false result throws) in a Proxy
// frame above the top one; its result arrives on the operands. The frame
// reads its trap at its first step (pc 0), so a getter path that starts it
// (call_accessor) does not re-enter one.
fn proxy_start(job: Job&, method: i64, p: Value, k: Value, receiver: Value = Value.Undefined, value: Value = Value.Undefined, throws: bool = false) {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: value, receiver: receiver, input: p, output: k, index: if throws { 1 } else { 0 }, native: Native.Proxy { method: method as u8 } });
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
// GetMethod(handler, trap name) of the frame's proxy.
fn proxy_trap(job: Job&, at: i64, method: i64) {
    let heap .= job.heap;
    let handler = node(heap, job.frames[at].input.id()).namespace_value;
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
    if is_proxy(heap, trap) { proxy_invoke(heap, trap, proxy_method::Call); return; }
    if !callable(heap, trap) { type_error(job, "Proxy: trap is not a function"); return; }
    if method <= proxy_method::GetOwnProperty { job.frames[at].callee = trap; }
    job.frames[at].pc = 2;
    var f = trap; var this_value = node(heap, p.id()).namespace_value;
    // The arguments all[first..count]: the target, then the key for all
    // but the enumeration methods, [[Set]]'s value and the receiver of
    // [[Get]] and [[Set]].
    let frame = job.frames[at];
    let all:   Value[4] = [node(heap, p.id()).value, frame.output, if method == proxy_method::Set { frame.callee } else { frame.receiver }, frame.receiver];
    let count: i64      = if method == proxy_method::Set { 4 } else if method == proxy_method::Get { 3 } else if method == proxy_method::GetPrototypeOf || method == proxy_method::OwnPropertyKeys { 1 } else { 2 };
    var first: i64 = 0;
    // Call(trap, handler, args). Function.prototype.call as the trap calls
    // its receiver with its first argument as `this`, as the dispatch loop's
    // call does; a native-frame built-in runs in its own frame above this
    // one, a plain one completes here.
    loop {
        if is_proxy(heap, f) { proxy_invoke(heap, f, proxy_method::Call); return; }
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
    // Number, String, Symbol and Symbol.for convert an object first
    // argument (the target, always) in a Convert frame: the general call
    // path's Forward frame runs them.
    let converts, hint = argument_converts(heap, f, all[first..count]);
    if converts { start_bound(job, f, this_value, all[first..count], job.operands.len); return; }
    let r = call_builtin(job, node(heap, f.id()).function, this_value, all[first..count]);
    if heap.fault == Fault.None { push_operand(job, r); }
}
// The method of the target: a proxy target's in this frame, from pc 0 at
// its next step; any other target's, now.
fn proxy_forward(job: Job&, at: i64, method: i64) {
    let heap .= job.heap; let frame = job.frames[at];
    let target = node(heap, frame.input.id()).value;
    if is_proxy(heap, target) { job.frames[at].input = target; job.frames[at].pc = 0; return; }
    if method == proxy_method::GetPrototypeOf { finish_native(job, frame, prototype(heap, target.id())); return; }
    if !listable(job, target) { return; }
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
        6 => { property_invariants(job, at, method, job.operands.pop()); },
        // A forwarded getter's value is [[Get]]'s; a setter's completion
        // makes [[Set]] true.
        7 => { let v = job.operands.pop(); finish_native(job, frame, if method == proxy_method::Get { v } else { boolean(true) }); },
        // ([[GetOwnProperty]]) a descriptor field's presence, from a proxy
        // trap result's [[HasProperty]].
        8 => { if descriptor_present(job, at, Builtin.Proxy, 5, truthy(heap, job.operands.pop())) && descriptor_fields(job, at, Builtin.Proxy, 5) { descriptor_done(job, at); } },
        // ([[Set]]) both conversions of an array receiver's new `length`
        // arrived: the write, as Reflect.set's (reflect_length_step).
        9 => { proxy_length(job, at); },
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
// Steps 9 to 21: no duplicates; every non-configurable key of the target
// present; with a non-extensible target, exactly its keys.
fn own_keys_done(job: Job&, at: i64) {
    let heap .= job.heap; let frame = job.frames[at];
    let list: i64 = frame.callee.id(); let n: i64 = node(heap, list).key_count;
    if !charge(heap, n) { return; }
    var words: i64[>..] = [];
    for i in n { words.push(key_word(property_row(heap, node(heap, list).keys + i).key)); }
    sort(words);
    for i in 1..n { if words[i] == words[i - 1] { type_error(job, "Proxy: duplicate key in the ownKeys trap result"); return; } }
    let target = node(heap, frame.input.id()).value;
    let ext = extensible(heap, target);
    if heap.fault != Fault.None { return; }
    var keys: OwnKey[>..] = [];
    if !listable(job, target) || !own_property_keys(heap, target, keys) || !charge(heap, keys.len) { return; }
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
// after the target's own descriptor and extensibility.
fn descriptor_result(job: Job&, at: i64, r: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if !r.is_object() && !r.is_undefined() { type_error(job, "Proxy: getOwnPropertyDescriptor trap result is neither an object nor undefined"); return; }
    let target = node(heap, frame.input.id()).value;
    if is_proxy(heap, target) { unsupported_step(job, Builtin.Proxy, "[[GetOwnProperty]] of a Proxy target in a trap's invariant checks"); return; }
    if !listable(job, target) { return; }
    let td = target_descriptor(job, target, frame.output);
    if heap.fault != Fault.None { return; }
    if r.is_undefined() {
        if td.is_object() {
            if !flag_field(heap, td, field::Configurable) { type_error(job, "Proxy: a non-configurable property of the target is reported missing"); return; }
            if !extensible(heap, target) { type_error(job, "Proxy: a property of a non-extensible target is reported missing"); return; }
        }
        if heap.fault == Fault.None { finish_native(job, frame, Value.Undefined); }
        return;
    }
    let ext = extensible(heap, target);
    let d = new_descriptor(heap);
    if heap.fault != Fault.None { return; }
    node_mut(heap, d.id()).namespace_value = if td.is_object() { td } else { Value.Null };
    job.frames[at].receiver = r; job.frames[at].callee = d; job.frames[at].index = 0; job.frames[at].argument_count = if ext { 1 } else { 0 };
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
fn prototype_result(job: Job&, at: i64, r: Value) {
    let heap .= job.heap; let frame = job.frames[at];
    if !r.is_object() && !r.is_null() { type_error(job, "Proxy: getPrototypeOf trap result is neither an object nor null"); return; }
    let target = node(heap, frame.input.id()).value;
    let ext = extensible(heap, target);
    if heap.fault != Fault.None { return; }
    if !ext && !same_value(heap, r, prototype(heap, target.id())) { type_error(job, "Proxy: the getPrototypeOf trap result differs from a non-extensible target's prototype"); return; }
    finish_native(job, frame, r);
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
// [[DefineOwnProperty]], which fails by name (light-cljs-891).
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
        if !writable(job, target.id()) { heap.fault = Fault.NativeBoundary; return; }
        // A lazy closure's `length` or `name` is made before it goes (delete_op).
        if key_is(heap, k, "length") || key_is(heap, k, "name") { prepare_function(job, target, k, true); }
        let ok = delete_property(heap, target, k);
        if heap.fault == Fault.None { proxy_answer(job, at, method, ok, false); }
        return;
    }
    let setter, ok = receiver_set(job, target, k, frame.callee, frame.receiver);
    if proxy_pending(heap, proxy_method::Set) { let p, x = take_proxy(heap); proxy_restart(job, at, p); return; }
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
    if !ok && job.frames[at].index != 0 {
        if method == proxy_method::Delete { type_error(job, if trap { "Proxy: the deleteProperty trap returned false" } else { "could not delete property" }); }
        else { type_error(job, if trap { "Proxy: the set trap returned false" } else { "Proxy: the target refused the assignment" }); }
        return;
    }
    finish_native(job, job.frames[at], boolean(ok));
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
        let ext = extensible(heap, node(heap, frame.input.id()).value);
        if heap.fault != Fault.None { return; }
        if !ext { type_error(job, if has { "Proxy: a property of a non-extensible target is reported missing" } else { "Proxy: a property of a non-extensible target is reported deleted" }); return; }
    }
    finish_native(job, frame, boolean(!has));
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
