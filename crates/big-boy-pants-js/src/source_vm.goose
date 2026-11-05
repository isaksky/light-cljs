import std;
import binary;
import math;
import .program;
import .vm;
import .export_schema;

namespace source;

// Serial admission authority; it is never part of a worker heap snapshot.
var next_identity: i64 = 1;

// Fixed-stride values, never Goose pointers. Tags follow GSP primitive tags;
// tag 5 is a checked session-local heap ID. Strings are UTF-16 pool ranges.
struct Value { tag: i64 = 0, number: f64 = 0, a: i64 = 0, b: i64 = 0, session: i64 = 0 }
// kind: GSP constructor kind (0 none, 1 ordinary, 2 base class, 3 derived
// class, 4 default derived class).
struct Function { start: i64, count: i64, constants: i64, children: i64, params: i64, locals: i64, captures: i64, strict: bool, kind: i64 = 0 }
struct Op { code: i64, arg: i64, offset: i64, start: i64, end: i64, depth: i64 = -1 }
// One decoded instruction per validated `Op` row (light-cljs-785; decision 1
// of docs/javascript-coverage.md): a fixed-mode enum dispatched by `match` in
// `step`. Payloads are the row's validated operand; jump targets are absolute
// instruction indices. `Op` rows remain the validation and provenance record
// (source spans, byte offsets, the opcode a fault names). An opcode this VM
// does not implement decodes to `Unsupported` and fails by name when it runs.
enum Ins {
    Const { k: u32 }, Undefined, Load { slot: u32 }, Store { slot: u32 }, Pop, Dup, Binary { op: u8 }, Not,
    Jump { to: u32 }, JumpUnless { to: u32 }, Return, NewObject, NewArray, Try { to: u32 }, Throw, EndTry,
    Unary { op: u8 }, Numeric2, Closure { child: u32 }, Call { argc: u32 }, CallMethod { argc: u32 },
    Arguments, Rest { skip: u32 }, Callee, Capture { slot: u32 }, NewCell { immutable: bool },
    InitCell, ReadCell, WriteCell, GetNamed { k: u32 }, GetIndexed, SetNamed { k: u32 }, SetNamedChain { k: u32 },
    SetIndexed, Append, CheckValue, This { sloppy: bool }, Global, GetGlobal { k: u32, throws: bool },
    Literal { kind: u8, count: u32 }, Form { op: u32 }, Unsupported { opcode: u8 },
}
// The row's stack effect from the generated registry table, resolved once at
// decode (-1 marks a row the table refuses; load never admits one).
struct Effect { pop: i32, push: i32 }
root struct Code {
    source_bytes: u8[>..]         = [],
    functions:    Function[>..]   = [], constants: Value[>..] = [], children: i64[>..] = [],
    ops:          Op[>..]         = [], units: u16[>..] = [], valid: bool = false,
    error:        tiny::LoadError = tiny::LoadError.None, error_pc: i64 = -1,
    ins:          Ins[>..]        = [], effects: Effect[>..] = [], decoded: i64 = 0,
}
// Unsupported: an exported opcode (or constant kind) this VM does not
// implement ran; Job.fault_opcode names it. Like every fault except the
// TypeError, ReferenceError and HostEffect throws, JavaScript cannot catch it.
enum Fault { None, InvalidState, StaleID, MemoryLimit, WorkLimit, InstructionLimit, StackLimit, TypeError, ReferenceError, ConstAssignment, UnsupportedOperand, NativeBoundary, DependencyNeed, EffectContract, HostEffect, Unsupported }
struct Limits { nodes: i64 = 100000, values: i64 = 1048576, units: i64 = 1048576, properties: i64 = 1048576, work: i64 = 10000000, steps: i64 = 1000000, frames: i64 = 256, stack: i64 = 65536 }
// All heap tables append until session teardown; IDs are never recycled.
// kind: plain=0, array=1, closure=2, cell=3. Captures and cell values never
// reference frame scratch. Property chains are indices, not Goose references.
// Runtime additions: builtin=4, list/vector/map/set=5/6/7/8,
// symbol/keyword=9/10, scalar occurrence=11, sequence cursor=12,
// opaque caught runtime error=13, read-only unmapped arguments=14,
// finite deferred map=15 (value=callback, namespace_value=input,
// start/count=reserved memo range, length=completed callbacks).
// Closure immutable marks fnApplyNull's immutable receiver convention.
// Errors preserve object identity/truthiness;
// property access/coercion and form transport reject unsupported inspection.
// Metadata/origin belong to the occurrence, independently of value equality.
struct Node { kind: i64, function: i64 = 0, start: i64 = 0, count: i64 = 0, head: i64 = -1, length: i64 = 0, initialized: bool = false, immutable: bool = false, value: Value = Value {}, namespace_value: Value = Value { tag: 1 }, metadata: Value = Value {}, origin: i64 = -1, source: i64 = -1, span_start: i64 = 0, span_end: i64 = 0 }
struct SourceRange { start: i64, count: i64 }
struct Property { key: Value, value: Value, next: i64 }
root struct Heap {
    code:         const Code&, identity: i64 = 0, limits: Limits = Limits {},
    nodes:        Node[>..]                  = [], values: Value[>..] = [], properties: Property[>..] = [],
    units:        u16[>..]                   = [], constants: Value[>..] = [], work: i64 = 0,
    source_bytes: u8[>..]                    = [], source_ranges: SourceRange[>..] = [],
    fault:        Fault                      = Fault.None, global: Value = Value {}, call_method: Value = Value {}, boot_count: i64 = 0, initializer_count: i64 = 0, producer_count: i64 = 0,
    live_jobs:    i64                        = 0,
}
// Actual arguments occupy bounded job-local scratch after declared locals.
// Copies live across yields/helper suspension and disappear on frame unwind.
struct Frame { function: i64, pc: i64, local_base: i64, operand_base: i64, callee: Value, receiver: Value, argument_count: i64 = 0, input: Value = Value {}, output: Value = Value {}, index: i64 = 0 }
struct Write { object: Value, key: Value, value: Value }
struct Handler { frame: i64, pc: i64, operands: i64, locals: i64 }
root struct Job {
    heap:            Heap&, frames: Frame[>..<] = [], locals: Value[>..<] = [], operands: Value[>..<] = [],
    writes:          Write[>..]                 = [], status: tiny::Status = tiny::Status.Ready, result: Value = Value {},
    handlers:        Handler[>..<]              = [],
    fault:           Fault                      = Fault.None, fault_pc: i64 = -1, fault_function: i64 = -1, fault_opcode: i64 = -1, thrown: bool = false,
    steps:           i64                        = 0, claimed: bool = false, registered: bool = false,
    effects_enabled: bool                       = false, effect_pending: bool = false, effect_kind: i64 = -1,
    helpers_enabled: bool                       = false,
    protected_nodes: i64                        = 0,
    root_receiver:   Value                      = Value {},
    effect_args:     Value[8]                   = [Value {}; 8], effect_count: i64 = 0, effect_frame: Frame = Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value {}, receiver: Value {} },
}
fn number(n: f64) -> Value { return Value { tag: 3, number: n }; }
fn boolean(b: bool) -> Value { return Value { tag: 2, a: if b { 1 } else { 0 } }; }
fn reject(code: Code&, error: tiny::LoadError, pc: i64) -> bool { code.error = error; code.error_pc = pc; return false; }
// Every opcode's stack effect and control flow come from the generated
// registry table (export_stack_effect, light-cljs-783); there is no
// hand-written opcode list. `captures` is the child function's capture count,
// read only when the effect's multiplied count is that (source 2).
fn effect_word(e: i64, op: Op, captures: i64) -> i64, i64 {
    if e < 0 { return -1, -1; }
    let source = (e >> 24) & 15;
    let n = if source == 1 { op.arg } else if source == 2 { captures } else if source == 3 { export_form_arity(op.arg) } else { 0 };
    if n < 0 { return -1, -1; }
    return (e & 255) + ((e >> 16) & 255) * n, (e >> 8) & 255;
}
fn stack_effect(code: const Code&, f: Function, op: Op) -> i64, i64 {
    let e = export_stack_effect(op.code);
    let captures = if e >= 0 && ((e >> 24) & 15) == 2 { code.functions[code.children[f.children + op.arg]].captures } else { 0 };
    return effect_word(e, op, captures);
}
// Flow: 0 next, 1 jump, 2 branch, 3 catch (target starts with the exception
// pushed), 4 end. Branching opcodes carry a byte-offset target operand.
fn flow(code: i64) -> i64 { return export_stack_effect(code) >> 28; }
fn branches(code: i64) -> bool { return ((export_descriptor(code) >> 8) & 255) == 3; }
fn target(code: const Code&, f: Function, offset: i64) -> i64 {
    var lo = f.start; var hi = f.start + f.count;
    while lo < hi { let mid = lo + (hi - lo) / 2; if code.ops[mid].offset < offset { lo = mid + 1; } else { hi = mid; } }
    return lo;
}
fn instruction(op: Op) -> Ins {
    let a = op.arg as u32;
    return match op.code {
        1 => Ins.Const { k: a }, 2 => Ins.Undefined, 3 => Ins.Load { slot: a }, 4 => Ins.Store { slot: a },
        5 => Ins.Pop, 6 => Ins.Dup,
        7, 8, 9, 10, 11, 33, 34, 38, 39, 40, 41, 73, 74, 75, 76, 77 => Ins.Binary { op: op.code as u8 },
        12 => Ins.Not, 13 => Ins.Jump { to: a }, 14 => Ins.JumpUnless { to: a }, 15 => Ins.Return,
        16, 84 => Ins.NewObject, 87 => Ins.NewArray, 56 => Ins.Try { to: a }, 55 => Ins.Throw, 58 => Ins.EndTry,
        35, 36, 112, 113, 114 => Ins.Unary { op: op.code as u8 }, 37 => Ins.Numeric2,
        43 => Ins.Closure { child: a }, 44 => Ins.Call { argc: a }, 83 => Ins.CallMethod { argc: a },
        101 => Ins.Arguments, 179 => Ins.Rest { skip: a }, 45 => Ins.Callee, 46 => Ins.Capture { slot: a },
        47, 198 => Ins.NewCell { immutable: false }, 197 => Ins.NewCell { immutable: true },
        48 => Ins.InitCell, 49 => Ins.ReadCell, 50 => Ins.WriteCell,
        79 => Ins.GetNamed { k: a }, 90 => Ins.GetIndexed,
        80, 151 => Ins.SetNamed { k: a }, 81 => Ins.SetNamedChain { k: a }, 91, 152 => Ins.SetIndexed, 88 => Ins.Append,
        161 => Ins.CheckValue, 86 => Ins.This { sloppy: true }, 145 => Ins.This { sloppy: false }, 105 => Ins.Global,
        106 => Ins.GetGlobal { k: a, throws: true }, 107 => Ins.GetGlobal { k: a, throws: false },
        191 => Ins.Literal { kind: 5, count: a }, 192 => Ins.Literal { kind: 6, count: a },
        193 => Ins.Literal { kind: 7, count: a * 2 }, 195 => Ins.Literal { kind: 8, count: a },
        194 => Ins.Form { op: a },
        _ => Ins.Unsupported { opcode: op.code as u8 },
    };
}
// Decode every function appended since the last call; earlier rows are
// immutable, so this is idempotent. Every Code owner calls it after it appends
// validated rows (load, append_code, snapshot restore, callable install).
fn decode(code: Code&) {
    if code.ins.len < code.ops.len { code.ins.resize(code.ops.len, Ins.Pop); code.effects.resize(code.ops.len, Effect { pop: -1, push: -1 }); }
    for i in code.decoded..code.functions.len {
        let f = code.functions[i];
        for pc in f.start..(f.start + f.count) {
            let op = code.ops[pc]; let pop, push = stack_effect(code, f, op);
            code.ins[pc] = instruction(op); code.effects[pc] = Effect { pop: pop as i32, push: push as i32 };
        }
    }
    code.decoded = code.functions.len;
}
// GSP validation precedes decoding; flow is independently checked here for
// every function, including unreachable child functions. No source fallback.
fn load(code: Code&, image: const program::Image&) -> bool {
    assert(code.functions.len == 0 && !code.valid);
    if !image.valid || program::validate(image.bytes) != program::Error.None { return reject(code, tiny::LoadError.Empty, -1); }
    let bytes = image.bytes[..]; let l = program::layout(bytes);
    code.source_bytes.append(bytes[l.source..l.functions]);
    for i in program::word(bytes, 12) {
        let at = l.functions + i * 52; let flags = program::word(bytes, at + 48);
        code.functions.push(Function { start: program::word(bytes, at), count: program::word(bytes, at + 4), constants: program::word(bytes, at + 8), children: program::word(bytes, at + 16), params: program::word(bytes, at + 24), locals: program::word(bytes, at + 28), captures: program::word(bytes, at + 32), strict: (flags & 1) == 1, kind: (flags >> 8) & 255 });
    }
    for i in program::word(bytes, 16) {
        let at = l.constants + i * 12; let tag = program::word(bytes, at);
        var value = Value { tag: tag };
        if tag == 3 { let n, ok = binary::read_f64le(bytes, at + 4); assert(ok); value.number = n; }
        // Symbol (6) keeps its well-known index. No BigInt (7) operation is
        // implemented, so its validated digits stay in the image; `Const`
        // fails Unsupported before either value reaches a JavaScript value.
        else if tag == 6 { value.a = program::word(bytes, at + 4); }
        else if tag != 7 { value.a = program::word(bytes, at + 4); value.b = program::word(bytes, at + 8); }
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
            if branches(op.code) { code.ops[i].arg = target(code, f, op.arg); }
        }
        var work: i64[>..<] = [f.start]; code.ops[f.start].depth = 0;
        while work.len > 0 {
            let pc = work.pop(); let op = code.ops[pc]; let pop, push = stack_effect(code, f, op);
            if op.depth < pop { return reject(code, tiny::LoadError.Underflow, pc); }
            let after = op.depth - pop + push;
            if after > 65536 { return reject(code, tiny::LoadError.Limits, pc); }
            let next_flow = flow(op.code);
            if next_flow == 4 { continue; }
            for edge in (if next_flow == 2 || next_flow == 3 { 2 } else { 1 }) {
                let next = if next_flow != 0 && edge == 0 { op.arg } else { pc + 1 };
                let expected = after + (if next_flow == 3 && edge == 0 { 1 } else { 0 });
                if next < f.start || next >= f.start + f.count { return reject(code, tiny::LoadError.Fallthrough, pc); }
                let depth = code.ops[next].depth;
                if depth >= 0 && depth != expected { return reject(code, tiny::LoadError.Join, next); }
                if depth < 0 { code.ops[next].depth = expected; work.push(next); }
            }
        }
    }
    decode(code); code.valid = true; return true;
}
fn charge(heap: Heap&, count: i64 = 1) -> bool {
    if heap.fault != Fault.None { return false; }
    if count < 0 || count > heap.limits.work - heap.work { heap.fault = Fault.WorkLimit; return false; }
    heap.work += count; return true;
}
// Merge independently validated immutable function trees before admission.
// Function/capture/constant references relocate; source byte spans stay exact
// within the concatenated source owner. No executable work happens here.
fn append_code(out: Code&, other: const Code&) -> i64 {
    assert(other.valid);
    let functions = out.functions.len; let ops = out.ops.len; let constants = out.constants.len;
    let children = out.children.len; let units = out.units.len; let bytes = out.source_bytes.len;
    for f in other.functions { var n = f; n.start += ops; n.constants += constants; n.children += children; out.functions.push(n); }
    for op in other.ops { var n = op; n.start += bytes; n.end += bytes; if branches(n.code) { n.arg += ops; } out.ops.push(n); }
    for v in other.constants { var n = v; if n.tag == 4 { n.a += units; } out.constants.push(n); }
    for id in other.children { out.children.push(functions + id); }
    out.units.append(other.units); out.source_bytes.append(other.source_bytes); decode(out); out.valid = true;
    return functions;
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
    heap.identity = reserve_identity();
    if heap.identity == 0 { return memory(heap); }
    if heap.code.units.len > heap.limits.units || heap.code.constants.len > heap.limits.values { return memory(heap); }
    if !charge(heap, heap.code.units.len + heap.code.constants.len) { return false; }
    heap.units.append(heap.code.units);
    for constant in heap.code.constants { var v = constant; if v.tag == 4 { v.session = heap.identity; } heap.constants.push(v); }
    heap.global = allocate(heap, Node { kind: 0 });
    heap.call_method = builtin(heap, 14);
    let global_name = text(heap, "globalThis");
    return put(heap, heap.global, global_name, heap.global);
}
// Call only on the serial coordinator, including before native worker spawn.
// Workers receive one fresh private identity, never this admission authority.
fn reserve_identity() -> i64 {
    if next_identity > 2147483647 { return 0; }
    let result = next_identity; next_identity += 1; return result;
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
    let x = form_value(heap, v);
    return match x.tag { 0 => 0.0 / 0.0, 1 => 0.0, 2 => x.a, 3 => x.number, _ => { heap.fault = Fault.UnsupportedOperand; 0.0 } };
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
    if (node.kind == 2 || node.kind == 4) && key_is(heap, k, "call") { return heap.call_method; }
    // Unmapped, read-only arguments subset. Mutation, callee and prototypes
    // fail explicitly; no frame slot or parameter alias escapes to the heap.
    if node.kind == 14 {
        if key_is(heap, k, "length") { return number(node.count); }
        let i = index(heap, k);
        if i >= 0 { return if i < node.count { heap.values[node.start + i] } else { Value {} }; }
        heap.fault = Fault.UnsupportedOperand; return Value {};
    }
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
    if heap.nodes[object.a].kind == 1 && key_is(heap, k, "length") { return array_length(heap, object, value); }
    var array_index: i64 = -1;
    if heap.nodes[object.a].kind == 1 {
        array_index = index(heap, k);
        if heap.fault != Fault.None { return false; }
        if array_index < 0 { heap.fault = Fault.UnsupportedOperand; return false; }
        if array_index >= heap.limits.values { return memory(heap); }
    }
    var id = heap.nodes[object.a].head;
    while id >= 0 {
        if !charge(heap) { return false; }
        let p = heap.properties[id]; if equal(heap, p.key, k) { heap.properties[id].value = value; return true; } id = p.next;
    }
    if heap.fault != Fault.None { return false; }
    if heap.properties.len >= heap.limits.properties { return memory(heap); }
    let previous = heap.nodes[object.a].head; let next = heap.properties.len;
    heap.properties.push(Property { key: k, value: value, next: previous }); heap.nodes[object.a].head = next;
    if array_index >= heap.nodes[object.a].length { heap.nodes[object.a].length = array_index + 1; }
    return heap.fault == Fault.None;
}
// Bounded numeric length writes only. Removed property records remain charged
// until teardown, but no longer participate in lookup or later extension.
// Preflight the full walk before mutation so a work rejection is atomic.
fn array_length(heap: Heap&, object: Value, value: Value) -> bool {
    if value.tag != 3 || !(value.number >= 0 && value.number <= 4294967295.0) || value.number != trunc(value.number) { heap.fault = Fault.UnsupportedOperand; return false; }
    let length = value.number as i64;
    if length > heap.limits.values { return memory(heap); }
    if length < heap.nodes[object.a].length {
        var id = heap.nodes[object.a].head;
        while id >= 0 {
            if !charge(heap, 2 + heap.properties[id].key.b) { return false; }
            let i = index(heap, heap.properties[id].key);
            if heap.fault != Fault.None { return false; }
            if i < 0 { heap.fault = Fault.UnsupportedOperand; return false; }
            id = heap.properties[id].next;
        }
        var previous: i64 = -1; id = heap.nodes[object.a].head;
        while id >= 0 {
            let p = heap.properties[id];
            // put admitted canonical indices. The preflight charged this
            // second decode, so mutation below has no remaining failure point.
            var i: i64 = 0;
            for at in p.key.b { i = i * 10 + (heap.units[p.key.a + at] - 48); }
            if i >= length {
                if previous < 0 { heap.nodes[object.a].head = p.next; }
                else { heap.properties[previous].next = p.next; }
            } else { previous = id; }
            id = p.next;
        }
    }
    heap.nodes[object.a].length = length; return true;
}
fn closure(heap: Heap&, function: i64, captures: const Value[:]) -> Value {
    if function < 0 || function >= heap.code.functions.len || captures.len != heap.code.functions[function].captures { heap.fault = Fault.InvalidState; return Value {}; }
    if captures.len > heap.limits.values - heap.values.len || !charge(heap, captures.len) { if heap.fault == Fault.None { memory(heap); } return Value {}; }
    for v in captures { if !valid(heap, v) { return Value {}; } }
    let start = heap.values.len; heap.values.append(captures);
    return allocate(heap, Node { kind: 2, function: function, start: start, count: captures.len });
}
fn unwind_maps(job: Job&, keep: i64) {
    for i in keep..job.frames.len {
        let f = job.frames[i]; if f.function == -2 { job.heap.nodes[f.input.a].initialized = false; }
    }
}
fn fail(job: Job&, fault: Fault, frame: Frame) {
    job.heap.fault = Fault.None;
    var reported = fault;
    if (fault == Fault.TypeError || fault == Fault.ReferenceError || fault == Fault.HostEffect) && job.handlers.len > 0 {
        let error = allocate(job.heap, Node { kind: 13 });
        if job.heap.fault == Fault.None {
            let handler = job.handlers.pop();
            unwind_maps(job, handler.frame + 1);
            job.frames.resize(handler.frame + 1); job.locals.resize(handler.locals); job.operands.resize(handler.operands);
            job.frames[handler.frame].pc = handler.pc; job.operands.push(error); return;
        }
        reported = job.heap.fault; job.heap.fault = Fault.None;
    }
    unwind_maps(job, 0);
    job.status = tiny::Status.Failed; job.fault = reported; job.fault_function = frame.function; job.fault_pc = frame.pc;
    let code .= job.heap.code;
    job.fault_opcode = if reported == Fault.Unsupported && frame.function >= 0 && frame.pc >= 0 && frame.pc < code.ops.len { code.ops[frame.pc].code } else { -1 };
}
// The fault with the unimplemented operation's registry name, for example
// `Fault.Unsupported:construct` or `Fault.Unsupported:const(BigInt)`.
fn describe_fault(out: u8[>..]&, code: const Code&, job: const Job&) {
    format(out, job.fault);
    if job.fault == Fault.Unsupported && job.fault_opcode >= 0 { out.push(':'); unsupported_operation(out, code, job); }
}
// Just the operation: `construct`, `const(BigInt)`.
fn unsupported_operation(out: u8[>..]&, code: const Code&, job: const Job&) {
    if job.fault != Fault.Unsupported || job.fault_opcode < 0 { return; }
    out.append(export_opcode_name(job.fault_opcode));
    if job.fault_opcode == 1 {
        let tag = code.constants[code.functions[job.fault_function].constants + code.ops[job.fault_pc].arg].tag;
        out.append(if tag == 6 { "(Symbol)" } else if tag == 7 { "(BigInt)" } else { "" });
    }
}
fn raise(job: Job&, value: Value, frame: Frame) {
    if job.handlers.len == 0 { job.result = value; job.thrown = true; fail(job, Fault.HostEffect, frame); return; }
    let handler = job.handlers.pop();
    unwind_maps(job, handler.frame + 1);
    job.frames.resize(handler.frame + 1); job.locals.resize(handler.locals); job.operands.resize(handler.operands);
    job.frames[handler.frame].pc = handler.pc; job.operands.push(value);
}
fn enter(job: Job&, callee: Value, receiver: Value, arguments: const Value[:]) -> bool {
    let heap .= job.heap;
    if !valid(heap, callee) || !valid(heap, receiver) { return false; }
    if callee.tag != 5 || heap.nodes[callee.a].kind != 2 { heap.fault = Fault.TypeError; return false; }
    let id = heap.nodes[callee.a].function; let f = heap.code.functions[id];
    // A class constructor has no [[Call]]: calling it throws TypeError.
    if f.kind >= 2 { heap.fault = Fault.TypeError; return false; }
    if job.frames.len >= heap.limits.frames || f.locals > heap.limits.stack - job.locals.len || arguments.len > heap.limits.stack - job.locals.len - f.locals { heap.fault = Fault.StackLimit; return false; }
    if !charge(heap, f.locals + arguments.len) { return false; }
    for v in arguments { if !valid(heap, v) { return false; } }
    let base = job.locals.len; job.locals.resize(base + f.locals, Value {});
    for value, i in arguments { if i < f.params { job.locals[base + i] = value; } }
    job.locals.append(arguments);
    job.frames.push(Frame { function: id, pc: f.start, local_base: base, operand_base: job.operands.len, callee: callee, receiver: receiver, argument_count: arguments.len });
    return true;
}
fn start(job: Job&, callee: Value, arguments: const Value[:], receiver: Value = Value {}) -> bool {
    if job.registered || job.status != tiny::Status.Ready { return false; }
    assert(job.frames.len == 0 && job.locals.len == 0 && job.operands.len == 0 && job.steps == 0);
    if !enter(job, callee, receiver, arguments) { job.fault = job.heap.fault; job.heap.fault = Fault.None; job.status = tiny::Status.Failed; return false; }
    job.heap.live_jobs += 1; job.registered = true; return true;
}
// Explicitly retire a terminal owner or abandon a suspended job. Snapshot
// capture of a helper continuation preserves active iterators for relocation;
// ordinary abandonment releases their realization markers for later retry.
// Scope exit alone is not observable; a forgotten owner keeps capture closed.
fn retire(job: Job&, suspending: bool = false) -> bool {
    if job.claimed { return false; }
    if suspending && (!job.effect_pending || job.status != tiny::Status.Ready) { return false; }
    if !suspending { unwind_maps(job, 0); }
    if job.registered { assert(job.heap.live_jobs > 0); job.heap.live_jobs -= 1; job.registered = false; }
    if job.status == tiny::Status.Ready { job.status = tiny::Status.Failed; job.fault = Fault.InvalidState; }
    return true;
}
// Only a retired, unclaimed job can release its logical tables. Values are
// integer handles, so no Goose references into these arrays escape the job.
// Data-stack pages remain available for the next dispatch until thread exit.
fn reset(job: Job&) -> bool {
    if job.claimed || job.registered || job.heap.live_jobs != 0 { return false; }
    job.frames.clear(); job.locals.clear(); job.operands.clear(); job.writes.clear(); job.handlers.clear();
    job.status = tiny::Status.Ready; job.result = Value {}; job.fault = Fault.None; job.fault_pc = -1; job.fault_function = -1; job.thrown = false;
    job.steps = 0; job.effects_enabled = false; job.helpers_enabled = false; job.effect_pending = false; job.effect_kind = -1;
    job.protected_nodes = 0; job.root_receiver = Value {}; job.effect_args = [Value {}; 8]; job.effect_count = 0;
    job.effect_frame = Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value {}, receiver: Value {} };
    return true;
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

// Bounded persistent flat ranges. Updates copy only the changed container's
// child IDs; input occurrences and untouched subtrees retain exact identity.
fn nil() -> Value { return Value { tag: 1 }; }
fn form_value(heap: Heap&, v: Value) -> Value {
    if !valid(heap, v) { return Value {}; }
    return if v.tag == 5 && heap.nodes[v.a].kind == 11 { heap.nodes[v.a].value } else { v };
}
fn form_kind(heap: Heap&, v: Value) -> i64 {
    let x = form_value(heap, v);
    return if x.tag == 5 { heap.nodes[x.a].kind } else { -1 };
}
fn form_collection(heap: Heap&, kind: i64, children: const Value[:], template: Node = Node { kind: 0 }) -> Value {
    if kind < 5 || kind > 8 || (kind == 7 && children.len % 2 != 0) { heap.fault = Fault.TypeError; return Value {}; }
    if children.len > heap.limits.values - heap.values.len || heap.nodes.len >= heap.limits.nodes { memory(heap); return Value {}; }
    if !charge(heap, children.len) { return Value {}; }
    for child in children { if !valid(heap, child) { return Value {}; } }
    var node = template; node.kind = kind; node.start = heap.values.len; node.count = children.len;
    heap.values.append(children); return allocate(heap, node);
}
// Ordinary constructors normalize equal keys/members. LiteralMap and the
// compiler occurrence bridge deliberately retain positional source operands.
fn form_construct(heap: Heap&, kind: i64, children: const Value[:]) -> Value {
    if kind != 7 && kind != 8 { return form_collection(heap, kind, children); }
    let stride: i64 = if kind == 7 { 2 } else { 1 };
    if children.len % stride != 0 { heap.fault = Fault.TypeError; return Value {}; }
    if !charge(heap, children.len) { return Value {}; }
    var normalized: Value[>..] = [];
    for i in (children.len / stride) {
        let key = children[i * stride]; var found: i64 = -1;
        for j in (normalized.len / stride) { if form_equal(heap, normalized[j * stride], key) { found = j * stride; break; } }
        if heap.fault != Fault.None { return Value {}; }
        if found >= 0 { if kind == 7 { normalized[found + 1] = children[i * stride + 1]; } }
        else { normalized.push(key); if kind == 7 { normalized.push(children[i * stride + 1]); } }
    }
    return form_collection(heap, kind, normalized);
}
fn form_count(heap: Heap&, v: Value) -> i64 {
    let x = form_value(heap, v);
    if x.tag <= 1 { return 0; }
    if x.tag == 4 { return x.b; }
    if x.tag == 5 { let n = heap.nodes[x.a]; if n.kind >= 5 && n.kind <= 8 || n.kind == 12 { return if n.kind == 7 { n.count / 2 } else { n.count }; } }
    heap.fault = Fault.TypeError; return 0;
}
fn form_nth(heap: Heap&, v: Value, i: i64, fallback: Value = Value { tag: 1 }) -> Value {
    let x = form_value(heap, v);
    if x.tag <= 1 { return fallback; }
    if x.tag == 4 { return if i >= 0 && i < x.b { string(heap, [heap.units[x.a + i]]) } else { fallback }; }
    if x.tag == 5 { let n = heap.nodes[x.a]; if n.kind == 5 || n.kind == 6 || n.kind == 12 { return if i >= 0 && i < n.count { heap.values[n.start + i] } else { fallback }; } }
    heap.fault = Fault.TypeError; return Value {};
}
recursive fn form_equal(heap: Heap&, a: Value, b: Value, depth: i64 = 0) -> bool {
    if !charge(heap) { return false; }
    if depth >= 128 { heap.fault = Fault.StackLimit; return false; }
    if !valid(heap, a) || !valid(heap, b) { return false; }
    if a.tag == 5 && b.tag == 5 && a.a == b.a { return true; }
    let x = form_value(heap, a); let y = form_value(heap, b);
    if x.tag <= 1 && y.tag <= 1 { return true; }
    if x.tag != y.tag { return false; }
    if x.tag != 5 { return equal(heap, x, y); }
    let n = heap.nodes[x.a]; let m = heap.nodes[y.a];
    if n.kind == 15 || m.kind == 15 { heap.fault = Fault.NativeBoundary; return false; }
    if n.kind < 5 || m.kind < 5 || n.kind == 13 || m.kind == 13 || n.kind == 14 || m.kind == 14 { return x.a == y.a; }
    if n.kind != m.kind && !((n.kind == 5 || n.kind == 6 || n.kind == 12) && (m.kind == 5 || m.kind == 6 || m.kind == 12)) { return false; }
    if n.kind == 9 || n.kind == 10 { return equal(heap, n.value, m.value) && equal(heap, n.namespace_value, m.namespace_value); }
    if n.count != m.count { return false; }
    if n.kind == 7 || n.kind == 8 {
        let stride: i64 = if n.kind == 7 { 2 } else { 1 };
        for i in (n.count / stride) {
            var found = false;
            for j in (m.count / stride) {
                if form_equal(heap, heap.values[n.start + i * stride], heap.values[m.start + j * stride], depth + 1) {
                    // Match form_get: later equal keys cannot replace the
                    // first match when its value differs.
                    found = stride == 1 || form_equal(heap, heap.values[n.start + i * stride + 1], heap.values[m.start + j * stride + 1], depth + 1);
                    break;
                }
            }
            if !found { return false; }
        }
        return true;
    }
    for i in n.count { if !form_equal(heap, heap.values[n.start + i], heap.values[m.start + i], depth + 1) { return false; } }
    return true;
}
fn form_assoc(heap: Heap&, v: Value, k: Value, value: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if kind != 6 && kind != 7 { heap.fault = Fault.TypeError; return Value {}; }
    let n = heap.nodes[x.a]; var at: i64 = -1;
    if kind == 6 {
        let i = form_value(heap, k);
        if i.tag != 3 || !(i.number >= 0 && i.number <= n.count) || i.number != (i.number as i64) { heap.fault = Fault.TypeError; return Value {}; }
        at = i.number as i64;
    } else {
        for i in (n.count / 2) { if form_equal(heap, heap.values[n.start + i * 2], k) { at = i * 2 + 1; break; } }
    }
    if !charge(heap, n.count + 2) { return Value {}; }
    let replacing = at >= 0 && at < n.count;
    let count = n.count + (if replacing { 0 } else if kind == 7 { 2 } else { 1 });
    if count > heap.limits.values - heap.values.len || heap.nodes.len >= heap.limits.nodes { memory(heap); return Value {}; }
    if !charge(heap, count) { return Value {}; }
    // Validate exactly the resulting children, including only retained old
    // slots. A replaced stale value is absent from the new range.
    if replacing {
        for child in heap.values[n.start..(n.start + at)] { if !valid(heap, child) { return Value {}; } }
        if !valid(heap, value) { return Value {}; }
        for child in heap.values[(n.start + at + 1)..(n.start + n.count)] { if !valid(heap, child) { return Value {}; } }
    } else {
        for child in heap.values[n.start..(n.start + n.count)] { if !valid(heap, child) { return Value {}; } }
        if kind == 7 && !valid(heap, k) { return Value {}; } if !valid(heap, value) { return Value {}; }
    }
    var node = n; node.kind = kind; node.origin = -1; node.start = heap.values.len; node.count = count;
    // allocate charges the final work unit before any child append. Nothing
    // below can fail; old ranges and occurrence/metadata handles stay intact.
    let result = allocate(heap, node); if heap.fault != Fault.None { return Value {}; }
    if replacing {
        heap.values.append(heap.values[n.start..(n.start + at)]); heap.values.push(value);
        heap.values.append(heap.values[(n.start + at + 1)..(n.start + n.count)]);
    } else {
        heap.values.append(heap.values[n.start..(n.start + n.count)]);
        if kind == 7 { heap.values.push(k); } heap.values.push(value);
    }
    return result;
}
fn form_conj(heap: Heap&, v: Value, value: Value) -> Value {
    let x = form_value(heap, v); let kind = if x.tag <= 1 { 5 } else { form_kind(heap, x) };
    if kind != 5 && kind != 6 { heap.fault = Fault.TypeError; return Value {}; }
    let n = if x.tag <= 1 { Node { kind: 5 } } else { heap.nodes[x.a] };
    if !charge(heap, n.count + 1) { return Value {}; }
    let count = n.count + 1;
    if count > heap.limits.values - heap.values.len || heap.nodes.len >= heap.limits.nodes { memory(heap); return Value {}; }
    if !charge(heap, count) { return Value {}; }
    if kind == 5 && !valid(heap, value) { return Value {}; }
    for child in heap.values[n.start..(n.start + n.count)] { if !valid(heap, child) { return Value {}; } }
    if kind == 6 && !valid(heap, value) { return Value {}; }
    var node = n; node.kind = kind; node.origin = -1; node.start = heap.values.len; node.count = count;
    let result = allocate(heap, node); if heap.fault != Fault.None { return Value {}; }
    if kind == 5 { heap.values.push(value); }
    heap.values.append(heap.values[n.start..(n.start + n.count)]);
    if kind == 6 { heap.values.push(value); }
    return result;
}
fn form_get(heap: Heap&, v: Value, k: Value, fallback: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if x.tag <= 1 { return fallback; }
    if kind == 6 || x.tag == 4 {
        let i = form_value(heap, k); if i.tag != 3 || !(i.number >= 0 && i.number < form_count(heap, x)) || i.number != (i.number as i64) { return fallback; }
        return form_nth(heap, x, i.number as i64, fallback);
    }
    if kind != 7 { heap.fault = Fault.TypeError; return Value {}; }
    let n = heap.nodes[x.a];
    for i in (n.count / 2) { if form_equal(heap, heap.values[n.start + i * 2], k) { return heap.values[n.start + i * 2 + 1]; } }
    return fallback;
}
fn form_contains(heap: Heap&, v: Value, k: Value) -> bool {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if x.tag <= 1 { return false; }
    if kind == 6 || x.tag == 4 {
        let i = form_value(heap, k);
        return i.tag == 3 && i.number >= 0 && i.number < form_count(heap, x) && i.number == trunc(i.number);
    }
    if kind == 7 || kind == 8 {
        let n = heap.nodes[x.a]; let stride: i64 = if kind == 7 { 2 } else { 1 };
        for i in (n.count / stride) { if form_equal(heap, heap.values[n.start + i * stride], k) { return true; } if heap.fault != Fault.None { return false; } }
        return false;
    }
    heap.fault = Fault.UnsupportedOperand; return false;
}
// Eager bounded syntax-quote assembly. Only sequential form ranges and nil
// are admitted; arbitrary lazy/JS/protocol collections remain unsupported.
// Children keep their occurrence identity, metadata and source provenance.
fn form_concat(heap: Heap&, colls: const Value[:]) -> Value {
    var count: i64 = 0;
    for coll in colls {
        if !charge(heap, 2) { return Value {}; }
        let x = form_value(heap, coll); if heap.fault != Fault.None { return Value {}; }
        if x.tag <= 1 { continue; }
        let kind = form_kind(heap, x);
        if kind != 5 && kind != 6 && kind != 12 { heap.fault = Fault.UnsupportedOperand; return Value {}; }
        let n = heap.nodes[x.a];
        if n.count > heap.limits.values - heap.values.len - count { memory(heap); return Value {}; }
        if !charge(heap, n.count * 2) { return Value {}; }
        for child in heap.values[n.start..(n.start + n.count)] { if !valid(heap, child) { return Value {}; } }
        count += n.count;
    }
    let start = heap.values.len;
    let result = allocate(heap, Node { kind: 12, start: start, count: count });
    if heap.fault != Fault.None { return Value {}; }
    // Preflight above accounted both validation and copying. Append only
    // after every failure point; the inputs are immutable stable ranges.
    for coll in colls {
        let x = form_value(heap, coll); if x.tag <= 1 { continue; }
        let n = heap.nodes[x.a]; heap.values.append(heap.values[n.start..(n.start + n.count)]);
    }
    return result;
}
fn form_sequence(heap: Heap&, v: Value, skip: i64, empty_nil: bool) -> Value {
    let x = form_value(heap, v); let count = form_count(heap, x);
    if heap.fault != Fault.None { return Value {}; }
    if count <= skip { return if empty_nil { nil() } else { form_collection(heap, 5, []) }; }
    let kind = form_kind(heap, x);
    if kind != 5 && kind != 6 && kind != 12 { heap.fault = Fault.NativeBoundary; return Value {}; }
    let n = heap.nodes[x.a]; return allocate(heap, Node { kind: 12, start: n.start + skip, count: n.count - skip });
}
fn named_supported(heap: Heap&, ns: Value, name: Value) -> bool {
    if !charge(heap, name.b + (if ns.tag == 4 { ns.b } else { 0 })) { return false; }
    if ns.tag == 4 { for i in ns.b { if heap.units[ns.a + i] == 47 { return false; } } if ns.b == 0 && name.b == 0 { return false; } }
    else { for i in name.b { if heap.units[name.a + i] == 47 && name.b != 1 { return false; } } }
    return true;
}
fn form_op(heap: Heap&, op: i64, args: const Value[:]) -> Value {
    for v in args {
        if !valid(heap, v) { return Value {}; }
        // Partial/lazy protocol operations are outside this full-demand
        // target slice. Never inspect an uninitialized memo as a form.
        if v.tag == 5 && heap.nodes[v.a].kind == 15 { heap.fault = Fault.NativeBoundary; return Value {}; }
    }
    let a = args[0];
    match op {
        0, 1 => {
            if (a.tag > 1 && a.tag != 4) || args[1].tag != 4 { heap.fault = Fault.TypeError; return Value {}; }
            if !named_supported(heap, a, args[1]) { if heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; } return Value {}; }
            return allocate(heap, Node { kind: if op == 0 { 10 } else { 9 }, value: args[1], namespace_value: if a.tag <= 1 { nil() } else { a } });
        },
        2 => { let kind = form_kind(heap, a); return match kind { 5, 12 => text(heap, "list"), 6 => text(heap, "vector"), 7 => text(heap, "map"), 8 => text(heap, "set"), 9 => text(heap, "symbol"), 10 => text(heap, "keyword"), _ => nil() }; },
        5, 19 => { return number(form_count(heap, a)); },
        6, 25, 26 => { let i = form_value(heap, args[1]); if i.tag != 3 || !(i.number >= -2147483648.0 && i.number <= 2147483647.0) || i.number != (i.number as i64) || (op != 26 && (i.number < 0 || i.number >= form_count(heap, a))) { heap.fault = Fault.TypeError; return Value {}; } return form_nth(heap, a, i.number as i64, if op == 26 { args[2] } else { nil() }); },
        7, 27 => { return form_get(heap, a, args[1], args[2]); },
        72 => { return form_get(heap, a, args[1], nil()); },
        28 => { return boolean(form_contains(heap, a, args[1])); },
        35 => { return form_concat(heap, args); },
        36 => {
            let x = form_value(heap, a); if x.tag <= 1 { return form_concat(heap, []); }
            let kind = form_kind(heap, x);
            if kind != 5 && kind != 6 && kind != 12 { heap.fault = Fault.UnsupportedOperand; return Value {}; }
            let n = heap.nodes[x.a]; return form_concat(heap, heap.values[n.start..(n.start + n.count)]);
        },
        8, 29 => { return form_assoc(heap, a, args[1], args[2]); },
        10, 31 => { return form_conj(heap, a, args[1]); },
        11 => { return boolean(form_equal(heap, a, args[1])); },
        13, 33 => { return if a.tag == 5 && heap.nodes[a.a].metadata.tag != 0 { heap.nodes[a.a].metadata } else { nil() }; },
        14, 34 => { let kind = form_kind(heap, a); let meta = form_value(heap, args[1]); if !(kind >= 5 && kind <= 9 || kind == 12) || (meta.tag > 1 && form_kind(heap, meta) != 7) { heap.fault = Fault.TypeError; return Value {}; } var n = heap.nodes[a.a]; n.metadata = if meta.tag <= 1 { Value {} } else { args[1] }; n.origin = -1; return allocate(heap, n); },
        15 => { return form_value(heap, a); },
        18 => { let v = form_value(heap, a); return boolean(v.tag > 1 && !(v.tag == 2 && v.a == 0)); },
        20, 22, 23 => { return form_sequence(heap, a, if op == 20 { 0 } else { 1 }, op != 22); },
        21 => { return form_nth(heap, a, 0); },
        65 => { let v = form_value(heap, a); let kind = form_kind(heap, v); if kind != 5 && kind != 6 { heap.fault = Fault.TypeError; return Value {}; } let n = heap.nodes[v.a]; return form_collection(heap, 7, heap.values[n.start..(n.start + n.count)]); },
        69 => { if form_kind(heap, a) != 7 && form_kind(heap, a) != 8 { heap.fault = Fault.TypeError; return Value {}; } return a; },
        80 => { let kind = form_kind(heap, a); return boolean(kind == 6 || kind == 7 || kind == 8 || kind == 9 || kind == 10); },
        81 => {
            let value = form_value(heap, a);
            if value.tag == 5 && (heap.nodes[value.a].kind == 2 || heap.nodes[value.a].kind == 4) { return heap.call_method; }
            heap.fault = Fault.NativeBoundary; return Value {};
        },
        60 => {
            let arity = args[3]; let kind = form_kind(heap, a);
            if arity.tag != 3 || (arity.number != 1 && arity.number != 2) { heap.fault = Fault.UnsupportedOperand; return Value {}; }
            if kind == 9 || kind == 10 { return form_get(heap, args[1], a, args[2]); }
            if kind == 7 { return form_get(heap, a, args[1], args[2]); }
            if kind == 8 {
                let n = heap.nodes[a.a];
                for i in n.count { if !charge(heap) { return Value {}; } let v = heap.values[n.start + i]; if form_equal(heap, v, args[1]) { return v; } if heap.fault != Fault.None { return Value {}; } }
                return args[2];
            }
            if kind == 6 {
                let i = form_value(heap, args[1]);
                if i.tag != 3 || !(i.number >= 0 && i.number < form_count(heap, a)) { heap.fault = Fault.UnsupportedOperand; return Value {}; }
                // Legacy vector calls truncate after bounds validation.
                return form_nth(heap, a, i.number as! i64);
            }
            heap.fault = Fault.NativeBoundary; return Value {};
        },
        82 => {
            if a.tag != 5 || heap.nodes[a.a].kind != 0 || args[1].tag > 1 { heap.fault = Fault.NativeBoundary; return Value {}; }
            if args[2].tag != 4 { heap.fault = Fault.TypeError; return Value {}; }
            let value, found = own_property(heap, a, args[2]);
            // A compiler root read is a dependency request, even if the JS
            // value would otherwise be undefined. No live-job parking/replay
            // is supported: prerequisites must be resolved before admission.
            if !found && heap.fault == Fault.None { heap.fault = Fault.DependencyNeed; } return value;
        },
        92 => {
            if a.tag != 5 || heap.nodes[a.a].kind != 2 { heap.fault = Fault.TypeError; return Value {}; }
            var node = heap.nodes[a.a]; node.immutable = true;
            return allocate(heap, node);
        },
        93 => { return if a.tag == 5 && heap.nodes[a.a].kind == 2 && heap.nodes[a.a].immutable { nil() } else { a }; },
        _ => { heap.fault = Fault.NativeBoundary; return Value {}; },
    }
}
// Enumerated native runtime leaves: 0 macroReaderFunction (Pants uses
// occurrence-aware leaves directly), 1 inc, 2 mapv, 3 vec, 4 into, 16 sequence.
fn builtin(heap: Heap&, id: i64) -> Value { return allocate(heap, Node { kind: 4, function: id }); }
fn install_field(heap: Heap&, object: Value, name: const u8[:], value: Value) -> bool { let k = text(heap, name); return put(heap, object, k, value); }
fn boot_runtime(heap: Heap&) -> Value {
    if heap.boot_count != 0 { heap.fault = Fault.InvalidState; return Value {}; }
    let rt = allocate(heap, Node { kind: 0 }); let core = allocate(heap, Node { kind: 0 });
    install_field(heap, rt, "core", core);
    install_field(heap, heap.global, "__lightNativeRuntime", rt);
    install_field(heap, core, "macroReaderFunction", builtin(heap, 0));
    install_field(heap, core, "inc", builtin(heap, 1)); install_field(heap, core, "mapv", builtin(heap, 2));
    install_field(heap, core, "vec", builtin(heap, 3)); install_field(heap, core, "into", builtin(heap, 4));
    install_field(heap, core, "sequence", builtin(heap, 16));
    install_field(heap, core, "mapSeq", builtin(heap, 17));
    install_field(heap, core, "list", builtin(heap, 18));
    install_field(heap, rt, "symbol", builtin(heap, 19));
    // The emitted call classifier compares against this constructor identity.
    // Executing regular expressions remains an explicit native boundary.
    install_field(heap, rt, "intrinsicRegExp", builtin(heap, 20));
    // Gensym is serviced only by an explicitly enabled coordinator contract.
    // Other shared-state leaves are installed explicitly by the compiler.
    install_field(heap, core, "gensym", builtin(heap, 5));
    install_field(heap, rt, "vectorFromArray", builtin(heap, 10));
    install_field(heap, rt, "listFromArray", builtin(heap, 11));
    install_field(heap, rt, "mapFromArray", builtin(heap, 12));
    install_field(heap, rt, "setFromArray", builtin(heap, 13));
    install_field(heap, rt, "fnRest", builtin(heap, 15));
    if heap.fault == Fault.None { heap.boot_count += 1; }
    return rt;
}
fn native_value(heap: Heap&, id: i64, args: const Value[:]) -> Value {
    if id == 18 { return form_construct(heap, 5, args); }
    if id == 19 && args.len == 2 { return form_op(heap, 1, args); }
    if id == 17 && args.len == 2 {
        // The generated target maps a nonchunked finite rest list. Vectors,
        // transducers, multiple sources and nested lazy inputs have different
        // demand/chunk semantics and remain explicit boundaries.
        let input = args[1]; let kind = form_kind(heap, input);
        if input.tag > 1 && kind != 5 && kind != 12 { heap.fault = Fault.NativeBoundary; return Value {}; }
        let count = form_count(heap, input);
        if heap.fault != Fault.None { return Value {}; }
        if count > heap.limits.values - heap.values.len || heap.nodes.len >= heap.limits.nodes { memory(heap); return Value {}; }
        if !charge(heap, count) { return Value {}; }
        let start = heap.values.len; heap.values.resize(start + count, Value {});
        return allocate(heap, Node { kind: 15, start: start, count: count, value: args[0], namespace_value: input });
    }
    if id == 16 && args.len == 1 {
        let kind = form_kind(heap, args[0]); if heap.fault != Fault.None { return Value {}; }
        if kind == 5 || kind == 12 || kind == 15 { return args[0]; }
        return form_sequence(heap, args[0], 0, false);
    }
    if id == 15 && args.len == 2 {
        let object = args[0]; let skip = args[1];
        if !valid(heap, object) || object.tag != 5 || heap.nodes[object.a].kind != 14 || skip.tag != 3 || !(skip.number >= 0 && skip.number <= 65535) || skip.number != (skip.number as i64) { heap.fault = Fault.UnsupportedOperand; return Value {}; }
        let node = heap.nodes[object.a]; let offset = min(node.count, skip.number as i64);
        return form_collection(heap, 5, heap.values[(node.start + offset)..(node.start + node.count)]);
    }
    if id >= 10 && id <= 13 && args.len == 1 {
        let array = args[0];
        if !valid(heap, array) || array.tag != 5 || heap.nodes[array.a].kind != 1 { heap.fault = Fault.TypeError; return Value {}; }
        let count = heap.nodes[array.a].length;
        if count > heap.limits.values - heap.values.len { memory(heap); return Value {}; }
        if !charge(heap, count) { return Value {}; }
        var values: Value[>..] = [];
        for i in count { values.push(get(heap, array, key(heap, number(i)))); if heap.fault != Fault.None { return Value {}; } }
        return form_construct(heap, if id == 10 { 6 } else if id == 11 { 5 } else if id == 12 { 7 } else { 8 }, values);
    }
    if id == 0 && args.len == 1 { return args[0]; }
    if id == 1 && args.len == 1 { return number(numeric(heap, args[0]) + 1); }
    if id == 3 && args.len == 1 {
        let kind = form_kind(heap, args[0]); if kind == 6 { return args[0]; }
        if kind == 5 || kind == 12 { let n = heap.nodes[args[0].a]; return form_collection(heap, 6, heap.values[n.start..(n.start + n.count)]); }
    }
    if id == 4 && args.len == 2 {
        var out = args[0]; let count = form_count(heap, args[1]);
        for i in count { out = form_conj(heap, out, form_nth(heap, args[1], i)); if heap.fault != Fault.None { break; } }
        return out;
    }
    heap.fault = Fault.NativeBoundary; return Value {};
}

// Save the request at the call continuation. No host runs under the claim.
fn request_effect(job: Job&, id: i64, args: const Value[:], frame: Frame) {
    if !job.effects_enabled { job.heap.fault = Fault.NativeBoundary; return; }
    if args.len > 8 { job.heap.fault = Fault.EffectContract; return; }
    job.effect_pending = true; job.effect_kind = id; job.effect_count = args.len; job.effect_frame = frame;
    for value, i in args { job.effect_args[i] = value; }
}
fn deliver_effect(job: Job&, value: Value, fault: Fault = Fault.None) -> bool {
    if job.claimed || !job.effect_pending || job.status != tiny::Status.Ready { return false; }
    job.effect_pending = false;
    if fault != Fault.None { fail(job, fault, job.effect_frame); return true; }
    if !valid(job.heap, value) { fail(job, job.heap.fault, job.effect_frame); return true; }
    job.operands.push(value); return true;
}
// Full-demand consumers run callbacks before touching their operands. The
// consumer instruction stays at its saved PC; only this iterator advances.
// Thus suspension, budget cuts and retry after a caught callback throw retain
// completed elements without repeating their effects. No host evaluates JS.
fn demand_map(job: Job&, value: Value) -> bool {
    let heap .= job.heap;
    if !charge(heap) { return false; }
    if !valid(heap, value) || value.tag != 5 || heap.nodes[value.a].kind != 15 { return false; }
    if heap.nodes[value.a].initialized { heap.fault = Fault.NativeBoundary; return false; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return false; }
    let node = heap.nodes[value.a];
    heap.nodes[value.a].initialized = true;
    job.frames.push(Frame { function: -2, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: node.value, receiver: nil(), input: value, index: node.length });
    return true;
}
fn demand_maps(job: Job&, values: const Value[:], nested: bool = false) -> bool {
    for value in values {
        if demand_map(job, value) { return true; }
        if job.heap.fault != Fault.None { return false; }
        if nested && value.tag == 5 {
            let n = job.heap.nodes[value.a];
            if n.kind == 5 || n.kind == 6 || n.kind == 12 {
                for i in n.count { if demand_map(job, job.heap.values[n.start + i]) { return true; } if job.heap.fault != Fault.None { return false; } }
            }
        }
    }
    return false;
}
fn step(job: Job&) {
    let heap .= job.heap; let code .= heap.code;
    if job.frames.len == 0 { fail(job, Fault.InvalidState, Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value {}, receiver: Value {} }); return; }
    let frame = job.frames[job.frames.len - 1];
    if frame.function == -2 {
        if !charge(heap) { fail(job, heap.fault, frame); return; }
        if job.steps >= heap.limits.steps { fail(job, Fault.InstructionLimit, frame); return; }
        job.steps += 1;
        let node = heap.nodes[frame.input.a];
        if frame.pc == 1 {
            let value = job.operands.pop(); heap.values[node.start + frame.index] = value;
            heap.nodes[frame.input.a].length += 1;
            job.frames[job.frames.len - 1].index += 1; job.frames[job.frames.len - 1].pc = 0;
        } else if frame.index == node.count {
            // Completed maps become ordinary cursors without copying or
            // replacing their identity; all aliases see the same memo range.
            heap.nodes[frame.input.a].kind = 12;
            heap.nodes[frame.input.a].initialized = false;
            heap.nodes[frame.input.a].value = Value {}; heap.nodes[frame.input.a].namespace_value = nil();
            job.frames.pop();
        } else {
            if job.operands.len >= heap.limits.stack { fail(job, Fault.StackLimit, frame); return; }
            let arg = form_nth(heap, node.namespace_value, frame.index);
            job.frames[job.frames.len - 1].pc = 1;
            if frame.callee.tag == 5 && heap.nodes[frame.callee.a].kind == 4 {
                let id = heap.nodes[frame.callee.a].function;
                if id >= 5 && id <= 8 { request_effect(job, id, [arg], frame); }
                else { job.operands.push(native_value(heap, id, [arg])); }
            } else { enter(job, frame.callee, nil(), [arg]); }
        }
        if heap.fault != Fault.None { fail(job, heap.fault, frame); }
        return;
    }
    // mapv is a native collection iterator, with source callback calls saved
    // explicitly. A budget cut never replays a callback or loses its capture.
    if frame.function == -1 {
        if !charge(heap) { fail(job, heap.fault, frame); return; }
        if job.steps >= heap.limits.steps { fail(job, Fault.InstructionLimit, frame); return; }
        job.steps += 1;
        if frame.pc == 1 {
            let value = job.operands.pop(); let out = form_conj(heap, frame.output, value);
            job.frames[job.frames.len - 1].output = out; job.frames[job.frames.len - 1].pc = 0;
        } else if frame.index >= form_count(heap, frame.input) {
            job.frames.pop(); job.operands.resize(frame.operand_base);
            if job.frames.len == 0 { job.result = frame.output; job.status = tiny::Status.Complete; } else { job.operands.push(frame.output); }
        } else {
            let arg = form_nth(heap, frame.input, frame.index);
            job.frames[job.frames.len - 1].index += 1; job.frames[job.frames.len - 1].pc = 1;
            if frame.callee.tag == 5 && heap.nodes[frame.callee.a].kind == 4 {
                let id = heap.nodes[frame.callee.a].function;
                if id >= 5 && id <= 8 { request_effect(job, id, [arg], frame); }
                else { let v = native_value(heap, id, [arg]); job.operands.push(v); }
            } else { enter(job, frame.callee, Value {}, [arg]); }
        }
        if heap.fault != Fault.None { fail(job, heap.fault, frame); }
        return;
    }
    let f = code.functions[frame.function];
    if frame.pc < f.start || frame.pc >= f.start + f.count || frame.pc >= code.ins.len { fail(job, Fault.InvalidState, frame); return; }
    if job.steps >= heap.limits.steps { fail(job, Fault.InstructionLimit, frame); return; }
    if !charge(heap) { fail(job, heap.fault, frame); return; }
    let ins = code.ins[frame.pc]; let effect = code.effects[frame.pc]; let pop = effect.pop; let push = effect.push;
    if job.operands.len - frame.operand_base < pop { fail(job, Fault.InvalidState, frame); return; }
    if push - pop > heap.limits.stack - job.operands.len { fail(job, Fault.StackLimit, frame); return; }
    var demanded = false;
    match ins {
        Form x => { if x.op == 5 || x.op == 19 || x.op == 35 || x.op == 36 { demanded = demand_maps(job, job.operands[(job.operands.len - pop)..], x.op == 36); } },
        Call c => { demanded = demand_call(job, c.argc, false); },
        CallMethod c => { demanded = demand_call(job, c.argc, true); },
        _ => {},
    }
    if heap.fault != Fault.None { fail(job, heap.fault, frame); return; }
    if demanded { return; }
    job.steps += 1; job.frames[job.frames.len - 1].pc += 1;
    match ins {
        Const c => {
            let v = heap.constants[f.constants + c.k];
            // Symbol and BigInt constants have no VM value yet.
            if v.tag >= 6 { heap.fault = Fault.Unsupported; } else { job.operands.push(v); }
        },
        Undefined => { job.operands.push(Value {}); },
        Load l => { job.operands.push(job.locals[frame.local_base + l.slot]); },
        Store s => { job.locals[frame.local_base + s.slot] = job.operands.pop(); },
        Pop => { job.operands.pop(); },
        Dup => { let v = job.operands[job.operands.len - 1]; job.operands.push(v); },
        Binary b => { let y = job.operands.pop(); let x = job.operands.pop(); let v = binary_value(heap, b.op, x, y); job.operands.push(v); },
        Not => { let v = job.operands.pop(); job.operands.push(boolean(!truthy(v))); },
        Jump j => { job.frames[job.frames.len - 1].pc = j.to; },
        JumpUnless j => { let v = job.operands.pop(); if !truthy(v) { job.frames[job.frames.len - 1].pc = j.to; } },
        Return => {
            while job.handlers.len > 0 && job.handlers[job.handlers.len - 1].frame >= job.frames.len - 1 { job.handlers.pop(); }
            let v = job.operands.pop(); job.frames.pop(); job.locals.resize(frame.local_base); job.operands.resize(frame.operand_base);
            if job.frames.len == 0 { job.result = v; job.status = tiny::Status.Complete; } else { job.operands.push(v); }
        },
        NewObject => { let v = allocate(heap, Node { kind: 0 }); job.operands.push(v); },
        NewArray => { let v = allocate(heap, Node { kind: 1 }); job.operands.push(v); },
        Try t => { job.handlers.push(Handler { frame: job.frames.len - 1, pc: t.to, operands: job.operands.len, locals: job.locals.len }); },
        Throw => { let value = job.operands.pop(); raise(job, value, frame); },
        EndTry => { if job.handlers.len == 0 || job.handlers[job.handlers.len - 1].frame != job.frames.len - 1 { heap.fault = Fault.InvalidState; } else { job.handlers.pop(); } },
        Unary u => { let v = job.operands.pop(); let n = numeric(heap, v); job.operands.push(number(if u.op == 35 { -n } else if u.op == 113 { n + 1 } else if u.op == 114 { n - 1 } else { n })); },
        Numeric2 => { let b = job.operands.pop(); let a = job.operands.pop(); let x = numeric(heap, a); let y = numeric(heap, b); job.operands.push(number(x)); job.operands.push(number(y)); },
        Closure c => {
            let child = code.children[f.children + c.child]; let count = code.functions[child].captures; let base = job.operands.len - count;
            let v = closure(heap, child, job.operands[base..]); job.operands.resize(base); job.operands.push(v);
        },
        Call c => { call(job, frame, c.argc, false); },
        CallMethod c => { call(job, frame, c.argc, true); },
        Arguments => { arguments(job, frame, f, false, 0); },
        Rest r => { arguments(job, frame, f, true, r.skip); },
        Callee => { job.operands.push(frame.callee); },
        Capture c => { let node = heap.nodes[frame.callee.a]; job.operands.push(heap.values[node.start + c.slot]); },
        NewCell c => { let v = allocate(heap, Node { kind: 3, immutable: c.immutable }); job.operands.push(v); },
        InitCell => { cell(job, 48); },
        ReadCell => { cell(job, 49); },
        WriteCell => { cell(job, 50); },
        GetNamed g => { get_property(job, heap.constants[f.constants + g.k]); },
        GetIndexed => { let v = job.operands.pop(); let k = key(heap, v); get_property(job, k); },
        SetNamed s => { let value = job.operands.pop(); set_property(job, value, heap.constants[f.constants + s.k], false); },
        SetNamedChain s => { let value = job.operands.pop(); set_property(job, value, heap.constants[f.constants + s.k], true); },
        SetIndexed => { let value = job.operands.pop(); let v = job.operands.pop(); let k = key(heap, v); set_property(job, value, k, false); },
        Append => {
            let value = job.operands.pop(); let object = job.operands[job.operands.len - 1];
            let k = if valid(heap, object) && object.tag == 5 && heap.nodes[object.a].kind == 1 { key(heap, number(heap.nodes[object.a].length)) } else { heap.fault = Fault.TypeError; Value {} };
            set_property(job, value, k, true);
        },
        CheckValue => { let value = job.operands.pop(); if valid(heap, value) && value.tag == 5 && heap.nodes[value.a].kind == 3 { heap.fault = Fault.UnsupportedOperand; } job.operands.push(value); },
        This t => {
            var receiver = frame.receiver;
            if t.sloppy && !f.strict && (receiver.tag == 0 || receiver.tag == 1) { receiver = heap.global; }
            else if t.sloppy && !f.strict && receiver.tag != 5 { heap.fault = Fault.UnsupportedOperand; }
            job.operands.push(receiver);
        },
        Global => { job.operands.push(heap.global); },
        GetGlobal g => {
            let k = heap.constants[f.constants + g.k];
            let v, found = own_property(heap, heap.global, k);
            if !found {
                if job.protected_nodes > 0 { heap.fault = Fault.NativeBoundary; }
                // Keep prototype-dependent reads explicitly unsupported.
                get(heap, heap.global, k);
                if g.throws && heap.fault == Fault.None { heap.fault = Fault.ReferenceError; }
            }
            job.operands.push(v);
        },
        Literal l => {
            let base = job.operands.len - (l.count);
            let value = form_construct(heap, l.kind, job.operands[base..]); job.operands.resize(base); job.operands.push(value);
        },
        Form x => {
            let op = x.op; let base = job.operands.len - export_form_arity(op);
            // The live compiler adapter opts into a demand at the exact saved
            // intrinsic-call continuation. Sealed-plan users retain the
            // explicit DependencyNeed failure from form_op below.
            var waiting = false;
            if op == 82 && job.helpers_enabled {
                let args = job.operands[base..];
                if valid(heap, args[0]) && args[0].tag == 5 && heap.nodes[args[0].a].kind == 0 && args[1].tag <= 1 && args[2].tag == 4 {
                    let value, found = own_property(heap, args[0], args[2]);
                    if !found && heap.fault == Fault.None { request_effect(job, 9, args, frame); waiting = true; }
                }
            }
            if waiting { job.operands.resize(base); }
            else { let value = form_op(heap, op, job.operands[base..]); job.operands.resize(base); job.operands.push(value); }
        },
        // Every other registry opcode exports and validates but is not
        // implemented here: it fails the job by name, uncatchably, when it
        // runs. Never a fallback or a no-op (light-cljs-783).
        Unsupported => { heap.fault = Fault.Unsupported; },
    }
    if heap.fault != Fault.None { fail(job, heap.fault, frame); }
}
// A call's map demand (mapv/vec/into over a deferred map) runs before the
// call consumes its operands; see demand_map.
fn demand_call(job: Job&, argc: i64, method: bool) -> bool {
    let heap .= job.heap;
    let base = job.operands.len - argc - (if method { 2 } else { 1 });
    var callee = job.operands[base + (if method { 1 } else { 0 })];
    var first = job.operands.len - argc;
    if valid(heap, callee) && callee.tag == 5 && heap.nodes[callee.a].kind == 4 && heap.nodes[callee.a].function == 14 && method {
        callee = job.operands[base]; if first < job.operands.len { first += 1; }
    }
    if valid(heap, callee) && callee.tag == 5 && heap.nodes[callee.a].kind == 4 {
        let id = heap.nodes[callee.a].function;
        if id == 3 || id == 4 { return demand_maps(job, job.operands[first..]); }
    }
    return false;
}
fn call(job: Job&, frame: Frame, argc: i64, method: bool) {
    let heap .= job.heap;
    let base = job.operands.len - argc - (if method { 2 } else { 1 });
    var callee = job.operands[base + (if method { 1 } else { 0 })];
    var receiver = if method { job.operands[base] } else { Value {} };
    var first = job.operands.len - argc;
    // Function.prototype.call forwards its method receiver as callee.
    // Detached calls fail; missing thisArg is exactly undefined.
    if valid(heap, callee) && callee.tag == 5 && heap.nodes[callee.a].kind == 4 && heap.nodes[callee.a].function == 14 {
        callee = receiver; receiver = if first < job.operands.len { job.operands[first] } else { Value {} };
        if first < job.operands.len { first += 1; }
    }
    if valid(heap, callee) && callee.tag == 5 && heap.nodes[callee.a].kind == 4 {
        let id = heap.nodes[callee.a].function;
        let args = job.operands[first..];
        if id == 2 && args.len == 2 {
            if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; }
            else {
                let input = args[1]; let callback = args[0]; let out = form_collection(heap, 6, []);
                form_count(heap, input);
                job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: callback, receiver: Value {}, input: input, output: out });
                job.operands.resize(base);
            }
        } else if id >= 5 && id <= 8 { request_effect(job, id, args, frame); job.operands.resize(base); }
        else { let value = native_value(heap, id, args); job.operands.resize(base); job.operands.push(value); }
    } else if enter(job, callee, receiver, job.operands[first..]) { job.frames[job.frames.len - 1].operand_base = base; job.operands.resize(base); }
}
// `arguments` (unmapped, read-only) or a rest array after `skip` parameters.
fn arguments(job: Job&, frame: Frame, f: Function, rest: bool, skip_count: i64) {
    let heap .= job.heap;
    let start = frame.local_base + f.locals;
    let skip = if rest { min(skip_count, frame.argument_count) } else { 0 };
    let args = job.locals[(start + skip)..(start + frame.argument_count)];
    if !rest {
        if args.len > heap.limits.values - heap.values.len || heap.nodes.len >= heap.limits.nodes { memory(heap); }
        else if charge(heap, args.len) {
            let offset = heap.values.len; heap.values.append(args);
            job.operands.push(allocate(heap, Node { kind: 14, start: offset, count: args.len }));
        }
    } else {
        let array = allocate(heap, Node { kind: 1 });
        for value, i in args { if !put(heap, array, key(heap, number(i)), value) { break; } }
        job.operands.push(array);
    }
}
// Cell opcodes: 48 initialize, 49 read, 50 write.
fn cell(job: Job&, op: i64) {
    let heap .= job.heap;
    let value = if op == 49 { Value {} } else { job.operands.pop() }; let cell = job.operands.pop();
    if valid(heap, cell) && cell.tag == 5 && heap.nodes[cell.a].kind == 3 {
        let node = heap.nodes[cell.a];
        if op == 49 || op == 50 {
            if !node.initialized { heap.fault = Fault.ReferenceError; }
            else if op == 50 && node.immutable { heap.fault = Fault.ConstAssignment; }
            else if op == 49 { job.operands.push(node.value); }
            else { heap.nodes[cell.a].value = value; record(job, cell, Value {}, value); }
        } else if node.initialized && node.immutable { heap.fault = Fault.ConstAssignment; }
        else { heap.nodes[cell.a].value = value; heap.nodes[cell.a].initialized = true; record(job, cell, Value {}, value); }
    } else if heap.fault == Fault.None { heap.fault = Fault.TypeError; }
}
fn get_property(job: Job&, k: Value) {
    let heap .= job.heap;
    let object = job.operands.pop();
    if job.root_receiver.tag == 5 && object.tag == 5 && object.session == job.root_receiver.session && object.a == job.root_receiver.a {
        let value, found = own_property(heap, object, k);
        // Only the compiler root intrinsic may suspend and demand a
        // definition. A raw JS bypass must not observe stale undefined.
        if !found { heap.fault = Fault.DependencyNeed; } job.operands.push(value);
    } else if valid(heap, object) && object.tag == 5 && (heap.nodes[object.a].kind == 2 || heap.nodes[object.a].kind == 4) && key_is(heap, k, "call") {
        job.operands.push(get(heap, object, k));
    } else if object.tag == 5 && object.a >= 0 && object.a < job.protected_nodes {
        let value, found = own_property(heap, object, k);
        let optional = object.a == heap.global.a && (key_is(heap, k, "__lightCompilerRootReceiver") || key_is(heap, k, "__lightCompilerRootContext"));
        if !found && !optional { heap.fault = Fault.NativeBoundary; } job.operands.push(value);
    } else { let value = get(heap, object, k); job.operands.push(value); }
}
// chain: the assignment expression yields the object (81, 88), else the value.
fn set_property(job: Job&, value: Value, k: Value, chain: bool) {
    let heap .= job.heap;
    let object = job.operands.pop();
    if object.tag == 5 && object.a >= 0 && object.a < job.protected_nodes { heap.fault = Fault.NativeBoundary; }
    else if put(heap, object, k, value) { record(job, object, k, value); }
    job.operands.push(if chain { object } else { value });
}
fn claim(job: Job&) -> bool { if job.claimed || job.effect_pending || job.status != tiny::Status.Ready { return false; } job.claimed = true; return true; }
fn run_claimed(job: Job&, budget: i64) -> tiny::Outcome {
    if !job.claimed { return tiny::Outcome.Busy; }
    defer { job.claimed = false; }
    var used: i64 = 0;
    while job.status == tiny::Status.Ready && !job.effect_pending && (budget < 0 || used < budget) { step(job); used += 1; }
    return match job.status { Ready => tiny::Outcome.Yielded, Complete => tiny::Outcome.Complete, Failed => tiny::Outcome.Failed };
}
fn run(job: Job&, budget: i64) -> tiny::Outcome {
    if job.claimed { return tiny::Outcome.Busy; }
    if job.effect_pending { return tiny::Outcome.Yielded; }
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
