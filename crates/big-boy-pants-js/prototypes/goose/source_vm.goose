import std;
import binary;
import .program;
import .vm;
import .export_schema;

namespace source;

// Serial admission authority; it is never part of a worker heap snapshot.
var next_identity: i64 = 1;

// Fixed-stride values, never Goose pointers. Tags follow GSP primitive tags;
// tag 5 is a checked session-local heap ID. Strings are UTF-16 pool ranges.
struct Value { tag: i64 = 0, number: f64 = 0, a: i64 = 0, b: i64 = 0, session: i64 = 0 }
struct Function { start: i64, count: i64, constants: i64, children: i64, params: i64, locals: i64, captures: i64, strict: bool }
struct Op { code: i64, arg: i64, offset: i64, start: i64, end: i64, depth: i64 = -1 }
root struct Code {
    source_bytes: u8[>..]         = [],
    functions:    Function[>..]   = [], constants: Value[>..] = [], children: i64[>..] = [],
    ops:          Op[>..]         = [], units: u16[>..] = [], valid: bool = false,
    error:        tiny::LoadError = tiny::LoadError.None, error_pc: i64 = -1,
}
enum Fault { None, InvalidState, StaleID, MemoryLimit, WorkLimit, InstructionLimit, StackLimit, TypeError, ReferenceError, ConstAssignment, UnsupportedOperand, NativeBoundary }
struct Limits { nodes: i64 = 100000, values: i64 = 1048576, units: i64 = 1048576, properties: i64 = 1048576, work: i64 = 10000000, steps: i64 = 1000000, frames: i64 = 256, stack: i64 = 65536 }
// All heap tables append until session teardown; IDs are never recycled.
// kind: plain=0, array=1, closure=2, cell=3. Captures and cell values never
// reference frame scratch. Property chains are indices, not Goose references.
struct Node { kind: i64, function: i64 = 0, start: i64 = 0, count: i64 = 0, head: i64 = -1, length: i64 = 0, initialized: bool = false, immutable: bool = false, value: Value = Value {} }
struct Property { key: Value, value: Value, next: i64 }
root struct Heap {
    code:  const Code&, identity: i64 = 0, limits: Limits = Limits {},
    nodes: Node[>..]                  = [], values: Value[>..] = [], properties: Property[>..] = [],
    units: u16[>..]                   = [], constants: Value[>..] = [], work: i64 = 0,
    fault: Fault                      = Fault.None, global: Value = Value {},
}
struct Frame { function: i64, pc: i64, local_base: i64, operand_base: i64, callee: Value, receiver: Value }
struct Write { object: Value, key: Value, value: Value }
root struct Job {
    heap:   Heap&, frames: Frame[>..<] = [], locals: Value[>..<] = [], operands: Value[>..<] = [],
    writes: Write[>..]                 = [], status: tiny::Status = tiny::Status.Ready, result: Value = Value {},
    fault:  Fault                      = Fault.None, fault_pc: i64 = -1, fault_function: i64 = -1,
    steps:  i64                        = 0, claimed: bool = false,
}
fn number(n: f64) -> Value { return Value { tag: 3, number: n }; }
fn boolean(b: bool) -> Value { return Value { tag: 2, a: if b { 1 } else { 0 } }; }
fn reject(code: Code&, error: tiny::LoadError, pc: i64) -> bool { code.error = error; code.error_pc = pc; return false; }
fn stack_effect(code: const Code&, f: Function, op: Op) -> i64, i64 {
    let n = op.arg;
    match op.code {
        1, 2, 3, 16, 45, 46, 47, 84, 86, 87, 105, 106, 107, 145, 197, 198 => { return 0, 1; },
        4, 5, 14, 15, 139 => { return 1, 0; },
        6 => { return 1, 2; },
        7, 8, 9, 10, 11, 21, 22, 25, 26, 33, 34, 38, 39, 40, 41, 73, 74, 75, 76, 77, 80, 81, 88, 90, 151 => { return 2, 1; },
        12, 20, 27, 35, 36, 49, 78, 79, 85, 108, 112, 113, 114, 137, 138, 150, 161 => { return 1, 1; },
        23, 24, 91, 152 => { return 3, 1; },
        37 => { return 2, 2; },
        48, 50 => { return 2, 0; },
        13, 109, 110, 140, 141, 143, 144 => { return 0, 0; },
        18, 191, 192, 195 => { return n, 1; },
        19, 193 => { return n * 2, 1; },
        194 => { return export_form_arity(n), 1; },
        43 => { return code.functions[code.children[f.children + n]].captures, 1; },
        44 => { return n + 1, 1; },
        83 => { return n + 2, 1; },
        _ => { return -1, -1; },
    };
}
fn target(code: const Code&, f: Function, offset: i64) -> i64 {
    var lo = f.start; var hi = f.start + f.count;
    while lo < hi { let mid = lo + (hi - lo) / 2; if code.ops[mid].offset < offset { lo = mid + 1; } else { hi = mid; } }
    return lo;
}
// GSP validation precedes decoding; flow is independently checked here for
// every function, including unreachable child functions. No source fallback.
fn load(code: Code&, image: const program::Image&) -> bool {
    assert(code.functions.len == 0 && !code.valid);
    if !image.valid || program::validate(image.bytes) != program::Error.None { return reject(code, tiny::LoadError.Empty, -1); }
    let bytes = image.bytes[..]; let l = program::layout(bytes);
    code.source_bytes.append(bytes[l.source..l.functions]);
    for i in program::word(bytes, 12) {
        let at = l.functions + i * 52;
        code.functions.push(Function { start: program::word(bytes, at), count: program::word(bytes, at + 4), constants: program::word(bytes, at + 8), children: program::word(bytes, at + 16), params: program::word(bytes, at + 24), locals: program::word(bytes, at + 28), captures: program::word(bytes, at + 32), strict: (program::word(bytes, at + 48) & 1) == 1 });
    }
    for i in program::word(bytes, 16) {
        let at = l.constants + i * 12; let tag = program::word(bytes, at);
        var value = Value { tag: tag };
        if tag == 3 { let n, ok = binary::read_f64le(bytes, at + 4); assert(ok); value.number = n; }
        else { value.a = program::word(bytes, at + 4); value.b = program::word(bytes, at + 8); }
        code.constants.push(value);
    }
    for i in program::word(bytes, 20) { code.children.push(program::word(bytes, l.children + i * 4)); }
    for i in program::word(bytes, 24) {
        let at = l.instructions + i * 20;
        code.ops.push(Op { code: program::word(bytes, at) & 255, arg: program::word(bytes, at + 4), offset: program::word(bytes, at + 16), start: program::word(bytes, at + 8), end: program::word(bytes, at + 12) });
    }
    for i in program::word(bytes, 28) { let at = l.units + i * 2; code.units.push(bytes[at] | ((bytes[at + 1] as u16) << 8)); }
    for f in code.functions {
        for i in f.start..(f.start + f.count) {
            let op = code.ops[i]; let pop, push = stack_effect(code, f, op);
            if pop < 0 || push < 0 { return reject(code, tiny::LoadError.Unknown, i); }
            if op.code == 13 || op.code == 14 { code.ops[i].arg = target(code, f, op.arg); }
        }
        var work: i64[>..<] = [f.start]; code.ops[f.start].depth = 0;
        while work.len > 0 {
            let pc = work.pop(); let op = code.ops[pc]; let pop, push = stack_effect(code, f, op);
            if op.depth < pop { return reject(code, tiny::LoadError.Underflow, pc); }
            let after = op.depth - pop + push;
            if after > 65536 { return reject(code, tiny::LoadError.Limits, pc); }
            if op.code == 15 { continue; }
            for edge in (if op.code == 14 { 2 } else { 1 }) {
                let next = if (op.code == 13 || op.code == 14) && edge == 0 { op.arg } else { pc + 1 };
                if next < f.start || next >= f.start + f.count { return reject(code, tiny::LoadError.Fallthrough, pc); }
                let depth = code.ops[next].depth;
                if depth >= 0 && depth != after { return reject(code, tiny::LoadError.Join, next); }
                if depth < 0 { code.ops[next].depth = after; work.push(next); }
            }
        }
    }
    code.valid = true; return true;
}
fn charge(heap: Heap&, count: i64 = 1) -> bool {
    if heap.fault != Fault.None { return false; }
    if count < 0 || count > heap.limits.work - heap.work { heap.fault = Fault.WorkLimit; return false; }
    heap.work += count; return true;
}
fn memory(heap: Heap&) -> bool { heap.fault = Fault.MemoryLimit; return false; }
fn valid(heap: Heap&, v: Value) -> bool {
    if heap.fault != Fault.None { return false; }
    if v.tag < 0 || v.tag > 5 || (v.tag == 2 && v.a != 0 && v.a != 1) { heap.fault = Fault.InvalidState; return false; }
    if v.tag == 4 || v.tag == 5 {
        if v.session != heap.identity { heap.fault = Fault.StaleID; return false; }
        if v.a < 0 || (v.tag == 5 && v.a >= heap.nodes.len) || (v.tag == 4 && (v.b < 0 || v.a > heap.units.len || v.b > heap.units.len - v.a)) { heap.fault = Fault.StaleID; return false; }
    }
    return true;
}
fn allocate(heap: Heap&, node: Node) -> Value {
    if !charge(heap) { return Value {}; }
    if heap.nodes.len >= heap.limits.nodes { memory(heap); return Value {}; }
    let id = heap.nodes.len; heap.nodes.push(node); return Value { tag: 5, a: id, session: heap.identity };
}
fn string(heap: Heap&, units: const u16[:]) -> Value {
    if !charge(heap, units.len) { return Value {}; }
    if units.len > heap.limits.units - heap.units.len { memory(heap); return Value {}; }
    let start = heap.units.len; heap.units.append(units);
    return Value { tag: 4, a: start, b: units.len, session: heap.identity };
}
fn text(heap: Heap&, bytes: const u8[:]) -> Value {
    var units: u16[>..] = []; for byte in bytes { units.push(byte); }
    return string(heap, units);
}
fn initialize(heap: Heap&) -> bool {
    if !heap.code.valid || heap.identity != 0 || heap.nodes.len != 0 || heap.units.len != 0 || heap.constants.len != 0 { heap.fault = Fault.InvalidState; return false; }
    if next_identity > 2147483647 { return memory(heap); }
    heap.identity = next_identity; next_identity += 1;
    if heap.code.units.len > heap.limits.units || heap.code.constants.len > heap.limits.values { return memory(heap); }
    if !charge(heap, heap.code.units.len + heap.code.constants.len) { return false; }
    heap.units.append(heap.code.units);
    for constant in heap.code.constants { var v = constant; if v.tag == 4 { v.session = heap.identity; } heap.constants.push(v); }
    heap.global = allocate(heap, Node { kind: 0 });
    let global_name = text(heap, "globalThis");
    return put(heap, heap.global, global_name, heap.global);
}
fn truthy(v: Value) -> bool { return match v.tag { 0, 1 => false, 2 => v.a != 0, 3 => v.number != 0 && v.number == v.number, 4 => v.b != 0, _ => true }; }
fn equal(heap: Heap&, a: Value, b: Value) -> bool {
    if !valid(heap, a) || !valid(heap, b) || a.tag != b.tag { return false; }
    return match a.tag {
        0, 1 => true, 2 => a.a == b.a, 3 => a.number == b.number,
        4 => { if !charge(heap, a.b + b.b) { return false; } heap.units[a.a..(a.a + a.b)] == heap.units[b.a..(b.a + b.b)] },
        _ => a.a == b.a,
    };
}
fn numeric(heap: Heap&, v: Value) -> f64 {
    if !valid(heap, v) { return 0; }
    return match v.tag { 0 => 0.0 / 0.0, 1 => 0.0, 2 => v.a, 3 => v.number, _ => { heap.fault = Fault.UnsupportedOperand; 0.0 } };
}
fn key(heap: Heap&, value: Value) -> Value {
    if !valid(heap, value) { return Value {}; }
    if value.tag == 4 { return value; }
    if value.tag == 3 && value.number >= 0 && value.number < 4294967295.0 && value.number == (value.number as i64) { let bytes = str(value.number as i64); return text(heap, bytes); }
    heap.fault = Fault.UnsupportedOperand; return Value {};
}
fn key_is(heap: Heap&, value: Value, bytes: const u8[:]) -> bool {
    if value.tag != 4 || value.b != bytes.len || !charge(heap, bytes.len) { return false; }
    for byte, i in bytes { if heap.units[value.a + i] != byte { return false; } } return true;
}
fn index(heap: Heap&, k: Value) -> i64 {
    if k.tag != 4 || k.b == 0 || k.b > 10 { return -1; }
    if !charge(heap, k.b) { return -1; }
    var n: i64 = 0;
    for i in k.b { let c = heap.units[k.a + i]; if c < 48 || c > 57 || (i == 0 && c == 48 && k.b > 1) { return -1; } n = n * 10 + (c - 48); }
    return if n < 4294967295 { n } else { -1 };
}
// Return presence separately: an own property's value can be undefined.
fn own_property(heap: Heap&, object: Value, k: Value) -> Value, bool {
    var id = heap.nodes[object.a].head;
    while id >= 0 {
        if !charge(heap) { return Value {}, false; }
        let property = heap.properties[id];
        if equal(heap, property.key, k) { return property.value, true; }
        id = property.next;
    }
    return Value {}, false;
}
fn get(heap: Heap&, object: Value, k: Value) -> Value {
    if !valid(heap, object) || !valid(heap, k) { return Value {}; }
    if k.tag != 4 { heap.fault = Fault.InvalidState; return Value {}; }
    if object.tag == 4 {
        if key_is(heap, k, "length") { return number(object.b); }
        let i = index(heap, k); if i >= 0 { if i >= object.b { return Value {}; } let unit = heap.units[object.a + i]; return string(heap, [unit]); }
        heap.fault = Fault.UnsupportedOperand; return Value {};
    }
    if object.tag != 5 { heap.fault = if object.tag <= 1 { Fault.TypeError } else { Fault.UnsupportedOperand }; return Value {}; }
    let node = heap.nodes[object.a];
    if node.kind == 1 && key_is(heap, k, "length") { return number(node.length); }
    if node.kind >= 2 { heap.fault = Fault.UnsupportedOperand; return Value {}; }
    let value, found = own_property(heap, object, k);
    if found { return value; }
    if heap.fault != Fault.None { return Value {}; }
    if node.kind == 1 && index(heap, k) < 0 { heap.fault = Fault.UnsupportedOperand; return Value {}; }
    // Prototype-dependent behavior is deliberately unsupported. A missing own
    // ordinary data property is undefined, except known prototype names.
    if key_is(heap, k, "__proto__") || key_is(heap, k, "constructor") || key_is(heap, k, "toString") || key_is(heap, k, "valueOf") || key_is(heap, k, "call") || key_is(heap, k, "apply") || key_is(heap, k, "hasOwnProperty") || key_is(heap, k, "isPrototypeOf") || key_is(heap, k, "propertyIsEnumerable") || key_is(heap, k, "toLocaleString") || key_is(heap, k, "__defineGetter__") || key_is(heap, k, "__defineSetter__") || key_is(heap, k, "__lookupGetter__") || key_is(heap, k, "__lookupSetter__") { heap.fault = Fault.UnsupportedOperand; }
    return Value {};
}
fn put(heap: Heap&, object: Value, k: Value, value: Value) -> bool {
    if !valid(heap, object) || !valid(heap, k) || !valid(heap, value) { return false; }
    if object.tag != 5 || k.tag != 4 || heap.nodes[object.a].kind >= 2 { heap.fault = if object.tag == 5 { Fault.UnsupportedOperand } else { Fault.TypeError }; return false; }
    if key_is(heap, k, "__proto__") { heap.fault = Fault.UnsupportedOperand; return false; }
    if heap.nodes[object.a].kind == 1 && key_is(heap, k, "length") { heap.fault = Fault.UnsupportedOperand; return false; }
    var id = heap.nodes[object.a].head;
    while id >= 0 {
        if !charge(heap) { return false; }
        let p = heap.properties[id]; if equal(heap, p.key, k) { heap.properties[id].value = value; return true; } id = p.next;
    }
    if heap.fault != Fault.None { return false; }
    if heap.properties.len >= heap.limits.properties { return memory(heap); }
    let previous = heap.nodes[object.a].head; let next = heap.properties.len;
    heap.properties.push(Property { key: k, value: value, next: previous }); heap.nodes[object.a].head = next;
    if heap.nodes[object.a].kind == 1 { let i = index(heap, k); if i >= heap.nodes[object.a].length { heap.nodes[object.a].length = i + 1; } }
    return heap.fault == Fault.None;
}
fn closure(heap: Heap&, function: i64, captures: const Value[:]) -> Value {
    if function < 0 || function >= heap.code.functions.len || captures.len != heap.code.functions[function].captures { heap.fault = Fault.InvalidState; return Value {}; }
    if captures.len > heap.limits.values - heap.values.len || !charge(heap, captures.len) { if heap.fault == Fault.None { memory(heap); } return Value {}; }
    for v in captures { if !valid(heap, v) { return Value {}; } }
    let start = heap.values.len; heap.values.append(captures);
    return allocate(heap, Node { kind: 2, function: function, start: start, count: captures.len });
}
fn fail(job: Job&, fault: Fault, frame: Frame) { job.heap.fault = Fault.None; job.status = tiny::Status.Failed; job.fault = fault; job.fault_function = frame.function; job.fault_pc = frame.pc; }
fn enter(job: Job&, callee: Value, receiver: Value, arguments: const Value[:]) -> bool {
    let heap .= job.heap;
    if !valid(heap, callee) || !valid(heap, receiver) { return false; }
    if callee.tag != 5 || heap.nodes[callee.a].kind != 2 { heap.fault = Fault.TypeError; return false; }
    let id = heap.nodes[callee.a].function; let f = heap.code.functions[id];
    if job.frames.len >= heap.limits.frames || f.locals > heap.limits.stack - job.locals.len { heap.fault = Fault.StackLimit; return false; }
    if !charge(heap, f.locals + arguments.len) { return false; }
    for v in arguments { if !valid(heap, v) { return false; } }
    let base = job.locals.len; job.locals.resize(base + f.locals, Value {});
    for value, i in arguments { if i < f.params { job.locals[base + i] = value; } }
    job.frames.push(Frame { function: id, pc: f.start, local_base: base, operand_base: job.operands.len, callee: callee, receiver: receiver });
    return true;
}
fn start(job: Job&, callee: Value, arguments: const Value[:], receiver: Value = Value {}) -> bool {
    assert(job.frames.len == 0 && job.locals.len == 0 && job.operands.len == 0 && job.steps == 0);
    if !enter(job, callee, receiver, arguments) { job.fault = job.heap.fault; job.heap.fault = Fault.None; job.status = tiny::Status.Failed; return false; } return true;
}
fn record(job: Job&, object: Value, k: Value, value: Value) {
    if job.writes.len >= job.heap.limits.properties { memory(job.heap); return; }
    job.writes.push(Write { object: object, key: k, value: value });
}
fn binary_value(heap: Heap&, op: i64, a: Value, b: Value) -> Value {
    if op == 11 { return boolean(equal(heap, a, b)); }
    if op == 41 {
        if (a.tag == 0 || a.tag == 1) && (b.tag == 0 || b.tag == 1) { return boolean(true); }
        if a.tag == b.tag { return boolean(equal(heap, a, b)); }
        if a.tag <= 1 || b.tag <= 1 { return boolean(false); }
        if (a.tag <= 3 && b.tag <= 3) { return boolean(numeric(heap, a) == numeric(heap, b)); }
        heap.fault = Fault.UnsupportedOperand; return Value {};
    }
    if op == 73 && a.tag == 4 && b.tag == 4 {
        if !valid(heap, a) || !valid(heap, b) { return Value {}; }
        if a.b + b.b > heap.limits.units - heap.units.len { memory(heap); return Value {}; }
        var units: u16[>..] = []; units.append(heap.units[a.a..(a.a + a.b)]); units.append(heap.units[b.a..(b.a + b.b)]); return string(heap, units);
    }
    if op >= 74 && op <= 77 && a.tag == 4 && b.tag == 4 {
        if !valid(heap, a) || !valid(heap, b) || !charge(heap, a.b + b.b) { return Value {}; }
        var c: i64 = 0; for i in min(a.b, b.b) { let x = heap.units[a.a + i]; let y = heap.units[b.a + i]; if x != y { c = if x < y { -1 } else { 1 }; break; } } if c == 0 { c = if a.b < b.b { -1 } else if a.b > b.b { 1 } else { 0 }; }
        return boolean(if op == 74 { c < 0 } else if op == 75 { c > 0 } else if op == 76 { c <= 0 } else { c >= 0 });
    }
    let x = numeric(heap, a); let y = numeric(heap, b);
    if heap.fault != Fault.None { return Value {}; }
    return match tiny::number_binary(op, x, y) {
        Number n => number(n.value), Bool b => boolean(b.value), _ => { heap.fault = Fault.InvalidState; Value {} },
    };
}

fn step(job: Job&) {
    let heap .= job.heap; let code .= heap.code;
    if job.frames.len == 0 { fail(job, Fault.InvalidState, Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value {}, receiver: Value {} }); return; }
    let frame = job.frames[job.frames.len - 1]; let f = code.functions[frame.function];
    if frame.pc < f.start || frame.pc >= f.start + f.count { fail(job, Fault.InvalidState, frame); return; }
    if job.steps >= heap.limits.steps { fail(job, Fault.InstructionLimit, frame); return; }
    if !charge(heap) { fail(job, heap.fault, frame); return; }
    let op = code.ops[frame.pc]; let pop, push = stack_effect(code, f, op);
    if job.operands.len - frame.operand_base < pop { fail(job, Fault.InvalidState, frame); return; }
    if push - pop > heap.limits.stack - job.operands.len { fail(job, Fault.StackLimit, frame); return; }
    job.steps += 1; job.frames[job.frames.len - 1].pc += 1;
    match op.code {
        1 => { job.operands.push(heap.constants[f.constants + op.arg]); },
        2 => { job.operands.push(Value {}); },
        3 => { job.operands.push(job.locals[frame.local_base + op.arg]); },
        4 => { job.locals[frame.local_base + op.arg] = job.operands.pop(); },
        5 => { job.operands.pop(); },
        6 => { let v = job.operands[job.operands.len - 1]; job.operands.push(v); },
        7, 8, 9, 10, 11, 33, 34, 38, 39, 40, 41, 73, 74, 75, 76, 77 => { let b = job.operands.pop(); let a = job.operands.pop(); let v = binary_value(heap, op.code, a, b); job.operands.push(v); },
        12 => { let v = job.operands.pop(); job.operands.push(boolean(!truthy(v))); },
        13 => { job.frames[job.frames.len - 1].pc = op.arg; },
        14 => { let v = job.operands.pop(); if !truthy(v) { job.frames[job.frames.len - 1].pc = op.arg; } },
        15 => {
            let v = job.operands.pop(); job.frames.pop(); job.locals.resize(frame.local_base); job.operands.resize(frame.operand_base);
            if job.frames.len == 0 { job.result = v; job.status = tiny::Status.Complete; } else { job.operands.push(v); }
        },
        16, 84, 87 => { let v = allocate(heap, Node { kind: if op.code == 87 { 1 } else { 0 } }); job.operands.push(v); },
        35, 36, 112, 113, 114 => { let v = job.operands.pop(); let n = numeric(heap, v); job.operands.push(number(if op.code == 35 { -n } else if op.code == 113 { n + 1 } else if op.code == 114 { n - 1 } else { n })); },
        37 => { let b = job.operands.pop(); let a = job.operands.pop(); let x = numeric(heap, a); let y = numeric(heap, b); job.operands.push(number(x)); job.operands.push(number(y)); },
        43 => {
            let child = code.children[f.children + op.arg]; let count = code.functions[child].captures; let base = job.operands.len - count;
            let v = closure(heap, child, job.operands[base..]); job.operands.resize(base); job.operands.push(v);
        },
        44, 83 => {
            let base = job.operands.len - op.arg - (if op.code == 83 { 2 } else { 1 });
            let callee = job.operands[base + (if op.code == 83 { 1 } else { 0 })];
            let receiver = if op.code == 83 { job.operands[base] } else { Value {} };
            if enter(job, callee, receiver, job.operands[(job.operands.len - op.arg)..]) { job.frames[job.frames.len - 1].operand_base = base; job.operands.resize(base); }
        },
        45 => { job.operands.push(frame.callee); },
        46 => { let node = heap.nodes[frame.callee.a]; job.operands.push(heap.values[node.start + op.arg]); },
        47, 197, 198 => { let v = allocate(heap, Node { kind: 3, immutable: op.code == 197 }); job.operands.push(v); },
        48, 50, 49 => {
            let value = if op.code == 49 { Value {} } else { job.operands.pop() }; let cell = job.operands.pop();
            if valid(heap, cell) && cell.tag == 5 && heap.nodes[cell.a].kind == 3 {
                let node = heap.nodes[cell.a];
                if op.code == 49 || op.code == 50 {
                    if !node.initialized { heap.fault = Fault.ReferenceError; }
                    else if op.code == 50 && node.immutable { heap.fault = Fault.ConstAssignment; }
                    else if op.code == 49 { job.operands.push(node.value); }
                    else { heap.nodes[cell.a].value = value; record(job, cell, Value {}, value); }
                } else if node.initialized && node.immutable { heap.fault = Fault.ConstAssignment; }
                else { heap.nodes[cell.a].value = value; heap.nodes[cell.a].initialized = true; record(job, cell, Value {}, value); }
            } else if heap.fault == Fault.None { heap.fault = Fault.TypeError; }
        },
        79, 90 => {
            let k = if op.code == 90 { let v = job.operands.pop(); key(heap, v) } else { heap.constants[f.constants + op.arg] };
            let object = job.operands.pop(); let value = get(heap, object, k); job.operands.push(value);
        },
        80, 81, 91, 151, 152, 88 => {
            let value = job.operands.pop();
            let k = if op.code == 91 || op.code == 152 { let v = job.operands.pop(); key(heap, v) } else if op.code == 88 {
                let object = job.operands[job.operands.len - 1];
                if valid(heap, object) && object.tag == 5 && heap.nodes[object.a].kind == 1 { key(heap, number(heap.nodes[object.a].length)) } else { heap.fault = Fault.TypeError; Value {} }
            } else { heap.constants[f.constants + op.arg] };
            let object = job.operands.pop(); if put(heap, object, k, value) { record(job, object, k, value); }
            job.operands.push(if op.code == 81 || op.code == 88 { object } else { value });
        },
        161 => { let value = job.operands.pop(); if valid(heap, value) && value.tag == 5 && heap.nodes[value.a].kind == 3 { heap.fault = Fault.UnsupportedOperand; } job.operands.push(value); },
        86, 145 => {
            var receiver = frame.receiver;
            if op.code == 86 && !f.strict && (receiver.tag == 0 || receiver.tag == 1) { receiver = heap.global; }
            else if op.code == 86 && !f.strict && receiver.tag != 5 { heap.fault = Fault.UnsupportedOperand; }
            job.operands.push(receiver);
        },
        105 => { job.operands.push(heap.global); },
        106, 107 => {
            let k = heap.constants[f.constants + op.arg];
            let v, found = own_property(heap, heap.global, k);
            if !found {
                // Keep prototype-dependent reads explicitly unsupported.
                get(heap, heap.global, k);
                if op.code == 106 && heap.fault == Fault.None { heap.fault = Fault.ReferenceError; }
            }
            job.operands.push(v);
        },
        // Runtime/form leaves and global declaration semantics are explicit
        // admission boundaries for the next runtime/bridge slice.
        _ => { heap.fault = if (op.code >= 18 && op.code <= 27) || (op.code >= 191 && op.code <= 195) { Fault.NativeBoundary } else { Fault.UnsupportedOperand }; },
    }
    if heap.fault != Fault.None { fail(job, heap.fault, frame); }
}
fn claim(job: Job&) -> bool { if job.claimed || job.status != tiny::Status.Ready { return false; } job.claimed = true; return true; }
fn run_claimed(job: Job&, budget: i64) -> tiny::Outcome {
    if !job.claimed { return tiny::Outcome.Busy; }
    defer { job.claimed = false; }
    var used: i64 = 0;
    while job.status == tiny::Status.Ready && (budget < 0 || used < budget) { step(job); used += 1; }
    return match job.status { Ready => tiny::Outcome.Yielded, Complete => tiny::Outcome.Complete, Failed => tiny::Outcome.Failed };
}
fn run(job: Job&, budget: i64) -> tiny::Outcome {
    if job.claimed { return tiny::Outcome.Busy; }
    if job.status == tiny::Status.Complete { return tiny::Outcome.Complete; }
    if job.status == tiny::Status.Failed { return tiny::Outcome.Failed; }
    assert(claim(job)); return run_claimed(job, budget);
}
// Canonical scalar/identity output, never a lossy JS print/read bridge.
fn canonical(heap: Heap&, v: Value, out: u8[>..]&) {
    if !valid(heap, v) { format(out, "invalid:", heap.fault); return; }
    match v.tag {
        0 => { out.append("undefined"); }, 1 => { out.append("null"); }, 2 => { out.append(if v.a == 1 { "boolean:true" } else { "boolean:false" }); },
        3 => { if v.number != v.number { out.append("number:NaN"); } else if v.number == 0 && 1.0 / v.number < 0 { out.append("number:-0"); } else { format(out, "number:", v.number); } },
        4 => { out.append("utf16:["); for i in v.b { if i > 0 { out.push(','); } format(out, heap.units[v.a + i]); } out.push(']'); },
        5 => { format(out, "identity:", v.session, ":", v.a, ":", heap.nodes[v.a].kind); },
        _ => { out.append("invalid:tag"); },
    }
}
