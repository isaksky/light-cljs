import std;
import binary;
import math;
import .source_vm;
import .realm;
import .array_methods;
import .function_calls;
import .object_statics;

namespace source;

// The Math object (light-cljs-901; decisions 3, 5 and 6 of
// docs/javascript-coverage.md; ECMAScript 21.3; the Pants engine's
// rust/src/vm/builtin.rs math, vm/native.rs MathHypot and MathRandom and
// vm/native/sum_precise.rs; QuickJS's js_math_funcs).
//
// Math is the fixed realm row math_row(), an ordinary object inheriting
// %Object.prototype% (realm.goose build_realm): its 37 functions in
// QuickJS's order, writable and configurable, its @@toStringTag "Math"
// (configurable only) and its eight value properties, static data
// (MATH_CONSTANTS), neither writable, enumerable nor configurable.
//
// Every function but random and sumPrecise has a conversion plan
// (coerce.goose: plan::OneNumber, TwoNumbers, Numbers, OneUint32,
// TwoUint32), so an object argument's ToPrimitive has run in a Coerce frame
// before math_builtin, which converts the primitives in argument order
// (primitive_number: a symbol's TypeError, a BigInt's named failure) and
// computes; min, max and hypot convert every argument, even after a NaN or
// an infinity, and charge one work unit an argument (hypot two: its
// conversion and its scaled sum), all in one step (light-cljs-913 budgets
// long calls in steps, as the Pants engine does). The results are the
// Pants engine's: floor, ceil, trunc, abs, sqrt, and the
// platform's libm for the transcendental functions, as both oracles use on
// this machine; Goose's `math` module has sin to pow, and the rest (sinh,
// cosh, tanh, asinh, acosh, atanh, expm1, log1p and cbrt) are libm's own
// through math_libm.h, which every VM program's sidecar links. `goose run`
// (TinyCC, the --debug runs without `build`) has no libm, so there each of
// those fails by name instead of answering an approximation. pow is
// Number::exponentiate (`**`, light-cljs-829); round rounds half up; fround
// and f16round round to binary32 and binary16 (ties to even, directly from
// binary64); log2 is exact for powers of two.
//
// Math.random is the Pants engine's stream (splitmix64 over a 64-bit state
// with a fixed seed of zero), held in Heap.random: a state field, so the
// worker reset restores the image's state after every job and a job's
// sequence never depends on its worker's history (decision 3), and a
// snapshot or saved continuation carries it, so a resumed continuation and
// the job's promise jobs continue its stream (bbp-325).
//
// Math.sumPrecise runs the iterator protocol, which runs user code, so it
// is a SumPrecise native frame (decision 5) whose steps visit at most one
// element: GetIterator (source_vm.goose get_iterator), then per element
// `next` (a frame above, or a built-in), its result's `done` and `value`
// (getters above), and the value added to the exact sum. A value that is
// not a Number is a TypeError that closes the iterator (IteratorClose: its
// `return` is read and called, and whatever it does the TypeError is
// rethrown), as the 2^53 - 1 element limit's RangeError, checked first, does;
// an exception from `next`, `done` or `value` does not close it. The frame:
//   callee  the iterator record, from pc Record on
//   input   next()'s result while its `done` and `value` are read; while
//           the iterator closes, the exception
//   output  the exact sum (sum_layout): a job array of Number elements no
//           JavaScript reaches, the Pants engine's fixed-point totals in
//           units of 2^-1074 as 32-bit limbs, positive and negative
//           apart, and a flags element (sum_flag::)
//   index   the elements added
//   pc      the value awaited (sum_pc::), which arrives on the operands
// It is frame state and a job row, so in-job reclamation keeps and moves
// it (reclaim.goose marks every frame's values), a saved continuation
// carries it (continuations.goose SavedFrame) and snapshot validation
// checks it (snapshot.goose sum_frame_valid).

// The platform's libm functions Goose's `math` module lacks (math_libm.h).
extern fn libm_available() -> bool = "goose_math_libm";
extern fn libm_sinh(x: f64) -> f64 = "goose_math_sinh";
extern fn libm_cosh(x: f64) -> f64 = "goose_math_cosh";
extern fn libm_tanh(x: f64) -> f64 = "goose_math_tanh";
extern fn libm_asinh(x: f64) -> f64 = "goose_math_asinh";
extern fn libm_acosh(x: f64) -> f64 = "goose_math_acosh";
extern fn libm_atanh(x: f64) -> f64 = "goose_math_atanh";
extern fn libm_expm1(x: f64) -> f64 = "goose_math_expm1";
extern fn libm_log1p(x: f64) -> f64 = "goose_math_log1p";
extern fn libm_cbrt(x: f64) -> f64 = "goose_math_cbrt";

// A binary64's bits and the binary64 of bits.
fn f64_bits(x: f64) -> u64 { let one: f64[1] = [x]; let b, ok = binary::read_u64le(bytes_of(one), 0); assert(ok); return b; }
fn bits_f64(b: u64) -> f64 { let one: u64[1] = [b]; let x, ok = binary::read_f64le(bytes_of(one), 0); assert(ok); return x; }

// ---- The plain functions
// Math's function b of `args` (the planned positions primitives by now).
fn math_builtin(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    match b {
        MathRandom => { return number(next_random(heap)); },
        MathMin, MathMax, MathHypot => { return math_variadic(job, b, args); },
        _ => {},
    }
    let x = primitive_number(job, b, arg(args, 0));
    if heap.fault != Fault.None { return Value.Undefined; }
    match b {
        MathAtan2, MathPow, MathImul => {
            let y = primitive_number(job, b, arg(args, 1));
            if heap.fault != Fault.None { return Value.Undefined; }
            if b == Builtin.MathImul { let p = ((to_uint32(x) as u64) * (to_uint32(y) as u64)) & 4294967295; let r: f64 = signed32(p as i64); return number(r); }
            return number(if b == Builtin.MathPow { exponentiate(x, y) } else { atan2(x, y) });
        },
        MathSinh, MathCosh, MathTanh, MathAsinh, MathAcosh, MathAtanh, MathExpm1, MathLog1p, MathCbrt => {
            if !libm_available() { return unsupported_step(job, b, "the platform's libm, which goose run does not link"); }
            return number(libm_unary(b, x));
        },
        _ => { return number(math_unary(b, x)); },
    }
}
// The one-argument functions Goose computes itself or through its `math`
// module.
fn math_unary(b: Builtin, x: f64) -> f64 {
    return match b {
        MathAbs => magnitude(x), MathFloor => floor(x), MathCeil => ceil(x), MathTrunc => trunc(x), MathSqrt => sqrt(x),
        MathRound => math_round(x), MathSign => if x != x || x == 0 { x } else if x > 0 { 1.0 } else { -1.0 },
        MathAcos => acos(x), MathAsin => asin(x), MathAtan => atan(x), MathCos => cos(x), MathSin => sin(x), MathTan => tan(x),
        MathExp => exp(x), MathLog => log(x), MathLog2 => math_log2(x), MathLog10 => log10(x),
        MathFround => fround(x), MathF16round => f16round(x),
        MathClz32 => { let n: u64 = to_uint32(x) as u64; let z: f64 = clz(n) - 32; z },
        _ => 0.0 / 0.0,
    };
}
fn libm_unary(b: Builtin, x: f64) -> f64 {
    return match b {
        MathSinh => libm_sinh(x), MathCosh => libm_cosh(x), MathTanh => libm_tanh(x), MathAsinh => libm_asinh(x), MathAcosh => libm_acosh(x), MathAtanh => libm_atanh(x),
        MathExpm1 => libm_expm1(x), MathLog1p => libm_log1p(x), _ => libm_cbrt(x),
    };
}
// |x| with its sign bit clear, as C's fabs: +0 for -0 (std's abs keeps -0).
inline fn magnitude(x: f64) -> f64 { return if x <= 0 { 0.0 - x } else { x }; }
// Math.round (ECMAScript 21.3.2.28): half up, -0 kept and given for
// (-0.5, -0]; never x + 0.5, which double-rounds 0.5's predecessor (the
// Pants engine's).
fn math_round(x: f64) -> f64 {
    if !(x - x == 0) || x == 0 { return x; }
    let f = floor(x);
    let r = if x - f < 0.5 { f } else { f + 1 };
    return if r == 0 { if x < 0 { -0.0 } else { 0.0 } } else { r };
}
// Math.log2: exact for powers of two, subnormals included; the platform's
// log2 otherwise (the Pants engine's math_log2).
fn math_log2(x: f64) -> f64 {
    if x > 0 && x - x == 0 {
        let bits = f64_bits(x); let e: i64 = (bits >> 52) as i64; let fraction = bits & 4503599627370495;
        if e != 0 && fraction == 0 { let r: f64 = e - 1023; return r; }
        if e == 0 && popcount(fraction) == 1 { let r: f64 = ctz(fraction) - 1074; return r; }
    }
    return log2(x);
}
// Math.fround: binary32, ties to even.
fn fround(x: f64) -> f64 { let f: f32 = x as f32; let r: f64 = f; return r; }
// Math.f16round: binary16, rounded directly from binary64 (an intermediate
// binary32 could round onto a binary16 midpoint), ties to even; every NaN
// is NaN (the Pants engine's object/typed_array.rs round_binary16).
fn f16round(x: f64) -> f64 {
    let bits = f64_bits(x);
    let negative = (bits >> 63) != 0;
    let e: i64 = ((bits >> 52) & 2047) as i64;
    if e == 2047 { return if (bits & 4503599627370495) != 0 { 0.0 / 0.0 } else { x }; }
    let exponent = e - 1023;
    if exponent < -25 { return if negative { -0.0 } else { 0.0 }; }
    if exponent > 15 { return if negative { -1.0 / 0.0 } else { 1.0 / 0.0 }; }
    let significand: u64 = (bits & 4503599627370495) | 4503599627370496;
    // Subnormals have a fixed 2^-24 quantum; normals keep eleven
    // significant bits.
    let shift: u64 = if exponent < -14 { (28 - exponent) as u64 } else { 42 };
    let one:   u64 = 1;
    let truncated = significand >> shift; let remainder = significand & ((one << shift) - 1); let halfway = one << (shift - 1);
    let rounded = truncated + (if remainder > halfway || remainder == halfway && (truncated & 1) != 0 { one } else { 0 });
    // A carry moves into the exponent, at the subnormal and the largest
    // finite boundaries too.
    let magnitude: u64 = if exponent < -14 { rounded } else { (((exponent + 14) as u64) << 10) + rounded };
    let field = (magnitude >> 10) & 31; let fraction = magnitude & 1023;
    var r: f64 = 0;
    if field == 0 { let f: f64 = fraction; r = f / 16777216.0; }
    else if field == 31 { r = 1.0 / 0.0; }
    else { r = bits_f64(((field + 1008) << 52) | (fraction << 42)); }
    return if negative { -r } else { r };
}
// Math.min, max and hypot: every argument converted in order, each charged
// to the work limit (hypot also its scaled sum).
fn math_variadic(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if !charge(heap, if b == Builtin.MathHypot { 2 * args.len } else { args.len }) { return Value.Undefined; }
    // min and max: NaN wins; -0 is below +0 (ECMAScript 21.3.2.24 and 25).
    // hypot: the largest magnitude, NaN ignored as Rust's f64::max does.
    var m: f64 = if b == Builtin.MathMin { 1.0 / 0.0 } else if b == Builtin.MathMax { -1.0 / 0.0 } else { 0.0 };
    for v in args {
        let n = primitive_number(job, b, v);
        if heap.fault != Fault.None { return Value.Undefined; }
        if b == Builtin.MathHypot { let a = magnitude(n); if a > m { m = a; } continue; }
        let maximum = b == Builtin.MathMax;
        if m != m || n != n { m = 0.0 / 0.0; }
        else if n == 0 && m == 0 {
            let nn = (f64_bits(n) >> 63) != 0; let mn = (f64_bits(m) >> 63) != 0;
            let negative = if maximum { nn && mn } else { nn || mn };
            m = if negative { -0.0 } else { 0.0 };
        } else if maximum && n > m || !maximum && n < m { m = n; }
    }
    if b != Builtin.MathHypot { return number(m); }
    // An infinity wins over a NaN (ECMAScript 21.3.2.18); then the sum of
    // squares scaled by the largest magnitude, with Kahan's correction (the
    // Pants engine's MathHypot).
    if !(m - m == 0) { return number(1.0 / 0.0); }
    var sum: f64 = 0; var correction: f64 = 0;
    for v in args {
        let n = numeric(heap, form_value(heap, v));
        if n == 0 { continue; }
        let scaled = n / m;
        let term = scaled * scaled - correction;
        let total = sum + term;
        correction = (total - sum) - term;
        sum = total;
    }
    return number(m * sqrt(sum));
}
// The next number of the job's Math.random stream: splitmix64, the state
// advanced once a call, the top 53 bits of its mix scaled into [0, 1).
fn next_random(heap: Heap&) -> f64 {
    let gamma: u64 = 11400714819323198485;
    heap.random = heap.random + gamma;
    var z = heap.random;
    z = (z ^ (z >> 30)) * 13787848793156543929;
    z = (z ^ (z >> 27)) * 10723151780598845931;
    z = z ^ (z >> 31);
    let top: f64 = z >> 11;
    return top * (1.0 / 9007199254740992.0);
}

// ---- Math.sumPrecise
// The value a SumPrecise frame awaits (its pc).
let sum_pc::Iterable: i64 = 0;
let sum_pc::Record: i64 = 1;
let sum_pc::Step: i64 = 2;
let sum_pc::Done: i64 = 3;
let sum_pc::Item: i64 = 4;
let sum_pc::Close: i64 = 5;
let sum_pc::Return: i64 = 6;
let sum_pc::Closed: i64 = 7;
// The exact sum's elements: a finite binary64 is at most 2^2098 units of
// 2^-1074 and at most 2^53 - 1 of them are added, so 68 limbs of 32 bits
// hold either total; then the flags.
let sum_layout::Limbs: i64 = 68;
let sum_layout::Flags: i64 = 136;
let sum_layout::Size: i64 = 137;
let sum_flag::NaN: u64 = 1;
let sum_flag::PositiveInfinity: u64 = 2;
let sum_flag::NegativeInfinity: u64 = 4;
// A value other than -0 was added: the empty sum and a sum of -0s are -0.
let sum_flag::Nonzero: u64 = 8;
let sum_flag::All: u64 = 15;
// The element limit, 2^53 - 1.
let sum_layout::MaxElements: i64 = 9007199254740991;

// Start Math.sumPrecise(items) as a SumPrecise frame whose operand base is
// `base` (native_start's contract): GetIterator runs from its first step,
// which takes the iterable from its operands.
fn start_sum_precise(job: Job&, items: Value, base: i64) {
    let heap .= job.heap;
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    let start = add_values(heap, [], sum_layout::Size, number(0));
    if start < 0 { if heap.fault == Fault.None { memory(heap); } return; }
    let count = sum_layout::Size;
    let sum = allocate(heap, Node { kind: node_kind::Array, start: start as u32, count: count as u32, length: count as u32 });
    if heap.fault != Fault.None { return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: sum_pc::Iterable, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: Value.Undefined, output: sum, native: Native.SumPrecise });
    push_operand(job, items);
}
// One step of the SumPrecise frame at the top: the awaited value arrived.
fn sum_precise_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if job.operands.len != frame.operand_base + 1 { heap.fault = Fault.InvalidState; return; }
    let v = job.operands.pop();
    match frame.pc {
        sum_pc::Iterable => { job.frames[at].pc = sum_pc::Record; get_iterator(job, v); },
        sum_pc::Record => {
            if !v.is_object() { heap.fault = Fault.InvalidState; return; }
            job.frames[at].callee = v;
            sum_next(job, at);
        },
        sum_pc::Step => { sum_result(job, at, v); },
        sum_pc::Done => { sum_done(job, at, truthy(heap, form_value(heap, v))); },
        sum_pc::Item => { sum_item(job, at, v); },
        sum_pc::Close => { sum_close(job, at, v); },
        sum_pc::Return => { sum_return(job, at, v); },
        sum_pc::Closed => { sum_rethrow(job, at); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// IteratorStepValue: call `next`; its result arrives at pc Step.
fn sum_next(job: Job&, at: i64) {
    let heap .= job.heap; let record = job.frames[at].callee;
    let it = record_field(job, record, "iterator");
    let next = record_field(job, record, "next");
    if heap.fault != Fault.None { return; }
    if !is_callable(heap, next) { type_error(job, "iterator next is not a function"); return; }
    job.frames[at].pc = sum_pc::Step; job.frames[at].input = Value.Undefined;
    call_accessor(job, next, it, []);
}
// `next`'s result r: an object, whose `done` is read (a getter above).
fn sum_result(job: Job&, at: i64, r: Value) {
    let heap .= job.heap;
    if !r.is_object() { type_error(job, "iterator result is not an object"); return; }
    job.frames[at].input = r;
    let ready, done = read_field(job, at, Builtin.MathSumPrecise, r, "done", sum_pc::Done);
    if ready { sum_done(job, at, truthy(heap, form_value(heap, done))); }
}
// IteratorComplete: the sum is delivered, or the item's `value` read.
fn sum_done(job: Job&, at: i64, done: bool) {
    let heap .= job.heap;
    if done {
        let frame = job.frames[at];
        let total = sum_finish(heap, frame.output);
        if heap.fault == Fault.None { finish_native(job, frame, number(total)); }
        return;
    }
    let ready, v = read_field(job, at, Builtin.MathSumPrecise, job.frames[at].input, "value", sum_pc::Item);
    if ready { sum_item(job, at, v); }
}
// Item v: past the element limit a RangeError, not a Number a TypeError,
// either closing the iterator (the frame's Catch handler at pc Close
// receives it); a Number is added, and the next element asked for.
fn sum_item(job: Job&, at: i64, v: Value) {
    let heap .= job.heap;
    let x = form_value(heap, v);
    if heap.fault != Fault.None { return; }
    job.frames[at].input = Value.Undefined;
    // The limit is checked before the value's type (the Pants engine's
    // order).
    if job.frames[at].index >= sum_layout::MaxElements {
        close_guard(job, at, sum_pc::Close);
        job.detail.clear(); for c in "too many elements" { job.detail.push(c); }
        heap.fault = Fault.RangeError; return;
    }
    if !x.is_number() { close_guard(job, at, sum_pc::Close); type_error(job, "not a number"); return; }
    job.frames[at].index += 1;
    sum_add(heap, job.frames[at].output, x.num());
    if heap.fault != Fault.None { return; }
    sum_next(job, at);
}
// IteratorClose after an abrupt completion: the exception e arrived at the
// frame's handler. `return` is read and called with a handler of its own
// (pc Closed); whatever it does, e is rethrown.
fn sum_close(job: Job&, at: i64, e: Value) {
    let heap .= job.heap;
    job.frames[at].input = e;
    let it = record_field(job, job.frames[at].callee, "iterator");
    if heap.fault != Fault.None { return; }
    close_guard(job, at, sum_pc::Closed);
    let ready, m = read_field(job, at, Builtin.MathSumPrecise, it, "return", sum_pc::Return);
    if heap.fault != Fault.None { return; }
    if ready { sum_return(job, at, m); }
}
fn sum_return(job: Job&, at: i64, method: Value) {
    let heap .= job.heap;
    let m = form_value(heap, method);
    if heap.fault != Fault.None { return; }
    if m.tag() <= value_tag::Null || !is_callable(heap, m) { sum_rethrow(job, at); return; }
    let it = record_field(job, job.frames[at].callee, "iterator");
    if heap.fault != Fault.None { return; }
    job.frames[at].pc = sum_pc::Closed;
    call_accessor(job, m, it, []);
}
// Rethrow the exception that closed the iterator.
fn sum_rethrow(job: Job&, at: i64) {
    close_unguard(job, at, sum_pc::Closed);
    let frame = job.frames[at];
    raise(job, frame.input, frame);
}

// The exact sum's limb or flags element i of sum array `sum`.
fn sum_word(heap: const Heap&, sum: Value, i: i64) -> u64 {
    let v = slot(heap, node(heap, sum.id()).start + i);
    return v.num() as u64;
}
fn set_sum_word(heap: Heap&, sum: Value, i: i64, w: u64) { let f: f64 = w; set_slot(heap, node(heap, sum.id()).start + i, number(f)); }
// Add Number x to the sum: its significand at its exponent's place in the
// total of its sign, carried up through the limbs.
fn sum_add(heap: Heap&, sum: Value, x: f64) {
    let bits = f64_bits(x);
    let negative = (bits >> 63) != 0;
    var flags = sum_word(heap, sum, sum_layout::Flags);
    if bits != 9223372036854775808 { flags = flags | sum_flag::Nonzero; }
    if !(x - x == 0) {
        if x != x { flags = flags | sum_flag::NaN; } else if negative { flags = flags | sum_flag::NegativeInfinity; } else { flags = flags | sum_flag::PositiveInfinity; }
        set_sum_word(heap, sum, sum_layout::Flags, flags); return;
    }
    set_sum_word(heap, sum, sum_layout::Flags, flags);
    let e:           i64 = ((bits >> 52) & 2047) as i64;
    let significand: u64 = (bits & 4503599627370495) | (if e == 0 { 0 } else { 4503599627370496 });
    if significand == 0 { return; }
    let shift: i64 = if e == 0 { 0 } else { e - 1 };
    let total: i64 = if negative { sum_layout::Limbs } else { 0 };
    let s:     u64 = (shift % 32) as u64; var at = shift / 32;
    let mask:  u64 = 4294967295;
    let lo = significand & mask; let hi = significand >> 32;
    let parts: u64[3] = if s == 0 { [lo, hi, 0] } else { [(lo << s) & mask, ((hi << s) | (lo >> (32 - s))) & mask, hi >> (32 - s)] };
    var carry: u64 = 0; var i: i64 = 0;
    while i < 3 || carry != 0 {
        if at >= sum_layout::Limbs { heap.fault = Fault.InvalidState; return; }
        let w = sum_word(heap, sum, total + at) + (if i < 3 { parts[i] } else { 0 }) + carry;
        set_sum_word(heap, sum, total + at, w & mask); carry = w >> 32;
        at += 1; i += 1;
    }
}
// The sum, rounded once to binary64 (ties to even): NaN for a NaN or both
// infinities, an infinity for one, else the difference of the totals; -0
// when nothing but -0 was added, and an infinity only when that rounding
// overflows.
fn sum_finish(heap: const Heap&, sum: Value) -> f64 {
    let flags = sum_word(heap, sum, sum_layout::Flags);
    if (flags & sum_flag::NaN) != 0 || (flags & sum_flag::PositiveInfinity) != 0 && (flags & sum_flag::NegativeInfinity) != 0 { return 0.0 / 0.0; }
    if (flags & sum_flag::PositiveInfinity) != 0 { return 1.0 / 0.0; }
    if (flags & sum_flag::NegativeInfinity) != 0 { return -1.0 / 0.0; }
    var p: u64[68] = [0; 68]; var q: u64[68] = [0; 68];
    for i in sum_layout::Limbs { p[i] = sum_word(heap, sum, i); q[i] = sum_word(heap, sum, sum_layout::Limbs + i); }
    var negative = false; var k = sum_layout::Limbs - 1;
    while k >= 0 { if p[k] != q[k] { negative = q[k] > p[k]; break; } k -= 1; }
    // The larger total minus the smaller.
    var m:      u64[68] = if negative { q } else { p }; let small: u64[68] = if negative { p } else { q };
    var borrow: u64     = 0;
    for i in sum_layout::Limbs {
        let b = small[i] + borrow;
        if m[i] >= b { m[i] = m[i] - b; borrow = 0; } else { m[i] = m[i] + 4294967296 - b; borrow = 1; }
    }
    var high = sum_layout::Limbs - 1; while high >= 0 && m[high] == 0 { high -= 1; }
    if high < 0 { return if (flags & sum_flag::Nonzero) == 0 { -0.0 } else { 0.0 }; }
    let sign: u64 = if negative { 9223372036854775808 } else { 0 };
    let length = high * 32 + 64 - clz(m[high]);
    // Below 2^52 units the sum is a subnormal: its units are its bits.
    if length <= 52 { return bits_f64(sign | m[0] | (m[1] << 32)); }
    let shift = length - 53;
    var significand: u64 = 0;
    let one: u64 = 1;
    for j in 53 { if sum_bit(m, shift + j) { significand = significand | (one << (j as u64)); } }
    if shift > 0 && sum_bit(m, shift - 1) {
        var sticky = false; var j: i64 = 0;
        while !sticky && j < shift - 1 { if sum_bit(m, j) { sticky = true; } j += 1; }
        if sticky || (significand & 1) != 0 { significand += 1; }
    }
    var exponent = length - 52;
    if significand == 9007199254740992 { significand = significand >> 1; exponent += 1; }
    if exponent >= 2047 { return bits_f64(sign | 9218868437227405312); }
    return bits_f64(sign | ((exponent as u64) << 52) | (significand & 4503599627370495));
}
inline fn sum_bit(m: const u64[:], i: i64) -> bool { return ((m[i / 32] >> ((i % 32) as u64)) & 1) != 0; }
// Whether sum array `sum` of a saved SumPrecise frame holds an exact sum:
// every limb an integer of 32 bits, the flags known (snapshot.goose).
fn sum_words_valid(values: const Value[:]) -> bool {
    if values.len != sum_layout::Size { return false; }
    for v, i in values {
        if !v.is_number() { return false; }
        let n = v.num(); let top: f64 = if i == sum_layout::Flags { 15.0 } else { 4294967295.0 };
        if !(n >= 0 && n <= top) || trunc(n) != n { return false; }
    }
    return true;
}
