import std;
import binary;
import math;
import .program;
import .vm;
import .export_schema;

namespace source;

// Serial admission authority; it is never part of a worker heap snapshot.
var next_identity: i64 = 1;

// A JavaScript value: a 9-byte fixed-mode enum (light-cljs-785, decision 2
// of docs/javascript-coverage.md), never a Goose pointer. Object links a row
// of `heap.nodes`; String links a header in `heap.strings`, whose code
// units live in `heap.latin1`, one byte a unit, when every unit fits
// (Latin-1), and otherwise in `heap.units` as UTF-16 (light-cljs-800). The
// choice is canonical, so equal strings share an encoding. No value carries a session: a heap's rows are
// the only thing its links count from, so a copied image needs no
// relocation. Symbol keeps a well-known symbol index and BigInt no payload
// (constants only: `Const` fails Unsupported on both). Empty marks an array
// hole; it never leaves an array run (`valid` refuses it elsewhere).
// tag() gives the GSP primitive tag the rest of the module compares.
enum Value { Undefined, Null, Bool { v: bool }, Number { v: f64 }, String { s: u32 }, Symbol { s: u32 }, BigInt { b: u32 }, Object { o: u32 }, Empty }
// A string header. `wide` runs are UTF-16 units; a heap header that is not
// wide is a Latin-1 run in heap.latin1, and a heap string is wide only when
// some unit is above 255. Code and wire strings (Code, bridge graphs,
// artifacts) are always wide.
struct Str { start: u32, len: u32, wide: bool = true }
inline fn tag(v: Value) -> i64 { return match v { Undefined => 0, Null => 1, Bool => 2, Number => 3, String => 4, Object => 5, Symbol => 6, BigInt => 7, Empty => -1 }; }
// Every link between VM tables is a plain u32 row ID (light-cljs-800,
// decision 2), never a Goose index type: rows cross threads in every
// snapshot, saved continuation and message. no_row() marks an absent link;
// no table reaches it (snapshot::validate caps every table far below).
inline fn no_row() -> u32 { return 4294967295; }
// The object row, or no_row() for a non-object.
inline fn id(v: Value) -> u32 { return match v { Object x => x.o, _ => no_row() }; }
// The string header, or no_row() for a non-string.
inline fn sid(v: Value) -> u32 { return match v { String x => x.s, _ => no_row() }; }
inline fn num(v: Value) -> f64 { return match v { Number x => x.v, _ => 0 }; }
inline fn flag(v: Value) -> bool { return match v { Bool x => x.v, _ => false }; }
// A Symbol constant's well-known index.
fn symbol_index(v: Value) -> i64 { return match v { Symbol x => x.s, _ => -1 }; }
inline fn object(row: i64) -> Value { return Value.Object { o: row as u32 }; }
// Row access names its table (light-cljs-800): a row of another table, or
// past this one's end, fails here with the table's name. The explicit check
// is what Goose's bounds check would do, so the read itself keeps none.
// Readers take a const heap; *_mut and set_* write.
inline fn node(heap: const Heap&, o: i64) -> const Node& { if o < 0 || o >= heap.nodes.len { abort("row is not in table nodes"); } return heap.nodes[o]; }
inline fn node_mut(heap: Heap&, o: i64) -> Node& { if o < 0 || o >= heap.nodes.len { abort("row is not in table nodes"); } return heap.nodes[o]; }
fn set_node(heap: Heap&, o: i64, n: Node) { if o < 0 || o >= heap.nodes.len { abort("row is not in table nodes"); } heap.nodes[o] = n; }
inline fn slot(heap: const Heap&, row: i64) -> Value { if row < 0 || row >= heap.values.len { abort("row is not in table values"); } return heap.values[row]; }
inline fn set_slot(heap: Heap&, row: i64, v: Value) { if row < 0 || row >= heap.values.len { abort("row is not in table values"); } heap.values[row] = v; }
fn run(heap: const Heap&, start: i64, count: i64) -> const Value[:] { if start < 0 || count < 0 || start > heap.values.len || count > heap.values.len - start { abort("run is not in table values"); } return heap.values[start..(start + count)]; }
fn property_row(heap: const Heap&, row: i64) -> const Property& { if row < 0 || row >= heap.properties.len { abort("row is not in table properties"); } return heap.properties[row]; }
fn property_mut(heap: Heap&, row: i64) -> Property& { if row < 0 || row >= heap.properties.len { abort("row is not in table properties"); } return heap.properties[row]; }
fn set_property_row(heap: Heap&, row: i64, p: Property) { if row < 0 || row >= heap.properties.len { abort("row is not in table properties"); } heap.properties[row] = p; }
fn header(heap: const Heap&, s: i64) -> Str { if s < 0 || s >= heap.strings.len { abort("row is not in table strings"); } return heap.strings[s]; }
fn constant(heap: const Heap&, f: const Function&, k: i64) -> Value { let row = f.constants + k; if row < 0 || row >= heap.constants.len { abort("row is not in table constants"); } return heap.constants[row]; }
fn source_range(heap: const Heap&, r: i64) -> SourceRange { if r < 0 || r >= heap.source_ranges.len { abort("row is not in table source_ranges"); } return heap.source_ranges[r]; }
fn str_len(heap: const Heap&, v: Value) -> i64 { return if v.tag() != 4 { 0 } else { header(heap, v.sid()).len }; }
// The run of a wide (narrow) string; a header naming a run past its table
// fails by that table's name.
fn wide_run(heap: const Heap&, s: Str) -> const u16[:] {
    if !s.wide || s.start > heap.units.len || s.len > heap.units.len - s.start { abort("run is not in table units"); }
    return heap.units[s.start..(s.start + s.len)];
}
fn narrow_run(heap: const Heap&, s: Str) -> const u8[:] {
    if s.wide || s.start > heap.latin1.len || s.len > heap.latin1.len - s.start { abort("run is not in table latin1"); }
    return heap.latin1[s.start..(s.start + s.len)];
}
// Unit i of a string value, i < str_len.
fn unit(heap: const Heap&, v: Value, i: i64) -> u16 {
    let s = header(heap, v.sid());
    return if s.wide { wide_run(heap, s)[i] } else { narrow_run(heap, s)[i] };
}
// The string's UTF-16 units, appended (export to wire formats and tests).
fn append_utf16(heap: const Heap&, v: Value, out: u16[>..]&) {
    let s = header(heap, v.sid());
    if s.wide { out.append(wide_run(heap, s)); } else { for b in narrow_run(heap, s) { out.push(b); } }
}
fn utf16(heap: const Heap&, v: Value) -> u16[>..] { var out: u16[>..] = []; append_utf16(heap, v, out); return out; }
// Equal code units. Encodings are canonical, so different ones differ.
fn same_text(heap: const Heap&, a: Value, b: Value) -> bool {
    if a.sid() == b.sid() { return true; }
    let x = header(heap, a.sid()); let y = header(heap, b.sid());
    if x.len != y.len || x.wide != y.wide { return false; }
    return if x.wide { wide_run(heap, x) == wide_run(heap, y) } else { narrow_run(heap, x) == narrow_run(heap, y) };
}
fn fits_latin1(units: const u16[:]) -> bool { for u in units { if u > 255 { return false; } } return true; }
// A string value over units already in the table (a view or a copied image
// range); only the 16-byte header is new.
// Plain twins: a value's or row's fully written fields, for byte-exact
// comparison. An enum's unused payload bytes are not part of its value (an
// overwritten slot keeps the old payload), so VM rows are never compared or
// hashed with bytes_of directly. NaN payloads and signed zeros keep their bits.
struct Plain { tag: i64, number: f64 = 0, payload: i64 = 0 }
fn plain(v: Value) -> Plain {
    return match v {
        Number n => Plain { tag: 3, number: n.v }, Bool b => Plain { tag: 2, payload: if b.v { 1 } else { 0 } },
        String s => Plain { tag: 4, payload: s.s }, Object o => Plain { tag: 5, payload: o.o },
        Symbol s => Plain { tag: 6, payload: s.s }, BigInt b => Plain { tag: 7, payload: b.b }, _ => Plain { tag: v.tag() },
    };
}
fn plain_bytes(v: Value, out: u8[>..]&) { out.append(bytes_of([plain(v)])); }
// Bit-exact value identity: NaN equals itself, -0 differs from +0.
fn same_bits(a: Value, b: Value) -> bool { return bytes_of([plain(a)]) == bytes_of([plain(b)]); }
fn plain_bytes(n: Node, out: u8[>..]&) {
    out.append(bytes_of([n.kind, n.function, n.start, n.count, n.keys, n.key_count, n.key_capacity, n.length, n.origin, n.source, n.span_start, n.span_end])); out.append(bytes_of([n.initialized, n.immutable]));
    plain_bytes(n.value, out); plain_bytes(n.namespace_value, out); plain_bytes(n.metadata, out);
    for p in n.slots { plain_bytes(p, out); }
}
fn native_code(n: Native) -> i64 { return match n { None => 0, Mapv => 1, MapSeq => 2, ToPrimitive t => 3 + (t.op as i64) * 4 + (t.method as i64) * 1024 }; }
fn plain_bytes(f: Frame, out: u8[>..]&) {
    out.append(bytes_of([f.function, f.pc, f.local_base, f.operand_base, f.argument_count, f.index, native_code(f.native)]));
    plain_bytes(f.callee, out); plain_bytes(f.receiver, out); plain_bytes(f.input, out); plain_bytes(f.output, out);
}
fn plain_bytes(w: Write, out: u8[>..]&) { plain_bytes(w.object, out); plain_bytes(w.key, out); plain_bytes(w.value, out); }
fn plain_bytes(p: Property, out: u8[>..]&) { plain_bytes(p.key, out); plain_bytes(p.value, out); }
fn plain_rows<T>(rows: const T[:]) -> u8[>..] { var out: u8[>..] = []; for r in rows { plain_bytes(r, out); } return out; }
// kind: GSP constructor kind (0 none, 1 ordinary, 2 base class, 3 derived
// class, 4 default derived class).
struct Function { start: u32, count: u32, constants: u32, children: u32, params: u32, locals: u32, captures: u32, strict: bool, kind: i64 = 0 }
struct Op { code: i64, arg: i64, offset: i64, start: i64, end: i64, depth: i64 = -1 }
// One decoded instruction per validated `Op` row (light-cljs-785; decision 1
// of docs/javascript-coverage.md): a fixed-mode enum dispatched by `match` in
// `execute`. Payloads are the row's validated operand; jump targets are absolute
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
    // Finally blocks and completion records (light-cljs-786): the Pants
    // opcodes, with their stack contracts in rust/src/vm/instr.rs. Pending
    // is PendingThrow (118), PendingException (170) or PendingCancel (172).
    PushFinally { to: u32 }, PopFinally, EndFinally, CallFinally, DropCompletion, Pending { what: u8 },
}
// The row's stack effect from the generated registry table, resolved once at
// decode (-1 marks a row the table refuses; load never admits one).
struct Effect { pop: i32, push: i32 }
root struct Code {
    source_bytes: u8[>..]         = [],
    functions:    Function[>..]   = [], constants: Value[>..] = [], children: u32[>..] = [],
    ops:          Op[>..]         = [], units: u16[>..] = [], strings: Str[>..] = [], valid: bool = false,
    error:        tiny::LoadError = tiny::LoadError.None, error_pc: i64 = -1,
    ins:          Ins[>..]        = [], effects: Effect[>..] = [], decoded: i64 = 0,
    // Each decoded function's deepest operand height, from the validated
    // depths: a frame with that much stack room skips the per-instruction
    // stack checks (light-cljs-801).
    heights: u32[>..] = [],
}
// Unsupported: an exported opcode (or constant kind) this VM does not
// implement ran; Job.fault_opcode names it. Like every fault except the
// TypeError, ReferenceError, HostEffect and StackLimit throws (catchable(),
// light-cljs-786), JavaScript cannot catch it. Cancelled: the job's consumer
// cancelled it (cancel()); its finally blocks ran first.
enum Fault { None, InvalidState, StaleID, MemoryLimit, WorkLimit, InstructionLimit, StackLimit, TypeError, ReferenceError, ConstAssignment, UnsupportedOperand, NativeBoundary, DependencyNeed, EffectContract, HostEffect, Unsupported, Cancelled }
// Table limits count the rows a heap added above its image (Heap.image),
// not table lengths (light-cljs-792 constraint 4); each table checks its limit
// in its one allocation function. The call limits are the Pants engine's
// VmLimits::DEFAULT (light-cljs-786, rust/src/vm/frames.rs): at most `frames`
// live frames, the entry frame and native frames included, and at most
// `stack` values in the job's locals and operands together; a call or
// instruction that would pass either fails with a catchable StackLimit
// before it changes anything. Neither depends on the native stack: the
// dispatch loop never recurses per JavaScript call.
struct Limits { nodes: i64 = 100000, values: i64 = 1048576, units: i64 = 1048576, properties: i64 = 1048576, work: i64 = 10000000, steps: i64 = 1000000, frames: i64 = 10000, stack: i64 = 1048576 }
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
// Links (function, start, keys, source) and counts are plain u32 rows;
// source is no_row() for a node without provenance. origin is the compiler
// occurrence the node came from, not a VM row, and stays -1 when absent.
struct Node { kind: i64, function: u32 = 0, start: u32 = 0, count: u32 = 0, keys: u32 = 0, key_count: u32 = 0, key_capacity: u32 = 0, length: u32 = 0, initialized: bool = false, immutable: bool = false, value: Value = Value.Undefined, namespace_value: Value = Value.Null, metadata: Value = Value.Undefined, origin: i64 = -1, source: u32 = 4294967295, span_start: u32 = 0, span_end: u32 = 0, slots: Property[2] = [Property {}; 2] }
struct SourceRange { start: u32, count: u32 }
// An own property record (light-cljs-785, light-cljs-800). An object's
// key_count records, oldest first, are its header's inline slots and then
// its overflow run: record i < inline_slots() is slots[i], record i above is
// heap.properties[keys + i - inline_slots()], in a run of key_capacity rows.
// An object with at most two properties allocates no run; inline slots past
// key_count are cleared. There are no linked records.
struct Property { key: Value = Value.Undefined, value: Value = Value.Undefined }
// Table lengths when the heap became an image (restore); 0 for a heap built
// by initialize. Allocation limits count rows above these tops.
struct Tops { nodes: i64 = 0, values: i64 = 0, properties: i64 = 0, units: i64 = 0, strings: i64 = 0, latin1: i64 = 0 }
root struct Heap {
    code:          const Code&, identity: i64 = 0, limits: Limits = Limits {},
    nodes:         Node[>..]                  = [], values: Value[>..] = [], properties: Property[>..] = [],
    units:         u16[>..]                   = [], latin1: u8[>..] = [], strings: Str[>..] = [], constants: Value[>..] = [], work: i64 = 0,
    source_bytes:  u8[>..]                    = [], source_ranges: SourceRange[>..] = [],
    property_rows: i64                        = 0, image: Tops = Tops {},
    fault:         Fault                      = Fault.None, global: Value = Value.Undefined, call_method: Value = Value.Undefined, boot_count: i64 = 0, initializer_count: i64 = 0, producer_count: i64 = 0,
    live_jobs:     i64                        = 0,
}
// Native frames (light-cljs-786, decision 4): an operation that must run
// JavaScript from inside an instruction or built-in pushes a frame of
// function -1 whose `native` names the operation, with its whole state in
// the frame's fields (pc its phase, index its position, input/output/callee
// its values), and returns to the dispatch loop; nothing is a callback and
// nothing recurses in Goose. run_claimed steps the top native frame
// (native_step, a `match` on this enum). The protocol: a native step that
// needs JavaScript enters the function's frame above its own and sets its pc
// to the phase that consumes the result; the callee's Return pushes its value
// on the operands, at the native frame's operand base, and the next step pops
// it. An exception the callee does not catch unwinds through the native frame
// like any frame: raise() pops it and unwind_natives() lets it release its
// state (a deferred map's claim), so native frames never catch. A finished
// operation pops its frame, cuts the operands to its base and pushes its
// result for the frame below (or completes the job).
//   Mapv         mapv: callee the callback, input the source, output the
//                vector so far, index the next element; pc 1 awaits a result.
//   MapSeq       a deferred map's callbacks (demand_map): input the map,
//                index the next element; pc 1 awaits a result.
//   ToPrimitive  OrdinaryToPrimitive of the ordinary-object operands of the
//                binary operator `op`, left (input) then right (output),
//                index the operand being converted, method 0 valueOf and 1
//                toString; pc 1 awaits the method's result. The operator
//                then runs on the converted operands.
enum Native { None, Mapv, MapSeq, ToPrimitive { op: u8, method: u8 } }
// Actual arguments occupy bounded job-local scratch after declared locals.
// Copies live across yields/helper suspension and disappear on frame unwind.
struct Frame { function: i64, pc: i64, local_base: i64, operand_base: i64, callee: Value, receiver: Value, argument_count: i64 = 0, input: Value = Value.Undefined, output: Value = Value.Undefined, index: i64 = 0, native: Native = Native.None }
struct Write { object: Value, key: Value, value: Value }
// The handler stack (light-cljs-786; the Pants engine's vm/exception.rs):
// every entry belongs to one frame and records the operand and local heights
// to restore. Catch and Finally are handlers whose pc is their block. The
// others are pending completions, the reason a finally block of that frame
// is running: Normal (PopFinally), Return with `value`, Throw with the
// exception `value`, Cancel (the consumer's cancellation, cancel()), and
// Jump, a break or continue (CallFinally) resuming at pc. EndFinally pops the
// frame's pending completion and resumes it. Every entry is plain data: a
// suspended job saves its handlers with its frames.
enum HandlerKind { Catch, Finally, Normal, Return, Throw, Cancel, Jump }
struct Handler { frame: i64, pc: i64, operands: i64, locals: i64, kind: HandlerKind = HandlerKind.Catch, value: Value = Value.Undefined }
root struct Job {
    heap:            Heap&, frames: Frame[>..<] = [], locals: Value[>..<] = [], operands: Value[>..<] = [],
    writes:          Write[>..]                 = [], status: tiny::Status = tiny::Status.Ready, result: Value = Value.Undefined,
    handlers:        Handler[>..<]              = [],
    fault:           Fault                      = Fault.None, fault_pc: i64 = -1, fault_function: i64 = -1, fault_opcode: i64 = -1, thrown: bool = false,
    steps:           i64                        = 0, claimed: bool = false, registered: bool = false,
    effects_enabled: bool                       = false, effect_pending: bool = false, effect_kind: i64 = -1,
    helpers_enabled: bool                       = false,
    protected_nodes: i64                        = 0,
    // The job's write boundary (light-cljs-795, decision 3): node rows below
    // owned_nodes, and the [start, end) node ranges in foreign that
    // publications installed during the job, are rows the job did not
    // allocate. A JavaScript-visible write into one faults NativeBoundary
    // before it mutates; nothing is undone silently by the worker's reset.
    owned_nodes:   i64   = 0, foreign: u32[>..] = [],
    root_receiver: Value = Value.Undefined,
    // The compiler generated-name scope gensym numbers in (light-cljs-798):
    // the scope prefix and the next number of its count, 0 outside a scope.
    gensym_prefix: u8[>..] = [], gensym_next: i64 = 0,
    // Set by cancel(): the job is running its finally blocks for its
    // consumer's cancellation and then fails with Fault.Cancelled.
    cancelling:  bool     = false,
    effect_args: Value[8] = [Value.Undefined; 8], effect_count: i64 = 0, effect_frame: Frame = Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined },
}
inline fn number(n: f64) -> Value { return Value.Number { v: n }; }
inline fn boolean(b: bool) -> Value { return Value.Bool { v: b }; }
// A code constant's units: Code keeps its own string headers over its units.
fn code_chars(code: const Code&, v: Value) -> const u16[:] { let s = code.strings[v.sid()]; return code.units[s.start..(s.start + s.len)]; }
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
        57 => Ins.PushFinally { to: a }, 59 => Ins.PopFinally, 60 => Ins.EndFinally, 71 => Ins.CallFinally, 72 => Ins.DropCompletion,
        118, 170, 172 => Ins.Pending { what: op.code as u8 },
        _ => Ins.Unsupported { opcode: op.code as u8 },
    };
}
// Decode every function appended since the last call; earlier rows are
// immutable, so this is idempotent. Every Code owner calls it after it appends
// validated rows (load, append_code, snapshot restore, callable install).
fn decode(code: Code&) {
    // heights[i] is function i's: a Code reset (snapshot::reset) clears it
    // with the functions it describes.
    assert(code.heights.len == code.decoded);
    if code.ins.len < code.ops.len { code.ins.resize(code.ops.len, Ins.Pop); code.effects.resize(code.ops.len, Effect { pop: -1, push: -1 }); }
    for i in code.decoded..code.functions.len {
        let f = code.functions[i]; var height: i64 = 0;
        for pc in f.start..(f.start + f.count) {
            let op = code.ops[pc]; let pop, push = stack_effect(code, f, op);
            code.ins[pc] = instruction(op); code.effects[pc] = Effect { pop: pop as i32, push: push as i32 };
            if op.depth >= 0 { height = max(height, max(op.depth, op.depth - pop + push)); }
        }
        code.heights.push(height as u32);
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
        code.functions.push(Function { start: program::word(bytes, at) as u32, count: program::word(bytes, at + 4) as u32, constants: program::word(bytes, at + 8) as u32, children: program::word(bytes, at + 16) as u32, params: program::word(bytes, at + 24) as u32, locals: program::word(bytes, at + 28) as u32, captures: program::word(bytes, at + 32) as u32, strict: (flags & 1) == 1, kind: (flags >> 8) & 255 });
    }
    for i in program::word(bytes, 16) {
        let at = l.constants + i * 12; let tag = program::word(bytes, at);
        var value: Value = Value.Undefined;
        if tag == 3 { let n, ok = binary::read_f64le(bytes, at + 4); assert(ok); value = number(n); }
        // Symbol (6) keeps its well-known index. No BigInt (7) operation is
        // implemented, so its validated digits stay in the image; `Const`
        // fails Unsupported before either value reaches a JavaScript value.
        else if tag == 6 { value = Value.Symbol { s: program::word(bytes, at + 4) as u32 }; }
        else if tag == 7 { value = Value.BigInt { b: 0 }; }
        else if tag == 1 { value = Value.Null; }
        else if tag == 2 { value = boolean(program::word(bytes, at + 4) != 0); }
        else if tag == 4 { code.strings.push(Str { start: program::word(bytes, at + 4) as u32, len: program::word(bytes, at + 8) as u32 }); value = Value.String { s: (code.strings.len - 1) as u32 }; }
        code.constants.push(value);
    }
    for i in program::word(bytes, 20) { code.children.push(program::word(bytes, l.children + i * 4) as u32); }
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
    for f in other.functions { var n = f; n.start += ops as u32; n.constants += constants as u32; n.children += children as u32; out.functions.push(n); }
    for op in other.ops { var n = op; n.start += bytes; n.end += bytes; if branches(n.code) { n.arg += ops; } out.ops.push(n); }
    let strings = out.strings.len;
    for s in other.strings { out.strings.push(Str { start: s.start + units as u32, len: s.len }); }
    for v in other.constants { out.constants.push(if v.tag() == 4 { Value.String { s: (v.sid() + strings) as u32 } } else { v }); }
    for id in other.children { out.children.push((functions + id) as u32); }
    out.units.append(other.units); out.source_bytes.append(other.source_bytes); decode(out); out.valid = true;
    return functions;
}
fn memory(heap: Heap&) -> bool { heap.fault = Fault.MemoryLimit; return false; }
fn valid(heap: Heap&, v: Value) -> bool {
    if heap.fault != Fault.None { return false; }
    return match v {
        Empty, Symbol, BigInt => { heap.fault = Fault.InvalidState; false },
        Object o => if o.o < heap.nodes.len { true } else { heap.fault = Fault.StaleID; false },
        String s => if s.s < heap.strings.len { true } else { heap.fault = Fault.StaleID; false },
        _ => true,
    };
}
// One allocation function per table (light-cljs-800; light-cljs-792
// constraints 1 and 4): only these grow a table. Each refuses, with
// MemoryLimit and nothing appended, rows past its limit counted above the
// heap's image tops, so a truncation to the image (light-cljs-795) needs a top
// only here. add_properties is the property table's. Constants and source
// provenance are installed with code by compiler services, never by a job, so
// their tables have no job limit.
fn room(heap: Heap&, length: i64, top: i64, limit: i64, count: i64) -> bool {
    if count < 0 || count > limit - (length - top) { return memory(heap); }
    return true;
}
// `nodes` then `extra` copies of `fill`; returns the first row, or -1.
fn add_nodes(heap: Heap&, nodes: const Node[:], extra: i64 = 0, fill: Node = Node { kind: 0 }) -> i64 {
    if extra < 0 || !room(heap, heap.nodes.len, heap.image.nodes, heap.limits.nodes, nodes.len + extra) { return -1; }
    let at = heap.nodes.len; heap.nodes.append(nodes); for i in extra { heap.nodes.push(fill); }
    return at;
}
fn allocate(heap: Heap&, node: Node) -> Value {
    if !charge(heap) { return Value.Undefined; }
    let at = add_nodes(heap, [node]);
    return if at < 0 { Value.Undefined } else { object(at) };
}
// `values` then `extra` copies of `fill` (array spare capacity is holes);
// returns the first row, or -1.
fn add_values(heap: Heap&, values: const Value[:], extra: i64 = 0, fill: Value = Value.Undefined) -> i64 {
    if extra < 0 || !room(heap, heap.values.len, heap.image.values, heap.limits.values, values.len + extra) { return -1; }
    let at = heap.values.len; heap.values.append(values); for i in extra { heap.values.push(fill); }
    return at;
}
// Whether `count` more value rows fit, for preflights that must fail before
// any other table changes.
fn values_fit(heap: const Heap&, count: i64) -> bool { return count >= 0 && count <= heap.limits.values - (heap.values.len - heap.image.values); }
fn nodes_fit(heap: const Heap&, count: i64) -> bool { return count >= 0 && count <= heap.limits.nodes - (heap.nodes.len - heap.image.nodes); }
// Code-unit rows the heap added above its image, in both unit tables: the
// unit limit counts them together.
fn unit_rows(heap: const Heap&) -> i64 { return heap.units.len - heap.image.units + heap.latin1.len - heap.image.latin1; }
fn units_fit(heap: const Heap&, count: i64) -> bool { return count >= 0 && count <= heap.limits.units - unit_rows(heap); }
fn add_units(heap: Heap&, units: const u16[:]) -> i64 {
    if !units_fit(heap, units.len) { memory(heap); return -1; }
    let at = heap.units.len; heap.units.append(units); return at;
}
fn add_latin1(heap: Heap&, bytes: const u8[:]) -> i64 {
    if !units_fit(heap, bytes.len) { memory(heap); return -1; }
    let at = heap.latin1.len; heap.latin1.append(bytes); return at;
}
// The string header table's one allocation function, over a run already in
// its unit table (a view, a copied image range or new units): `wide` names
// heap.units, otherwise heap.latin1. Callers keep the encoding canonical.
// Header rows share the unit limit: an empty string still takes one.
fn string_ref(heap: Heap&, start: i64, len: i64, wide: bool) -> Value {
    let length = if wide { heap.units.len } else { heap.latin1.len };
    if start < 0 || len < 0 || start > length || len > length - start { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if !room(heap, heap.strings.len, heap.image.strings, heap.limits.units, 1) { return Value.Undefined; }
    heap.strings.push(Str { start: start as u32, len: len as u32, wide: wide }); return Value.String { s: (heap.strings.len - 1) as u32 };
}
// A new string of these UTF-16 units in its canonical encoding, without a
// work charge (callers charge or preflight); Undefined with MemoryLimit.
fn import_text(heap: Heap&, units: const u16[:]) -> Value {
    if !units_fit(heap, units.len) || heap.strings.len - heap.image.strings >= heap.limits.units { memory(heap); return Value.Undefined; }
    if !fits_latin1(units) { return string_ref(heap, add_units(heap, units), units.len, true); }
    var bytes: u8[>..] = []; for u in units { bytes.push(u as u8); }
    return string_ref(heap, add_latin1(heap, bytes), bytes.len, false);
}
fn add_constants(heap: Heap&, values: const Value[:]) -> i64 { let at = heap.constants.len; heap.constants.append(values); return at; }
fn add_source_bytes(heap: Heap&, bytes: const u8[:]) -> i64 { let at = heap.source_bytes.len; heap.source_bytes.append(bytes); return at; }
fn add_source_range(heap: Heap&, r: SourceRange) -> i64 { heap.source_ranges.push(r); return heap.source_ranges.len - 1; }
fn string(heap: Heap&, units: const u16[:]) -> Value {
    if !charge(heap, units.len) { return Value.Undefined; }
    return import_text(heap, units);
}
// Bytes as units, one a byte: always Latin-1.
fn text(heap: Heap&, bytes: const u8[:]) -> Value {
    if !charge(heap, bytes.len) { return Value.Undefined; }
    if !units_fit(heap, bytes.len) || heap.strings.len - heap.image.strings >= heap.limits.units { memory(heap); return Value.Undefined; }
    return string_ref(heap, add_latin1(heap, bytes), bytes.len, false);
}
// One unit of an existing string as a new string, in the same encoding.
fn unit_string(heap: Heap&, v: Value, i: i64) -> Value { return string(heap, [unit(heap, v, i)]); }
fn initialize(heap: Heap&) -> bool {
    if !heap.code.valid || heap.identity != 0 || heap.nodes.len != 0 || heap.units.len != 0 || heap.latin1.len != 0 || heap.strings.len != 0 || heap.constants.len != 0 { heap.fault = Fault.InvalidState; return false; }
    heap.identity = reserve_identity();
    if heap.identity == 0 { return memory(heap); }
    var units: i64 = 0; for s in heap.code.strings { units += s.len; }
    if units > heap.limits.units || heap.code.strings.len > heap.limits.units || heap.code.constants.len > heap.limits.values { return memory(heap); }
    if !charge(heap, units + heap.code.constants.len) { return false; }
    // The heap starts empty, so each code string keeps its header index; its
    // units take their canonical encoding.
    for s in heap.code.strings { import_text(heap, heap.code.units[s.start..(s.start + s.len)]); }
    add_constants(heap, heap.code.constants);
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
// Strings are truthy unless empty; truthy reads the header.
inline fn truthy(heap: const Heap&, v: Value) -> bool { return match v { Undefined, Null, Empty => false, Bool b => b.v, Number n => n.v != 0 && n.v == n.v, String s => heap.strings[s.s].len != 0, _ => true }; }
fn equal(heap: Heap&, a: Value, b: Value) -> bool {
    if !valid(heap, a) || !valid(heap, b) || a.tag() != b.tag() { return false; }
    return match a.tag() {
        0, 1 => true, 2 => a.flag() == b.flag(), 3 => a.num() == b.num(),
        4 => { if !charge(heap, str_len(heap, a) + str_len(heap, b)) { return false; } same_text(heap, a, b) },
        _ => a == b,
    };
}
fn numeric(heap: Heap&, v: Value) -> f64 {
    if !valid(heap, v) { return 0; }
    let x = form_value(heap, v);
    return match x.tag() { 0 => 0.0 / 0.0, 1 => 0.0, 2 => if x.flag() { 1.0 } else { 0.0 }, 3 => x.num(), _ => { heap.fault = Fault.UnsupportedOperand; 0.0 } };
}
fn key(heap: Heap&, value: Value) -> Value {
    if !valid(heap, value) { return Value.Undefined; }
    if value.tag() == 4 { return value; }
    if value.tag() == 3 && value.num() >= 0 && value.num() < 4294967295.0 && value.num() == (value.num() as i64) { let bytes = str(value.num() as i64); return text(heap, bytes); }
    heap.fault = Fault.UnsupportedOperand; return Value.Undefined;
}
fn key_is(heap: Heap&, value: Value, bytes: const u8[:]) -> bool {
    if value.tag() != 4 || str_len(heap, value) != bytes.len || !charge(heap, bytes.len) { return false; }
    let s = header(heap, value.sid());
    if !s.wide { return narrow_run(heap, s) == bytes; }
    let units = wide_run(heap, s);
    for byte, i in bytes { if units[i] != byte { return false; } } return true;
}
fn index(heap: Heap&, k: Value) -> i64 {
    if k.tag() != 4 { return -1; }
    // An array index is decimal digits: always Latin-1.
    let s = header(heap, k.sid());
    if s.wide || s.len == 0 || s.len > 10 { return -1; }
    if !charge(heap, s.len) { return -1; }
    let digits = narrow_run(heap, s);
    var n: i64 = 0;
    for c, i in digits { if c < 48 || c > 57 || (i == 0 && c == 48 && digits.len > 1) { return -1; } n = n * 10 + (c - 48); }
    return if n < 4294967295 { n } else { -1 };
}
// Arrays (light-cljs-785). A dense array (kind 1) keeps its elements in its
// own run heap.values[start..start + count], indexed by integer; count is the
// run's capacity and `length` the JavaScript length, which may exceed it. A
// slot never written, or cut off by a shorter length, holds a hole. A hole
// never leaves its run: element reads report it absent (undefined), and
// valid() refuses it anywhere else. A write past the capacity copies the
// elements into a new, larger run at the end of the table and clears the old
// run to undefined; tables stay append-only, so the old run remains charged
// until teardown, as removed property records did before. A write far past
// the run (sparse_write) turns the array into a dictionary array
// (`initialized` set, no run) whose elements are index-keyed property
// records, as every array's were before, so a sparse array costs a record an
// element, not a slot an index.
fn hole() -> Value { return Value.Empty; }
// The canonical array index of a number key (what key() then index() would
// give), or -1.
fn number_index(v: Value) -> i64 {
    return if v.tag() == 3 && v.num() >= 0 && v.num() < 4294967295.0 && v.num() == (v.num() as i64) { v.num() as i64 } else { -1 };
}
fn own_element(heap: Heap&, array: Value, i: i64) -> Value, bool {
    if node(heap, array.id()).initialized { let k = key(heap, number(i)); return own_record(heap, array, k); }
    // Field reads: a whole Node copy would dominate an element access.
    if i >= node(heap, array.id()).length || i >= node(heap, array.id()).count { return Value.Undefined, false; }
    let v = slot(heap, node(heap, array.id()).start + i);
    if v.tag() == -1 { return Value.Undefined, false; }
    return v, true;
}
// More than doubling the run would leave most of its slots holes.
fn sparse_write(count: i64, i: i64) -> bool { return i >= count && i > 2 * count + 1024; }
fn set_element(heap: Heap&, array: Value, i: i64, value: Value) -> bool {
    if i >= heap.limits.values { return memory(heap); }
    if !node(heap, array.id()).initialized && sparse_write(node(heap, array.id()).count, i) && !to_dictionary(heap, array) { return false; }
    if node(heap, array.id()).initialized {
        let k = key(heap, number(i));
        if heap.fault != Fault.None || !put_record(heap, array, k, value) { return false; }
        if i >= node(heap, array.id()).length { node_mut(heap, array.id()).length = (i + 1) as u32; }
        return true;
    }
    let count = node(heap, array.id()).count;
    if i >= count {
        let old = node(heap, array.id()).start;
        var capacity = max(i + 1, max(count * 2, 4));
        if !values_fit(heap, capacity) { capacity = i + 1; }
        if !values_fit(heap, capacity) { return memory(heap); }
        if !charge(heap, capacity + count) { return false; }
        let start = add_values(heap, run(heap, old, count), capacity - count, hole());
        // No node owns the old run now: holes may live only in array runs.
        for at in old..(old + count) { set_slot(heap, at, Value.Undefined); }
        node_mut(heap, array.id()).start = start as u32; node_mut(heap, array.id()).count = capacity as u32;
    } else if !charge(heap) { return false; }
    set_slot(heap, node(heap, array.id()).start + i, value);
    if i >= node(heap, array.id()).length { node_mut(heap, array.id()).length = (i + 1) as u32; }
    return true;
}
// Move a dense array's elements into index-keyed property records, a new key
// run at the table's end. Every failure point precedes the header change, so
// a rejection leaves the array as it was (records and keys built so far stay
// charged, unowned).
fn to_dictionary(heap: Heap&, array: Value) -> bool {
    let o = array.id(); let start = node(heap, o).start; let count = node(heap, o).count; let end = min(node(heap, o).length, count);
    var records: i64 = 0; for i in end { if slot(heap, start + i).tag() != -1 { records += 1; } }
    let spill = max(0, records - inline_slots());
    let run = add_properties(heap, spill);
    if run < 0 { return false; }
    // Records are built aside: the header changes only after every failure point.
    var built: Property[>..] = [];
    for i in end {
        let v = slot(heap, start + i);
        if v.tag() == -1 { continue; }
        let k = key(heap, number(i));
        if heap.fault != Fault.None { return false; }
        built.push(Property { key: k, value: v });
    }
    if !charge(heap, count) { return false; }
    for row in start..(start + count) { set_slot(heap, row, Value.Undefined); }
    node_mut(heap, o).keys = run as u32; node_mut(heap, o).key_count = records as u32; node_mut(heap, o).key_capacity = spill as u32; node_mut(heap, o).initialized = true; node_mut(heap, o).count = 0;
    for p, i in built { set_own_at(heap, o, i, p); }
    return true;
}
// The one allocation point of the property table: `count` cleared rows at
// its end, charged to the rows the heap has added above its image
// (limits.properties counts allocated rows, spare capacity included, not live
// records; property_rows is their running total for snapshot validation). Returns the run's
// first row, or -1 with MemoryLimit.
fn inline_slots() -> i64 { return 2; }
// Record i of an object (see Property); i < key_count.
fn own_at(heap: const Heap&, o: i64, i: i64) -> Property {
    let n .= node(heap, o);
    return if i < inline_slots() { n.slots[i] } else { property_row(heap, n.keys + i - inline_slots()) };
}
fn set_own_at(heap: Heap&, o: i64, i: i64, p: Property) {
    if i < inline_slots() { node_mut(heap, o).slots[i] = p; } else { set_property_row(heap, node(heap, o).keys + i - inline_slots(), p); }
}
// Records an object keeps in its overflow run.
fn overflow(n: const Node&) -> i64 { return max(0, n.key_count - inline_slots()); }
fn add_properties(heap: Heap&, count: i64) -> i64 {
    if !room(heap, heap.properties.len, heap.image.properties, heap.limits.properties, count) { return -1; }
    let at = heap.properties.len; heap.properties.resize(at + count, Property {}); heap.property_rows += count;
    return at;
}
// Newest first: the order the former linked records were visited in, so a
// lookup charges the same work.
fn own_record(heap: Heap&, object: Value, k: Value) -> Value, bool {
    let o = object.id(); var i = node(heap, o).key_count;
    while i > 0 {
        i -= 1;
        if !charge(heap) { return Value.Undefined, false; }
        let property = own_at(heap, o, i);
        if equal(heap, property.key, k) { return property.value, true; }
    }
    return Value.Undefined, false;
}
// Return presence separately: an own property's value can be undefined.
fn own_property(heap: Heap&, object: Value, k: Value) -> Value, bool {
    if node(heap, object.id()).kind == 1 {
        let i = index(heap, k);
        if i < 0 { return Value.Undefined, false; }
        if node(heap, object.id()).initialized { return own_record(heap, object, k); }
        return own_element(heap, object, i);
    }
    return own_record(heap, object, k);
}
fn get(heap: Heap&, object: Value, k: Value) -> Value {
    if !valid(heap, object) || !valid(heap, k) { return Value.Undefined; }
    if k.tag() != 4 { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if object.tag() == 4 {
        if key_is(heap, k, "length") { return number(str_len(heap, object)); }
        let i = index(heap, k); if i >= 0 { if i >= str_len(heap, object) { return Value.Undefined; } return unit_string(heap, object, i); }
        heap.fault = Fault.UnsupportedOperand; return Value.Undefined;
    }
    if object.tag() != 5 { heap.fault = if object.tag() <= 1 { Fault.TypeError } else { Fault.UnsupportedOperand }; return Value.Undefined; }
    let row = node(heap, object.id());
    if (row.kind == 2 || row.kind == 4) && key_is(heap, k, "call") { return heap.call_method; }
    // Unmapped, read-only arguments subset. Mutation, callee and prototypes
    // fail explicitly; no frame slot or parameter alias escapes to the heap.
    if row.kind == 14 {
        if key_is(heap, k, "length") { return number(row.count); }
        let i = index(heap, k);
        if i >= 0 { return if i < row.count { slot(heap, row.start + i) } else { Value.Undefined }; }
        heap.fault = Fault.UnsupportedOperand; return Value.Undefined;
    }
    if row.kind == 1 && key_is(heap, k, "length") { return number(row.length); }
    if row.kind >= 2 { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
    let value, found = own_property(heap, object, k);
    if found { return value; }
    if heap.fault != Fault.None { return Value.Undefined; }
    if row.kind == 1 && index(heap, k) < 0 { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
    // Prototype-dependent behavior is deliberately unsupported. A missing own
    // ordinary data property is undefined, except known prototype names.
    if key_is(heap, k, "__proto__") || key_is(heap, k, "constructor") || key_is(heap, k, "toString") || key_is(heap, k, "valueOf") || key_is(heap, k, "call") || key_is(heap, k, "apply") || key_is(heap, k, "hasOwnProperty") || key_is(heap, k, "isPrototypeOf") || key_is(heap, k, "propertyIsEnumerable") || key_is(heap, k, "toLocaleString") || key_is(heap, k, "__defineGetter__") || key_is(heap, k, "__defineSetter__") || key_is(heap, k, "__lookupGetter__") || key_is(heap, k, "__lookupSetter__") { heap.fault = Fault.UnsupportedOperand; }
    return Value.Undefined;
}
fn put(heap: Heap&, object: Value, k: Value, value: Value) -> bool {
    if !valid(heap, object) || !valid(heap, k) || !valid(heap, value) { return false; }
    if object.tag() != 5 || k.tag() != 4 || node(heap, object.id()).kind >= 2 { heap.fault = if object.tag() == 5 { Fault.UnsupportedOperand } else { Fault.TypeError }; return false; }
    if key_is(heap, k, "__proto__") { heap.fault = Fault.UnsupportedOperand; return false; }
    if node(heap, object.id()).kind == 1 {
        if key_is(heap, k, "length") { return array_length(heap, object, value); }
        let i = index(heap, k);
        if heap.fault != Fault.None { return false; }
        if i < 0 { heap.fault = Fault.UnsupportedOperand; return false; }
        return set_element(heap, object, i, value);
    }
    return put_record(heap, object, k, value);
}
// A new key goes at the end of the object's run. A full run moves to a run of
// twice the capacity at the table's end, allocated (and charged, spare rows
// included) through add_properties; the old rows are cleared and stay charged:
// tables are append-only within a job. A rejection changes nothing.
fn put_record(heap: Heap&, object: Value, k: Value, value: Value) -> bool {
    let o = object.id(); let count = node(heap, o).key_count;
    var i = count;
    while i > 0 {
        i -= 1;
        if !charge(heap) { return false; }
        let p = own_at(heap, o, i);
        if equal(heap, p.key, k) { set_own_at(heap, o, i, Property { key: p.key, value: value }); return true; }
    }
    if heap.fault != Fault.None { return false; }
    let spilled = count - inline_slots();
    if spilled >= 0 && spilled == node(heap, o).key_capacity {
        let start = node(heap, o).keys; let capacity = max(4, spilled * 2);
        let at = add_properties(heap, capacity);
        if at < 0 { return false; }
        for i in spilled { set_property_row(heap, at + i, property_row(heap, start + i)); set_property_row(heap, start + i, Property {}); }
        node_mut(heap, o).keys = at as u32; node_mut(heap, o).key_capacity = capacity as u32;
    }
    set_own_at(heap, o, count, Property { key: k, value: value }); node_mut(heap, o).key_count = count + 1;
    return heap.fault == Fault.None;
}
// Bounded numeric length writes only. A shorter length turns a dense array's
// cut-off slots back into holes, or drops a dictionary array's cut-off
// records from its run (their rows stay charged until teardown); the work is charged before
// anything changes, so a work rejection is atomic. A longer length writes
// nothing.
fn array_length(heap: Heap&, object: Value, value: Value) -> bool {
    if value.tag() != 3 || !(value.num() >= 0 && value.num() <= 4294967295.0) || value.num() != trunc(value.num()) { heap.fault = Fault.UnsupportedOperand; return false; }
    let length = value.num() as i64;
    if length > heap.limits.values { return memory(heap); }
    if node(heap, object.id()).initialized { return dictionary_length(heap, object, length); }
    let start = node(heap, object.id()).start; let end = min(node(heap, object.id()).length, node(heap, object.id()).count);
    if length < end {
        if !charge(heap, end - length) { return false; }
        for i in length..end { set_slot(heap, start + i, hole()); }
    }
    node_mut(heap, object.id()).length = length as u32; return true;
}
// Preflight the full walk before mutation so a work rejection is atomic.
fn dictionary_length(heap: Heap&, object: Value, length: i64) -> bool {
    if length < node(heap, object.id()).length {
        let o = object.id(); let count = node(heap, o).key_count;
        for r in count {
            if !charge(heap, 2 + str_len(heap, own_at(heap, o, r).key)) { return false; }
            let i = index(heap, own_at(heap, o, r).key);
            if heap.fault != Fault.None { return false; }
            if i < 0 { heap.fault = Fault.UnsupportedOperand; return false; }
        }
        // Compact the kept records in order; the cut-off rows are cleared.
        var kept: i64 = 0;
        for r in count {
            let p = own_at(heap, o, r);
            // set_element admitted canonical indices. The preflight charged
            // this second decode, so mutation below cannot fail.
            var i: i64 = 0;
            for c in narrow_run(heap, header(heap, p.key.sid())) { i = i * 10 + (c - 48); }
            if i < length { set_own_at(heap, o, kept, p); kept += 1; }
        }
        for r in kept..count { set_own_at(heap, o, r, Property {}); }
        node_mut(heap, o).key_count = kept as u32;
    }
    node_mut(heap, object.id()).length = length as u32; return true;
}
fn closure(heap: Heap&, function: i64, captures: const Value[:]) -> Value {
    if function < 0 || function >= heap.code.functions.len || captures.len != heap.code.functions[function].captures { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if !values_fit(heap, captures.len) || !charge(heap, captures.len) { if heap.fault == Fault.None { memory(heap); } return Value.Undefined; }
    for v in captures { if !valid(heap, v) { return Value.Undefined; } }
    let start = add_values(heap, captures);
    return allocate(heap, Node { kind: 2, function: function as u32, start: start as u32, count: captures.len as u32 });
}
// The native frames from `keep` up release their state as an unwind passes
// them (see Native): a deferred map gives up its claim, so a later demand
// retries it. No native frame catches.
fn unwind_natives(job: Job&, keep: i64) {
    for i in keep..job.frames.len {
        let f = job.frames[i];
        match f.native { MapSeq => { node_mut(job.heap, f.input.id()).initialized = false; }, _ => {} }
    }
}
// The minimal error object (light-cljs-786, until the realm exists): a node
// of kind 13 whose `function` is error_code of the catchable fault it was
// raised for. JavaScript sees an object with identity and truthiness; its
// properties stay unsupported. Uncaught, it reports that fault again.
fn error_code(fault: Fault) -> u32 { return match fault { TypeError => 1, ReferenceError => 2, HostEffect => 3, StackLimit => 4, _ => 0 }; }
fn error_fault(code: i64) -> Fault { return match code { 1 => Fault.TypeError, 2 => Fault.ReferenceError, 3 => Fault.HostEffect, 4 => Fault.StackLimit, _ => Fault.InvalidState }; }
// The JavaScript error name of a catchable fault, as the Pants engine and
// QuickJS name it (a call-depth or value-stack overflow is QuickJS's
// InternalError "stack overflow").
fn error_name(fault: Fault) -> const u8[:] { return match fault { TypeError => "TypeError", ReferenceError => "ReferenceError", StackLimit => "InternalError", _ => "Error" }; }
fn catchable(fault: Fault) -> bool { return error_code(fault) != 0; }
// Whether an exception raised now would reach a catch or finally block.
fn catches(job: const Job&) -> bool {
    for h in job.handlers { if h.kind == HandlerKind.Catch || h.kind == HandlerKind.Finally { return true; } }
    return false;
}
// End the job with `reason` at `frame`'s instruction: no catch or finally
// block runs. A job cancelling (cancel()) ends Cancelled whatever ended its
// cleanup, an uncaught exception, a limit or any other fault, as the Pants
// engine's terminal cleanup does; its thrown value is no result.
fn terminate(job: Job&, reason: Fault, frame: Frame) {
    unwind_natives(job, 0);
    let fault = if job.cancelling { Fault.Cancelled } else { reason };
    if job.cancelling { job.thrown = false; job.result = Value.Undefined; }
    job.status = tiny::Status.Failed; job.fault = fault; job.fault_function = frame.function; job.fault_pc = frame.pc;
    let code .= job.heap.code;
    job.fault_opcode = if fault == Fault.Unsupported && frame.function >= 0 && frame.pc >= 0 && frame.pc < code.ops.len { code.ops[frame.pc].code } else { -1 };
}
// A fault at `frame`'s instruction. A catchable one some handler would see
// is raised as a minimal error object; every other fault ends the job.
fn fail(job: Job&, fault: Fault, frame: Frame) {
    job.heap.fault = Fault.None;
    var reported = fault;
    if catchable(fault) && catches(job) {
        let error = allocate(job.heap, Node { kind: 13, function: error_code(fault) });
        if job.heap.fault == Fault.None { raise(job, error, frame); return; }
        reported = job.heap.fault; job.heap.fault = Fault.None;
    }
    terminate(job, reported, frame);
}
// Unwind `h`, a handler of a live frame: the frames above its frame are
// popped (their native frames released) and its heights restored.
fn unwind_to(job: Job&, h: Handler) -> bool {
    if h.frame < 0 || h.frame >= job.frames.len || h.operands > job.operands.len || h.locals > job.locals.len { return false; }
    unwind_natives(job, h.frame + 1);
    job.frames.resize(h.frame + 1); job.locals.resize(h.locals); job.operands.resize(h.operands);
    job.frames[h.frame].pc = h.pc;
    return true;
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
        let tag = code.constants[code.functions[job.fault_function].constants + code.ops[job.fault_pc].arg].tag();
        out.append(if tag == 6 { "(Symbol)" } else if tag == 7 { "(BigInt)" } else { "" });
    }
}
// Raise `value` (light-cljs-786): unwind to the innermost catch or finally
// handler of any frame, discarding the pending completions passed on the way,
// and continue there: a catch block starts with the exception pushed, a
// finally block with a pending Throw completion. A pending cancellation it
// would discard ends the cleanup instead (Cancelled). Uncaught, an error
// object reports its fault and any other value is a thrown result. Never
// recurses, whatever the number of frames.
fn raise(job: Job&, value: Value, frame: Frame) {
    while job.handlers.len > 0 {
        let h = job.handlers.pop();
        if h.kind == HandlerKind.Cancel { terminate(job, Fault.Cancelled, frame); return; }
        if h.kind != HandlerKind.Catch && h.kind != HandlerKind.Finally { continue; }
        if !unwind_to(job, h) { terminate(job, Fault.InvalidState, frame); return; }
        if h.kind == HandlerKind.Catch { job.operands.push(value); }
        else { job.handlers.push(Handler { frame: h.frame, pc: h.pc, operands: h.operands, locals: h.locals, kind: HandlerKind.Throw, value: value }); }
        return;
    }
    if value.tag() == 5 && node(job.heap, value.id()).kind == 13 { terminate(job, error_fault(node(job.heap, value.id()).function), frame); return; }
    job.result = value; job.thrown = true; terminate(job, Fault.HostEffect, frame);
}
// Return `value` from the frame at `top` (light-cljs-786): its innermost
// finally handler, if any, becomes a pending Return completion and its block
// runs first (the entries above it, catch handlers and the pending
// completions this return overrides, are discarded); otherwise the frame is
// left and the value delivered to the frame below, or the job completes. A
// return that would discard a pending cancellation ends the cleanup.
fn return_value(job: Job&, top: i64, value: Value, at: i64) {
    while job.handlers.len > 0 && job.handlers[job.handlers.len - 1].frame >= top {
        let h = job.handlers[job.handlers.len - 1];
        if h.frame == top && h.kind == HandlerKind.Cancel { terminate(job, Fault.Cancelled, frame_at(job, top, at)); return; }
        if h.frame == top && h.kind == HandlerKind.Finally {
            if h.operands < job.frames[top].operand_base || h.operands > job.operands.len { terminate(job, Fault.InvalidState, frame_at(job, top, at)); return; }
            job.operands.resize(h.operands);
            job.handlers[job.handlers.len - 1] = Handler { frame: h.frame, pc: h.pc, operands: h.operands, locals: h.locals, kind: HandlerKind.Return, value: value };
            job.frames[top].pc = h.pc; return;
        }
        job.handlers.pop();
    }
    let lb = job.frames[top].local_base; let ob = job.frames[top].operand_base;
    job.frames.pop(); job.locals.resize(lb); job.operands.resize(ob);
    if job.frames.len == 0 { job.result = value; job.status = tiny::Status.Complete; } else { job.operands.push(value); }
}
// Whether the innermost handler entry belongs to frame `top` and is `kind`.
fn own(job: const Job&, top: i64, kind: HandlerKind) -> bool {
    return job.handlers.len > 0 && job.handlers[job.handlers.len - 1].frame == top && job.handlers[job.handlers.len - 1].kind == kind;
}
fn pending(kind: HandlerKind) -> bool { return kind != HandlerKind.Catch && kind != HandlerKind.Finally; }
// The handler instructions of the frame at `top`, run at `at` with its pc
// written back past `at`. A mismatch with the handler stack (possible only in
// hand-assembled code) fails with InvalidState, as the Pants engine's
// Internal. PushFinally pushes a finally handler at the operand height;
// PopFinally turns it into a pending Normal completion; CallFinally turns it
// into a pending Jump to the next instruction and runs the block; EndFinally
// resumes the pending completion; DropCompletion discards it (a jump out of
// the finally block), except a pending cancellation, whose cleanup ends.
fn push_handler(job: Job&, top: i64, kind: HandlerKind, pc: i64) {
    if job.handlers.len >= job.heap.limits.stack { job.heap.fault = Fault.StackLimit; return; }
    job.handlers.push(Handler { frame: top, pc: pc, operands: job.operands.len, locals: job.locals.len, kind: kind });
}
fn pop_finally(job: Job&, top: i64) {
    if !own(job, top, HandlerKind.Finally) { job.heap.fault = Fault.InvalidState; return; }
    job.handlers[job.handlers.len - 1].kind = HandlerKind.Normal;
}
fn call_finally(job: Job&, top: i64, at: i64) {
    if !own(job, top, HandlerKind.Finally) || job.handlers[job.handlers.len - 1].operands != job.operands.len { job.heap.fault = Fault.InvalidState; return; }
    let target = job.handlers[job.handlers.len - 1].pc;
    job.handlers[job.handlers.len - 1].kind = HandlerKind.Jump; job.handlers[job.handlers.len - 1].pc = at + 1;
    job.frames[top].pc = target;
}
fn end_finally(job: Job&, top: i64, at: i64) {
    if job.handlers.len == 0 || job.handlers[job.handlers.len - 1].frame != top || !pending(job.handlers[job.handlers.len - 1].kind) { job.heap.fault = Fault.InvalidState; return; }
    let h = job.handlers.pop();
    match h.kind {
        Return => { return_value(job, top, h.value, at); },
        Throw => { raise(job, h.value, frame_at(job, top, at)); },
        Jump => { if job.operands.len != h.operands { job.heap.fault = Fault.InvalidState; return; } job.frames[top].pc = h.pc; },
        Cancel => { unwind_cancel(job, frame_at(job, top, at)); },
        _ => {},
    }
}
fn drop_completion(job: Job&, top: i64, at: i64) {
    if job.handlers.len == 0 || job.handlers[job.handlers.len - 1].frame != top || !pending(job.handlers[job.handlers.len - 1].kind) { job.heap.fault = Fault.InvalidState; return; }
    if job.handlers[job.handlers.len - 1].kind == HandlerKind.Cancel { terminate(job, Fault.Cancelled, frame_at(job, top, at)); return; }
    job.handlers.pop();
}
// PendingThrow, PendingException and PendingCancel: what the frame's
// innermost entry says about the running finally block.
fn pending_value(job: const Job&, top: i64, what: i64) -> Value {
    if what == 172 { return boolean(own(job, top, HandlerKind.Cancel)); }
    let thrown = own(job, top, HandlerKind.Throw);
    if what == 118 { return boolean(thrown); }
    return if thrown { job.handlers[job.handlers.len - 1].value } else { Value.Undefined };
}
// Cancellation (light-cljs-786; the Pants engine's PendingCancel contract,
// rust/src/task.rs "Cancellation and finally blocks"). The consumer delivers
// it at a poll point, between runs: it is an abrupt completion that no catch
// block intercepts and no JavaScript sees as a value, but that runs every
// finally block, innermost first, each with a pending Cancel completion
// (PendingCancel reports it) whose EndFinally goes on to the next. A pending
// effect is dropped. Nothing overrides it: a return, exception or jump that
// would discard it, an effect request, or any fault ends the cleanup. When
// none is left the job fails with Fault.Cancelled. The cleanup runs under the
// job's budget and limits like any code. Returns false for a job that is
// claimed or not running.
fn cancel(job: Job&) -> bool {
    if job.claimed || job.status != tiny::Status.Ready || !job.registered { return false; }
    if job.cancelling { return true; }
    job.cancelling = true; job.effect_pending = false; job.effect_count = 0;
    let frame = if job.frames.len > 0 { job.frames[job.frames.len - 1] } else { Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined } };
    unwind_cancel(job, frame); return true;
}
// Unwind the cancellation to the innermost finally handler of any frame,
// skipping catch handlers and discarding pending completions, or end it.
fn unwind_cancel(job: Job&, frame: Frame) {
    while job.handlers.len > 0 {
        let h = job.handlers.pop();
        if h.kind != HandlerKind.Finally { continue; }
        if !unwind_to(job, h) { break; }
        job.handlers.push(Handler { frame: h.frame, pc: h.pc, operands: h.operands, locals: h.locals, kind: HandlerKind.Cancel });
        return;
    }
    terminate(job, Fault.Cancelled, frame);
}
fn enter(job: Job&, callee: Value, receiver: Value, arguments: const Value[:]) -> bool {
    if !valid(job.heap, callee) || !valid(job.heap, receiver) { return false; }
    return push_frame(job, callee, receiver, arguments, true, job.operands.len);
}
// Push the callee's frame. `checked` validates the receiver's and arguments'
// rows (enter, for values from outside the job). A call from the dispatch
// loop skips that (light-cljs-801): its callee, receiver and arguments are
// the job's operands, which hold only valid values, because every value
// enters a job validated (start, deliver_effect, load-validated constants,
// table rows written by validated writes or a validated image) and rows are
// never removed while a job runs. `operands` is the callee's operand base:
// the caller's operand height once the call has consumed its operands. The
// call limits (Limits) count the frame and the values it adds above that.
fn push_frame(job: Job&, callee: Value, receiver: Value, arguments: const Value[:], checked: bool, operands: i64) -> bool {
    let heap .= job.heap;
    if callee.tag() != 5 || node(heap, callee.id()).kind != 2 { heap.fault = Fault.TypeError; return false; }
    let id = node(heap, callee.id()).function; let f .= heap.code.functions[id];
    // A class constructor has no [[Call]]: calling it throws TypeError.
    if f.kind >= 2 { heap.fault = Fault.TypeError; return false; }
    if job.frames.len >= heap.limits.frames || f.locals + arguments.len > heap.limits.stack - job.locals.len - operands { heap.fault = Fault.StackLimit; return false; }
    if !charge(heap, f.locals + arguments.len) { return false; }
    if checked { for v in arguments { if !valid(heap, v) { return false; } } }
    let base = job.locals.len; job.locals.resize(base + f.locals, Value.Undefined);
    for value, i in arguments { if i < f.params { job.locals[base + i] = value; } }
    job.locals.append(arguments);
    job.frames.push(Frame { function: id, pc: f.start, local_base: base, operand_base: operands, callee: callee, receiver: receiver, argument_count: arguments.len });
    return true;
}
fn start(job: Job&, callee: Value, arguments: const Value[:], receiver: Value = Value.Undefined) -> bool {
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
    if !suspending { unwind_natives(job, 0); }
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
    job.status = tiny::Status.Ready; job.result = Value.Undefined; job.fault = Fault.None; job.fault_pc = -1; job.fault_function = -1; job.thrown = false;
    job.steps = 0; job.effects_enabled = false; job.helpers_enabled = false; job.effect_pending = false; job.effect_kind = -1; job.cancelling = false;
    job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear(); job.root_receiver = Value.Undefined; job.gensym_prefix.clear(); job.gensym_next = 0; job.effect_args = [Value.Undefined; 8]; job.effect_count = 0;
    job.effect_frame = Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined };
    return true;
}
// The write log holds named property writes only: the typed outcome's
// `observed` effects and the compiler's write counts read it. The job reset
// needs no log (decision 3), so element stores and cell writes, which no
// reader looks at, are not logged (light-cljs-801).
fn record(job: Job&, object: Value, k: Value, value: Value) {
    if job.writes.len >= job.heap.limits.properties { memory(job.heap); return; }
    job.writes.push(Write { object: object, key: k, value: value });
}
fn binary_value(heap: Heap&, op: i64, a: Value, b: Value) -> Value {
    if op == 11 { return boolean(equal(heap, a, b)); }
    if op == 41 {
        if (a.tag() == 0 || a.tag() == 1) && (b.tag() == 0 || b.tag() == 1) { return boolean(true); }
        if a.tag() == b.tag() { return boolean(equal(heap, a, b)); }
        if a.tag() <= 1 || b.tag() <= 1 { return boolean(false); }
        if (a.tag() <= 3 && b.tag() <= 3) { return boolean(numeric(heap, a) == numeric(heap, b)); }
        heap.fault = Fault.UnsupportedOperand; return Value.Undefined;
    }
    if op == 73 && a.tag() == 4 && b.tag() == 4 {
        if !valid(heap, a) || !valid(heap, b) { return Value.Undefined; }
        if !units_fit(heap, str_len(heap, a) + str_len(heap, b)) { memory(heap); return Value.Undefined; }
        let x = header(heap, a.sid()); let y = header(heap, b.sid());
        if !x.wide && !y.wide {
            if !charge(heap, x.len + y.len) { return Value.Undefined; }
            var bytes: u8[>..] = []; bytes.append(narrow_run(heap, x)); bytes.append(narrow_run(heap, y));
            if !units_fit(heap, bytes.len) || heap.strings.len - heap.image.strings >= heap.limits.units { memory(heap); return Value.Undefined; }
            return string_ref(heap, add_latin1(heap, bytes), bytes.len, false);
        }
        var units: u16[>..] = []; append_utf16(heap, a, units); append_utf16(heap, b, units); return string(heap, units);
    }
    if op >= 74 && op <= 77 && a.tag() == 4 && b.tag() == 4 {
        if !valid(heap, a) || !valid(heap, b) || !charge(heap, str_len(heap, a) + str_len(heap, b)) { return Value.Undefined; }
        let p = str_len(heap, a); let q = str_len(heap, b);
        var c: i64 = 0; for i in min(p, q) { let x = unit(heap, a, i); let y = unit(heap, b, i); if x != y { c = if x < y { -1 } else { 1 }; break; } } if c == 0 { c = if p < q { -1 } else if p > q { 1 } else { 0 }; }
        return boolean(if op == 74 { c < 0 } else if op == 75 { c > 0 } else if op == 76 { c <= 0 } else { c >= 0 });
    }
    let x = numeric(heap, a); let y = numeric(heap, b);
    if heap.fault != Fault.None { return Value.Undefined; }
    return match tiny::number_binary(op, x, y) {
        Number n => number(n.value), Bool b => boolean(b.value), _ => { heap.fault = Fault.InvalidState; Value.Undefined },
    };
}

// Bounded persistent flat ranges. Updates copy only the changed container's
// child IDs; input occurrences and untouched subtrees retain exact identity.
fn nil() -> Value { return Value.Null; }
fn form_value(heap: Heap&, v: Value) -> Value {
    if !valid(heap, v) { return Value.Undefined; }
    return if v.tag() == 5 && node(heap, v.id()).kind == 11 { node(heap, v.id()).value } else { v };
}
fn form_kind(heap: Heap&, v: Value) -> i64 {
    let x = form_value(heap, v);
    return if x.tag() == 5 { node(heap, x.id()).kind } else { -1 };
}
fn form_collection(heap: Heap&, kind: i64, children: const Value[:], template: Node = Node { kind: 0 }) -> Value {
    if kind < 5 || kind > 8 || (kind == 7 && children.len % 2 != 0) { heap.fault = Fault.TypeError; return Value.Undefined; }
    if !values_fit(heap, children.len) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, children.len) { return Value.Undefined; }
    for child in children { if !valid(heap, child) { return Value.Undefined; } }
    var made = template; made.kind = kind; made.start = heap.values.len as u32; made.count = children.len as u32;
    add_values(heap, children); return allocate(heap, made);
}
// Ordinary constructors normalize equal keys/members. LiteralMap and the
// compiler occurrence bridge deliberately retain positional source operands.
fn form_construct(heap: Heap&, kind: i64, children: const Value[:]) -> Value {
    if kind != 7 && kind != 8 { return form_collection(heap, kind, children); }
    let stride: i64 = if kind == 7 { 2 } else { 1 };
    if children.len % stride != 0 { heap.fault = Fault.TypeError; return Value.Undefined; }
    if !charge(heap, children.len) { return Value.Undefined; }
    var normalized: Value[>..] = [];
    for i in (children.len / stride) {
        let key = children[i * stride]; var found: i64 = -1;
        for j in (normalized.len / stride) { if form_equal(heap, normalized[j * stride], key) { found = j * stride; break; } }
        if heap.fault != Fault.None { return Value.Undefined; }
        if found >= 0 { if kind == 7 { normalized[found + 1] = children[i * stride + 1]; } }
        else { normalized.push(key); if kind == 7 { normalized.push(children[i * stride + 1]); } }
    }
    return form_collection(heap, kind, normalized);
}
fn form_count(heap: Heap&, v: Value) -> i64 {
    let x = form_value(heap, v);
    if x.tag() <= 1 { return 0; }
    if x.tag() == 4 { return str_len(heap, x); }
    if x.tag() == 5 { let n = node(heap, x.id()); if n.kind >= 5 && n.kind <= 8 || n.kind == 12 { return if n.kind == 7 { n.count / 2 } else { n.count }; } }
    heap.fault = Fault.TypeError; return 0;
}
fn form_nth(heap: Heap&, v: Value, i: i64, fallback: Value = Value.Null) -> Value {
    let x = form_value(heap, v);
    if x.tag() <= 1 { return fallback; }
    if x.tag() == 4 { return if i >= 0 && i < str_len(heap, x) { unit_string(heap, x, i) } else { fallback }; }
    if x.tag() == 5 { let n = node(heap, x.id()); if n.kind == 5 || n.kind == 6 || n.kind == 12 { return if i >= 0 && i < n.count { slot(heap, n.start + i) } else { fallback }; } }
    heap.fault = Fault.TypeError; return Value.Undefined;
}
recursive fn form_equal(heap: Heap&, a: Value, b: Value, depth: i64 = 0) -> bool {
    if !charge(heap) { return false; }
    if depth >= 128 { heap.fault = Fault.StackLimit; return false; }
    if !valid(heap, a) || !valid(heap, b) { return false; }
    if a.tag() == 5 && a == b { return true; }
    let x = form_value(heap, a); let y = form_value(heap, b);
    if x.tag() <= 1 && y.tag() <= 1 { return true; }
    if x.tag() != y.tag() { return false; }
    if x.tag() != 5 { return equal(heap, x, y); }
    let n = node(heap, x.id()); let m = node(heap, y.id());
    if n.kind == 15 || m.kind == 15 { heap.fault = Fault.NativeBoundary; return false; }
    if n.kind < 5 || m.kind < 5 || n.kind == 13 || m.kind == 13 || n.kind == 14 || m.kind == 14 { return x == y; }
    if n.kind != m.kind && !((n.kind == 5 || n.kind == 6 || n.kind == 12) && (m.kind == 5 || m.kind == 6 || m.kind == 12)) { return false; }
    if n.kind == 9 || n.kind == 10 { return equal(heap, n.value, m.value) && equal(heap, n.namespace_value, m.namespace_value); }
    if n.count != m.count { return false; }
    if n.kind == 7 || n.kind == 8 {
        let stride: i64 = if n.kind == 7 { 2 } else { 1 };
        for i in (n.count / stride) {
            var found = false;
            for j in (m.count / stride) {
                if form_equal(heap, slot(heap, n.start + i * stride), slot(heap, m.start + j * stride), depth + 1) {
                    // Match form_get: later equal keys cannot replace the
                    // first match when its value differs.
                    found = stride == 1 || form_equal(heap, slot(heap, n.start + i * stride + 1), slot(heap, m.start + j * stride + 1), depth + 1);
                    break;
                }
            }
            if !found { return false; }
        }
        return true;
    }
    for i in n.count { if !form_equal(heap, slot(heap, n.start + i), slot(heap, m.start + i), depth + 1) { return false; } }
    return true;
}
fn form_assoc(heap: Heap&, v: Value, k: Value, value: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if kind != 6 && kind != 7 { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id()); var at: i64 = -1;
    if kind == 6 {
        let i = form_value(heap, k);
        if i.tag() != 3 || !(i.num() >= 0 && i.num() <= n.count) || i.num() != (i.num() as i64) { heap.fault = Fault.TypeError; return Value.Undefined; }
        at = i.num() as i64;
    } else {
        for i in (n.count / 2) { if form_equal(heap, slot(heap, n.start + i * 2), k) { at = i * 2 + 1; break; } }
    }
    if !charge(heap, n.count + 2) { return Value.Undefined; }
    let replacing = at >= 0 && at < n.count;
    let count = n.count + (if replacing { 0 } else if kind == 7 { 2 } else { 1 });
    if !values_fit(heap, count) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, count) { return Value.Undefined; }
    // Validate exactly the resulting children, including only retained old
    // slots. A replaced stale value is absent from the new range.
    if replacing {
        for child in run(heap, n.start, at) { if !valid(heap, child) { return Value.Undefined; } }
        if !valid(heap, value) { return Value.Undefined; }
        for child in run(heap, n.start + at + 1, n.count - at - 1) { if !valid(heap, child) { return Value.Undefined; } }
    } else {
        for child in run(heap, n.start, n.count) { if !valid(heap, child) { return Value.Undefined; } }
        if kind == 7 && !valid(heap, k) { return Value.Undefined; } if !valid(heap, value) { return Value.Undefined; }
    }
    var made = n; made.kind = kind; made.origin = -1; made.start = heap.values.len as u32; made.count = count;
    // allocate charges the final work unit before any child append. Nothing
    // below can fail (the preflight reserved the rows); old ranges and
    // occurrence/metadata handles stay intact.
    let result = allocate(heap, made); if heap.fault != Fault.None { return Value.Undefined; }
    if replacing {
        add_values(heap, run(heap, n.start, at)); add_values(heap, [value]);
        add_values(heap, run(heap, n.start + at + 1, n.count - at - 1));
    } else {
        add_values(heap, run(heap, n.start, n.count));
        if kind == 7 { add_values(heap, [k, value]); } else { add_values(heap, [value]); }
    }
    return result;
}
fn form_conj(heap: Heap&, v: Value, value: Value) -> Value {
    let x = form_value(heap, v); let kind = if x.tag() <= 1 { 5 } else { form_kind(heap, x) };
    if kind != 5 && kind != 6 { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = if x.tag() <= 1 { Node { kind: 5 } } else { node(heap, x.id()) };
    if !charge(heap, n.count + 1) { return Value.Undefined; }
    let count = n.count + 1;
    if !values_fit(heap, count) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, count) { return Value.Undefined; }
    if kind == 5 && !valid(heap, value) { return Value.Undefined; }
    for child in run(heap, n.start, n.count) { if !valid(heap, child) { return Value.Undefined; } }
    if kind == 6 && !valid(heap, value) { return Value.Undefined; }
    var made = n; made.kind = kind; made.origin = -1; made.start = heap.values.len as u32; made.count = count;
    let result = allocate(heap, made); if heap.fault != Fault.None { return Value.Undefined; }
    if kind == 5 { add_values(heap, [value]); }
    add_values(heap, run(heap, n.start, n.count));
    if kind == 6 { add_values(heap, [value]); }
    return result;
}
fn form_get(heap: Heap&, v: Value, k: Value, fallback: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if x.tag() <= 1 { return fallback; }
    if kind == 6 || x.tag() == 4 {
        let i = form_value(heap, k); if i.tag() != 3 || !(i.num() >= 0 && i.num() < form_count(heap, x)) || i.num() != (i.num() as i64) { return fallback; }
        return form_nth(heap, x, i.num() as i64, fallback);
    }
    if kind != 7 { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id());
    for i in (n.count / 2) { if form_equal(heap, slot(heap, n.start + i * 2), k) { return slot(heap, n.start + i * 2 + 1); } }
    return fallback;
}
fn form_contains(heap: Heap&, v: Value, k: Value) -> bool {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if x.tag() <= 1 { return false; }
    if kind == 6 || x.tag() == 4 {
        let i = form_value(heap, k);
        return i.tag() == 3 && i.num() >= 0 && i.num() < form_count(heap, x) && i.num() == trunc(i.num());
    }
    if kind == 7 || kind == 8 {
        let n = node(heap, x.id()); let stride: i64 = if kind == 7 { 2 } else { 1 };
        for i in (n.count / stride) { if form_equal(heap, slot(heap, n.start + i * stride), k) { return true; } if heap.fault != Fault.None { return false; } }
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
        if !charge(heap, 2) { return Value.Undefined; }
        let x = form_value(heap, coll); if heap.fault != Fault.None { return Value.Undefined; }
        if x.tag() <= 1 { continue; }
        let kind = form_kind(heap, x);
        if kind != 5 && kind != 6 && kind != 12 { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
        let n = node(heap, x.id());
        if !values_fit(heap, count + n.count) { memory(heap); return Value.Undefined; }
        if !charge(heap, n.count * 2) { return Value.Undefined; }
        for child in run(heap, n.start, n.count) { if !valid(heap, child) { return Value.Undefined; } }
        count += n.count;
    }
    let start = heap.values.len;
    let result = allocate(heap, Node { kind: 12, start: start as u32, count: count as u32 });
    if heap.fault != Fault.None { return Value.Undefined; }
    // Preflight above accounted both validation and copying. Append only
    // after every failure point; the inputs are immutable stable ranges.
    for coll in colls {
        let x = form_value(heap, coll); if x.tag() <= 1 { continue; }
        let n = node(heap, x.id()); add_values(heap, run(heap, n.start, n.count));
    }
    return result;
}
fn form_sequence(heap: Heap&, v: Value, skip: i64, empty_nil: bool) -> Value {
    let x = form_value(heap, v); let count = form_count(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if count <= skip { return if empty_nil { nil() } else { form_collection(heap, 5, []) }; }
    let kind = form_kind(heap, x);
    if kind != 5 && kind != 6 && kind != 12 { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    let n = node(heap, x.id()); return allocate(heap, Node { kind: 12, start: (n.start + skip) as u32, count: (n.count - skip) as u32 });
}
fn named_supported(heap: Heap&, ns: Value, name: Value) -> bool {
    let n = utf16(heap, name);
    if !charge(heap, n.len + str_len(heap, ns)) { return false; }
    if ns.tag() == 4 { for i in str_len(heap, ns) { if unit(heap, ns, i) == 47 { return false; } } if str_len(heap, ns) == 0 && n.len == 0 { return false; } }
    else { for c in n { if c == 47 && n.len != 1 { return false; } } }
    return true;
}
fn form_op(heap: Heap&, op: i64, args: const Value[:]) -> Value {
    for v in args {
        if !valid(heap, v) { return Value.Undefined; }
        // Partial/lazy protocol operations are outside this full-demand
        // target slice. Never inspect an uninitialized memo as a form.
        if v.tag() == 5 && node(heap, v.id()).kind == 15 { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    }
    let a = args[0];
    match op {
        0, 1 => {
            if (a.tag() > 1 && a.tag() != 4) || args[1].tag() != 4 { heap.fault = Fault.TypeError; return Value.Undefined; }
            if !named_supported(heap, a, args[1]) { if heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; } return Value.Undefined; }
            return allocate(heap, Node { kind: if op == 0 { 10 } else { 9 }, value: args[1], namespace_value: if a.tag() <= 1 { nil() } else { a } });
        },
        2 => { let kind = form_kind(heap, a); return match kind { 5, 12 => text(heap, "list"), 6 => text(heap, "vector"), 7 => text(heap, "map"), 8 => text(heap, "set"), 9 => text(heap, "symbol"), 10 => text(heap, "keyword"), _ => nil() }; },
        5, 19 => { return number(form_count(heap, a)); },
        6, 25, 26 => { let i = form_value(heap, args[1]); if i.tag() != 3 || !(i.num() >= -2147483648.0 && i.num() <= 2147483647.0) || i.num() != (i.num() as i64) || (op != 26 && (i.num() < 0 || i.num() >= form_count(heap, a))) { heap.fault = Fault.TypeError; return Value.Undefined; } return form_nth(heap, a, i.num() as i64, if op == 26 { args[2] } else { nil() }); },
        7, 27 => { return form_get(heap, a, args[1], args[2]); },
        72 => { return form_get(heap, a, args[1], nil()); },
        28 => { return boolean(form_contains(heap, a, args[1])); },
        35 => { return form_concat(heap, args); },
        36 => {
            let x = form_value(heap, a); if x.tag() <= 1 { return form_concat(heap, []); }
            let kind = form_kind(heap, x);
            if kind != 5 && kind != 6 && kind != 12 { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
            let n = node(heap, x.id()); return form_concat(heap, run(heap, n.start, n.count));
        },
        8, 29 => { return form_assoc(heap, a, args[1], args[2]); },
        10, 31 => { return form_conj(heap, a, args[1]); },
        11 => { return boolean(form_equal(heap, a, args[1])); },
        13, 33 => { return if a.tag() == 5 && node(heap, a.id()).metadata.tag() != 0 { node(heap, a.id()).metadata } else { nil() }; },
        14, 34 => { let kind = form_kind(heap, a); let meta = form_value(heap, args[1]); if !(kind >= 5 && kind <= 9 || kind == 12) || (meta.tag() > 1 && form_kind(heap, meta) != 7) { heap.fault = Fault.TypeError; return Value.Undefined; } var n = node(heap, a.id()); n.metadata = if meta.tag() <= 1 { Value.Undefined } else { args[1] }; n.origin = -1; return allocate(heap, n); },
        15 => { return form_value(heap, a); },
        18 => { let v = form_value(heap, a); return boolean(v.tag() > 1 && !(v.tag() == 2 && !v.flag())); },
        20, 22, 23 => { return form_sequence(heap, a, if op == 20 { 0 } else { 1 }, op != 22); },
        21 => { return form_nth(heap, a, 0); },
        65 => { let v = form_value(heap, a); let kind = form_kind(heap, v); if kind != 5 && kind != 6 { heap.fault = Fault.TypeError; return Value.Undefined; } let n = node(heap, v.id()); return form_collection(heap, 7, run(heap, n.start, n.count)); },
        69 => { if form_kind(heap, a) != 7 && form_kind(heap, a) != 8 { heap.fault = Fault.TypeError; return Value.Undefined; } return a; },
        80 => { let kind = form_kind(heap, a); return boolean(kind == 6 || kind == 7 || kind == 8 || kind == 9 || kind == 10); },
        81 => {
            let value = form_value(heap, a);
            if value.tag() == 5 && (node(heap, value.id()).kind == 2 || node(heap, value.id()).kind == 4) { return heap.call_method; }
            heap.fault = Fault.NativeBoundary; return Value.Undefined;
        },
        60 => {
            let arity = args[3]; let kind = form_kind(heap, a);
            if arity.tag() != 3 || (arity.num() != 1 && arity.num() != 2) { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
            if kind == 9 || kind == 10 { return form_get(heap, args[1], a, args[2]); }
            if kind == 7 { return form_get(heap, a, args[1], args[2]); }
            if kind == 8 {
                let n = node(heap, a.id());
                for i in n.count { if !charge(heap) { return Value.Undefined; } let v = slot(heap, n.start + i); if form_equal(heap, v, args[1]) { return v; } if heap.fault != Fault.None { return Value.Undefined; } }
                return args[2];
            }
            if kind == 6 {
                let i = form_value(heap, args[1]);
                if i.tag() != 3 || !(i.num() >= 0 && i.num() < form_count(heap, a)) { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
                // Legacy vector calls truncate after bounds validation.
                return form_nth(heap, a, i.num() as! i64);
            }
            heap.fault = Fault.NativeBoundary; return Value.Undefined;
        },
        82 => {
            if a.tag() != 5 || node(heap, a.id()).kind != 0 || args[1].tag() > 1 { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
            if args[2].tag() != 4 { heap.fault = Fault.TypeError; return Value.Undefined; }
            let value, found = own_property(heap, a, args[2]);
            // A compiler root read is a dependency request, even if the JS
            // value would otherwise be undefined. No live-job parking/replay
            // is supported: prerequisites must be resolved before admission.
            if !found && heap.fault == Fault.None { heap.fault = Fault.DependencyNeed; } return value;
        },
        92 => {
            if a.tag() != 5 || node(heap, a.id()).kind != 2 { heap.fault = Fault.TypeError; return Value.Undefined; }
            var row = node(heap, a.id()); row.immutable = true;
            return allocate(heap, row);
        },
        93 => { return if a.tag() == 5 && node(heap, a.id()).kind == 2 && node(heap, a.id()).immutable { nil() } else { a }; },
        _ => { heap.fault = Fault.NativeBoundary; return Value.Undefined; },
    }
}
// Enumerated native runtime leaves: 0 macroReaderFunction (Pants uses
// occurrence-aware leaves directly), 1 inc, 2 mapv, 3 vec, 4 into, 16 sequence.
fn builtin(heap: Heap&, id: i64) -> Value { return allocate(heap, Node { kind: 4, function: id as u32 }); }
fn install_field(heap: Heap&, object: Value, name: const u8[:], value: Value) -> bool { let k = text(heap, name); return put(heap, object, k, value); }
fn boot_runtime(heap: Heap&) -> Value {
    if heap.boot_count != 0 { heap.fault = Fault.InvalidState; return Value.Undefined; }
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
    // Gensym numbers in the job's compiler generated-name scope (gensym).
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
        if input.tag() > 1 && kind != 5 && kind != 12 { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
        let count = form_count(heap, input);
        if heap.fault != Fault.None { return Value.Undefined; }
        if !values_fit(heap, count) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
        if !charge(heap, count) { return Value.Undefined; }
        let start = add_values(heap, [], count);
        return allocate(heap, Node { kind: 15, start: start as u32, count: count as u32, value: args[0], namespace_value: input });
    }
    if id == 16 && args.len == 1 {
        let kind = form_kind(heap, args[0]); if heap.fault != Fault.None { return Value.Undefined; }
        if kind == 5 || kind == 12 || kind == 15 { return args[0]; }
        return form_sequence(heap, args[0], 0, false);
    }
    if id == 15 && args.len == 2 {
        let object = args[0]; let skip = args[1];
        if !valid(heap, object) || object.tag() != 5 || node(heap, object.id()).kind != 14 || skip.tag() != 3 || !(skip.num() >= 0 && skip.num() <= 65535) || skip.num() != (skip.num() as i64) { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
        let n = node(heap, object.id()); let offset = min(n.count as i64, skip.num() as i64);
        return form_collection(heap, 5, run(heap, n.start + offset, n.count - offset));
    }
    if id >= 10 && id <= 13 && args.len == 1 {
        let array = args[0];
        if !valid(heap, array) || array.tag() != 5 || node(heap, array.id()).kind != 1 { heap.fault = Fault.TypeError; return Value.Undefined; }
        let count = node(heap, array.id()).length;
        if !values_fit(heap, count) { memory(heap); return Value.Undefined; }
        if !charge(heap, count) { return Value.Undefined; }
        var values: Value[>..] = [];
        for i in count { let v, found = own_element(heap, array, i); values.push(v); }
        return form_construct(heap, if id == 10 { 6 } else if id == 11 { 5 } else if id == 12 { 7 } else { 8 }, values);
    }
    if id == 0 && args.len == 1 { return args[0]; }
    if id == 1 && args.len == 1 { return number(numeric(heap, args[0]) + 1); }
    if id == 3 && args.len == 1 {
        let kind = form_kind(heap, args[0]); if kind == 6 { return args[0]; }
        if kind == 5 || kind == 12 { let n = node(heap, args[0].id()); return form_collection(heap, 6, run(heap, n.start, n.count)); }
    }
    if id == 4 && args.len == 2 {
        var out = args[0]; let count = form_count(heap, args[1]);
        for i in count { out = form_conj(heap, out, form_nth(heap, args[1], i)); if heap.fault != Fault.None { break; } }
        return out;
    }
    heap.fault = Fault.NativeBoundary; return Value.Undefined;
}

// cljs.core/gensym in the compiler generated-name scope of the work the job
// does (light-cljs-798): str(prefix), or G__, then the scope prefix and the
// next number of the scope count, which the compiler hands the job and takes
// back with its result, so a name is the V8 build one and unique in the build.
// Outside a scope (gensym_next 0), and for a prefix str() is not supported on
// here, gensym stays an explicit NativeBoundary fault.
fn gensym(job: Job&, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if job.gensym_next <= 0 || job.gensym_next >= 9007199254740991 || args.len > 1 { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    var units: u16[>..] = [];
    if args.len == 0 { units.append([71, 95, 95]); } else {
        // A prefix read from a form arrives as its scalar occurrence.
        let a = form_value(heap, args[0]); if !valid(heap, a) { return Value.Undefined; }
        let kind = if a.tag() == 5 { node(heap, a.id()).kind } else { -1 };
        if a.tag() == 4 { append_utf16(heap, a, units); }
        else if kind == 9 || kind == 10 {
            let n = node(heap, a.id()); if kind == 10 { units.push(58); }
            if n.namespace_value.tag() == 4 { append_utf16(heap, n.namespace_value, units); units.push(47); }
            append_utf16(heap, n.value, units);
        } else { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    }
    for b in job.gensym_prefix { units.push(b); }
    for b in str(job.gensym_next) { units.push(b); }
    // V8 reads a name with a slash as a namespace and a name; not supported here.
    for u in units { if u == 47 { heap.fault = Fault.NativeBoundary; return Value.Undefined; } }
    if !charge(heap, units.len) { return Value.Undefined; }
    let name = allocate(heap, Node { kind: 9, value: string(heap, units), namespace_value: nil() });
    if heap.fault == Fault.None { job.gensym_next += 1; }
    return name;
}
// Save the request at the call continuation. No host runs under the claim.
fn request_effect(job: Job&, id: i64, args: const Value[:], frame: Frame) {
    // A cleanup for a cancellation cannot wait: the request ends it.
    if job.cancelling { job.heap.fault = Fault.Cancelled; return; }
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
    if !valid(heap, value) || value.tag() != 5 || node(heap, value.id()).kind != 15 { return false; }
    if node(heap, value.id()).initialized { heap.fault = Fault.NativeBoundary; return false; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return false; }
    let row = node(heap, value.id());
    node_mut(heap, value.id()).initialized = true;
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: row.value, receiver: nil(), input: value, index: row.length, native: Native.MapSeq });
    return true;
}
fn demand_maps(job: Job&, values: const Value[:], nested: bool = false) -> bool {
    for value in values {
        if demand_map(job, value) { return true; }
        if job.heap.fault != Fault.None { return false; }
        if nested && value.tag() == 5 {
            let n = node(job.heap, value.id());
            if n.kind == 5 || n.kind == 6 || n.kind == 12 {
                for i in n.count { if demand_map(job, slot(job.heap, n.start + i)) { return true; } if job.heap.fault != Fault.None { return false; } }
            }
        }
    }
    return false;
}
// A native frame's step (see Native), dispatched by `match` on its
// operation, with the per-step budget, instruction and work checks of every
// step. A frame of no operation fails InvalidState.
fn native_step(job: Job&) {
    let heap .= job.heap;
    if job.frames.len == 0 { fail(job, Fault.InvalidState, Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined }); return; }
    let frame = job.frames[job.frames.len - 1];
    match frame.native { None => { fail(job, Fault.InvalidState, frame); return; }, _ => {} }
    if !charge(heap) { fail(job, heap.fault, frame); return; }
    if job.steps >= heap.limits.steps { fail(job, Fault.InstructionLimit, frame); return; }
    job.steps += 1;
    match frame.native {
        MapSeq => { map_seq_step(job, frame); },
        Mapv => { mapv_step(job, frame); },
        ToPrimitive t => { to_primitive_step(job, frame, t.op, t.method); },
        None => {},
    }
    if heap.fault != Fault.None { fail(job, heap.fault, frame); }
}
// A deferred map's callbacks, one element a step.
fn map_seq_step(job: Job&, frame: Frame) {
    let heap .= job.heap;
    let row = node(heap, frame.input.id());
    if frame.pc == 1 {
        let value = job.operands.pop(); set_slot(heap, row.start + frame.index, value);
        node_mut(heap, frame.input.id()).length += 1;
        job.frames[job.frames.len - 1].index += 1; job.frames[job.frames.len - 1].pc = 0;
    } else if frame.index == row.count {
        // Completed maps become ordinary cursors without copying or
        // replacing their identity; all aliases see the same memo range.
        node_mut(heap, frame.input.id()).kind = 12;
        node_mut(heap, frame.input.id()).initialized = false;
        node_mut(heap, frame.input.id()).value = Value.Undefined; node_mut(heap, frame.input.id()).namespace_value = nil();
        job.frames.pop();
    } else {
        if job.operands.len >= heap.limits.stack - job.locals.len { heap.fault = Fault.StackLimit; return; }
        let arg = form_nth(heap, row.namespace_value, frame.index);
        job.frames[job.frames.len - 1].pc = 1;
        if frame.callee.tag() == 5 && node(heap, frame.callee.id()).kind == 4 {
            let id = node(heap, frame.callee.id()).function;
            if id == 5 && job.gensym_next > 0 { job.operands.push(gensym(job, [arg])); }
            else if id >= 5 && id <= 8 { request_effect(job, id, [arg], frame); }
            else { job.operands.push(native_value(heap, id, [arg])); }
        } else { enter(job, frame.callee, nil(), [arg]); }
    }
}
// mapv is a native collection iterator, with source callback calls saved
// explicitly. A budget cut never replays a callback or loses its capture.
fn mapv_step(job: Job&, frame: Frame) {
    let heap .= job.heap;
    if frame.pc == 1 {
        let value = job.operands.pop(); let out = form_conj(heap, frame.output, value);
        job.frames[job.frames.len - 1].output = out; job.frames[job.frames.len - 1].pc = 0;
    } else if frame.index >= form_count(heap, frame.input) {
        finish_native(job, frame, frame.output);
    } else {
        let arg = form_nth(heap, frame.input, frame.index);
        job.frames[job.frames.len - 1].index += 1; job.frames[job.frames.len - 1].pc = 1;
        if frame.callee.tag() == 5 && node(heap, frame.callee.id()).kind == 4 {
            let id = node(heap, frame.callee.id()).function;
            if id == 5 && job.gensym_next > 0 { job.operands.push(gensym(job, [arg])); }
            else if id >= 5 && id <= 8 { request_effect(job, id, [arg], frame); }
            else { let v = native_value(heap, id, [arg]); job.operands.push(v); }
        } else { enter(job, frame.callee, Value.Undefined, [arg]); }
    }
}
// Pop a finished native frame and deliver its result to the frame below.
fn finish_native(job: Job&, frame: Frame, result: Value) {
    job.frames.pop(); job.operands.resize(frame.operand_base);
    if job.frames.len == 0 { job.result = result; job.status = tiny::Status.Complete; } else { job.operands.push(result); }
}
// Whether a binary operator's operand is an ordinary object, which converts
// through ToPrimitive before the operator runs.
fn ordinary(heap: const Heap&, v: Value) -> bool { return v.tag() == 5 && node(heap, v.id()).kind == 0; }
// Start the resumable ToPrimitive (hint default) of a binary operator `op`
// whose operands x and y include an ordinary object (light-cljs-786): a
// native frame converts them, left to right, and then runs the operator. The
// frame below has already consumed the operands.
fn to_primitive(job: Job&, op: i64, x: Value, y: Value) {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: Value.Undefined, input: x, output: y, native: Native.ToPrimitive { op: op as u8, method: 0 } });
}
// An own data property by name, newest record first, as own_record looks.
fn own_named(heap: Heap&, object: Value, name: const u8[:]) -> Value, bool {
    let o = object.id(); var i = node(heap, o).key_count;
    while i > 0 {
        i -= 1;
        if !charge(heap) { return Value.Undefined, false; }
        let p = own_at(heap, o, i);
        if key_is(heap, p.key, name) { return p.value, true; }
    }
    return Value.Undefined, false;
}
// One step of OrdinaryToPrimitive with hint default (number order: valueOf,
// then toString), the Pants engine's Convert native (rust/src/vm/native.rs).
// The operand being converted is index 0 (input) or 1 (output); an operand
// that is not an ordinary object is left as it is for the operator. A
// callable own method is called with the object as receiver; a primitive
// result replaces the operand, an object result tries the next method, and a
// non-callable property is skipped. Without prototypes (light-cljs-787), a
// method the object does not own is the prototype's: UnsupportedOperand; an
// object that converts by neither method is a TypeError.
fn to_primitive_step(job: Job&, frame: Frame, op: u8, method: u8) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if frame.pc == 1 {
        // A scalar form (kind 11: a primitive with its provenance, as a
        // helper's value arrives) is its primitive.
        let result = form_value(heap, job.operands.pop());
        job.frames[at].pc = 0;
        if result.tag() == 5 { job.frames[at].native = Native.ToPrimitive { op: op, method: method + 1 }; return; }
        if frame.index == 0 { job.frames[at].input = result; } else { job.frames[at].output = result; }
        job.frames[at].index += 1; job.frames[at].native = Native.ToPrimitive { op: op, method: 0 };
        return;
    }
    if frame.index >= 2 {
        let value = binary_value(heap, op, frame.input, frame.output);
        if heap.fault == Fault.None { finish_native(job, frame, value); }
        return;
    }
    let v = if frame.index == 0 { frame.input } else { frame.output };
    if !ordinary(heap, v) { job.frames[at].index += 1; return; }
    if method >= 2 { heap.fault = Fault.TypeError; return; }
    let m, found = own_named(heap, v, if method == 0 { "valueOf" } else { "toString" });
    if heap.fault != Fault.None { return; }
    if !found { heap.fault = Fault.UnsupportedOperand; return; }
    if m.tag() == 5 && node(heap, m.id()).kind == 4 { heap.fault = Fault.UnsupportedOperand; return; }
    if m.tag() == 5 && node(heap, m.id()).kind == 2 { job.frames[at].pc = 1; enter(job, m, v, []); return; }
    job.frames[at].native = Native.ToPrimitive { op: op, method: method + 1 };
}
// The dispatch loop runs the active frame in place (light-cljs-801): its
// function, pc, local and operand bases are locals for the whole loop, not a
// Frame copied every instruction, and are written back to the frame row only
// where the frame stack changes or a helper may read them: calls, returns,
// throws, faults, effect suspension, demands and the budget cut. A call into
// bytecode and a return to a bytecode caller start the next frame's pass
// without leaving the loop.
//
// Every instruction still costs one step, one work unit and one unit of the
// run's budget, and the limits fault at the same instruction as before, with
// the same counts: `fuel` is how many instructions may run before one of the
// three is spent, so the loop counts down a local and settles the counts into
// job.steps, heap.work and the budget before any helper that charges work,
// reads the counts or may fault, and at every exit. Only when the fuel is
// spent does an instruction take the exact checks (budget, InstructionLimit,
// WorkLimit). A frame whose operand base leaves room for its function's
// deepest validated height (Code.heights) cannot underflow or exceed
// limits.stack, so only a frame without that room checks each instruction's
// stack effect, as before.
inline fn fuel(job: const Job&, allowance: i64, used: i64) -> i64 {
    var n = min(job.heap.limits.steps - job.steps, job.heap.limits.work - job.heap.work);
    if allowance >= 0 { n = min(n, allowance - used); }
    return max(n, 0);
}
// Settle `ran` instructions run in place: a step, a work unit and a budget
// unit each.
inline fn settle(job: Job&, ran: i64&, used: i64&) { job.steps += ran; job.heap.work += ran; used += ran; ran = 0; }
// The frame row at `top` as the instruction at `at` saw it: fault locations
// and effect continuations name the instruction, not its successor.
inline fn frame_at(job: const Job&, top: i64, at: i64) -> Frame { var frame = job.frames[top]; frame.pc = at; return frame; }
// After a helper ran for the instruction at `at` (its pc already written
// back): report its fault, and whether the frame at `top` is still active and
// running, so the loop may continue in place.
inline fn in_place(job: Job&, top: i64, at: i64) -> bool {
    if job.heap.fault != Fault.None { fail(job, job.heap.fault, frame_at(job, top, at)); return false; }
    return job.frames.len == top + 1 && job.status == tiny::Status.Ready && !job.effect_pending;
}
// Numbers on both sides of a binary operator: what binary_value computes for
// them, without its coercions (no charge, no fault).
inline fn arithmetic(op: i64, x: f64, y: f64) -> Value {
    return match op {
        7, 73 => number(x + y), 8 => number(x - y), 9 => number(x * y), 33 => number(x / y), 34 => number(x % y),
        11, 41 => boolean(x == y), 38, 75 => boolean(x > y), 39, 76 => boolean(x <= y), 40, 77 => boolean(x >= y), _ => boolean(x < y),
    };
}
// Run bytecode frames until the job stops or suspends, the active frame is a
// native one, or `allowance` instructions ran (-1: no budget). Returns the
// budget units used: one an instruction dispatched, including a faulting one
// and one that only started a demand.
fn execute(job: Job&, allowance: i64) -> i64 {
    let heap .= job.heap; let code .= heap.code;
    // The instructions do not change while a job runs.
    let program = code.ins[..];
    var used: i64 = 0;
    while job.status == tiny::Status.Ready && !job.effect_pending && job.frames.len > 0 && job.frames[job.frames.len - 1].function >= 0 && (allowance < 0 || used < allowance) {
        let top = job.frames.len - 1;
        let fid = job.frames[top].function; let lb = job.frames[top].local_base; let ob = job.frames[top].operand_base;
        var pc = job.frames[top].pc;
        let f .= code.functions[fid];
        if pc < f.start || pc >= f.start + f.count || pc >= program.len { let here = job.frames[top]; fail(job, Fault.InvalidState, here); return used + 1; }
        // Validated code keeps a frame's operand height at its pc's depth; a
        // frame entered at any other height takes the per-instruction checks.
        let checked = job.operands.len - ob != code.ops[pc].depth || job.locals.len + ob + code.heights[fid] > heap.limits.stack;
        var ran: i64 = 0; var left = fuel(job, allowance, used); var at = pc;
        // Each pass of this loop dispatches one instruction; it breaks when
        // the frame stops being the active one or a fault stops it.
        loop {
            if ran == left {
                settle(job, ran, used);
                if allowance >= 0 && used >= allowance { job.frames[top].pc = pc; return used; }
                if job.steps >= heap.limits.steps { heap.fault = Fault.InstructionLimit; } else if heap.work >= heap.limits.work { heap.fault = Fault.WorkLimit; }
                if heap.fault != Fault.None { job.frames[top].pc = pc; used += 1; fail(job, heap.fault, frame_at(job, top, pc)); return used; }
                left = fuel(job, allowance, used);
            }
            at = pc; ran += 1;
            if checked {
                // As before: the work unit and budget unit are spent, the step is not.
                let e = code.effects[at];
                let bad = if job.operands.len - ob < e.pop { Fault.InvalidState } else if e.push - e.pop > heap.limits.stack - job.locals.len - job.operands.len { Fault.StackLimit } else { Fault.None };
                if bad != Fault.None { settle(job, ran, used); job.steps -= 1; job.frames[top].pc = at; fail(job, bad, frame_at(job, top, at)); break; }
            }
            pc = at + 1;
            // An arm that runs a helper first settles the counts and writes
            // its continuation back, and afterwards continues in place only if
            // the frame is still the active one; an arm that may fault on its
            // own breaks with heap.fault set.
            match program[at] {
                Const c => {
                    let v = heap.constants[f.constants + c.k];
                    // Symbol and BigInt constants have no VM value yet.
                    if v.tag() >= 6 { heap.fault = Fault.Unsupported; break; }
                    job.operands.push(v);
                },
                Undefined => { job.operands.push(Value.Undefined); },
                Load l => { job.operands.push(job.locals[lb + l.slot]); },
                Store s => { job.locals[lb + s.slot] = job.operands.pop(); },
                Pop => { job.operands.pop(); },
                Dup => { let v = job.operands[job.operands.len - 1]; job.operands.push(v); },
                Binary b => {
                    let y = job.operands.pop(); let x = job.operands.pop();
                    if x.tag() == 3 && y.tag() == 3 { job.operands.push(arithmetic(b.op, x.num(), y.num())); }
                    else if b.op == 73 && (ordinary(heap, x) || ordinary(heap, y)) {
                        // `+` on an object: resumable ToPrimitive in a native
                        // frame, which pushes the sum for this frame.
                        settle(job, ran, used); job.frames[top].pc = pc; to_primitive(job, b.op, x, y); break;
                    } else {
                        settle(job, ran, used); job.frames[top].pc = pc;
                        let v = binary_value(heap, b.op, x, y); job.operands.push(v);
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
                },
                Not => { let v = job.operands.pop(); job.operands.push(boolean(!truthy(heap, v))); },
                Jump j => { pc = j.to; },
                JumpUnless j => { let v = job.operands.pop(); if !truthy(heap, v) { pc = j.to; } },
                Return => {
                    settle(job, ran, used);
                    let v = job.operands.pop(); return_value(job, top, v, at);
                    break;
                },
                NewObject => {
                    settle(job, ran, used); job.frames[top].pc = pc; let v = allocate(heap, Node { kind: 0 }); job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                NewArray => {
                    settle(job, ran, used); job.frames[top].pc = pc; let v = allocate(heap, Node { kind: 1 }); job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Try t => { push_handler(job, top, HandlerKind.Catch, t.to); if heap.fault != Fault.None { break; } },
                PushFinally t => { push_handler(job, top, HandlerKind.Finally, t.to); if heap.fault != Fault.None { break; } },
                PopFinally => { pop_finally(job, top); if heap.fault != Fault.None { break; } },
                Pending p => { job.operands.push(pending_value(job, top, p.what)); },
                CallFinally => { settle(job, ran, used); job.frames[top].pc = pc; call_finally(job, top, at); break; },
                EndFinally => { settle(job, ran, used); job.frames[top].pc = pc; end_finally(job, top, at); break; },
                DropCompletion => { settle(job, ran, used); job.frames[top].pc = pc; drop_completion(job, top, at); break; },
                Throw => { settle(job, ran, used); job.frames[top].pc = pc; let value = job.operands.pop(); raise(job, value, frame_at(job, top, at)); break; },
                EndTry => {
                    if !own(job, top, HandlerKind.Catch) { heap.fault = Fault.InvalidState; break; }
                    job.handlers.pop();
                },
                Unary u => {
                    let v = job.operands.pop(); let n = if v.tag() == 3 { v.num() } else { numeric(heap, v) };
                    job.operands.push(number(if u.op == 35 { -n } else if u.op == 113 { n + 1 } else if u.op == 114 { n - 1 } else { n }));
                    if heap.fault != Fault.None { break; }
                },
                Numeric2 => {
                    let b = job.operands.pop(); let a = job.operands.pop();
                    let x = if a.tag() == 3 { a.num() } else { numeric(heap, a) }; let y = if b.tag() == 3 { b.num() } else { numeric(heap, b) };
                    job.operands.push(number(x)); job.operands.push(number(y));
                    if heap.fault != Fault.None { break; }
                },
                Closure c => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let child = code.children[f.children + c.child]; let count = code.functions[child].captures; let base = job.operands.len - count;
                    let v = closure(heap, child, job.operands[base..]); job.operands.resize(base); job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Call c => {
                    settle(job, ran, used);
                    if !invoke(job, top, at, c.argc, false) { break; }
                    left = fuel(job, allowance, used);
                },
                CallMethod c => {
                    settle(job, ran, used);
                    if !invoke(job, top, at, c.argc, true) { break; }
                    left = fuel(job, allowance, used);
                },
                Arguments => {
                    settle(job, ran, used); job.frames[top].pc = pc; let here = job.frames[top]; arguments(job, here, f, false, 0);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Rest r => {
                    settle(job, ran, used); job.frames[top].pc = pc; let here = job.frames[top]; arguments(job, here, f, true, r.skip);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Callee => { job.operands.push(job.frames[top].callee); },
                Capture c => { let row .= node(heap, job.frames[top].callee.id()); job.operands.push(slot(heap, row.start + c.slot)); },
                NewCell c => {
                    settle(job, ran, used); job.frames[top].pc = pc; let v = allocate(heap, Node { kind: 3, immutable: c.immutable }); job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                InitCell => { cell(job, 48); if heap.fault != Fault.None { break; } },
                ReadCell => { cell(job, 49); if heap.fault != Fault.None { break; } },
                WriteCell => { cell(job, 50); if heap.fault != Fault.None { break; } },
                GetNamed g => {
                    settle(job, ran, used); job.frames[top].pc = pc; get_property(job, heap.constants[f.constants + g.k]);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                GetIndexed => {
                    let v = job.operands[job.operands.len - 1]; let object = job.operands[job.operands.len - 2]; let i = number_index(v);
                    // A dense run read neither charges nor faults.
                    if i >= 0 && dense(job, object) && !node(heap, object.id()).initialized { job.operands.pop(); job.operands.pop(); let e, found = own_element(heap, object, i); job.operands.push(e); }
                    else {
                        settle(job, ran, used); job.frames[top].pc = pc; job.operands.pop();
                        if i >= 0 && dense(job, object) { job.operands.pop(); let e, found = own_element(heap, object, i); job.operands.push(e); }
                        else { let k = key(heap, v); get_property(job, k); }
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
                },
                SetNamed s => {
                    settle(job, ran, used); job.frames[top].pc = pc; let value = job.operands.pop(); set_property(job, value, heap.constants[f.constants + s.k], false);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                SetNamedChain s => {
                    settle(job, ran, used); job.frames[top].pc = pc; let value = job.operands.pop(); set_property(job, value, heap.constants[f.constants + s.k], true);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                SetIndexed => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let v = job.operands.pop(); let object = job.operands[job.operands.len - 1]; let i = number_index(v);
                    if i >= 0 && dense(job, object) && writable(job, object.id()) { job.operands.pop(); if valid(heap, value) { set_element(heap, object, i, value); } job.operands.push(value); }
                    else { let k = key(heap, v); set_property(job, value, k, false); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Append => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let object = job.operands[job.operands.len - 1];
                    if dense(job, object) && writable(job, object.id()) { let end = node(heap, object.id()).length; if valid(heap, value) { set_element(heap, object, end, value); } }
                    else {
                        let k = if valid(heap, object) && object.tag() == 5 && node(heap, object.id()).kind == 1 { key(heap, number(node(heap, object.id()).length)) } else { heap.fault = Fault.TypeError; Value.Undefined };
                        set_property(job, value, k, true);
                    }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                CheckValue => {
                    let value = job.operands.pop(); if valid(heap, value) && value.tag() == 5 && node(heap, value.id()).kind == 3 { heap.fault = Fault.UnsupportedOperand; } job.operands.push(value);
                    if heap.fault != Fault.None { break; }
                },
                This t => {
                    var receiver = job.frames[top].receiver;
                    if t.sloppy && !f.strict && (receiver.tag() == 0 || receiver.tag() == 1) { receiver = heap.global; }
                    else if t.sloppy && !f.strict && receiver.tag() != 5 { heap.fault = Fault.UnsupportedOperand; }
                    job.operands.push(receiver);
                    if heap.fault != Fault.None { break; }
                },
                Global => { job.operands.push(heap.global); },
                GetGlobal g => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let k = heap.constants[f.constants + g.k];
                    let v, found = own_property(heap, heap.global, k);
                    if !found {
                        if job.protected_nodes > 0 { heap.fault = Fault.NativeBoundary; }
                        // Keep prototype-dependent reads explicitly unsupported.
                        get(heap, heap.global, k);
                        if g.throws && heap.fault == Fault.None { heap.fault = Fault.ReferenceError; }
                    }
                    job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Literal l => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let base = job.operands.len - (l.count);
                    let value = form_construct(heap, l.kind, job.operands[base..]); job.operands.resize(base); job.operands.push(value);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Form x => {
                    settle(job, ran, used);
                    let op = x.op; let base = job.operands.len - export_form_arity(op);
                    if op == 5 || op == 19 || op == 35 || op == 36 {
                        // A demand starts before the step counts (see invoke).
                        let demanded = demand_maps(job, job.operands[(job.operands.len - code.effects[at].pop)..], op == 36);
                        if demanded || heap.fault != Fault.None { job.steps -= 1; job.frames[top].pc = at; if !demanded { fail(job, heap.fault, frame_at(job, top, at)); } break; }
                    }
                    job.frames[top].pc = pc;
                    // The live compiler adapter opts into a demand at the exact saved
                    // intrinsic-call continuation. Sealed-plan users retain the
                    // explicit DependencyNeed failure from form_op below.
                    var waiting = false;
                    if op == 82 && job.helpers_enabled {
                        let args = job.operands[base..];
                        if valid(heap, args[0]) && args[0].tag() == 5 && node(heap, args[0].id()).kind == 0 && args[1].tag() <= 1 && args[2].tag() == 4 {
                            let value, found = own_property(heap, args[0], args[2]);
                            // The answer is written into the receiver: only the
                            // compiler roots object or a row the job allocated
                            // (light-cljs-795), never a retained image row.
                            if !found && heap.fault == Fault.None && !(args[0] == job.root_receiver || writable(job, args[0].id())) { heap.fault = Fault.NativeBoundary; }
                            if !found && heap.fault == Fault.None { request_effect(job, 9, args, frame_at(job, top, at)); waiting = true; }
                        }
                    }
                    if waiting { job.operands.resize(base); }
                    else { let value = form_op(heap, op, job.operands[base..]); job.operands.resize(base); job.operands.push(value); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // Every other registry opcode exports and validates but is not
                // implemented here: it fails the job by name, uncatchably, when it
                // runs. Never a fallback or a no-op (light-cljs-783).
                Unsupported => { heap.fault = Fault.Unsupported; break; },
            }
        }
        // A fault an arm raised on its own counts its instruction, as before:
        // step, work and budget, with the frame's pc past it.
        if heap.fault != Fault.None { settle(job, ran, used); job.frames[top].pc = pc; fail(job, heap.fault, frame_at(job, top, at)); }
    }
    return used;
}
// Call and CallMethod at `at`, with the instruction's counts settled. A map
// demand (mapv/vec/into over a deferred map) runs first and, when it starts
// or faults, leaves the call at its pc with no step counted; otherwise the
// call runs with its continuation written back. A closure callee has no
// demand and takes the direct path. Returns whether the frame continues in
// place.
inline fn invoke(job: Job&, top: i64, at: i64, argc: i64, method: bool) -> bool {
    let base = job.operands.len - argc - (if method { 2 } else { 1 });
    let callee = job.operands[base + (if method { 1 } else { 0 })];
    if callee.tag() == 5 && node(job.heap, callee.id()).kind == 2 {
        job.frames[top].pc = at + 1;
        let receiver = if method { job.operands[base] } else { Value.Undefined };
        if push_frame(job, callee, receiver, job.operands[(job.operands.len - argc)..], false, base) { job.operands.resize(base); }
        return in_place(job, top, at);
    }
    let demanded = demand_call(job, argc, method);
    if demanded || job.heap.fault != Fault.None { job.steps -= 1; job.frames[top].pc = at; if !demanded { fail(job, job.heap.fault, frame_at(job, top, at)); } return false; }
    job.frames[top].pc = at + 1;
    call(job, top, at, argc, method);
    return in_place(job, top, at);
}
// A call's map demand (mapv/vec/into over a deferred map) runs before the
// call consumes its operands; see demand_map.
fn demand_call(job: Job&, argc: i64, method: bool) -> bool {
    let heap .= job.heap;
    let base = job.operands.len - argc - (if method { 2 } else { 1 });
    var callee = job.operands[base + (if method { 1 } else { 0 })];
    var first = job.operands.len - argc;
    if valid(heap, callee) && callee.tag() == 5 && node(heap, callee.id()).kind == 4 && node(heap, callee.id()).function == 14 && method {
        callee = job.operands[base]; if first < job.operands.len { first += 1; }
    }
    if valid(heap, callee) && callee.tag() == 5 && node(heap, callee.id()).kind == 4 {
        let id = node(heap, callee.id()).function;
        if id == 3 || id == 4 { return demand_maps(job, job.operands[first..]); }
    }
    return false;
}
// `at` is the call instruction, the continuation an effect request saves.
fn call(job: Job&, top: i64, at: i64, argc: i64, method: bool) {
    let heap .= job.heap;
    let base = job.operands.len - argc - (if method { 2 } else { 1 });
    var callee = job.operands[base + (if method { 1 } else { 0 })];
    var receiver = if method { job.operands[base] } else { Value.Undefined };
    var first = job.operands.len - argc;
    // Function.prototype.call forwards its method receiver as callee.
    // Detached calls fail; missing thisArg is exactly undefined.
    var kind: i64 = if valid(heap, callee) && callee.tag() == 5 { node(heap, callee.id()).kind } else { -1 };
    if kind == 4 && node(heap, callee.id()).function == 14 {
        callee = receiver; receiver = if first < job.operands.len { job.operands[first] } else { Value.Undefined };
        if first < job.operands.len { first += 1; }
        kind = if valid(heap, callee) && callee.tag() == 5 { node(heap, callee.id()).kind } else { -1 };
    }
    if kind == 4 {
        let id = node(heap, callee.id()).function;
        let args = job.operands[first..];
        if id == 2 && args.len == 2 {
            if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; }
            else {
                let input = args[1]; let callback = args[0]; let out = form_collection(heap, 6, []);
                form_count(heap, input);
                job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: callback, receiver: Value.Undefined, input: input, output: out, native: Native.Mapv });
                job.operands.resize(base);
            }
        } else if id == 5 && job.gensym_next > 0 { let value = gensym(job, args); job.operands.resize(base); job.operands.push(value); }
        else if id >= 5 && id <= 8 { request_effect(job, id, args, frame_at(job, top, at)); job.operands.resize(base); }
        else { let value = native_value(heap, id, args); job.operands.resize(base); job.operands.push(value); }
    } else if push_frame(job, callee, receiver, job.operands[first..], false, base) { job.operands.resize(base); }
}
// `arguments` (unmapped, read-only) or a rest array after `skip` parameters.
fn arguments(job: Job&, frame: Frame, f: const Function&, rest: bool, skip_count: i64) {
    let heap .= job.heap;
    let start = frame.local_base + f.locals;
    let skip = if rest { min(skip_count, frame.argument_count) } else { 0 };
    let args = job.locals[(start + skip)..(start + frame.argument_count)];
    if !rest {
        if !values_fit(heap, args.len) || !nodes_fit(heap, 1) { memory(heap); }
        else if charge(heap, args.len) {
            let offset = add_values(heap, args);
            job.operands.push(allocate(heap, Node { kind: 14, start: offset as u32, count: args.len as u32 }));
        }
    } else {
        let array = allocate(heap, Node { kind: 1 });
        if array.tag() == 5 { for value, i in args { if !valid(heap, value) || !set_element(heap, array, i, value) { break; } } }
        job.operands.push(array);
    }
}
// Cell opcodes: 48 initialize, 49 read, 50 write.
fn cell(job: Job&, op: i64) {
    let heap .= job.heap;
    let value = if op == 49 { Value.Undefined } else { job.operands.pop() }; let cell = job.operands.pop();
    if valid(heap, cell) && cell.tag() == 5 && node(heap, cell.id()).kind == 3 {
        // A closure environment the job did not allocate is read-only.
        if op != 49 && !writable(job, cell.id()) { heap.fault = Fault.NativeBoundary; return; }
        // The write log keeps no cell writes (light-cljs-801): nothing reads them.
        let row .= node_mut(heap, cell.id());
        if op == 49 || op == 50 {
            if !row.initialized { heap.fault = Fault.ReferenceError; }
            else if op == 50 && row.immutable { heap.fault = Fault.ConstAssignment; }
            else if op == 49 { job.operands.push(row.value); }
            else { row.value = value; }
        } else if row.initialized && row.immutable { heap.fault = Fault.ConstAssignment; }
        else { row.value = value; row.initialized = true; }
    } else if heap.fault == Fault.None { heap.fault = Fault.TypeError; }
}
fn get_property(job: Job&, k: Value) {
    let heap .= job.heap;
    let object = job.operands.pop();
    if job.root_receiver.tag() == 5 && object == job.root_receiver {
        let value, found = own_property(heap, object, k);
        // Only the compiler root intrinsic may suspend and demand a
        // definition. A raw JS bypass must not observe stale undefined.
        if !found { heap.fault = Fault.DependencyNeed; } job.operands.push(value);
    } else if valid(heap, object) && object.tag() == 5 && (node(heap, object.id()).kind == 2 || node(heap, object.id()).kind == 4) && key_is(heap, k, "call") {
        job.operands.push(get(heap, object, k));
    } else if object.tag() == 5 && object.id() < job.protected_nodes {
        let value, found = own_property(heap, object, k);
        let optional = object == heap.global && (key_is(heap, k, "__lightCompilerRootReceiver") || key_is(heap, k, "__lightCompilerRootContext"));
        if !found && !optional { heap.fault = Fault.NativeBoundary; } job.operands.push(value);
    } else { let value = get(heap, object, k); job.operands.push(value); }
}
// Whether the job allocated node row o (Job.owned_nodes), so JavaScript may
// write it: not the realm, the restored image or definition, nor a
// publication installed during the job.
fn writable(job: const Job&, o: i64) -> bool {
    if o < job.protected_nodes || o < job.owned_nodes { return false; }
    var i: i64 = 0;
    while i + 1 < job.foreign.len { if o >= job.foreign[i] && o < job.foreign[i + 1] { return false; } i += 2; }
    return true;
}
// Mark the node rows a publication installed in this job, from `start` to
// the table's end, as rows the job did not allocate.
fn install_foreign(job: Job&, start: i64) {
    let end = job.heap.nodes.len; if end > start { job.foreign.push(start as u32); job.foreign.push(end as u32); }
}
// An unprotected array other than the compiler roots object: element opcodes
// read and write its run directly, without building key strings.
fn dense(job: Job&, object: Value) -> bool {
    let heap .= job.heap;
    if object.tag() != 5 || !valid(heap, object) || object.id() < job.protected_nodes || node(heap, object.id()).kind != 1 { return false; }
    return !(job.root_receiver.tag() == 5 && object == job.root_receiver);
}
// chain: the assignment expression yields the object (81, 88), else the value.
fn set_property(job: Job&, value: Value, k: Value, chain: bool) {
    let heap .= job.heap;
    let object = job.operands.pop();
    if object.tag() == 5 && !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; }
    else if put(heap, object, k, value) { record(job, object, k, value); }
    job.operands.push(if chain { object } else { value });
}
fn claim(job: Job&) -> bool { if job.claimed || job.effect_pending || job.status != tiny::Status.Ready { return false; } job.claimed = true; return true; }
fn run_claimed(job: Job&, budget: i64) -> tiny::Outcome {
    if !job.claimed { return tiny::Outcome.Busy; }
    defer { job.claimed = false; }
    var used: i64 = 0;
    while job.status == tiny::Status.Ready && !job.effect_pending && (budget < 0 || used < budget) {
        if job.frames.len == 0 || job.frames[job.frames.len - 1].function < 0 { native_step(job); used += 1; }
        else { used += execute(job, if budget < 0 { -1 } else { budget - used }); }
    }
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
    match v.tag() {
        0 => { out.append("undefined"); }, 1 => { out.append("null"); }, 2 => { out.append(if v.flag() { "boolean:true" } else { "boolean:false" }); },
        3 => { if v.num() != v.num() { out.append("number:NaN"); } else if v.num() == 0 && 1.0 / v.num() < 0 { out.append("number:-0"); } else { format(out, "number:", v.num()); } },
        4 => { out.append("utf16:["); for i in str_len(heap, v) { if i > 0 { out.push(','); } format(out, unit(heap, v, i)); } out.push(']'); },
        5 => { format(out, "identity:", heap.identity, ":", v.id(), ":", node(heap, v.id()).kind); },
        _ => { out.append("invalid:tag"); },
    }
}
