import std;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .cells;
import .proxy;
import .templates;

namespace source;

// The forwarding built-ins (light-cljs-851; decisions 4 and 5 of
// docs/javascript-coverage.md): Function.prototype.call and apply,
// Reflect.apply and Reflect.construct. Semantics from the Pants engine's
// rust/src/vm/native.rs (NativeState::Apply), vm/native/reflect.rs
// (Reflect.construct) and vm/dispatch.rs (forward), QuickJS's
// js_function_apply, js_reflect_construct and build_arg_list, with the
// specification's steps (ECMAScript 20.2.3.1, 20.2.3.3, 28.1.1, 28.1.2 and
// 7.3.19 CreateListFromArrayLike).
//
// The dispatch loop's `call` runs `f.call(this, ...args)` itself, a direct
// call of f (its one unwrapped level). Every other reach of a forwarding
// built-in, a nested `call`, any `apply` and `Reflect` call, and one from a
// native frame (a callback, a trap, a ToPrimitive method), starts a Forward
// native frame (native_start) whose whole state is in the frame:
//   callee     the function to run: first the forwarding built-in itself
//   receiver   its this value; for a construction, newTarget
//   input      the argument list object while it is read (pc 0 to 2)
//   output     the forwarding hops so far, as a number
//   argument_count  the list's length, ToLength(Get(list, "length"))
//   index      k, the next index to read
//   operands   the arguments, at operands[operand_base..]
//   pc 0       awaits the length's value (a getter's result, or converted
//              by a Convert frame with hint number)
//   pc 1       reads element k (one a step), a getter's result at pc 2
//   pc 2       awaits element k from its getter
//   pc 3       runs callee: a forwarding built-in is resolved in place
//              (`call` shifts its arguments down by one; apply, Reflect.apply
//              and Reflect.construct read a new list from pc 0), anything
//              else is called (or, under Forward { construct: true },
//              constructed) with the arguments, as the dispatch loop's call
//              and construct run it
// The frame pops before the target runs: a bytecode target's frame takes
// its place, so a forwarded call is a tail call, and the result reaches the
// frame below at operand_base. A built-in target that would run its call
// again (a map demand) does so with the frame kept at pc 3; one that
// converts its receiver or arguments (light-cljs-900) runs in a Coerce
// frame in this one's place. A dense array list (element-run
// mode, its realm prototype) is read whole at its start, with no observable
// access; an arguments object, an ordinary object or an array in any other
// mode takes the specification's [[Get]] of each index, a getter running
// above the frame. A list longer than 65,535 is a RangeError before any
// element is read (QuickJS's "too many arguments", the spread limit of
// light-cljs-850), and the arguments also obey the value limit
// (StackLimit). Every hop counts against the frame limit, so a forwarding
// cycle (an array holding apply and itself) ends as the catchable stack
// overflow.
//
// Not implemented yet, each a named UnsupportedOperand (decision 1): a
// light-cljs runtime value or BigInt as the length, mapv over two
// arguments and a host-effect built-in reached through a forward frame
// that cannot save its continuation, and the constructions the dispatch
// loop's construct fails by name.

// Whether b forwards a call (native_builtin).
inline fn forwarding(b: Builtin) -> bool { return b == Builtin.FunctionPrototypeCall || b == Builtin.FunctionPrototypeApply || b == Builtin.ReflectApply || b == Builtin.ReflectConstruct; }
// The most arguments an argument list gives (QuickJS's JS_MAX_LOCAL_VARS).
inline fn max_list_arguments() -> i64 { return 65535; }

// IsCallable: a closure, a built-in function or a proxy of a callable
// target (a class constructor is callable; calling it throws TypeError).
fn is_callable(heap: const Heap&, f: Value) -> bool { return callable(heap, f) || is_proxy(heap, f) && proxy_callable(heap, f); }
// IsConstructor: an ordinary function or a class (not an arrow, method or
// generator); a built-in with [[Construct]] (constructor_builtin); a proxy
// or a bound function of a constructor (a target is an earlier row, so the
// walk ends).
fn is_constructor(heap: const Heap&, f: Value) -> bool {
    var t = f;
    while is_proxy(heap, t) || is_bound(heap, t) { t = node(heap, t.id()).value; }
    if !t.is_object() { return false; }
    let kind = node(heap, t.id()).kind;
    if kind == node_kind::Closure { return constructor_kind(heap, t.id()) != constructor::None; }
    return kind == node_kind::Builtin && constructor_builtin(builtin_of(heap, t.id()));
}
// Whether built-in b has [[Construct]]: the realm's constructors (Symbol's
// throws TypeError when `new` reaches it, but it is a newTarget), and the
// light-cljs runtime's leaves, which the V8 build constructs as ordinary
// JavaScript functions and construct fails by name. `new` of any other
// built-in is a TypeError (construct).
fn constructor_builtin(b: Builtin) -> bool {
    return match b {
        Object, Function, Array, Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError, Number, String, Boolean, Proxy, SymbolFunction, RegExp, Map, Set, WeakMap, WeakSet => true,
        // The V8 build's identical and range are arrow functions, no constructors
        // (light-cljs-876, light-cljs-877).
        Identical, Range => false,
        _ => runtime_leaf(b),
    };
}
// Whether b is one of the light-cljs runtime's leaves (RUNTIME_LEAVES).
fn runtime_leaf(b: Builtin) -> bool {
    for leaf in RUNTIME_LEAVES { if leaf.which == b { return true; } }
    return false;
}

// Start forwarding built-in b called with `receiver` and `args` (which may
// view the operands), its operand base `base` (native_start): a Forward
// frame at pc 3 whose callee is b's function object.
fn start_forward(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) { start_bound(job, object(realm_function_row(b)), receiver, args, base); }
// Start a call of bound function f (from start_forward, a forwarding
// built-in) with `receiver` and `args`, its operand base `base`: a Forward
// frame at pc 3 whose callee is f. Under `constructs` it is a [[Construct]]
// and `receiver` is newTarget.
fn start_bound(job: Job&, f: Value, receiver: Value, args: const Value[:], base: i64, constructs: bool = false) {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    if args.len > heap.limits.stack - job.locals.len - base { heap.fault = Fault.StackLimit; return; }
    var copied: Value[>..] = []; copied.append(args);
    job.operands.resize(base); job.operands.append(copied);
    job.frames.push(Frame { function: -1, pc: 3, local_base: job.locals.len, operand_base: base, callee: f, receiver: receiver, input: f, output: number(0), native: Native.Forward { construct: constructs } });
}
// One bound hop of the Forward frame at `at` (BoundFunction [[Call]] and
// [[Construct]], ECMAScript 10.4.1.1 and 10.4.1.2): the callee becomes the
// target, the bound arguments go before the frame's, and the receiver
// becomes the bound `this`, or, constructing, newTarget becomes the target
// when it was the bound function. A hop counts against the frame limit, and
// the arguments against the value limit (StackLimit), as in the Pants
// engine; there is no 65,535-argument cutoff (QuickJS checks only its
// stack). Returns false on a fault.
fn bound_hop(job: Job&, at: i64, constructs: bool) -> bool {
    let heap .= job.heap; let base = job.frames[at].operand_base;
    let f = job.frames[at].callee;
    let hops = job.frames[at].output.num() + 1;
    if hops > heap.limits.frames - job.frames.len { heap.fault = Fault.StackLimit; return false; }
    job.frames[at].output = number(hops);
    let n = node(heap, f.id());
    let count: i64 = n.count; let start: i64 = n.start;
    if count > heap.limits.stack - job.locals.len - job.operands.len { heap.fault = Fault.StackLimit; return false; }
    if count > 0 {
        // A work unit for each argument moved up and each bound one copied.
        let top = job.operands.len;
        if !charge(heap, top - base + count) { return false; }
        job.operands.resize(top + count, Value.Undefined);
        var i = top - 1;
        while i >= base { job.operands[i + count] = job.operands[i]; i -= 1; }
        for k in count { job.operands[base + k] = slot(heap, start + k); }
    }
    if !constructs { job.frames[at].receiver = n.namespace_value; }
    else if job.frames[at].receiver.is_object() && job.frames[at].receiver.id() == f.id() { job.frames[at].receiver = n.value; }
    job.frames[at].callee = n.value;
    return true;
}
// One step of the Forward frame at the top (see above).
fn forward_step(job: Job&, frame: Frame, constructs: bool) {
    let heap .= job.heap; let at = job.frames.len - 1;
    match frame.pc {
        0 => {
            let len, done = length_value(job, if constructs { Builtin.ReflectConstruct } else { Builtin.FunctionPrototypeApply });
            if !done { return; }
            if len > max_list_arguments() { job.detail.clear(); for c in "too many arguments" { job.detail.push(c); } heap.fault = Fault.RangeError; return; }
            job.frames[at].argument_count = len as i64; job.frames[at].pc = 1;
        },
        1 => {
            let k = frame.index;
            if k >= frame.argument_count { job.frames[at].pc = 3; return; }
            let o = frame.input;
            if node(heap, o.id()).kind == node_kind::Array && !node(heap, o.id()).initialized && node(heap, o.id()).proto.is_hole() {
                let v, _ = own_element(heap, o, k);
                if heap.fault != Fault.None { return; }
                if push_operand(job, v) { job.frames[at].index += 1; }
                return;
            }
            let key = index_key(heap, k);
            if heap.fault != Fault.None { return; }
            let v, getter = lookup(heap, o, key);
            if heap.fault != Fault.None { return; }
            if getter.is_object() { job.frames[at].pc = 2; run_getter(job, Builtin.FunctionPrototypeApply, getter, o); return; }
            if push_operand(job, v) { job.frames[at].index += 1; }
        },
        2 => { job.frames[at].index += 1; job.frames[at].pc = 1; },
        _ => { forward_run(job, at, constructs); },
    }
}
// pc 3: resolve the forwarding built-ins in place, then run the target.
fn forward_run(job: Job&, at: i64, constructs: bool) {
    let heap .= job.heap; let base = job.frames[at].operand_base;
    loop {
        if is_bound(heap, job.frames[at].callee) { if !bound_hop(job, at, constructs) { return; } continue; }
        if constructs { break; }
        let f = job.frames[at].callee;
        if !f.is_object() || node(heap, f.id()).kind != node_kind::Builtin { break; }
        let b = builtin_of(heap, f.id());
        if !forwarding(b) { break; }
        // A hop: a forwarding cycle ends at the frame limit.
        let hops = job.frames[at].output.num() + 1;
        if hops > heap.limits.frames - job.frames.len { heap.fault = Fault.StackLimit; return; }
        job.frames[at].output = number(hops);
        let count = job.operands.len - base;
        let a0 = if count > 0 { job.operands[base] } else { Value.Undefined };
        let a1 = if count > 1 { job.operands[base + 1] } else { Value.Undefined };
        let a2 = if count > 2 { job.operands[base + 2] } else { Value.Undefined };
        let this_value = job.frames[at].receiver;
        if b == Builtin.FunctionPrototypeCall {
            // call(thisArg, ...args): IsCallable(this), then the call.
            if !is_callable(heap, this_value) { type_error(job, "not a function"); return; }
            job.frames[at].callee = this_value; job.frames[at].receiver = a0;
            if count > 0 { for i in base..(job.operands.len - 1) { job.operands[i] = job.operands[i + 1]; } job.operands.pop(); }
            continue;
        }
        var target = this_value; var receiver = a0; var list = a1;
        if b == Builtin.FunctionPrototypeApply {
            if !is_callable(heap, target) { type_error(job, "not a function"); return; }
            if list.tag() <= value_tag::Null {
                job.operands.resize(base);
                job.frames[at].callee = target; job.frames[at].receiver = receiver;
                continue;
            }
        } else if b == Builtin.ReflectApply {
            target = a0; receiver = a1; list = a2;
            if !is_callable(heap, target) { type_error(job, "not a function"); return; }
        } else {
            // Reflect.construct(target, argumentsList, newTarget = target).
            target = a0; list = a1; receiver = if count > 2 { a2 } else { a0 };
            if !is_constructor(heap, target) || !is_constructor(heap, receiver) { type_error(job, "not a constructor"); return; }
        }
        if !list.is_object() { type_error(job, "an argument list must be an object"); return; }
        job.operands.resize(base);
        job.frames[at].callee = target; job.frames[at].receiver = receiver; job.frames[at].input = list; job.frames[at].index = 0;
        if b == Builtin.ReflectConstruct { job.frames[at].native = Native.Forward { construct: true }; }
        if !begin_list(job, at, list) { return; }
        return;
    }
    let frame = job.frames[at];
    let f = frame.callee; let receiver = frame.receiver;
    let argc = job.operands.len - base;
    if constructs {
        // Construct(target, args, newTarget), with target below its arguments.
        job.frames.pop();
        // new Number(o), new String(o): a Coerce frame converts the
        // arguments, then constructs (light-cljs-900).
        if coerces(heap, f, Value.Undefined, job.operands[base..], true) { start_coerce(job, f, Value.Undefined, job.operands[base..], base, true, receiver); return; }
        job.operands.push(Value.Undefined);
        var i = job.operands.len - 1;
        while i > base { job.operands[i] = job.operands[i - 1]; i -= 1; }
        job.operands[base] = f;
        construct(job, argc, receiver);
        return;
    }
    let kind: i64 = if f.is_object() { node(heap, f.id()).kind } else { node_kind::None };
    if kind != node_kind::Builtin {
        job.frames.pop();
        if push_frame(job, f, receiver, job.operands[base..], false, base) { job.operands.resize(base); }
        return;
    }
    let b = builtin_of(heap, f.id()); let id: i64 = node(heap, f.id()).function;
    let args = job.operands[base..];
    // A built-in that converts its receiver or arguments runs in a Coerce
    // frame in this one's place (light-cljs-900).
    if coerces(heap, f, receiver, args) { job.frames.pop(); start_coerce(job, f, receiver, args, base); return; }
    // vec and into realize a deferred map argument first (demand_call).
    if (b == Builtin.Vec || b == Builtin.Into) && demand_maps(job, args) { return; }
    if heap.fault != Fault.None { return; }
    if b == Builtin.Mapv && args.len == 2 { unsupported_step(job, b, "called through a forwarding built-in"); return; }
    job.frames.pop();
    if forces(heap, b, args) { start_force(job, b, args[0], base); return; }
    if b == Builtin.FunctionPrototypeToString { start_function_text(job, receiver, base); return; }
    if conditional_native(b) { start_conditional_native(job, b, receiver, args, base); return; }
    if native_builtin(b) { native_start(job, b, receiver, args, base); return; }
    if b == Builtin.Gensym && job.gensym_next > 0 { let v = gensym(job, args); job.operands.resize(base); job.operands.push(v); return; }
    if effect_builtin(b) { request_effect(job, id, args, frame); job.operands.resize(base); return; }
    // A revoke function reads its own row (light-cljs-891; proxy.goose).
    let v = call_plain(job, f, receiver, args);
    if heap.fault == Fault.None { job.operands.resize(base); deliver(job, v); }
}
// CreateListFromArrayLike(list) into the Forward frame at `at`: a dense
// array's elements at once (pc 3), any other object from Get(list,
// "length") (pc 0). Returns false on a fault.
fn begin_list(job: Job&, at: i64, list: Value) -> bool {
    let heap .= job.heap;
    let n = node(heap, list.id());
    if n.kind == node_kind::Array && !n.initialized && n.proto.is_hole() {
        let count: i64 = n.length;
        if count > max_list_arguments() { job.detail.clear(); for c in "too many arguments" { job.detail.push(c); } heap.fault = Fault.RangeError; return false; }
        if count > heap.limits.stack - job.locals.len - job.operands.len { heap.fault = Fault.StackLimit; return false; }
        if !charge(heap, count) { return false; }
        for i in count {
            let v, _ = own_element(heap, list, i);
            if heap.fault != Fault.None { return false; }
            job.operands.push(v);
        }
        job.frames[at].argument_count = count; job.frames[at].index = count; job.frames[at].pc = 3;
        return true;
    }
    job.frames[at].pc = 0;
    get_named(job, Builtin.FunctionPrototypeApply, list, "length");
    return heap.fault == Fault.None;
}

// ---- Bound functions (light-cljs-852)
// Function.prototype.bind and bound function exotic objects (ECMAScript
// 20.2.3.2, 10.4.1; the Pants engine's vm/function.rs BoundFunction and
// bind_with_metadata, QuickJS's js_function_bind and
// js_call_bound_function). A bound function is node_kind::BoundFunction
// (source_vm.goose gives its fields). Every call of one, from the dispatch
// loop's Call, CallMethod, Construct and spread calls, from call, apply
// and Reflect, and from the native frames that call a callback (map and
// forEach, mapv, ToPrimitive, join's toString, @@hasInstance, @@iterator,
// proxy traps, swap!, a delay's function), starts a Forward frame
// (start_bound) whose bound_hop steps replace it by its target before the
// target runs as any forwarded call does: a frame push, never Goose
// recursion. `new` of one is a TypeError unless its target is a
// constructor (is_constructor).
//
// bind runs as a Bind native frame (decision 5), since the target's
// `length` and `name` reads can run a getter:
//   callee     the target
//   receiver   the new bound function, made at the start
//   output     its `length`, from pc 1
//   pc 0       HasOwnProperty(target, "length"), then Get of it (a getter
//              runs above the frame); its value arrives at pc 1
//   pc 1       L = max(0, ToIntegerOrInfinity(length) - bound count) for a
//              Number, else 0; then Get(target, "name"), arriving at pc 2
//   pc 2       defines `length` and then `name` ("bound " + the name if it
//              is a string, else "bound "), each non-writable,
//              non-enumerable and configurable, and delivers the function.
// A proxy target's [[GetPrototypeOf]] and [[GetOwnProperty]] fail by name
// (proxy.goose); a getter that is no closure runs on the general call path
// (run_getter, call_accessor; light-cljs-881).

// Whether f is a bound function exotic object.
inline fn is_bound(heap: const Heap&, f: Value) -> bool { return f.is_object() && node(heap, f.id()).kind == node_kind::BoundFunction; }
// A bound function's [[Prototype]], its target's: the target's own link, or
// for a default one in a realm, the kind's default (a generator or async
// closure's is not %Function.prototype%, light-cljs-857).
fn bound_prototype(heap: const Heap&, target: Value) -> Value {
    let p = node(heap, target.id()).proto;
    return if p.is_hole() && heap.global.is_object() && node(heap, target.id()).kind == node_kind::Closure { prototype(heap, target.id()) } else { p };
}
// bind called with `target` as its this value and `args` (thisArg, then the
// bound arguments), its operand base `base`: BoundFunctionCreate, then the
// Bind frame for the metadata.
fn start_bind(job: Job&, target: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    if !is_callable(heap, target) { type_error(job, "not a function"); return; }
    if is_proxy(heap, target) { proxy_fault(heap, proxy_method::GetPrototypeOf); return; }
    let first = min(args.len, 1); let count = args.len - first;
    if !values_fit(heap, count) || !nodes_fit(heap, 1) { memory(heap); return; }
    if !charge(heap, count) { return; }
    let at = add_values(heap, args[first..]);
    if at < 0 { if heap.fault == Fault.None { memory(heap); } return; }
    let f = allocate(heap, Node { kind: node_kind::BoundFunction, start: at as u32, count: count as u32, value: target, namespace_value: arg(args, 0), proto: bound_prototype(heap, target) });
    if heap.fault != Fault.None { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: target, receiver: f, output: number(0), native: Native.Bind });
}
// One step of the Bind frame at the top (see above).
fn bind_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1; let b = Builtin.FunctionPrototypeBind;
    match frame.pc {
        0 => {
            let k = key_named(heap, "length");
            if heap.fault != Fault.None { return; }
            let own = own_entry(heap, frame.callee, k);
            if heap.fault != Fault.None { return; }
            job.frames[at].pc = 1;
            if !own.found { push_operand(job, Value.Undefined); return; }
            let v, getter = lookup(heap, frame.callee, k);
            if heap.fault != Fault.None { return; }
            if getter.is_object() { run_getter(job, b, getter, frame.callee); return; }
            push_operand(job, v);
        },
        1 => {
            let v = job.operands.pop();
            var l: f64 = 0;
            if v.is_number() {
                let bound: f64 = node(heap, frame.receiver.id()).count;
                let d = trunc(v.num()) - bound;
                if d > 0 { l = d; }
            }
            job.frames[at].output = number(l); job.frames[at].pc = 2;
            get_named(job, b, frame.callee, "name");
        },
        _ => {
            let name = job.operands.pop();
            let prefix = text(heap, "bound ");
            if heap.fault != Fault.None { return; }
            let full = if name.is_string() { concat_mixed(heap, prefix, name) } else { prefix };
            let length = key_named(heap, "length"); let key = key_named(heap, "name");
            if heap.fault != Fault.None { return; }
            let o = frame.receiver.id();
            if !add_own(heap, o, length, frame.output, attr_configurable()) || !add_own(heap, o, key, full, attr_configurable()) { if heap.fault == Fault.None { memory(heap); } return; }
            finish_native(job, frame, frame.receiver);
        },
    }
}

// ---- Function.prototype.toString and %ThrowTypeError% (light-cljs-853)
// Semantics from QuickJS (7d1c13c): js_function_toString (quickjs.c
// 42191) and js_throw_type_error (16343), with ECMAScript 20.2.3.5 and
// 10.2.4.1. The Pants engine implements neither (rust/src/vm/function.rs
// FUNCTION_PROTOTYPE_NAMES), so QuickJS is the oracle.
//
// toString of a function compiled from source is its source text
// ([[SourceText]], QuickJS's debug.source): the byte range the Rust
// frontend records for it (GSP version 4, Function.has_text), decoded from
// the image's immutable source (UTF-8, with run-time source's lone
// surrogates as WTF-8: program::source_text) to the UTF-16 string returned; Goose
// never re-parses source. Any other function (a built-in, a bound function,
// a proxy of a function, a function without text) gives QuickJS's
// "function " + ToString(Get(F, "name")) + "() {\n    [native code]\n}",
// with "function *", "async function " or "async function *" for a
// generator, async or async generator closure without text. A receiver
// that is not callable is a TypeError. The name's [[Get]] can run a getter
// and its ToString a user method, so the dispatch loop, a Forward frame and
// a Convert frame run it as the FunctionText native frame
// (start_function_text); run to completion (function_text, call_builtin's)
// such a name fails by name, as a proxy's [[Get]] does everywhere.
//
// %ThrowTypeError% is a realm built-in (decision 6) installed as the getter
// and setter of Function.prototype's `caller` and `arguments` accessors
// (configurable, not enumerable), with `length` 0 and `name` "" both
// non-configurable and no other property: QuickJS seals it, so the realm
// builds it non-extensible (builtin_metadata; a write or definition of a
// new key is refused and IsExtensible answers false, light-cljs-882). Called, it throws TypeError
// "invalid property access", except with no argument and a sloppy ordinary
// function (a closure of a non-strict `function` declaration or expression,
// QuickJS's has_prototype) as its receiver, where it returns undefined: so
// reading `f.caller` or `f.arguments` of such a function gives undefined,
// QuickJS's ES5 compatibility, and any write throws. It runs no JavaScript,
// so a property read or write reaching it as an accessor half runs it in
// place (accessor_builtin), never as a frame.

// Whether f is the realm's %ThrowTypeError%.
fn is_throw_type_error(heap: const Heap&, f: Value) -> bool {
    return f.is_object() && node(heap, f.id()).kind == node_kind::Builtin && builtin_of(heap, f.id()) == Builtin.ThrowTypeError;
}
// %ThrowTypeError% called with `receiver` and `argc` arguments.
fn throw_type_error(job: Job&, receiver: Value, argc: i64) -> Value {
    let heap .= job.heap;
    if argc == 0 && receiver.is_object() && node(heap, receiver.id()).kind == node_kind::Closure {
        let f .= heap.code.functions[node(heap, receiver.id()).function];
        if !f.strict && f.kind == constructor::Ordinary { return Value.Undefined; }
    }
    return type_error(job, "invalid property access");
}
// Run accessor half `f` in place when it is a built-in that runs no
// JavaScript (%ThrowTypeError%), with `receiver` and `argc` arguments:
// true and its result (or a fault); false for any other function, which
// the caller runs as a frame or fails by name.
fn accessor_builtin(job: Job&, f: Value, receiver: Value, argc: i64) -> bool, Value {
    if !is_throw_type_error(job.heap, f) { return false, Value.Undefined; }
    return true, throw_type_error(job, receiver, argc);
}
// Call accessor half f (a getter with no arguments, a setter with its
// value) with receiver o on the general call path (light-cljs-881,
// decision 4): %ThrowTypeError% in place (accessor_builtin), a closure as
// its frame, and any other callable (a built-in, a bound function) through
// a Forward frame (start_bound), which runs a plain built-in to completion,
// a native-frame built-in as its frame and a bound function's target. Its
// result arrives on the operands of the frame at the top when the call
// began, as a getter frame's return value does. A callable proxy's [[Call]]
// runs as its Proxy frame (light-cljs-891, proxy.goose proxy_call).
fn call_accessor(job: Job&, f: Value, o: Value, args: const Value[:]) {
    let heap .= job.heap;
    let done, v = accessor_builtin(job, f, o, args.len);
    if done { if heap.fault == Fault.None { push_operand(job, v); } return; }
    if !f.is_object() || f.id() >= heap.nodes.len { heap.fault = Fault.InvalidState; return; }
    let kind = node(heap, f.id()).kind;
    if kind == node_kind::Closure { push_frame(job, f, o, args, false, job.operands.len); return; }
    // lookup's proxy hit runs the proxy's [[Get]] for the receiver
    // (proxy.goose); any other proxy is a callable proxy's [[Call]].
    if kind == node_kind::Proxy { if args.len == 0 && is_hit(heap, f) { proxy_get(job, f, o); } else { proxy_call(job, f, o, args, job.operands.len, false); } return; }
    if kind != node_kind::Builtin && kind != node_kind::BoundFunction { heap.fault = Fault.TypeError; return; }
    start_bound(job, f, o, args, job.operands.len);
}
// The source text of closure f, if it has one: true and the string.
fn source_text(job: Job&, f: Value) -> bool, Value {
    let heap .= job.heap;
    if node(heap, f.id()).kind != node_kind::Closure { return false, Value.Undefined; }
    let code .= heap.code.functions[node(heap, f.id()).function];
    if !code.has_text { return false, Value.Undefined; }
    if !charge(heap, code.text_end - code.text_start) { return true, Value.Undefined; }
    // The loader and snapshot validation checked the range and the source's
    // encoding (program::source_text): a lone surrogate of run-time source
    // decodes to its exact unit (light-cljs-864).
    var units: u16[>..] = [];
    if !append_utf8(units, heap.code.source_bytes[code.text_start..code.text_end]) { heap.fault = Fault.InvalidState; return true, Value.Undefined; }
    return true, string(heap, units);
}
// The native form's prefix for function f: QuickJS's by function kind for
// a closure (one without source text), "function " for any other.
fn native_prefix(heap: const Heap&, f: Value) -> const u8[:] {
    if node(heap, f.id()).kind != node_kind::Closure { return "function "; }
    let code .= heap.code.functions[node(heap, f.id()).function];
    return if code.generator && code.asynchronous { "async function *" } else if code.generator { "function *" } else if code.asynchronous { "async function " } else { "function " };
}
// The native form of function f with `name`, a primitive: undefined gives
// "", a string itself, any other primitive its ToString (a Symbol's
// TypeError).
fn native_text(job: Job&, f: Value, name: Value) -> Value {
    let heap .= job.heap;
    var units: u16[>..] = [];
    for c in native_prefix(heap, f) { units.push(c); }
    if name.is_string() { append_utf16(heap, name, units); }
    else if !name.is_undefined() {
        var bytes: u8[>..] = [];
        if !primitive_text(heap, name, bytes) {
            if heap.fault == Fault.TypeError && job.detail.len == 0 { for c in "Cannot convert a Symbol value to a string" { job.detail.push(c); } }
            return Value.Undefined;
        }
        for c in bytes { units.push(c); }
    }
    for c in "() {\n    [native code]\n}" { units.push(c); }
    return string(heap, units);
}
// Function.prototype.toString with `receiver` as its this value, run to
// completion (call_builtin: a callback or a host's call). A name behind a
// getter or an object name needs the FunctionText frame, which only the
// dispatch loop, a forwarded call and a ToPrimitive method start; here it
// fails by name.
fn function_text(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap; let b = Builtin.FunctionPrototypeToString;
    if !is_callable(heap, receiver) { return type_error(job, "not a function"); }
    let has, text = source_text(job, receiver);
    if has { return text; }
    let k = key_named(heap, "name");
    if heap.fault != Fault.None { return Value.Undefined; }
    let name, getter = lookup(heap, receiver, k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if getter.is_object() || name.is_object() { return unsupported_step(job, b, "a name getter or object name outside a native frame"); }
    return native_text(job, receiver, name);
}
// Function.prototype.toString called from the dispatch loop, a Forward
// frame or a Convert frame, its operand base `base`: the source text at
// once, or else the FunctionText frame (decision 4), whose fields are
//   callee     the function
//   pc 0       awaits Get(F, "name") (a getter runs above the frame); an
//              object converts by ToString in a Convert frame above it
//              (hint string, light-cljs-828), and this step runs again on
//              the string; a primitive gives the native form.
fn start_function_text(job: Job&, receiver: Value, base: i64) {
    let heap .= job.heap;
    if !is_callable(heap, receiver) { type_error(job, "not a function"); return; }
    let has, text = source_text(job, receiver);
    if heap.fault != Fault.None { return; }
    if has { job.operands.resize(base); deliver(job, text); return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: receiver, receiver: Value.Undefined, native: Native.FunctionText });
    get_named(job, Builtin.FunctionPrototypeToString, receiver, "name");
}
// One step of the FunctionText frame at the top (see above).
fn function_text_step(job: Job&, frame: Frame) {
    let heap .= job.heap;
    let name = job.operands.pop();
    if name.is_object() {
        if !convertible(heap, name) { unsupported_step(job, Builtin.FunctionPrototypeToString, "a light-cljs runtime value as the name"); return; }
        job.operands.push(name); start_convert(job, Hint.String, true, 1, 1); return;
    }
    let v = native_text(job, frame.callee, name);
    if heap.fault == Fault.None { finish_native(job, frame, v); }
}
