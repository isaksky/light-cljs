import std;
import math;
import .source_vm;
import .realm;
import .array_callbacks;
import .proxy;

namespace source;

// Receiver and argument conversions of built-ins (light-cljs-900; decisions
// 2, 4, 5 and 6 of docs/javascript-coverage.md; ECMAScript 7.1; the Pants
// engine's NativeState::Convert targets, rust/src/vm/native.rs).
//
// ToNumber, ToString and their integer forms of an object run its
// @@toPrimitive, valueOf or toString, which is user code, so a built-in
// whose body is a plain Goose function (decision 5) cannot run them. It
// gets them from a *plan* instead: static data (decision 6), a PLANS row
// named by its BuiltinInfo.plan (plan::None, 0, for most built-ins), that
// says what its receiver and each argument position take:
//   this_step  ThisStep.None; String: RequireObjectCoercible, then ToString
//              (String.prototype's methods); Number: thisNumberValue
//              (Number.prototype's methods). It runs before any argument.
//   steps      the Step of each of the first four arguments, in order;
//   rest       the Step of every later argument (Math.max, min and hypot:
//              Number);
//   construct  whether [[Construct]] converts too (new Number(o), new
//              String(o)); otherwise only a call does, so Symbol's
//              construction throws before anything converts.
// A Step names the conversion of its position: Numeric (ToPrimitive with
// hint number, the primitive left for the body: Number(v)'s ToNumeric),
// Number, Integer (ToIntegerOrInfinity), Length, Uint16, Int32 and Uint32
// (all ToNumber first: hint number, then ToNumber), String (hint string,
// then ToString), or None (the body's own, no conversion here).
//
// Every path that calls a built-in asks `coerces` once it knows the
// function object, receiver and arguments, before any other dispatch: the
// dispatch loop's call and construction (call, construct_at,
// construct_super and, through them, spread calls), Function.prototype.call,
// apply, Reflect.apply, Reflect.construct and bound functions (forward_run),
// a callable proxy without an apply or construct trap (proxy_forward_call),
// the callbacks native frames make (builtin_callback, array_call,
// swap_call), ToPrimitive's methods (conversion_call), @@hasInstance
// (has_instance_call), @@iterator (get_iterator_method) and join
// (to_string_step). It is true when a planned position holds an object that
// converts (convertible); then start_coerce pushes one Coerce native frame
// (Native.Coerce, the built-in and whether it constructs its payload) at the
// call's operand base, holding the receiver and the arguments as its
// operands: index 0 is the receiver, index i + 1 argument i. Its step walks
// the positions left to right from the frame's `index`:
//   - a planned object converts in place, in a Convert frame above this
//     one (light-cljs-828: its ToPrimitive with the step's hint, then
//     ToNumber or ToString, except for Numeric), and the step runs again on
//     the primitive;
//   - a planned primitive stays as it is: the body converts primitives
//     itself, so `undefined` keeps meaning an omitted argument where the
//     body gives it that meaning; but one whose conversion throws (a
//     symbol, a BigInt until light-cljs-771, an undefined or null receiver
//     for String, a receiver that is no Number for Number) ends the walk:
//     the body runs at once and throws at that position, since every
//     position before it is primitive by then, and no later valueOf runs.
// Then the frame pops and runs the body on primitives at its operand base,
// as the caller would have (coerced_body: call_plain, or native_start or
// start_conditional_native for a built-in whose body is a frame of its
// own), or, for a construction, `construct` with the frame's newTarget
// (`input`). A call that has no object to convert pushes no frame: the
// body runs at once on the primitives. A built-in without a plan pays one
// table read (BuiltinInfo.plan). So a body converts its planned positions
// in plan order with the plain functions (primitive_number,
// primitive_string, then to_integer_or_infinity, to_int32 and the like).
//
// Conversions that are conditional or interleaved with other observable
// steps are not a plan's: padStart and padEnd convert the fill string only
// when padding is needed, split converts its limit after reading the
// separator's @@split, and startsWith, endsWith and includes run IsRegExp
// (a @@match read) before ToString of the search string. Such a built-in
// plans its unconditional prefix (its receiver and leading arguments) and
// gives the rest Step.None, converting those positions in its own native
// frame's phases with start_convert, as Array.prototype.join does. A step
// that only follows the conversions without running user code
// (toFixed's range check after ToIntegerOrInfinity) is the body's.
//
// RegExp's built-ins (light-cljs-932; regexp_object.goose) plan nothing
// (plan::None): the constructor's ToString of the pattern and the flags
// is conditional (an undefined one is "", a RegExp pattern gives its own
// text, an IsRegExp pattern's are its `source` and `flags`) and follows
// IsRegExp's @@match read, the identity rule's `constructor` read and
// newTarget's `prototype`, so its RegExp frame converts them in phases of
// its own; RegExp.prototype.toString converts what it reads, also in its
// frame; the getters and RegExp[@@species] take no arguments, and the
// getters check their receiver's slot without converting it.
// RegExp.prototype.exec and test (light-cljs-933) check their receiver (a
// RegExp's slot, an object) before ToString of their argument, which a
// plan would run first, and convert lastIndex after reading `exec` and
// lastIndex, so their frame converts both in phases of its own.
// String.prototype.replace and replaceAll and RegExp.prototype[@@replace]
// (light-cljs-934; regexp_replace.goose) plan nothing either: replace's
// ToString of its receiver follows GetMethod(searchValue, @@replace)
// (replaceAll's IsRegExp and `flags` reads before it), which a
// ThisStep.String would run after; its replacement converts only when it
// is not callable; @@replace's argument converts first, but its
// replacement's conversion is conditional too, and every later conversion
// (flags, lastIndex, each result's length, match, index and captures, a
// replacer's result, a named group) follows a read or a call. Their frames
// convert each in a phase of its own.
// Map and Set (light-cljs-943; collections.goose) plan nothing either:
// their keyed methods take keys and values as they are (SameValueZero,
// with no ToPropertyKey) and check their receiver's internal slots without
// converting it, the getters and @@species take no arguments, and the
// constructors convert nothing (their iterable's protocol runs in their
// Collection frame).
//
// The frame's state is its operands and fields (callee the built-in
// function object, input newTarget, argument_count the argument count,
// index the next position, pc 0), so in-job reclamation
// (reclaim.goose), saved continuations (continuations.goose SavedFrame) and
// snapshot validation (snapshot.goose native_valid) see it as any native
// frame's; a budget cut, limit or suspension falls between two
// conversions, never inside one.

enum Step { None, Numeric, Number, Integer, Length, Uint16, Int32, Uint32, String }
enum ThisStep { None, String, Number }
struct Plan { this_step: ThisStep, steps: Step[4], rest: Step, construct: bool }

// The plans, by row of PLANS; a built-in names its row in BuiltinInfo.plan.
let plan::None: u8 = 0;
// Number(v) and new Number(v): ToNumeric of the first argument.
let plan::Number: u8 = 1;
// String(v) and new String(v): ToString of the first argument (a primitive
// symbol is the body's SymbolDescriptiveString).
let plan::String: u8 = 2;
// Symbol(description) and Symbol.for(key): ToString of the first argument,
// when called.
let plan::Symbol: u8 = 3;
// isNaN(number) and isFinite(number): ToNumber.
let plan::OneNumber: u8 = 4;
// parseFloat(string): ToString.
let plan::OneString: u8 = 5;
// parseInt(string, radix): ToString, then ToInt32.
let plan::ParseInt: u8 = 6;
// Number.prototype.toString(radix), toFixed, toExponential and toPrecision:
// thisNumberValue, then ToIntegerOrInfinity (light-cljs-902).
let plan::NumberRadix: u8 = 7;
// Math.atan2(y, x) and Math.pow(base, exponent): ToNumber of both
// (light-cljs-901; math_object.goose).
let plan::TwoNumbers: u8 = 8;
// Math.min, max and hypot: ToNumber of every argument, in order.
let plan::Numbers: u8 = 9;
// Math.imul(x, y): ToUint32 of both; Math.clz32(x): ToUint32.
let plan::TwoUint32: u8 = 10;
let plan::OneUint32: u8 = 11;
// String methods' unconditional receiver conversion; code-unit access
// also converts its position. Searches convert later arguments in their
// native frame, after IsRegExp where required. String's statics use no
// plan: fromCodePoint validates each point before the next converts, and
// raw interleaves property reads with conversions (string_methods.goose).
let plan::StringReceiver: u8 = 12;
let plan::StringPosition: u8 = 13;
// String extraction's two indices, concat's arguments, HTML's attribute.
let plan::StringRange: u8 = 14;
let plan::StringConcat: u8 = 15;
let plan::StringAttribute: u8 = 16;
let PLANS: Plan[17] = [
    Plan { this_step: ThisStep.None, steps: [Step.None, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.Numeric, Step.None, Step.None, Step.None], rest: Step.None, construct: true },
    Plan { this_step: ThisStep.None, steps: [Step.String, Step.None, Step.None, Step.None], rest: Step.None, construct: true },
    Plan { this_step: ThisStep.None, steps: [Step.String, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.Number, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.String, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.String, Step.Int32, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.Number, steps: [Step.Integer, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.Number, Step.Number, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.Number, Step.Number, Step.Number, Step.Number], rest: Step.Number, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.Uint32, Step.Uint32, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.None, steps: [Step.Uint32, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.String, steps: [Step.None, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.String, steps: [Step.Integer, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.String, steps: [Step.Integer, Step.Integer, Step.None, Step.None], rest: Step.None, construct: false },
    Plan { this_step: ThisStep.String, steps: [Step.String, Step.String, Step.String, Step.String], rest: Step.String, construct: false },
    Plan { this_step: ThisStep.String, steps: [Step.String, Step.None, Step.None, Step.None], rest: Step.None, construct: false },
];
inline fn step_at(p: const Plan&, i: i64) -> Step { return if i < p.steps.len { p.steps[i] } else { p.rest }; }
// The plan row of built-in function object f.
inline fn plan_of(heap: const Heap&, f: Value) -> u8 { return BUILTINS[node(heap, f.id()).function].plan; }

// Whether a call of f (a construction when `constructing`) with `receiver`
// and `args` converts an object first, in a Coerce frame (start_coerce).
fn coerces(heap: const Heap&, f: Value, receiver: Value, args: const Value[:], constructing: bool = false) -> bool {
    if !f.is_object() || f.id() >= heap.nodes.len || node(heap, f.id()).kind != node_kind::Builtin { return false; }
    let p = plan_of(heap, f);
    if p == plan::None { return false; }
    let plan .= PLANS[p];
    if constructing && !plan.construct { return false; }
    if plan.this_step == ThisStep.String && convertible(heap, receiver) { return true; }
    for v, i in args { if step_at(plan, i) != Step.None && convertible(heap, v) { return true; } }
    return false;
}
// Start built-in f's conversions (see above) for a call with `receiver`
// and `args`, or a construction with newTarget `new_target`, whose operands
// begin at `base` (native_start's contract: the frame's result goes there).
// args may lie on the operands above base: they are copied first.
fn start_coerce(job: Job&, f: Value, receiver: Value, args: const Value[:], base: i64, constructing: bool = false, new_target: Value = Value.Undefined) {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    if args.len + 1 > heap.limits.stack - job.locals.len - base { heap.fault = Fault.StackLimit; return; }
    var copied: Value[>..] = []; copied.append(args);
    job.operands.resize(base); job.operands.push(receiver); job.operands.append(copied);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: f, receiver: Value.Undefined, argument_count: copied.len, input: new_target, native: Native.Coerce { which: builtin_of(heap, f.id()), construct: constructing } });
}
// Whether the conversion of planned primitive v under `step` throws (the
// body then throws it): a symbol's ToNumber or ToString, a BigInt's.
inline fn conversion_throws(step: Step, v: Value) -> bool { return step != Step.Numeric && (v.is_symbol() || v.is_bigint()); }
// One step of the Coerce frame at the top (see above).
fn coerce_step(job: Job&, frame: Frame, b: Builtin, constructing: bool) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let base = frame.operand_base; let count = frame.argument_count + 1;
    if frame.pc != 0 || job.operands.len != base + count { heap.fault = Fault.InvalidState; return; }
    let plan .= PLANS[plan_of(heap, frame.callee)];
    var i = frame.index;
    while i < count {
        let slot = base + i;
        let v = form_value(heap, job.operands[slot]);
        if i == 0 {
            if plan.this_step == ThisStep.Number && wrapper_type(if v.is_object() && node(heap, v.id()).kind == node_kind::Wrapper { node(heap, v.id()).value } else { v }) != 0 { break; }
            if plan.this_step == ThisStep.String {
                if v.tag() <= value_tag::Null || v.is_symbol() || v.is_bigint() { break; }
                if convertible(heap, v) { job.frames[at].index = i; start_convert(job, Hint.String, true, 1, job.operands.len - slot); return; }
            }
        } else {
            let step = step_at(plan, i - 1);
            if step != Step.None {
                if convertible(heap, v) { job.frames[at].index = i; start_convert(job, if step == Step.String { Hint.String } else { Hint.Number }, step != Step.Numeric, 1, job.operands.len - slot); return; }
                if conversion_throws(step, v) { break; }
            }
        }
        i += 1;
    }
    job.frames.pop();
    if constructing {
        job.operands[base] = frame.callee;
        construct(job, frame.argument_count, frame.input);
        return;
    }
    let receiver = job.operands[base];
    var args: Value[>..] = []; args.append(job.operands[(base + 1)..]);
    coerced_body(job, frame.callee, b, receiver, args, base);
}
// Run built-in f's body (b its Builtin) on converted operands, its result
// for the frame below at `base`.
fn coerced_body(job: Job&, f: Value, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    if native_builtin(b) { native_start(job, b, receiver, args, base); return; }
    if conditional_native(b) { start_conditional_native(job, b, receiver, args, base); return; }
    let v = call_plain(job, f, receiver, args);
    if job.heap.fault == Fault.None { job.operands.resize(base); deliver(job, v); }
}

// The integer conversions a body applies to a ToNumber result (ECMAScript
// 7.1.5 ToIntegerOrInfinity, 7.1.20 ToLength, 7.1.10 ToUint16; ToInt32 and
// ToUint32 are source_vm.goose's to_int32 and to_uint32).
fn to_integer_or_infinity(n: f64) -> f64 {
    if n != n { return 0; }
    let t = trunc(n);
    return if t == 0 { 0 } else { t };
}
fn to_length(n: f64) -> f64 {
    let t = to_integer_or_infinity(n);
    return if t <= 0 { 0 } else { min(t, 9007199254740991.0) };
}
inline fn to_uint16(n: f64) -> i64 { return to_uint32(n) & 65535; }
