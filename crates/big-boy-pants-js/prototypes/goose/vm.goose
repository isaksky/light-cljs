import std;

namespace tiny;

// Fixed-size tagged values: object identity never contains a Goose reference.
enum Value { Undefined, Null, Bool { value: bool }, Number { value: f64 }, Object { id: i64 } }
enum LoadError { None, Limits, Empty, Unknown, Truncated, Constant, Local, Branch, Underflow, Join, Fallthrough, ConstantType, Version }
enum Status { Ready, Complete, Failed }
enum Fault { None, UnsupportedOperand, ObjectLimit, InvalidState }
enum Outcome { Yielded, Complete, Failed, Busy }

struct Instruction { op: u8, arg: i64 = 0, offset: i64, depth: i64 = -1 }
root struct Program {
    instructions: Instruction[>..] = [],
    constants:    Value[>..]       = [],
    locals:       i64              = 0,
    max_stack:    i64              = 0,
    valid:        bool             = false,
    error:        LoadError        = LoadError.None,
    error_offset: i64              = 0,
}
root struct Job {
    program:  const Program&,
    locals:   Value[>..]  = [],
    operands: Value[>..<] = [],
    effects:  f64[>..]    = [],
    pc:       i64         = 0,
    status:   Status      = Status.Ready,
    result:   Value       = Value.Undefined,
    fault:    Fault       = Fault.None,
    fault_pc: i64         = -1,
    claimed:  bool        = false,
}
struct Session { next_object: i64 = 1 }

fn width(op: u8) -> i64 { if op == 1 || op == 3 || op == 4 || op == 13 || op == 14 { if op == 13 || op == 14 { 5 } else { 3 } } else { 1 } }
fn known(op: i64) -> bool { return (op >= 1 && op <= 17) || (op >= 33 && op <= 40) || (op >= 73 && op <= 77) || op == 112; }
fn pops(op: u8) -> i64 {
    if (op >= 7 && op <= 11) || op == 33 || op == 34 || (op >= 37 && op <= 40) || (op >= 73 && op <= 77) { return 2; }
    if op == 35 || op == 36 || op == 112 { return 1; }
    if op == 4 || op == 5 || op == 6 || op == 12 || op == 14 || op == 15 || op == 17 { return 1; }
    return 0;
}
fn pushes(op: u8) -> i64 {
    if op == 6 || op == 37 { return 2; }
    if (op >= 33 && op <= 40) || (op >= 73 && op <= 77) || op == 112 { return 1; }
    if op == 1 || op == 2 || op == 3 || (op >= 7 && op <= 12) || op == 16 || op == 17 { return 1; }
    return 0;
}
fn reject(p: Program&, error: LoadError, offset: i64) -> bool {
    p.error = error; p.error_offset = offset; return false;
}

// Version 2: u16 indices and u32 absolute byte addresses; check before narrowing.
fn emit(code: u8[>..]&, op: i64, arg: i64 = 0) -> bool {
    if !known(op) || arg < 0 || arg > (if op == 13 || op == 14 { 4294967295 } else { 65535 }) { return false; }
    let byte = op as u8;
    if width(byte) == 1 && arg != 0 { return false; }
    if code.len + width(byte) > 16777216 { return false; }
    code.push(byte);
    for i in (width(byte) - 1) { code.push(((arg >> (8 * i)) & 255) as u8); }
    return true;
}

// Load into a fresh owner. The caller must keep it immutable while jobs borrow it.
fn load(p: Program&, code: const u8[:], constants: const Value[:], locals: i64, version: i64 = 2) -> bool {
    assert(p.instructions.len == 0 && p.constants.len == 0 && !p.valid);
    if version != 2 { return reject(p, LoadError.Version, 0); }
    if code.len > 16777216 || constants.len > 65536 || locals < 0 || locals > 65536 { return reject(p, LoadError.Limits, 0); }
    if code.len == 0 { return reject(p, LoadError.Empty, 0); }
    for value in constants {
        if match value { Object => true, _ => false } { return reject(p, LoadError.ConstantType, 0); }
    }
    p.constants.append(constants); p.locals = locals;
    var starts: i64[>..] = []; starts.resize(code.len, -1);
    var pos = 0;
    while pos < code.len {
        let op = code[pos];
        if !known(op) { return reject(p, LoadError.Unknown, pos); }
        let size = width(op);
        if pos + size > code.len { return reject(p, LoadError.Truncated, pos); }
        var arg: i64 = 0;
        for i in (size - 1) { arg |= (code[pos + 1 + i] as i64) << (8 * i); }
        if op == 1 && arg >= constants.len { return reject(p, LoadError.Constant, pos); }
        if (op == 3 || op == 4) && arg >= locals { return reject(p, LoadError.Local, pos); }
        starts[pos] = p.instructions.len;
        p.instructions.push(Instruction { op: op, arg: arg, offset: pos });
        pos += size;
    }
    for &ins in p.instructions {
        if ins.op == 13 || ins.op == 14 {
            if ins.arg >= code.len || starts[ins.arg] < 0 { return reject(p, LoadError.Branch, ins.offset); }
            ins.arg = starts[ins.arg];
        }
    }
    var work: i64[>..<] = [0];
    p.instructions[0].depth = 0;
    while work.len > 0 {
        let index = work.pop();
        let ins = p.instructions[index];
        if ins.depth < pops(ins.op) { return reject(p, LoadError.Underflow, ins.offset); }
        let after = ins.depth - pops(ins.op) + pushes(ins.op);
        if after > p.max_stack { p.max_stack = after; }
        if ins.op == 15 { continue; }
        let count = if ins.op == 14 { 2 } else { 1 };
        for edge in count {
            let next = if (ins.op == 13 || ins.op == 14) && edge == 0 { ins.arg } else { index + 1 };
            if next >= p.instructions.len { return reject(p, LoadError.Fallthrough, ins.offset); }
            let depth = p.instructions[next].depth;
            if depth >= 0 && depth != after { return reject(p, LoadError.Join, p.instructions[next].offset); }
            if depth < 0 { p.instructions[next].depth = after; work.push(next); }
        }
    }
    p.valid = true;
    return true;
}

fn initialize(job: Job&, args: const Value[:]) {
    assert(job.program.valid && job.locals.len == 0 && job.operands.len == 0 && job.pc == 0 && !job.claimed);
    job.locals.resize(job.program.locals, Value.Undefined);
    for value, i in args { if i < job.locals.len { job.locals[i] = value; } }
}
fn truthy(v: Value) -> bool {
    return match v {
        Undefined, Null => false,
        Bool b => b.value,
        Number n => n.value != 0.0 && n.value == n.value,
        Object => true,
    };
}
fn equal(a: Value, b: Value) -> bool {
    return match a {
        Undefined => match b { Undefined => true, _ => false },
        Null => match b { Null => true, _ => false },
        Bool x => match b { Bool y => x.value == y.value, _ => false },
        Number x => match b { Number y => x.value == y.value, _ => false },
        Object x => match b { Object y => x.id == y.id, _ => false },
    };
}
fn claim(job: Job&) -> bool {
    if job.claimed || job.status != Status.Ready { return false; }
    job.claimed = true; return true;
}
fn fail(job: Job&, fault: Fault) {
    job.fault = fault; job.fault_pc = job.pc - 1; job.status = Status.Failed;
}
// Shared primitive arithmetic for bootstrap and source-derived frames.
fn number_binary(op: i64, x: f64, y: f64) -> Value {
    return match op {
        7, 73 => Value.Number { value: x + y },
        8 => Value.Number { value: x - y },
        9 => Value.Number { value: x * y },
        33 => Value.Number { value: x / y },
        34 => Value.Number { value: x % y },
        38, 75 => Value.Bool { value: x > y },
        39, 76 => Value.Bool { value: x <= y },
        40, 77 => Value.Bool { value: x >= y },
        _ => Value.Bool { value: x < y },
    };
}
fn step(session: Session&, job: Job&) {
    let p .= job.program;
    if !p.valid || job.pc < 0 || job.pc >= p.instructions.len || job.locals.len != p.locals { fail(job, Fault.InvalidState); return; }
    let ins = p.instructions[job.pc];
    job.pc += 1;
    if job.operands.len < pops(ins.op) { fail(job, Fault.InvalidState); return; }
    match ins.op {
        1 => { job.operands.push(p.constants[ins.arg]); },
        2 => { job.operands.push(Value.Undefined); },
        3 => { job.operands.push(job.locals[ins.arg]); },
        4 => { job.locals[ins.arg] = job.operands.pop(); },
        5 => { job.operands.pop(); },
        6 => { let v = job.operands[job.operands.len - 1]; job.operands.push(v); },
        7, 8, 9, 10, 33, 34, 38, 39, 40, 73, 74, 75, 76, 77 => {
            let b = job.operands.pop(); let a = job.operands.pop();
            match a {
                Number x => match b {
                    Number y => {
                        let v = number_binary(ins.op, x.value, y.value);
                        job.operands.push(v);
                    },
                    _ => { fail(job, Fault.UnsupportedOperand); },
                },
                _ => { fail(job, Fault.UnsupportedOperand); },
            }
        },
        35, 36, 112 => {
            let v = job.operands.pop();
            match v { Number n => { job.operands.push(Value.Number { value: if ins.op == 35 { -n.value } else { n.value } }); }, _ => { fail(job, Fault.UnsupportedOperand); } }
        },
        37 => {
            let b = job.operands.pop(); let a = job.operands.pop();
            if (match a { Number => true, _ => false }) && (match b { Number => true, _ => false }) { job.operands.push(a); job.operands.push(b); } else { fail(job, Fault.UnsupportedOperand); }
        },
        11 => { let b = job.operands.pop(); let a = job.operands.pop(); job.operands.push(Value.Bool { value: equal(a, b) }); },
        12 => { let v = job.operands.pop(); job.operands.push(Value.Bool { value: !truthy(v) }); },
        13 => { job.pc = ins.arg; },
        14 => { let v = job.operands.pop(); if !truthy(v) { job.pc = ins.arg; } },
        15 => { job.result = job.operands.pop(); job.status = Status.Complete; },
        16 => {
            if session.next_object > 2147483647 { fail(job, Fault.ObjectLimit); return; }
            let id = session.next_object; session.next_object += 1;
            job.operands.push(Value.Object { id: id });
        },
        17 => {
            let arg = job.operands.pop();
            match arg {
                Number n => {
                    job.effects.push(n.value);
                    job.operands.push(Value.Number { value: job.effects.len });
                },
                _ => { fail(job, Fault.UnsupportedOperand); },
            }
        },
        _ => { fail(job, Fault.InvalidState); },
    }
}
// The caller can hold a claim to test reentrant admission, then consume it here.
fn run_claimed(session: Session&, job: Job&, budget: i64) -> Outcome {
    if !job.claimed { return Outcome.Busy; }
    defer { job.claimed = false; }
    var used = 0;
    while job.status == Status.Ready && (budget < 0 || used < budget) {
        step(session, job); used += 1;
    }
    return match job.status { Ready => Outcome.Yielded, Complete => Outcome.Complete, Failed => Outcome.Failed };
}
fn run(session: Session&, job: Job&, budget: i64) -> Outcome {
    if job.claimed { return Outcome.Busy; }
    if job.status == Status.Complete { return Outcome.Complete; }
    if job.status == Status.Failed { return Outcome.Failed; }
    assert(claim(job));
    return run_claimed(session, job, budget);
}
