import std;
import .export_schema;

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

fn width(op: u8) -> i64 {
    return match op { opcode::Const, opcode::GetLocal, opcode::SetLocal => 3, opcode::Jump, opcode::JumpIfFalse => 5, _ => 1 };
}
fn known(op: i64) -> bool {
    return match op {
        opcode::Const, opcode::Undefined, opcode::GetLocal, opcode::SetLocal, opcode::Pop, opcode::Dup, opcode::Add, opcode::Sub, opcode::Mul, opcode::Lt, opcode::StrictEq, opcode::Not, opcode::Jump, opcode::JumpIfFalse, opcode::Return, opcode::NewObject, opcode::HostEffect => true,
        opcode::Div, opcode::Rem, opcode::Neg, opcode::ToNumber, opcode::ToNumberPair, opcode::Gt, opcode::Le, opcode::Ge => true,
        opcode::Plus, opcode::CompareLt, opcode::CompareGt, opcode::CompareLe, opcode::CompareGe, opcode::ToNumeric => true,
        _ => false,
    };
}
fn pops(op: u8) -> i64 {
    return match op {
        opcode::Add, opcode::Sub, opcode::Mul, opcode::Lt, opcode::StrictEq, opcode::Div, opcode::Rem, opcode::ToNumberPair, opcode::Gt, opcode::Le, opcode::Ge => 2,
        opcode::Plus, opcode::CompareLt, opcode::CompareGt, opcode::CompareLe, opcode::CompareGe => 2,
        opcode::Neg, opcode::ToNumber, opcode::ToNumeric => 1,
        opcode::SetLocal, opcode::Pop, opcode::Dup, opcode::Not, opcode::JumpIfFalse, opcode::Return, opcode::HostEffect => 1,
        _ => 0,
    };
}
fn pushes(op: u8) -> i64 {
    return match op {
        opcode::Dup, opcode::ToNumberPair => 2,
        opcode::Div, opcode::Rem, opcode::Neg, opcode::ToNumber, opcode::Gt, opcode::Le, opcode::Ge => 1,
        opcode::Plus, opcode::CompareLt, opcode::CompareGt, opcode::CompareLe, opcode::CompareGe, opcode::ToNumeric => 1,
        opcode::Const, opcode::Undefined, opcode::GetLocal, opcode::Add, opcode::Sub, opcode::Mul, opcode::Lt, opcode::StrictEq, opcode::Not, opcode::NewObject, opcode::HostEffect => 1,
        _ => 0,
    };
}
fn reject(p: Program&, error: LoadError, offset: i64) -> bool {
    p.error = error; p.error_offset = offset; return false;
}

// Version 2: u16 indices and u32 absolute byte addresses; check before narrowing.
fn emit(code: u8[>..]&, op: i64, arg: i64 = 0) -> bool {
    if !known(op) || arg < 0 || arg > (if op == opcode::Jump || op == opcode::JumpIfFalse { 4294967295 } else { 65535 }) { return false; }
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
        if op == opcode::Const && arg >= constants.len { return reject(p, LoadError.Constant, pos); }
        if (op == opcode::GetLocal || op == opcode::SetLocal) && arg >= locals { return reject(p, LoadError.Local, pos); }
        starts[pos] = p.instructions.len;
        p.instructions.push(Instruction { op: op, arg: arg, offset: pos });
        pos += size;
    }
    for &ins in p.instructions {
        if ins.op == opcode::Jump || ins.op == opcode::JumpIfFalse {
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
        if ins.op == opcode::Return { continue; }
        let count = if ins.op == opcode::JumpIfFalse { 2 } else { 1 };
        for edge in count {
            let next = if (ins.op == opcode::Jump || ins.op == opcode::JumpIfFalse) && edge == 0 { ins.arg } else { index + 1 };
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
        opcode::Add, opcode::Plus => Value.Number { value: x + y },
        opcode::Sub => Value.Number { value: x - y },
        opcode::Mul => Value.Number { value: x * y },
        opcode::Div => Value.Number { value: x / y },
        opcode::Rem => Value.Number { value: x % y },
        opcode::Gt, opcode::CompareGt => Value.Bool { value: x > y },
        opcode::Le, opcode::CompareLe => Value.Bool { value: x <= y },
        opcode::Ge, opcode::CompareGe => Value.Bool { value: x >= y },
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
        opcode::Const => { job.operands.push(p.constants[ins.arg]); },
        opcode::Undefined => { job.operands.push(Value.Undefined); },
        opcode::GetLocal => { job.operands.push(job.locals[ins.arg]); },
        opcode::SetLocal => { job.locals[ins.arg] = job.operands.pop(); },
        opcode::Pop => { job.operands.pop(); },
        opcode::Dup => { let v = job.operands[job.operands.len - 1]; job.operands.push(v); },
        opcode::Add, opcode::Sub, opcode::Mul, opcode::Lt, opcode::Div, opcode::Rem, opcode::Gt, opcode::Le, opcode::Ge, opcode::Plus, opcode::CompareLt, opcode::CompareGt, opcode::CompareLe, opcode::CompareGe => {
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
        opcode::Neg, opcode::ToNumber, opcode::ToNumeric => {
            let v = job.operands.pop();
            match v { Number n => { job.operands.push(Value.Number { value: if ins.op == opcode::Neg { -n.value } else { n.value } }); }, _ => { fail(job, Fault.UnsupportedOperand); } }
        },
        opcode::ToNumberPair => {
            let b = job.operands.pop(); let a = job.operands.pop();
            if (match a { Number => true, _ => false }) && (match b { Number => true, _ => false }) { job.operands.push(a); job.operands.push(b); } else { fail(job, Fault.UnsupportedOperand); }
        },
        opcode::StrictEq => { let b = job.operands.pop(); let a = job.operands.pop(); job.operands.push(Value.Bool { value: equal(a, b) }); },
        opcode::Not => { let v = job.operands.pop(); job.operands.push(Value.Bool { value: !truthy(v) }); },
        opcode::Jump => { job.pc = ins.arg; },
        opcode::JumpIfFalse => { let v = job.operands.pop(); if !truthy(v) { job.pc = ins.arg; } },
        opcode::Return => { job.result = job.operands.pop(); job.status = Status.Complete; },
        opcode::NewObject => {
            if session.next_object > 2147483647 { fail(job, Fault.ObjectLimit); return; }
            let id = session.next_object; session.next_object += 1;
            job.operands.push(Value.Object { id: id });
        },
        opcode::HostEffect => {
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
