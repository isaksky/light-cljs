import std;
import .vm;

// All programs here are synthetic, hand-assembled fixtures, not emitted JS.
fn num(n: f64) -> tiny::Value { tiny::Value.Number { value: n } }
fn number(v: tiny::Value) -> f64 {
    match v { Number n => { return n.value; }, _ => { abort("expected number"); } }
}
fn same_values(a: const tiny::Value[:], b: const tiny::Value[:]) -> bool {
    if a.len != b.len { return false; }
    for v, i in a { if !tiny::equal(v, b[i]) { return false; } }
    return true;
}
// Instantiate this helper for both integer and floating point storage.
fn same<T>(a: const T[:], b: const T[:]) -> bool {
    if a.len != b.len { return false; }
    for v, i in a { if v != b[i] { return false; } }
    return true;
}
fn rejected(code: const u8[:], constants: const tiny::Value[:], locals: i64, expected: tiny::LoadError, offset: i64) {
    var p = tiny::Program {};
    assert(!tiny::load(p, code, constants, locals));
    assert(!p.valid && p.error == expected && p.error_offset == offset);
}
fn validation() {
    rejected([], [], 0, tiny::LoadError.Empty, 0);
    rejected([0], [], 0, tiny::LoadError.Unknown, 0);
    rejected([2, 255], [], 0, tiny::LoadError.Unknown, 1);
    // Every operand family, and malformed unreachable bytes too.
    for op in [1, 3, 4, 13, 14] {
        var code: u8[>..] = []; code.push(op as u8);
        rejected(code, [], 0, tiny::LoadError.Truncated, 0);
    }
    rejected([1, 0, 15], [], 0, tiny::LoadError.Constant, 0);
    rejected([1, 1, 15], [num(1)], 0, tiny::LoadError.Constant, 0);
    rejected([3, 1, 15], [], 1, tiny::LoadError.Local, 0);
    rejected([2, 4, 1, 2, 15], [], 1, tiny::LoadError.Local, 1);
    rejected([13, 1, 2, 15], [], 0, tiny::LoadError.Branch, 0);
    rejected([13, 4, 2, 15], [], 0, tiny::LoadError.Branch, 0);
    rejected([13, 255, 2, 15], [], 0, tiny::LoadError.Branch, 0);
    rejected([15], [], 0, tiny::LoadError.Underflow, 0);
    rejected([2, 7, 15], [], 0, tiny::LoadError.Underflow, 1);
    rejected([2], [], 0, tiny::LoadError.Fallthrough, 0);
    rejected([2, 14, 0], [], 0, tiny::LoadError.Fallthrough, 1);
    // Both branches reach offset 4 with depths 0 and 1.
    rejected([2, 14, 4, 2, 2, 15], [], 0, tiny::LoadError.Join, 4);
    let object_constants: tiny::Value[1] = [tiny::Value.Object { id: 1 }];
    rejected([1, 0, 15], object_constants, 0, tiny::LoadError.ConstantType, 0);
    rejected([2, 15], [], -1, tiny::LoadError.Limits, 0);
    rejected([2, 15], [], 257, tiny::LoadError.Limits, 0);

    var constants: tiny::Value[>..] = []; constants.resize(256, num(42));
    var p = tiny::Program {};
    // Highest constant and local indices both execute correctly.
    assert(tiny::load(p, [1, 255, 4, 255, 3, 255, 15], constants, 256));
    var job = tiny::Job { program: p }; tiny::initialize(job, []);
    var session = tiny::Session {};
    assert(tiny::run(session, job, -1) == tiny::Outcome.Complete && number(job.result) == 42);
    constants.push(num(43));
    rejected([2, 15], constants, 0, tiny::LoadError.Limits, 0);

    var code: u8[>..] = [2, 13, 255];
    while code.len < 255 { code.push(2); }
    code.push(15);
    var max_address = tiny::Program {};
    assert(tiny::load(max_address, code, [], 0));
    var at_max = tiny::Job { program: max_address }; tiny::initialize(at_max, []);
    assert(tiny::run(session, at_max, -1) == tiny::Outcome.Complete);
    assert(at_max.pc == max_address.instructions.len);
    code.push(2);
    rejected(code, [], 0, tiny::LoadError.Limits, 0);
    var encoded: u8[>..] = [];
    assert(tiny::emit(encoded, 1, 255));
    let saved = copy(encoded);
    assert(!tiny::emit(encoded, 1, 256) && same(encoded[..], saved[..]));
    assert(!tiny::emit(encoded, 3, 256) && same(encoded[..], saved[..]));
    assert(!tiny::emit(encoded, 13, 256) && same(encoded[..], saved[..]));
    assert(!tiny::emit(encoded, 1, -1) && same(encoded[..], saved[..]));
    assert(!tiny::emit(encoded, 256) && same(encoded[..], saved[..]));
    while encoded.len < 256 { assert(tiny::emit(encoded, 2)); }
    assert(!tiny::emit(encoded, 2) && encoded.len == 256);
    print("validation: malformed encoding, control flow, and 255/256 boundaries passed");
}
fn scalar_operations() {
    var p = tiny::Program {};
    // (6 - 2) + (6 - 2) * 2 = 12; includes dup and all numeric arithmetic.
    assert(tiny::load(p, [1, 0, 1, 1, 8, 6, 1, 1, 9, 7, 15], [num(6), num(2)], 0));
    var job = tiny::Job { program: p }; tiny::initialize(job, []);
    var session = tiny::Session {};
    assert(tiny::run(session, job, -1) == tiny::Outcome.Complete && number(job.result) == 12);
    var boolean = tiny::Program {};
    let bool_constants: tiny::Value[1] = [tiny::Value.Bool { value: true }];
    assert(tiny::load(boolean, [2, 12, 1, 0, 11, 6, 5, 15], bool_constants, 0));
    var b = tiny::Job { program: boolean }; tiny::initialize(b, []);
    assert(tiny::run(session, b, -1) == tiny::Outcome.Complete);
    assert(tiny::equal(b.result, tiny::Value.Bool { value: true }));
    assert(!tiny::equal(tiny::Value.Null, tiny::Value.Undefined));
    assert(!tiny::equal(num(0), tiny::Value.Bool { value: false }));
    assert(!tiny::truthy(tiny::Value.Null) && !tiny::truthy(num(0)) && tiny::truthy(num(-1)));
    var numeric_loop = tiny::Program {};
    // i=0; sum=0; while(i<10) { sum=sum+i; i=i+1; } return sum;
    assert(tiny::load(numeric_loop,
        [1, 0, 4, 0, 1, 0, 4, 1, 3, 0, 1, 2, 10, 14, 31, 3, 1, 3, 0, 7, 4, 1, 3, 0, 1, 1, 7, 4, 0, 13, 8, 3, 1, 15],
        [num(0), num(1), num(10)], 2));
    var sum = tiny::Job { program: numeric_loop }; tiny::initialize(sum, []);
    assert(tiny::run(session, sum, -1) == tiny::Outcome.Complete && number(sum.result) == 45);
    assert(sum.effects.len == 0 && numeric_loop.max_stack == 2);
    print("numeric loop: sum 0 through 9 = 45 passed");
}
fn identity() {
    var session = tiny::Session {};
    var p = tiny::Program {};
    assert(tiny::load(p, [16, 4, 0, 3, 0, 15], [], 1));
    var retained: tiny::Value = tiny::Value.Undefined;
    {
        var job = tiny::Job { program: p }; tiny::initialize(job, []);
        assert(tiny::run(session, job, 1) == tiny::Outcome.Yielded);
        let allocated = job.operands[0];
        assert(tiny::run(session, job, -1) == tiny::Outcome.Complete);
        assert(tiny::equal(allocated, job.result) && tiny::equal(job.result, job.locals[0]));
        retained = job.result;
    }
    var later = tiny::Job { program: p }; tiny::initialize(later, []);
    assert(tiny::run(session, later, -1) == tiny::Outcome.Complete);
    assert(!tiny::equal(retained, later.result));
    assert(tiny::equal(retained, tiny::Value.Object { id: 1 }));
    assert(session.next_object == 3);
    var compare = tiny::Program {};
    assert(tiny::load(compare, [16, 6, 11, 16, 16, 11, 12, 11, 15], [], 0));
    var c = tiny::Job { program: compare }; tiny::initialize(c, []);
    assert(tiny::run(session, c, -1) == tiny::Outcome.Complete && tiny::equal(c.result, tiny::Value.Bool { value: true }));
    print("identity: round trip, fresh IDs, and retained result after job disposal passed");
}
fn load_loop(p: tiny::Program&) {
    // i=0; last=undefined; while(i<n) { last=counter(i); i=i+1; } return last;
    assert(tiny::load(p, [1, 0, 4, 1, 2, 4, 2, 3, 1, 3, 0, 10, 14, 28, 3, 1, 17, 4, 2, 3, 1, 1, 1, 7, 4, 1, 13, 7, 3, 2, 15], [num(0), num(1)], 3));
}
fn unchanged_at_zero(session: tiny::Session&, job: tiny::Job&) {
    let pc = job.pc; let status = job.status; let result = job.result;
    let fault = job.fault; let fault_pc = job.fault_pc; let claimed = job.claimed;
    let next_object = session.next_object;
    let locals = copy(job.locals); let operands = copy(job.operands); let effects = copy(job.effects);
    let program .= job.program;
    assert(tiny::run(session, job, 0) == tiny::Outcome.Yielded);
    assert(job.program .== program && job.pc == pc && job.status == status && tiny::equal(job.result, result));
    assert(job.fault == fault && job.fault_pc == fault_pc && job.claimed == claimed && session.next_object == next_object);
    assert(same_values(job.locals, locals) && same_values(job.operands, operands) && same(job.effects[..], effects[..]));
}
fn round_robin(budget: i64) {
    var p = tiny::Program {}; load_loop(p);
    var session = tiny::Session {};
    var a = tiny::Job { program: p }; tiny::initialize(a, [num(3)]);
    var b = tiny::Job { program: p }; tiny::initialize(b, [num(7)]);
    var expected_a = tiny::Job { program: p }; tiny::initialize(expected_a, [num(3)]);
    var expected_b = tiny::Job { program: p }; tiny::initialize(expected_b, [num(7)]);
    assert(tiny::run(session, expected_a, -1) == tiny::Outcome.Complete);
    assert(tiny::run(session, expected_b, -1) == tiny::Outcome.Complete);
    unchanged_at_zero(session, a);
    var rounds = 0;
    while a.status == tiny::Status.Ready || b.status == tiny::Status.Ready {
        if a.status == tiny::Status.Ready { unchanged_at_zero(session, a); tiny::run(session, a, budget); }
        if b.status == tiny::Status.Ready { unchanged_at_zero(session, b); tiny::run(session, b, budget); }
        rounds += 1; assert(rounds < 200);
    }
    assert(rounds > 1 && a.status == tiny::Status.Complete && b.status == tiny::Status.Complete);
    assert(tiny::equal(a.result, expected_a.result) && tiny::equal(b.result, expected_b.result));
    assert(number(a.result) == 3 && number(b.result) == 7);
    assert(same(a.effects[..], expected_a.effects[..]) && same(b.effects[..], expected_b.effects[..]));
    assert(same(a.effects[..], [0.0, 1.0, 2.0][..]) && same(b.effects[..], [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0][..]));
    assert(!a.claimed && !b.claimed);
    print("counter: two round-robin jobs, budget=", budget, ", rounds=", rounds, ", exact ordered effects passed");
}
fn failures_and_claims() {
    var p = tiny::Program {};
    assert(tiny::load(p, [2, 1, 0, 7, 15], [num(1)], 0));
    var job = tiny::Job { program: p }; tiny::initialize(job, []);
    var session = tiny::Session {};
    assert(tiny::claim(job) && !tiny::claim(job));
    assert(tiny::run(session, job, -1) == tiny::Outcome.Busy && job.claimed && job.pc == 0);
    assert(tiny::run_claimed(session, job, -1) == tiny::Outcome.Failed);
    assert(job.status == tiny::Status.Failed && job.fault == tiny::Fault.UnsupportedOperand && job.fault_pc == 2 && !job.claimed);
    let pc = job.pc;
    assert(tiny::run(session, job, -1) == tiny::Outcome.Failed && job.pc == pc && !tiny::claim(job));
    var effect = tiny::Program {};
    assert(tiny::load(effect, [16, 17, 15], [], 0));
    var bad_effect = tiny::Job { program: effect }; tiny::initialize(bad_effect, []);
    assert(tiny::run(session, bad_effect, -1) == tiny::Outcome.Failed && bad_effect.effects.len == 0 && !bad_effect.claimed);
    var good = tiny::Program {};
    assert(tiny::load(good, [1, 0, 15], [num(99)], 0));
    var survivor = tiny::Job { program: good }; tiny::initialize(survivor, []);
    assert(tiny::run(session, survivor, -1) == tiny::Outcome.Complete && number(survivor.result) == 99 && !survivor.claimed);
    var object = tiny::Program {}; assert(tiny::load(object, [16, 15], [], 0));
    session.next_object = 2147483647;
    var last = tiny::Job { program: object }; tiny::initialize(last, []);
    assert(tiny::run(session, last, -1) == tiny::Outcome.Complete);
    assert(tiny::equal(last.result, tiny::Value.Object { id: 2147483647 }));
    var exhausted = tiny::Job { program: object }; tiny::initialize(exhausted, []);
    assert(tiny::run(session, exhausted, -1) == tiny::Outcome.Failed && exhausted.fault == tiny::Fault.ObjectLimit && !exhausted.claimed);
    assert(session.next_object == 2147483648);
    print("ownership: duplicate claim, terminal failure, isolated succeeding job passed");
}
fn main() {
    validation(); scalar_operations(); identity(); round_robin(1); round_robin(5); failures_and_claims();
    print("synthetic Goose VM smoke passed");
}
