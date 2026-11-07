import std;
import .source_vm;
import .realm;
import .array_callbacks;
import .array_methods;
import .cells;
import .proxy;

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
// again (a Number or String conversion of its first argument, a map demand)
// does so with the frame kept at pc 3. A dense array list (element-run
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
// of a constructor.
fn is_constructor(heap: const Heap&, f: Value) -> bool {
    var t = f;
    while is_proxy(heap, t) { t = node(heap, t.id()).value; }
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
        Object, Function, Array, Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError, Number, String, Boolean, Proxy, SymbolFunction => true,
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
fn start_forward(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    var copied: Value[>..] = []; copied.append(args);
    job.operands.resize(base); job.operands.append(copied);
    let f = object(realm_function_row(b));
    job.frames.push(Frame { function: -1, pc: 3, local_base: job.locals.len, operand_base: base, callee: f, receiver: receiver, input: f, output: number(0), native: Native.Forward { construct: false } });
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
        // new Number(o), new String(o): the first argument converts first,
        // in place, and this step runs again on the primitive.
        let converts, hint = argument_converts(heap, f, job.operands[base..]);
        if converts { start_convert(job, hint, hint == Hint.String, 1, argc); return; }
        // Construct(target, args, newTarget), with target below its arguments.
        job.frames.pop();
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
    // Number(o), String(o): the conversion runs first, in place, and this
    // step runs again on the primitive.
    let converts, hint = argument_converts(heap, f, args);
    if converts { start_convert(job, hint, hint == Hint.String, 1, args.len); return; }
    // vec and into realize a deferred map argument first (demand_call).
    if (b == Builtin.Vec || b == Builtin.Into) && demand_maps(job, args) { return; }
    if heap.fault != Fault.None { return; }
    if b == Builtin.Mapv && args.len == 2 { unsupported_step(job, b, "called through a forwarding built-in"); return; }
    job.frames.pop();
    if forces(heap, b, args) { start_force(job, b, args[0], base); return; }
    if native_builtin(b) { native_start(job, b, receiver, args, base); return; }
    if b == Builtin.Gensym && job.gensym_next > 0 { let v = gensym(job, args); job.operands.resize(base); job.operands.push(v); return; }
    if effect_builtin(b) { request_effect(job, id, args, frame); job.operands.resize(base); return; }
    let v = call_builtin(job, id, receiver, args);
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
