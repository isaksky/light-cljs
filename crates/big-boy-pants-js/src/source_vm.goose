import std;
import binary;
import math;
import .program;
import .vm;
import .export_schema;
import .realm;
import .array_callbacks;
import .cells;
import .continuations;

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
// hole; it never leaves an array run (`valid` refuses it elsewhere). As an
// object header's prototype, Empty stands for the realm's %Object.prototype%,
// which does not exist yet (light-cljs-787; see Node). Accessor is internal
// too: an accessor property's slot, naming its row of `heap.accessors`; it
// never leaves a property slot or record.
// tag() gives the GSP primitive tag the rest of the module compares.
enum Value { Undefined, Null, Bool { v: bool }, Number { v: f64 }, String { s: u32 }, Symbol { s: u32 }, BigInt { b: u32 }, Object { o: u32 }, Empty, Accessor { a: u32 } }
// A string header. `wide` runs are UTF-16 units; a heap header that is not
// wide is a Latin-1 run in heap.latin1, and a heap string is wide only when
// some unit is above 255. Code and wire strings (Code, bridge graphs,
// artifacts) are always wide. `key` marks the interned property key of its
// text (light-cljs-787, intern): it is set only on a header row interning
// created, never on an existing row, so image rows never change.
struct Str { start: u32, len: u32, wide: bool = true, key: bool = false }
inline fn tag(v: Value) -> i64 { return match v { Undefined => 0, Null => 1, Bool => 2, Number => 3, String => 4, Object => 5, Symbol => 6, BigInt => 7, Empty => -1, Accessor => -2 }; }
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
checkfree inline fn node(heap: const Heap&, o: i64) -> const Node& { if o < 0 || o >= heap.nodes.len { abort("row is not in table nodes"); } return heap.nodes[o]; }
inline fn node_mut(heap: Heap&, o: i64) -> Node& { if o < 0 || o >= heap.nodes.len { abort("row is not in table nodes"); } return heap.nodes[o]; }
fn set_node(heap: Heap&, o: i64, n: Node) { if o < 0 || o >= heap.nodes.len { abort("row is not in table nodes"); } heap.nodes[o] = n; }
checkfree inline fn slot(heap: const Heap&, row: i64) -> Value { if row < 0 || row >= heap.values.len { abort("row is not in table values"); } return heap.values[row]; }
inline fn set_slot(heap: Heap&, row: i64, v: Value) { if row < 0 || row >= heap.values.len { abort("row is not in table values"); } heap.values[row] = v; }
fn run(heap: const Heap&, start: i64, count: i64) -> const Value[:] { if start < 0 || count < 0 || start > heap.values.len || count > heap.values.len - start { abort("run is not in table values"); } return heap.values[start..(start + count)]; }
checkfree fn property_row(heap: const Heap&, row: i64) -> const Property& { if row < 0 || row >= heap.properties.len { abort("row is not in table properties"); } return heap.properties[row]; }
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
        Symbol s => Plain { tag: 6, payload: s.s }, BigInt b => Plain { tag: 7, payload: b.b }, Accessor a => Plain { tag: -2, payload: a.a }, _ => Plain { tag: v.tag() },
    };
}
fn plain_bytes(v: Value, out: u8[>..]&) { out.append(bytes_of([plain(v)])); }
// Bit-exact value identity: NaN equals itself, -0 differs from +0.
fn same_bits(a: Value, b: Value) -> bool { return bytes_of([plain(a)]) == bytes_of([plain(b)]); }
fn plain_bytes(n: Node, out: u8[>..]&) {
    out.append(bytes_of([n.kind, n.function, n.start, n.count, n.keys, n.key_count, n.key_capacity, n.length, n.origin, n.source, n.span_start, n.span_end, n.shape])); out.append(bytes_of([n.initialized, n.immutable]));
    plain_bytes(n.value, out); plain_bytes(n.namespace_value, out); plain_bytes(n.metadata, out); plain_bytes(n.proto, out);
    for v in n.slots { plain_bytes(v, out); }
}
fn native_code(n: Native) -> i64 { return match n { None => 0, Mapv => 1, MapSeq => 2, Setter => 4, CopyData => 5, ToPrimitive t => 3 + (t.op as i64) * 4 + (t.method as i64) * 1024, ArrayCallbacks a => -1 - (code(a.which) as i64), Swap s => -1001 - (code(s.which) as i64), Force x => -2001 - (code(x.which) as i64), Resume r => 3001 + (r.kind as i64) }; }
fn plain_bytes(f: Frame, out: u8[>..]&) {
    out.append(bytes_of([f.function, f.pc, f.local_base, f.operand_base, f.argument_count, f.index, native_code(f.native)]));
    plain_bytes(f.callee, out); plain_bytes(f.receiver, out); plain_bytes(f.input, out); plain_bytes(f.output, out);
}
fn plain_bytes(w: Write, out: u8[>..]&) { plain_bytes(w.object, out); plain_bytes(w.key, out); plain_bytes(w.value, out); }
fn plain_bytes(p: Property, out: u8[>..]&) { plain_bytes(p.key, out); plain_bytes(p.value, out); out.push(p.attrs); }
fn plain_bytes(s: Shape, out: u8[>..]&) { out.append(bytes_of([s.parent, s.count, s.child, s.sibling])); out.push(s.attrs); plain_bytes(s.key, out); }
fn plain_bytes(p: Pair, out: u8[>..]&) { plain_bytes(p.get, out); plain_bytes(p.set, out); }
fn plain_rows<T>(rows: const T[:]) -> u8[>..] { var out: u8[>..] = []; for r in rows { plain_bytes(r, out); } return out; }
// kind: GSP constructor kind (0 none, 1 ordinary, 2 base class, 3 derived
// class, 4 default derived class); generator: a generator or async generator
// (export flag bit 16), which has an own `prototype` though its kind is 0;
// asynchronous: an async function or async generator (bit 17).
// name_start and name_len: the function's `name` (SetFunctionName, the
// Rust compiler's inferred name included) as a run of Code.units; length:
// its `length` (ExpectedArgumentCount) (light-cljs-806).
struct Function { start: u32, count: u32, constants: u32, children: u32, params: u32, locals: u32, captures: u32, strict: bool, kind: i64 = 0, generator: bool = false, asynchronous: bool = false, name_start: u32 = 0, name_len: u32 = 0, length: u32 = 0 }
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
    InitCell, ReadCell, WriteCell, GetNamed { k: u32 }, GetIndexed, SetNamed { k: u32, strict: bool }, DefineNamed { k: u32, half: u8 },
    SetIndexed { strict: bool }, DefineIndexed { half: u8 }, SetProto, Delete { strict: bool }, In, Append, CheckValue, This { sloppy: bool }, Global, GetGlobal { k: u32, throws: bool },
    Literal { kind: u8, count: u32 }, Form { op: u32 }, Unsupported { opcode: u8 },
    // new f(...) (94): only a built-in constructor whose construction is its
    // call (Delay, light-cljs-808); any other callee fails Unsupported, as an
    // unimplemented opcode does.
    Construct { argc: u32 },
    // Finally blocks and completion records (light-cljs-786): the Pants
    // opcodes, with their stack contracts in rust/src/vm/instr.rs. Pending
    // is PendingThrow (118), PendingException (170) or PendingCancel (172).
    PushFinally { to: u32 }, PopFinally, EndFinally, CallFinally, DropCompletion, Pending { what: u8 },
    // Key enumeration (light-cljs-804): for-in (99, 100), object rest (93,
    // 127, 128, 129) and spread (180).
    ForInStart, ForInNext, OwnKeys, IsEnumerable, CopyData, ToPropertyKey, RequireCoercible,
    // typeof (78, light-cljs-788).
    TypeOf,
    // Generators and async functions (light-cljs-790, continuations.goose):
    // GeneratorStart (123) only marks code; GeneratorInitialYield (125),
    // GeneratorYield (124), AsyncStart (131) and Await (132) detach and
    // settle activations. GeneratorYieldStar (126) and AsyncGeneratorStart
    // (181) stay Unsupported.
    GeneratorStart, GeneratorInitialYield, GeneratorYield, AsyncStart, Await,
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
// TypeError, ReferenceError, RangeError, HostEffect and StackLimit throws
// (catchable(), light-cljs-786), JavaScript cannot catch it. Cancelled: the job's consumer
// cancelled it (cancel()); its finally blocks ran first. UnhandledRejection
// and Unsettled end a job whose promise jobs have drained (light-cljs-790,
// continuations.goose): a promise was rejected with no reaction, the
// result its thrown value; or the entry returned a promise that can never
// settle, since nothing is left to run.
enum Fault { None, InvalidState, StaleID, MemoryLimit, WorkLimit, InstructionLimit, StackLimit, TypeError, ReferenceError, ConstAssignment, UnsupportedOperand, NativeBoundary, DependencyNeed, EffectContract, HostEffect, Unsupported, Cancelled, RangeError, UnhandledRejection, Unsettled }
// Table limits count the rows a heap added above its image (Heap.image),
// not table lengths (light-cljs-792 constraint 4); each table checks its limit
// in its one allocation function. The call limits are the Pants engine's
// VmLimits::DEFAULT (light-cljs-786, rust/src/vm/frames.rs): at most `frames`
// live frames, the entry frame and native frames included, and at most
// `stack` values in the job's locals and operands together; a call or
// instruction that would pass either fails with a catchable StackLimit
// before it changes anything. Neither depends on the native stack: the
// dispatch loop never recurses per JavaScript call.
// `shapes` bounds shape and accessor rows together (light-cljs-787);
// `conts` the continuation table's rows (light-cljs-790).
struct Limits { nodes: i64 = 100000, values: i64 = 1048576, units: i64 = 1048576, properties: i64 = 1048576, work: i64 = 10000000, steps: i64 = 1000000, frames: i64 = 10000, stack: i64 = 1048576, shapes: i64 = 1048576, conts: i64 = 1048576 }
// All heap tables append until session teardown; IDs are never recycled.
// kind: plain=0, array=1, closure=2, cell=3. Captures and cell values never
// reference frame scratch. Property chains are indices, not Goose references.
// Runtime additions: builtin=4, list/vector/map/set=5/6/7/8,
// symbol/keyword=9/10, scalar occurrence=11, sequence cursor=12,
// opaque caught runtime error=13, read-only unmapped arguments=14,
// finite deferred map=15 (value=callback, namespace_value=input,
// start/count=reserved memo range, length=completed callbacks), key list=16
// (a for-in walk's or an object spread's own keys, job-local; see "Key
// enumeration"), a job's own atom or volatile=17 (value=state,
// length=flavor, 0 atom or 1 volatile) and a process-wide cell's
// reference=18 (function=cell, length=flavor: 0 atom, 1 volatile or 2
// delay, whose value is its thunk when the job holds it; cells.goose) and a
// job's own delay=19 (value=thunk, pending while truthy, namespace_value=the
// value it realized, light-cljs-808), a generator object=20 (function=its
// Activation row in heap.conts or no_row(), length=its state) and a
// promise=21 (function=its Promise row in heap.conts) (light-cljs-790;
// continuations.goose). Errors, generators and promises are ordinary
// objects with internal slots: their own properties are kind 0's, without
// an element run.
// Closure immutable marks fnApplyNull's immutable receiver convention.
// Errors preserve object identity/truthiness;
// property access/coercion and form transport reject unsupported inspection.
// Metadata/origin belong to the occurrence, independently of value equality.
// Links (function, start, keys, source) and counts are plain u32 rows;
// source is no_row() for a node without provenance. origin is the compiler
// occurrence the node came from, not a VM row, and stays -1 when absent.
//
// Own properties (light-cljs-787; decision 2 of docs/javascript-coverage.md).
// An ordinary object (kind 0) keeps its key_count properties in one of two
// modes. Shape mode (`initialized` clear): `shape` names a row of
// heap.shapes whose chain holds every key and its attributes, oldest first;
// property i's value is slots[i] for i < inline_slots() and otherwise
// heap.values[keys + i - inline_slots()], in a run of key_capacity rows.
// Dictionary mode (`initialized` set, after a delete, an attribute change or
// more than shape_limit() keys; also a sparse array's elements): shape is 0
// and the properties are Property records heap.properties[keys + i], in
// insertion order, in a run of key_capacity rows, and slots are cleared. An
// accessor property's value is Value.Accessor naming its getter/setter row.
// In either mode its elements, the index-keyed data properties, are
// heap.values[start..start + count], holes where absent (light-cljs-804;
// element_key). `proto` is the [[Prototype]]: Null, an object, or Empty, the realm's
// %Object.prototype% (%Array.prototype% for an array, %Function.prototype%
// for a function), which light-cljs-788 builds; until then a lookup that
// reaches it finds nothing, except the names it will define, which fail
// UnsupportedOperand (see on_intrinsic). An array (kind 1) and a closure
// (kind 2) keep named properties the same way (light-cljs-805): a dense
// array and a closure in shape mode (their `start` and `count` are the
// elements or the captures), a dictionary array's names among its index
// records. Other kinds keep no own properties.
struct Node { kind: i64, function: u32 = 0, start: u32 = 0, count: u32 = 0, keys: u32 = 0, key_count: u32 = 0, key_capacity: u32 = 0, length: u32 = 0, initialized: bool = false, immutable: bool = false, value: Value = Value.Undefined, namespace_value: Value = Value.Null, metadata: Value = Value.Undefined, origin: i64 = -1, source: u32 = 4294967295, span_start: u32 = 0, span_end: u32 = 0, shape: u32 = 0, proto: Value = Value.Empty, slots: Value[2] = [Value.Undefined; 2] }
struct SourceRange { start: u32, count: u32 }
// A dictionary-mode property record (see Node): an interned key, the value
// (Accessor for an accessor property) and its attribute bits (attr_*).
struct Property { key: Value = Value.Undefined, value: Value = Value.Undefined, attrs: u8 = 0 }
// A shape (light-cljs-787): an immutable layout record. Row 0 is the empty
// shape; every other row adds `key` (interned) with `attrs` to its parent,
// as property `count - 1` of objects in that shape. Transitions: `child` is
// the newest shape made from this one and `sibling` the next older child of
// the same parent (no_row() ends either list), so objects built the same way
// share shapes. A job adds children to image shapes, so the reset restores
// the image's shape rows (snapshot::truncate, Heap.image_written).
struct Shape { parent: u32 = 0, key: Value = Value.Undefined, attrs: u8 = 0, count: u32 = 0, child: u32 = 4294967295, sibling: u32 = 4294967295 }
// An accessor property's getter and setter (Undefined when absent).
struct Pair { get: Value = Value.Undefined, set: Value = Value.Undefined }
// A slot of the interned-key index (light-cljs-787, intern): an open-addressed
// table, a power of two long, of key header rows and their text hashes.
struct KeySlot { row: u32 = 4294967295, hash: u32 = 0 }
// Table lengths when the heap became an image (restore); 0 for a heap built
// by initialize. Allocation limits count rows above these tops.
struct Tops { nodes: i64 = 0, values: i64 = 0, properties: i64 = 0, units: i64 = 0, strings: i64 = 0, latin1: i64 = 0, shapes: i64 = 0, accessors: i64 = 0, conts: i64 = 0 }
root struct Heap {
    code:  const Code&, identity: i64 = 0, limits: Limits = Limits {},
    nodes: Node[>..]                  = [], values: Value[>..] = [], properties: Property[>..] = [],
    // light-cljs-787: shapes, accessor pairs and the interned-key index, with
    // its occupied slot count. image_written: this job added a transition to
    // an image shape or a key to the index, so the reset restores both.
    shapes:        Shape[>..] = [], accessors: Pair[>..] = [], keys: KeySlot[>..] = [], interned: i64 = 0, image_written: bool = false,
    units:         u16[>..]   = [], latin1: u8[>..] = [], strings: Str[>..] = [], constants: Value[>..] = [], work: i64 = 0,
    source_bytes:  u8[>..]    = [], source_ranges: SourceRange[>..] = [],
    property_rows: i64        = 0, image: Tops = Tops {},
    // Detached activations, their handlers, promise states and reactions
    // (light-cljs-790; continuations.goose, Cont).
    conts: Cont[>..] = [],
    // pending: the key of the pending property (realm.goose) an
    // UnsupportedOperand reached, for its report; fail() takes and clears it.
    pending:   Value = Value.Undefined,
    fault:     Fault = Fault.None, global: Value = Value.Undefined, call_method: Value = Value.Undefined, boot_count: i64 = 0, initializer_count: i64 = 0, producer_count: i64 = 0,
    live_jobs: i64   = 0,
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
//                toString; pc 1 awaits the method's result, pc 2 the
//                method from its getter (light-cljs-805). The operator
//                then runs on the converted operands.
//   Setter       [[Set]] through an accessor's setter (light-cljs-787):
//                output the assignment's result for the frame below; pc 1
//                awaits the setter's return value, which it drops. (A
//                getter needs no native frame: its return value is the
//                read's result, so its frame is entered directly.)
//   CopyData     CopyDataProperties of an object spread (light-cljs-804):
//                callee the key list of the source's own keys, input the
//                source, output the target, index the next key, one key a
//                step; pc 1 awaits a getter's result for key index - 1.
//   ArrayCallbacks  a built-in that calls back into JavaScript, its Builtin
//                the payload: Array.prototype.map and forEach
//                (array_callbacks.goose, which gives its fields and phases).
//   Swap         swap! and swap-vals! (light-cljs-799; cells.goose, which
//                gives its fields and phases), its Builtin the payload.
//   Force        force, deref and derefProtocol of a delay (light-cljs-808;
//                cells.goose, which gives its fields and phases).
//   Resume       a generator's next (kind 0), return (1) or throw (2)
//                (light-cljs-790; continuations.goose): input the
//                generator, its activation attached above; pc 1 awaits the
//                value it yields or returns.
enum Native { None, Mapv, MapSeq, Setter, CopyData, ToPrimitive { op: u8, method: u8 }, ArrayCallbacks { which: Builtin }, Swap { which: Builtin }, Force { which: Builtin }, Resume { kind: u8 } }
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
// frame's pending completion and resumes it. Async (light-cljs-790) is an
// async function's completion boundary, its frame's first entry, `value`
// its promise: an exception that reaches it rejects the promise, and a
// return resolves it (continuations.goose). Every entry is plain data: a
// suspended job saves its handlers with its frames, and a detached
// activation its own (Cont.Saved).
enum HandlerKind { Catch, Finally, Normal, Return, Throw, Cancel, Jump, Async }
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
    cancelling: bool = false,
    // The message of the error a fault raises, when the faulting operation
    // names one (a built-in's TypeError), or the built-in step an
    // UnsupportedOperand did not implement (realm.goose); cleared by fail.
    detail:      u16[>..] = [],
    effect_args: Value[8] = [Value.Undefined; 8], effect_count: i64 = 0, effect_frame: Frame = Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined },
    // The cell and identity of every row the job loaded from or stored into
    // a cell (cells.goose), so it imports each once; saved with its
    // continuation.
    cell_rows: CellRow[>..] = [],
    // The compiler id of the macro job this is, which holds its delay claims
    // (0: no macro job, which claims none), and how many it holds unsettled
    // (light-cljs-808; cells.goose).
    cell_holder: i64 = 0, claims: i64 = 0,
    // The heap's code, writable, in a macro job: a function loaded from a
    // cell installs its code there (light-cljs-809; null elsewhere, where
    // such a load fails by name).
    cell_code: Code? = null,
    // Promise jobs (light-cljs-790; continuations.goose): the job's FIFO of
    // reactions, `queue_head` its next; `draining` once the entry has
    // finished with `entry`, its value; and the promises rejected while no
    // reaction was registered on them. All of it is the job's, saved with
    // its continuation and cleared by reset.
    queue: Task[>..] = [], queue_head: i64 = 0, draining: bool = false, entry: Value = Value.Undefined, rejections: u32[>..] = [],
}
// A row a job holds that names a process-wide cell (a cell reference) or
// has a process-wide identity (a value loaded from or stored in a cell, or
// a node of a publication it installed). count > 1: the rows row..row +
// count, a publication's, whose identities are identity..identity + count.
struct CellRow { row: i64, cell: i64 = -1, identity: i64 = 0, count: i64 = 1 }
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
        // Method (102), an object literal's method, getter or setter: obj c1..cn
        // → obj f, a closure like Function's; its home object only serves
        // `super`, whose opcodes are not implemented (light-cljs-787).
        43, 102 => Ins.Closure { child: a }, 44 => Ins.Call { argc: a }, 94 => Ins.Construct { argc: a }, 83 => Ins.CallMethod { argc: a },
        101 => Ins.Arguments, 179 => Ins.Rest { skip: a }, 45 => Ins.Callee, 46 => Ins.Capture { slot: a },
        47, 198 => Ins.NewCell { immutable: false }, 197 => Ins.NewCell { immutable: true },
        48 => Ins.InitCell, 49 => Ins.ReadCell, 50 => Ins.WriteCell,
        79 => Ins.GetNamed { k: a }, 90 => Ins.GetIndexed,
        80 => Ins.SetNamed { k: a, strict: false }, 151 => Ins.SetNamed { k: a, strict: true }, 91 => Ins.SetIndexed { strict: false }, 152 => Ins.SetIndexed { strict: true }, 88 => Ins.Append,
        81 => Ins.DefineNamed { k: a, half: 0 }, 103 => Ins.DefineNamed { k: a, half: 1 }, 104 => Ins.DefineNamed { k: a, half: 2 },
        130 => Ins.DefineIndexed { half: 0 }, 133 => Ins.DefineIndexed { half: 1 }, 134 => Ins.DefineIndexed { half: 2 },
        95 => Ins.SetProto, 97 => Ins.Delete { strict: false }, 153 => Ins.Delete { strict: true }, 98 => Ins.In,
        161 => Ins.CheckValue, 86 => Ins.This { sloppy: true }, 145 => Ins.This { sloppy: false }, 105 => Ins.Global,
        106 => Ins.GetGlobal { k: a, throws: true }, 107 => Ins.GetGlobal { k: a, throws: false },
        191 => Ins.Literal { kind: 5, count: a }, 192 => Ins.Literal { kind: 6, count: a },
        193 => Ins.Literal { kind: 7, count: a * 2 }, 195 => Ins.Literal { kind: 8, count: a },
        194 => Ins.Form { op: a },
        57 => Ins.PushFinally { to: a }, 59 => Ins.PopFinally, 60 => Ins.EndFinally, 71 => Ins.CallFinally, 72 => Ins.DropCompletion,
        118, 170, 172 => Ins.Pending { what: op.code as u8 },
        78 => Ins.TypeOf, 99 => Ins.ForInStart, 100 => Ins.ForInNext, 128 => Ins.OwnKeys, 129 => Ins.IsEnumerable, 180 => Ins.CopyData, 93 => Ins.ToPropertyKey, 127 => Ins.RequireCoercible,
        123 => Ins.GeneratorStart, 125 => Ins.GeneratorInitialYield, 124 => Ins.GeneratorYield, 131 => Ins.AsyncStart, 132 => Ins.Await,
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
    while code.ins.len < code.ops.len { code.ins.push(Ins.Pop); code.effects.push(Effect { pop: -1, push: -1 }); }
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
        code.functions.push(Function { start: program::word(bytes, at) as u32, count: program::word(bytes, at + 4) as u32, constants: program::word(bytes, at + 8) as u32, children: program::word(bytes, at + 16) as u32, params: program::word(bytes, at + 24) as u32, locals: program::word(bytes, at + 28) as u32, captures: program::word(bytes, at + 32) as u32, strict: (flags & 1) == 1, kind: (flags >> 8) & 255, generator: ((flags >> 16) & 1) == 1, asynchronous: ((flags >> 17) & 1) == 1, name_start: program::word(bytes, at + 36) as u32, name_len: program::word(bytes, at + 40) as u32, length: program::word(bytes, at + 44) as u32 });
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
checkfree fn charge(heap: Heap&, count: i64 = 1) -> bool {
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
    for f in other.functions { var n = f; n.start += ops as u32; n.constants += constants as u32; n.children += children as u32; n.name_start += units as u32; out.functions.push(n); }
    for op in other.ops { var n = op; n.start += bytes; n.end += bytes; if branches(n.code) { n.arg += ops; } out.ops.push(n); }
    let strings = out.strings.len;
    for s in other.strings { out.strings.push(Str { start: s.start + units as u32, len: s.len }); }
    for v in other.constants { out.constants.push(if v.tag() == 4 { Value.String { s: (v.sid() + strings) as u32 } } else { v }); }
    for id in other.children { out.children.push((functions + id) as u32); }
    out.units.append(other.units); out.source_bytes.append(other.source_bytes); decode(out); out.valid = true;
    return functions;
}
fn memory(heap: Heap&) -> bool { heap.fault = Fault.MemoryLimit; return false; }
// The heap's table lengths, as the image tops a restore or reset sets.
fn tops(heap: const Heap&) -> Tops { return Tops { nodes: heap.nodes.len, values: heap.values.len, properties: heap.properties.len, units: heap.units.len, strings: heap.strings.len, latin1: heap.latin1.len, shapes: heap.shapes.len, accessors: heap.accessors.len, conts: heap.conts.len }; }
fn valid(heap: Heap&, v: Value) -> bool {
    if heap.fault != Fault.None { return false; }
    return match v {
        // A well-known symbol (the realm's Symbol properties) is a value.
        Symbol x => if export_well_known_symbol(x.s).len > 0 { true } else { heap.fault = Fault.InvalidState; false },
        Empty, BigInt, Accessor => { heap.fault = Fault.InvalidState; false },
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
// String constants are interned keys (light-cljs-787), so a named property
// access needs no hashing: the constant is its key.
fn add_constants(heap: Heap&, values: const Value[:]) -> i64 {
    let at = heap.constants.len;
    for v in values { heap.constants.push(if v.tag() == 4 { intern(heap, v) } else { v }); }
    return at;
}
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
// Interned property keys (light-cljs-787). heap.keys indexes every key
// header (Str.key) by its text's hash, open-addressed with linear probing,
// at most half full. intern(k) is the key header of k's text: k itself if
// it is one, the indexed header with the same text, or else a new header row
// over k's units, which it indexes. Interning never marks an existing row,
// so image rows never change; it does write image slots of the index, so it
// marks the image written and the reset restores the index (Heap). Charges
// the text's length to hash it; a key header costs one compare.
fn key_hash(heap: const Heap&, s: Str) -> u32 {
    var h: u32 = 2166136261;
    if s.wide { for u in wide_run(heap, s) { h = (h ^ u) * 16777619; } } else { for b in narrow_run(heap, s) { h = (h ^ b) * 16777619; } }
    return h;
}
fn index_slot(keys: KeySlot[>..]&, row: u32, hash: u32) {
    let mask = keys.len - 1; var i = hash & mask;
    while keys[i].row != no_row() { i = (i + 1) & mask; }
    keys[i] = KeySlot { row: row, hash: hash };
}
fn intern(heap: Heap&, k: Value) -> Value {
    if k.tag() == 6 { return k; }
    if k.tag() != 4 { if heap.fault == Fault.None { heap.fault = Fault.InvalidState; } return Value.Undefined; }
    let s = header(heap, k.sid());
    if s.key { return k; }
    if !charge(heap, s.len + 1) { return Value.Undefined; }
    let h = key_hash(heap, s);
    if heap.keys.len > 0 {
        let mask = heap.keys.len - 1; var i = h & mask;
        while heap.keys[i].row != no_row() {
            let e = heap.keys[i];
            if e.hash == h && same_text(heap, Value.String { s: e.row }, k) { return Value.String { s: e.row }; }
            i = (i + 1) & mask;
        }
    }
    let c = string_ref(heap, s.start, s.len, s.wide);
    if c.tag() != 4 { return Value.Undefined; }
    heap.strings[c.sid()].key = true;
    if 2 * (heap.interned + 1) > heap.keys.len {
        let capacity = max(64, heap.keys.len * 2);
        if !charge(heap, capacity) { return Value.Undefined; }
        var old: KeySlot[>..] = []; for e in heap.keys { if e.row != no_row() { old.push(e); } }
        heap.keys.clear(); heap.keys.resize(capacity, KeySlot {});
        for e in old { index_slot(heap.keys, e.row, e.hash); }
    }
    index_slot(heap.keys, c.sid(), h); heap.interned += 1; heap.image_written = true;
    return c;
}
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
    // The realm's intrinsics are the first node rows (realm.goose).
    return build_realm(heap);
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
// A property key (light-cljs-787): a string, or a well-known symbol, which
// valid() refuses as a value but which is a key like any other.
fn key_valid(heap: Heap&, k: Value) -> bool {
    if k.tag() != 6 { return valid(heap, k); }
    if heap.fault != Fault.None { return false; }
    if export_well_known_symbol(symbol_index(k)).len == 0 { heap.fault = Fault.InvalidState; return false; }
    return true;
}
fn key(heap: Heap&, value: Value) -> Value {
    if !key_valid(heap, value) { return Value.Undefined; }
    if value.tag() == 4 || value.tag() == 6 { return value; }
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
// The interned key of array index i.
fn index_key(heap: Heap&, i: i64) -> Value { let k = key(heap, number(i)); return if heap.fault == Fault.None { intern(heap, k) } else { Value.Undefined }; }
fn own_element(heap: Heap&, array: Value, i: i64) -> Value, bool {
    if node(heap, array.id()).initialized {
        let k = index_key(heap, i);
        if heap.fault != Fault.None { return Value.Undefined, false; }
        let p = find_own(heap, array.id(), k);
        return p.value, p.found;
    }
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
    if !node(heap, array.id()).initialized && sparse_write(node(heap, array.id()).count, i) && !array_to_dictionary(heap, array) { return false; }
    if node(heap, array.id()).initialized {
        let k = index_key(heap, i);
        if heap.fault != Fault.None { return false; }
        let p = find_own(heap, array.id(), k);
        if heap.fault != Fault.None { return false; }
        if p.found { write_own(heap, array.id(), p.index, value); } else if !dict_append(heap, array.id(), Property { key: k, value: value, attrs: attrs_data() }) { return false; }
        if i >= node(heap, array.id()).length { node_mut(heap, array.id()).length = (i + 1) as u32; }
        return true;
    }
    if !element_room(heap, array.id(), i) { return false; }
    set_slot(heap, node(heap, array.id()).start + i, value);
    if i >= node(heap, array.id()).length { node_mut(heap, array.id()).length = (i + 1) as u32; }
    return true;
}
// Room for element i in node o's element run (an array's or an ordinary
// object's), charged: a write past the capacity copies the elements into a
// new, larger run at the end of the table, the new slots holes, and clears
// the old run to undefined. A rejection changes nothing.
fn element_room(heap: Heap&, o: i64, i: i64) -> bool {
    let count = node(heap, o).count;
    if i < count { return charge(heap); }
    let old = node(heap, o).start;
    var capacity = max(i + 1, max(count * 2, 4));
    if !values_fit(heap, capacity) { capacity = i + 1; }
    if !values_fit(heap, capacity) { return memory(heap); }
    if !charge(heap, capacity + count) { return false; }
    let start = add_values(heap, run(heap, old, count), capacity - count, hole());
    // No node owns the old run now: holes may live only in element runs.
    for at in old..(old + count) { set_slot(heap, at, Value.Undefined); }
    node_mut(heap, o).start = start as u32; node_mut(heap, o).count = capacity as u32;
    return true;
}
// Ordinary objects' index keys (light-cljs-804; decision 2: "integer-index
// keys stay separate and go to element runs"). An own data property of an
// ordinary object whose key is an array index and whose attributes are all
// set (CreateDataProperty's, attrs_data) is an element: slot i of the
// object's element run heap.values[start..start + count], as an array's are,
// a hole where the object has no element i, with no length. Any other
// index-keyed property (an accessor, other attributes, or an index far past
// the run, sparse_write) stays a shape key or dictionary record like a name.
// A key is in one place only: an element is never also a shape key, so a
// lookup tries the run first and the shape after. own_keys orders both
// together, indices first. Elements leave the shape path untouched: a name
// costs one header read more (element_key), and a shape never holds an index
// that the run could.
//
// The array index of interned key k, or -1; only a key whose first unit is a
// digit is decoded (index, charged by length).
fn element_key(heap: Heap&, k: Value) -> i64 {
    if k.tag() != 4 { return -1; }
    let s = header(heap, k.sid());
    if s.wide || s.len == 0 || s.len > 10 { return -1; }
    let c = narrow_run(heap, s)[0];
    if c < 48 || c > 57 { return -1; }
    return index(heap, k);
}
// Element i of ordinary object o, or a hole.
checkfree fn object_element(heap: const Heap&, o: i64, i: i64) -> Value {
    let n .= node(heap, o);
    return if i >= 0 && i < n.count { slot(heap, n.start + i) } else { Value.Empty };
}
// Add or replace element i of ordinary object o. A rejection changes nothing.
fn put_element(heap: Heap&, o: i64, i: i64, value: Value) -> bool {
    if i >= heap.limits.values { return memory(heap); }
    if !element_room(heap, o, i) { return false; }
    set_slot(heap, node(heap, o).start + i, value); return true;
}
// Whether a new data property with index i (-1: a name) of ordinary object o
// becomes an element rather than a shape key.
fn as_element(heap: const Heap&, o: i64, i: i64) -> bool { return i >= 0 && !sparse_write(node(heap, o).count, i); }
// Move a dense array's elements into index-keyed property records, a new key
// run at the table's end, followed by its named properties (light-cljs-805),
// in creation order with their attributes; its inline slots and overflow run
// are cleared. Every failure point precedes the header change, so a rejection
// leaves the array as it was (records and keys built so far stay charged,
// unowned).
fn array_to_dictionary(heap: Heap&, array: Value) -> bool {
    let o = array.id(); let start = node(heap, o).start; let count = node(heap, o).count; let end = min(node(heap, o).length, count);
    let named: i64 = node(heap, o).key_count;
    var records: i64 = 0; for i in end { if slot(heap, start + i).tag() != -1 { records += 1; } }
    let capacity = max(4, records + named);
    let run = add_properties(heap, capacity);
    if run < 0 { return false; }
    // Records are built aside: the header changes only after every failure point.
    var built: Property[>..] = [];
    for i in end {
        let v = slot(heap, start + i);
        if v.tag() == -1 { continue; }
        let k = index_key(heap, i);
        if heap.fault != Fault.None { return false; }
        built.push(Property { key: k, value: v, attrs: attrs_data() });
    }
    let base = built.len; built.resize(base + named, Property {});
    var s: i64 = node(heap, o).shape;
    while s != 0 { let e = shape_row(heap, s); built[base + e.count - 1] = Property { key: e.key, value: slot_value(heap, o, e.count - 1), attrs: e.attrs }; s = e.parent; }
    if !charge(heap, count + named) { return false; }
    for row in start..(start + count) { set_slot(heap, row, Value.Undefined); }
    for i in named { set_slot_value(heap, o, i, Value.Undefined); }
    node_mut(heap, o).keys = run as u32; node_mut(heap, o).key_count = built.len as u32; node_mut(heap, o).key_capacity = capacity as u32; node_mut(heap, o).initialized = true; node_mut(heap, o).count = 0; node_mut(heap, o).shape = 0;
    for p, i in built { set_dict_at(heap, o, i, p); }
    return true;
}

// ---- The property model (light-cljs-787; decision 2 of
// docs/javascript-coverage.md, the Pants engine's object.rs, object/shape.rs,
// vm/property.rs, vm/prototype.rs and vm/descriptor.rs for semantics).
//
// Keys are interned (intern): a property key is the one String header of its
// text with `key` set, or a well-known Symbol, so shapes, records and lookups
// compare key words, never text. An array-index key is the canonical decimal
// string; arrays keep their elements in element runs (above), and so do
// ordinary objects their index-keyed data properties (element_key, above);
// own_keys orders index keys first. Ordinary objects keep their other properties in shapes or dictionary
// records (Node). The internal methods (own_entry, lookup, assign, define_own,
// has_property, delete_property, own_keys) dispatch on the object kind by
// `match`, where the exotic kinds (arrays and their `length`, read-only
// arguments, functions' `call`) override the ordinary ones; a kind whose
// properties are not implemented yet fails UnsupportedOperand. None of them
// runs JavaScript: a getter or setter they find goes back to the dispatch
// loop (get_property, set_property), which runs it as a frame.

// Attribute bits of a property (a shape entry or a dictionary record).
inline fn attr_writable() -> u8 { return 1; }
inline fn attr_enumerable() -> u8 { return 2; }
inline fn attr_configurable() -> u8 { return 4; }
// CreateDataProperty's attributes, which assignment also gives a new property.
inline fn attrs_data() -> u8 { return 7; }
// An object literal's accessor (DefineGetter, DefineSetter): enumerable and
// configurable.
inline fn attrs_accessor() -> u8 { return 6; }
// The keys a shape-mode object holds before it turns to dictionary mode (the
// Pants engine's object/shape.rs).
fn shape_limit() -> i64 { return 64; }
checkfree inline fn inline_slots() -> i64 { return 2; }
// Values a shape-mode object keeps in its overflow run.
fn overflow(n: const Node&) -> i64 { return max(0, n.key_count - inline_slots()); }
checkfree inline fn shape_row(heap: const Heap&, s: i64) -> const Shape& { if s < 0 || s >= heap.shapes.len { abort("row is not in table shapes"); } return heap.shapes[s]; }
inline fn shape_mut(heap: Heap&, s: i64) -> Shape& { if s < 0 || s >= heap.shapes.len { abort("row is not in table shapes"); } return heap.shapes[s]; }
fn accessor_row(heap: const Heap&, a: i64) -> Pair { if a < 0 || a >= heap.accessors.len { abort("row is not in table accessors"); } return heap.accessors[a]; }
// The accessor pair an Accessor slot names.
fn pair(heap: const Heap&, v: Value) -> Pair { return match v { Accessor x => accessor_row(heap, x.a), _ => Pair {} }; }
inline fn is_accessor(v: Value) -> bool { return v.tag() == -2; }
// Shape and accessor rows count together against limits.shapes (Limits).
fn shape_room(heap: Heap&, count: i64) -> bool {
    return room(heap, heap.shapes.len + heap.accessors.len, heap.image.shapes + heap.image.accessors, heap.limits.shapes, count);
}
// The one allocation point of the property table: `count` cleared rows at
// its end, charged to the rows the heap has added above its image
// (limits.properties counts allocated rows, spare capacity included, not live
// records; property_rows is their running total for snapshot validation).
// Returns the run's first row, or -1 with MemoryLimit.
fn add_properties(heap: Heap&, count: i64) -> i64 {
    if !room(heap, heap.properties.len, heap.image.properties, heap.limits.properties, count) { return -1; }
    let at = heap.properties.len; for i in count { heap.properties.push(Property {}); } heap.property_rows += count;
    return at;
}
// The shape table's one allocation function: the empty shape, row 0, comes
// first. Returns the new row, or -1 with MemoryLimit.
fn add_shape(heap: Heap&, s: Shape) -> i64 {
    let root = if heap.shapes.len == 0 { 1 } else { 0 };
    if !shape_room(heap, 1 + root) { return -1; }
    if root == 1 { heap.shapes.push(Shape {}); }
    heap.shapes.push(s); return heap.shapes.len - 1;
}
// The accessor table's one allocation function.
fn add_accessor(heap: Heap&, p: Pair) -> i64 {
    if !shape_room(heap, 1) { return -1; }
    heap.accessors.push(p); return heap.accessors.len - 1;
}
// An interned key's identity: its header row, or a symbol's index above
// every row; -1 for anything else.
checkfree inline fn key_word(k: Value) -> i64 { return match k { String x => x.s, Symbol x => 4294967296 + (x.s as i64), _ => -1 }; }
// The child of shape s that adds key k with attrs, made if s has none. A job
// that links a child to an image shape marks the image written (Heap).
fn transition(heap: Heap&, s: i64, k: Value, attrs: u8) -> i64 {
    let w = key_word(k);
    var c: i64 = if s < heap.shapes.len { shape_row(heap, s).child } else { no_row() };
    while c != no_row() {
        if !charge(heap) { return -1; }
        let e .= shape_row(heap, c);
        if key_word(e.key) == w && e.attrs == attrs { return c; }
        c = e.sibling;
    }
    let count: i64 = if s == 0 { 1 } else { shape_row(heap, s).count + 1 };
    let older = if s < heap.shapes.len { shape_row(heap, s).child } else { no_row() };
    let at = add_shape(heap, Shape { parent: s as u32, key: k, attrs: attrs, count: count as u32, sibling: older });
    if at < 0 { return -1; }
    shape_mut(heap, s).child = at as u32;
    if s < heap.image.shapes { heap.image_written = true; }
    return at;
}
// The property of shape s whose key is k, newest first: its position and
// attributes, or -1. One work unit an entry visited.
checkfree fn shape_find(heap: Heap&, s: i64, k: Value) -> i64, u8 {
    let w = key_word(k); var at = s;
    while at != 0 {
        if !charge(heap) { return -1, 0; }
        let e .= shape_row(heap, at);
        if key_word(e.key) == w { return e.count - 1, e.attrs; }
        at = e.parent;
    }
    return -1, 0;
}
// Property i's value of a shape-mode object.
checkfree fn slot_value(heap: const Heap&, o: i64, i: i64) -> Value {
    let n .= node(heap, o);
    return if i >= 0 && i < inline_slots() { n.slots[i] } else { slot(heap, n.keys + i - inline_slots()) };
}
fn set_slot_value(heap: Heap&, o: i64, i: i64, v: Value) {
    if i >= 0 && i < inline_slots() { node_mut(heap, o).slots[i] = v; } else { set_slot(heap, node(heap, o).keys + i - inline_slots(), v); }
}
// Record i of a dictionary-mode object.
checkfree fn dict_at(heap: const Heap&, o: i64, i: i64) -> Property { return property_row(heap, node(heap, o).keys + i); }
fn set_dict_at(heap: Heap&, o: i64, i: i64, p: Property) { set_property_row(heap, node(heap, o).keys + i, p); }
// An own property found by find_own or own_entry: its position, its value
// (Accessor for an accessor property) and its attributes. `element`: an
// ordinary object's element, whose position is its index (own_entry).
struct Own { found: bool = false, index: i64 = -1, value: Value = Value.Undefined, attrs: u8 = 0, element: bool = false }
// The own property with interned key k of an ordinary object or a dictionary
// array, without the exotic cases (own_entry): newest first, one work unit
// an entry visited.
checkfree fn find_own(heap: Heap&, o: i64, k: Value) -> Own {
    if node(heap, o).initialized {
        let w = key_word(k); var i: i64 = node(heap, o).key_count;
        while i > 0 {
            i -= 1;
            if !charge(heap) { return Own {}; }
            let p = dict_at(heap, o, i);
            if key_word(p.key) == w { return Own { found: true, index: i, value: p.value, attrs: p.attrs }; }
        }
        return Own {};
    }
    let i, attrs = shape_find(heap, node(heap, o).shape, k);
    if i < 0 { return Own {}; }
    return Own { found: true, index: i, value: slot_value(heap, o, i), attrs: attrs };
}
// Replace own property i's value, keeping its attributes.
fn write_own(heap: Heap&, o: i64, i: i64, v: Value) {
    if node(heap, o).initialized { var p = dict_at(heap, o, i); p.value = v; set_dict_at(heap, o, i, p); } else { set_slot_value(heap, o, i, v); }
}
// Append a record to a dictionary-mode object. A full run moves to a run of
// twice the capacity at the table's end, allocated (and charged, spare rows
// included) through add_properties; the old rows are cleared and stay
// charged: tables are append-only within a job. A rejection changes nothing.
fn dict_append(heap: Heap&, o: i64, p: Property) -> bool {
    let count: i64 = node(heap, o).key_count;
    if count == node(heap, o).key_capacity {
        let start = node(heap, o).keys; let capacity = max(4, count * 2);
        let at = add_properties(heap, capacity);
        if at < 0 { return false; }
        for i in count { set_property_row(heap, at + i, property_row(heap, start + i)); set_property_row(heap, start + i, Property {}); }
        node_mut(heap, o).keys = at as u32; node_mut(heap, o).key_capacity = capacity as u32;
    }
    set_dict_at(heap, o, count, p); node_mut(heap, o).key_count = (count + 1) as u32;
    return true;
}
// Add own property k, which the object does not have, with value v (Accessor
// for an accessor) and attrs: the shape's transition and an inline slot or
// one in the overflow run of values, which moves to a run of twice the
// capacity when full as dict_append's does; past shape_limit() keys, a
// dictionary record. A rejection leaves the object as it was.
fn add_own(heap: Heap&, o: i64, k: Value, v: Value, attrs: u8) -> bool {
    if !node(heap, o).initialized && node(heap, o).key_count >= shape_limit() && !to_dictionary(heap, o) { return false; }
    if node(heap, o).initialized { return dict_append(heap, o, Property { key: k, value: v, attrs: attrs }); }
    let count: i64 = node(heap, o).key_count;
    let s = transition(heap, node(heap, o).shape, k, attrs);
    if s < 0 { return false; }
    let spilled = count - inline_slots();
    if spilled >= 0 && spilled == node(heap, o).key_capacity {
        let start = node(heap, o).keys; let capacity = max(4, spilled * 2);
        if !values_fit(heap, capacity) { return memory(heap); }
        let at = add_values(heap, run(heap, start, spilled), capacity - spilled);
        if at < 0 { return false; }
        for r in start..(start + spilled) { set_slot(heap, r, Value.Undefined); }
        node_mut(heap, o).keys = at as u32; node_mut(heap, o).key_capacity = capacity as u32;
    }
    node_mut(heap, o).shape = s as u32; node_mut(heap, o).key_count = (count + 1) as u32; set_slot_value(heap, o, count, v);
    return true;
}
// Move a shape-mode object's properties into dictionary records, in order,
// with their attributes; its slots and overflow run are cleared (the run
// stays charged). An array moves its elements too (array_to_dictionary),
// since `initialized` marks both. A rejection leaves the object as it was.
fn to_dictionary(heap: Heap&, o: i64) -> bool {
    if node(heap, o).kind == 1 { return array_to_dictionary(heap, object(o)); }
    let count: i64 = node(heap, o).key_count;
    if !charge(heap, count) { return false; }
    let capacity = max(4, count + 1);
    let at = add_properties(heap, capacity);
    if at < 0 { return false; }
    var s: i64 = node(heap, o).shape;
    while s != 0 { let e = shape_row(heap, s); set_property_row(heap, at + e.count - 1, Property { key: e.key, value: slot_value(heap, o, e.count - 1), attrs: e.attrs }); s = e.parent; }
    for i in count { set_slot_value(heap, o, i, Value.Undefined); }
    node_mut(heap, o).initialized = true; node_mut(heap, o).shape = 0; node_mut(heap, o).keys = at as u32; node_mut(heap, o).key_capacity = capacity as u32;
    return true;
}
// The record of own property i (find_own's position) once node o is in
// dictionary mode, moving it there first: a shape-mode object's property i
// is its record i, a dense array's follows its element records
// (array_to_dictionary, light-cljs-805). -1 on a rejection, which leaves the
// node as it was.
fn dictionary_at(heap: Heap&, o: i64, i: i64) -> i64 {
    if node(heap, o).initialized { return i; }
    let named: i64 = node(heap, o).key_count;
    if !to_dictionary(heap, o) { return -1; }
    return node(heap, o).key_count - named + i;
}
// SameValue (descriptor validation): numbers by bits but NaN equal to NaN,
// strings by text, everything else by identity.
fn same_value(heap: Heap&, a: Value, b: Value) -> bool {
    if a.tag() == 3 && b.tag() == 3 && a.num() != a.num() && b.num() != b.num() { return true; }
    if a.tag() == 4 && b.tag() == 4 { return same_text(heap, a, b); }
    return same_bits(a, b);
}
// [[DefineOwnProperty]] of an ordinary object (OrdinaryDefineOwnProperty,
// ValidateAndApplyPropertyDescriptor) for interned key k: with `half` 0 a
// complete data descriptor of `value` and attrs; with `half` 1 (getter) or 2
// (setter) an accessor descriptor whose other function is kept from an
// existing accessor, as an object literal's DefineGetter and DefineSetter
// merge. Returns whether the descriptor was accepted: a non-configurable
// property refuses to become configurable, to change enumerability or kind,
// and, if not writable, to become writable or change its value. An accepted
// change of attributes moves a shape-mode object to dictionary mode. An
// accessor pair is never written in place: a merge makes a new row, so an
// image row never changes. An ordinary object's element (light-cljs-804) is
// configurable: a complete data descriptor with every attribute replaces its
// value; any other turns it into a shape key or record, the element becoming
// a hole only once that property exists.
fn define_own(heap: Heap&, o: i64, k: Value, value: Value, attrs: u8, half: i64 = 0) -> bool {
    let i = if node(heap, o).kind == 0 { element_key(heap, k) } else { -1 };
    if heap.fault != Fault.None { return false; }
    let data = half == 0 && attrs == attrs_data();
    if i >= 0 && object_element(heap, o, i).tag() != -1 {
        if data { set_slot(heap, node(heap, o).start + i, value); return true; }
        var v = value;
        if half != 0 {
            let a = add_accessor(heap, if half == 1 { Pair { get: value } } else { Pair { set: value } });
            if a < 0 { return false; }
            v = Value.Accessor { a: a as u32 };
        }
        if !add_own(heap, o, k, v, attrs) { return false; }
        set_slot(heap, node(heap, o).start + i, hole()); return true;
    }
    let p = find_own(heap, o, k);
    if heap.fault != Fault.None { return false; }
    if !p.found && data && as_element(heap, o, i) { return put_element(heap, o, i, value); }
    var v = value;
    if half != 0 {
        var fns = if p.found && is_accessor(p.value) { pair(heap, p.value) } else { Pair {} };
        if half == 1 { fns.get = value; } else { fns.set = value; }
        // A non-configurable accessor accepts only its own function again,
        // with the same attributes (SameValue of the half), changing nothing.
        if p.found && p.attrs & attr_configurable() == 0 {
            let old = pair(heap, p.value);
            return is_accessor(p.value) && attrs == p.attrs && same_bits(if half == 1 { old.get } else { old.set }, value);
        }
        let a = add_accessor(heap, fns);
        if a < 0 { return false; }
        v = Value.Accessor { a: a as u32 };
    }
    if !p.found { return add_own(heap, o, k, v, attrs); }
    if p.attrs & attr_configurable() == 0 {
        if is_accessor(p.value) || attrs & attr_configurable() != 0 || (attrs & attr_enumerable()) != (p.attrs & attr_enumerable()) { return false; }
        if p.attrs & attr_writable() == 0 && (attrs & attr_writable() != 0 || !same_value(heap, p.value, value)) { return false; }
    }
    if attrs != p.attrs {
        let at = dictionary_at(heap, o, p.index);
        if at < 0 { return false; }
        set_dict_at(heap, o, at, Property { key: k, value: v, attrs: attrs });
        return true;
    }
    write_own(heap, o, p.index, v); return true;
}
// [[Delete]] of an ordinary object's own property (or a dictionary array's
// record) with interned key k: true when the object has no property k
// afterwards; false, changing nothing, for a non-configurable one. A delete
// moves a shape-mode object to dictionary mode; later records move down.
fn delete_own(heap: Heap&, o: i64, k: Value) -> bool {
    let p = find_own(heap, o, k);
    if heap.fault != Fault.None { return false; }
    if !p.found { return true; }
    if p.attrs & attr_configurable() == 0 { return false; }
    let at = dictionary_at(heap, o, p.index);
    if at < 0 { return false; }
    let count: i64 = node(heap, o).key_count;
    if !charge(heap, count - at) { return false; }
    for i in (at + 1)..count { set_dict_at(heap, o, i - 1, dict_at(heap, o, i)); }
    set_dict_at(heap, o, count - 1, Property {}); node_mut(heap, o).key_count = (count - 1) as u32;
    return true;
}
// Functions' own properties (light-cljs-805, light-cljs-806; the Pants
// engine's rust/src/vm/function.rs). A closure (kind 2) keeps its properties
// as an ordinary object does, in a shape or dictionary records beside its
// captures (`start`, `count`). Every closure has `length` (its code's
// ExpectedArgumentCount) and then `name` (its SetFunctionName name), both
// configurable only; code from a `function` declaration or expression
// (constructor kind 1, Function.kind) then has `prototype`, writable, not
// enumerable, not configurable: a new ordinary object whose non-enumerable
// `constructor` is the function. A closure without own properties has not
// made them yet (lazy_function): they come from its code row. Reads of
// `length` and `name`, `in`, hasOwnProperty and key listing answer them
// without making them (as the Pants engine's function::own_metadata does);
// a job's first read of `prototype`, and its first write, definition or
// delete of any key, makes all three in that order (prepare_function,
// materialize_function), so creation order is the specification's. They are
// made before any other property and a delete moves the closure to
// dictionary mode (`initialized`), so a closure with them made never looks
// lazy again. Arrows, methods and async functions have no `prototype`. A
// generator function's (light-cljs-790) is made the same way, a new object
// whose [[Prototype]] is %GeneratorPrototype% and which has no
// `constructor`. Not implemented, failing UnsupportedOperand by name: a class
// constructor's eager `prototype` and an async generator's, and therefore
// those functions' key lists; computed names (SetComputedName). Built-ins
// (kind 4) have their own `length` and `name` from the realm (light-cljs-788).
fn constructor_kind(heap: const Heap&, o: i64) -> i64 { return heap.code.functions[node(heap, o).function].kind; }
fn lazy_function(heap: const Heap&, o: i64) -> bool {
    let n .= node(heap, o);
    return n.kind == 2 && n.key_count == 0 && !n.initialized;
}
// Whether closure o's code gives it an own `prototype`: an ordinary
// function's or a (synchronous) generator's.
fn has_prototype(heap: const Heap&, o: i64) -> bool { let f .= heap.code.functions[node(heap, o).function]; return f.kind == 1 || f.generator && !f.asynchronous; }
fn lazy_prototype(heap: const Heap&, o: i64) -> bool { return lazy_function(heap, o) && has_prototype(heap, o); }
// Whether closure o's `prototype` is one this VM does not make (a class
// constructor's or an async generator's).
fn unsupported_prototype(heap: const Heap&, o: i64) -> bool { let f .= heap.code.functions[node(heap, o).function]; return f.kind >= 2 || f.generator && f.asynchronous; }
// Closure o's `length` and `name` from its code row.
fn function_length(heap: const Heap&, o: i64) -> Value { return number(heap.code.functions[node(heap, o).function].length); }
fn function_name(heap: Heap&, o: i64) -> Value {
    let f = heap.code.functions[node(heap, o).function];
    return string(heap, heap.code.units[f.name_start..(f.name_start + f.name_len)]);
}
// Whether closure o's own properties answer key k; otherwise it fails
// UnsupportedOperand (see above). An ordinary function's `prototype` must
// exist already: the job's operations make it first (prepare_function).
fn function_key(heap: Heap&, o: i64, k: Value) -> bool {
    if heap.fault != Fault.None { return false; }
    if k.tag() != 4 { return true; }
    if key_is(heap, k, "prototype") && (unsupported_prototype(heap, o) || lazy_prototype(heap, o)) {
        if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; }
    }
    return heap.fault == Fault.None;
}
// Whether a change to closure o from a Goose caller is refused: a lazy
// closure's properties must be made first (materialize_function, which a
// job's operations do), so it fails UnsupportedOperand.
fn lazy_change(heap: Heap&, o: i64) -> bool {
    if !lazy_function(heap, o) { return false; }
    if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; }
    return true;
}
// Make lazy closure o's `length`, `name` and, for an ordinary function,
// `prototype`. A rejection (MemoryLimit) leaves a fault set.
fn materialize_function(heap: Heap&, o: i64) -> bool {
    if !lazy_function(heap, o) { return heap.fault == Fault.None; }
    let length = key_named(heap, "length"); let name = key_named(heap, "name");
    let n = function_name(heap, o);
    if heap.fault != Fault.None || !add_own(heap, o, length, function_length(heap, o), attr_configurable()) || !add_own(heap, o, name, n, attr_configurable()) { return false; }
    if !has_prototype(heap, o) { return true; }
    let constructor = key_named(heap, "constructor"); let prototype = key_named(heap, "prototype");
    if heap.fault != Fault.None { return false; }
    let ordinary = constructor_kind(heap, o) == 1;
    let p = allocate(heap, if ordinary { Node { kind: 0 } } else { Node { kind: 0, proto: object(generator_prototype_row()) } });
    if heap.fault != Fault.None || ordinary && !add_own(heap, p.id(), constructor, object(o), attr_writable() | attr_configurable()) { return false; }
    return add_own(heap, o, prototype, p, attr_writable());
}
// [[GetOwnProperty]] by object kind (interned key k): ordinary objects their
// shapes or records; arrays their writable, non-enumerable, non-configurable
// `length`, their elements and their named properties (light-cljs-805: shape
// keys of a dense array, records of a dictionary array); read-only unmapped
// arguments their `length` and indices; closures their own properties (see
// above) and, failing those, `call` (Function.prototype's, until the realm),
// built-ins only `call`. Every other kind fails UnsupportedOperand: it is
// not implemented yet.
fn own_entry(heap: Heap&, object: Value, k: Value) -> Own {
    let o: i64 = object.id();
    match node(heap, o).kind {
        0, 13, 20, 21 => {
            if node(heap, o).count > 0 {
                let i = element_key(heap, k);
                if heap.fault != Fault.None { return Own {}; }
                let v = object_element(heap, o, i);
                if v.tag() != -1 { return Own { found: true, index: i, value: v, attrs: attrs_data(), element: true }; }
            }
            return find_own(heap, o, k);
        },
        1 => {
            if key_is(heap, k, "length") { return Own { found: true, value: number(node(heap, o).length), attrs: attr_writable() }; }
            let i = index(heap, k);
            if heap.fault != Fault.None { return Own {}; }
            if i < 0 || node(heap, o).initialized { return find_own(heap, o, k); }
            let v, found = own_element(heap, object, i);
            return Own { found: found, index: i, value: v, attrs: attrs_data() };
        },
        14 => {
            // Unmapped, read-only arguments subset. Mutation, callee and
            // prototypes fail explicitly; no frame slot or parameter alias
            // escapes to the heap.
            if key_is(heap, k, "length") { return Own { found: true, value: number(node(heap, o).count), attrs: attr_writable() | attr_configurable() }; }
            let i = index(heap, k);
            if i < 0 { if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; } return Own {}; }
            if i >= node(heap, o).count { return Own {}; }
            return Own { found: true, index: i, value: slot(heap, node(heap, o).start + i), attrs: attrs_data() };
        },
        2 => {
            // A lazy closure's `length` and `name` come from its code row.
            if k.tag() == 4 && lazy_function(heap, o) {
                if key_is(heap, k, "length") { return Own { found: true, value: function_length(heap, o), attrs: attr_configurable() }; }
                if key_is(heap, k, "name") { return Own { found: true, value: function_name(heap, o), attrs: attr_configurable() }; }
            }
            if !function_key(heap, o, k) { return Own {}; }
            return find_own(heap, o, k);
        },
        4 => { return find_own(heap, o, k); },
        _ => { if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; } return Own {}; },
    }
}
// [[Get]] of interned key k (OrdinaryGet through the prototype chain, each
// object's own lookup by kind), without running JavaScript: the value, or,
// for an accessor property with a getter, Undefined and the getter, which
// the caller runs with the original object as receiver. One work unit a
// prototype followed.
fn lookup(heap: Heap&, object: Value, k: Value) -> Value, Value {
    var at = object;
    loop {
        let p = own_entry(heap, at, k);
        if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
        if p.found {
            if is_pending(p.value) { pending_fault(heap, k); return Value.Undefined, Value.Undefined; }
            return if is_accessor(p.value) { Value.Undefined } else { p.value }, if is_accessor(p.value) { pair(heap, p.value).get } else { Value.Undefined };
        }
        let proto = prototype(heap, at.id());
        if proto.tag() != 5 { return Value.Undefined, Value.Undefined; }
        if !charge(heap) { return Value.Undefined, Value.Undefined; }
        at = proto;
    }
}
// own_entry for callers that need only whether the property is there and
// its attributes (key enumeration): an ordinary function's lazy `prototype`
// is a non-enumerable property not yet made, without a value.
fn enumerable_entry(heap: Heap&, object: Value, k: Value) -> Own {
    if lazy_prototype(heap, object.id()) && key_is(heap, k, "prototype") { return Own { found: true, attrs: attr_writable() }; }
    return own_entry(heap, object, k);
}
// [[HasProperty]] of interned key k: an own or inherited property, data or
// accessor (OrdinaryHasProperty).
fn has_property(heap: Heap&, object: Value, k: Value) -> bool {
    var at = object;
    loop {
        // An ordinary function's lazy `prototype` is present without being
        // made (light-cljs-805; the Pants engine's function::probe).
        if lazy_prototype(heap, at.id()) && key_is(heap, k, "prototype") { return true; }
        let p = own_entry(heap, at, k);
        if heap.fault != Fault.None { return false; }
        if p.found { return true; }
        let proto = prototype(heap, at.id());
        if proto.tag() != 5 { return false; }
        if !charge(heap) { return false; }
        at = proto;
    }
}
// [[Set]] of interned key k with the object as receiver (OrdinarySet,
// OrdinarySetWithOwnDescriptor), without running JavaScript. Returns the
// setter to run, with the object as receiver and the value as argument, when
// the property found is an accessor with one; otherwise Undefined and
// whether the assignment was accepted: a non-writable data property, own or
// inherited, or an accessor without a setter refuses it (a TypeError in
// strict code). An own writable data property is written; otherwise the
// object gets a new data property. Arrays write `length`, an element or a
// named property, closures their own properties (light-cljs-805); other
// kinds, a function's keys function_key refuses, and the accessors the
// pending realm prototypes define (Object.prototype's `__proto__`,
// Function.prototype's `arguments` and `caller`; light-cljs-788) fail
// UnsupportedOperand.
fn assign(heap: Heap&, object: Value, k: Value, value: Value) -> Value, bool {
    let o: i64 = object.id(); let kind = node(heap, o).kind;
    match kind {
        0, 4, 13, 20, 21 => {},
        1 => {
            if key_is(heap, k, "length") { return Value.Undefined, array_length(heap, object, value); }
            let i = index(heap, k);
            if heap.fault != Fault.None { return Value.Undefined, false; }
            if i >= 0 { return Value.Undefined, set_element(heap, object, i, value); }
        },
        2 => {
            if !function_key(heap, o, k) || lazy_change(heap, o) { return Value.Undefined, false; }
        },
        _ => { heap.fault = Fault.UnsupportedOperand; return Value.Undefined, false; },
    }
    // An element is a writable data property (light-cljs-804); only an
    // ordinary object keeps index keys outside its shape or records here.
    let i = if kind == 0 { element_key(heap, k) } else { -1 };
    if heap.fault != Fault.None { return Value.Undefined, false; }
    if object_element(heap, o, i).tag() != -1 { set_slot(heap, node(heap, o).start + i, value); return Value.Undefined, true; }
    let own = find_own(heap, o, k);
    if heap.fault != Fault.None { return Value.Undefined, false; }
    if own.found {
        if is_pending(own.value) { pending_fault(heap, k); return Value.Undefined, false; }
        if is_accessor(own.value) { let f = pair(heap, own.value).set; return f, f.tag() == 5; }
        if own.attrs & attr_writable() == 0 { return Value.Undefined, false; }
        write_own(heap, o, own.index, value); return Value.Undefined, true;
    }
    // A pending property on the chain (Object.prototype's `__proto__`,
    // Function.prototype's `arguments` and `caller`) fails by name.
    var proto = prototype(heap, o);
    while proto.tag() == 5 {
        if !charge(heap) { return Value.Undefined, false; }
        let p = own_entry(heap, proto, k);
        if heap.fault != Fault.None { return Value.Undefined, false; }
        if p.found {
            if is_pending(p.value) { pending_fault(heap, k); return Value.Undefined, false; }
            if is_accessor(p.value) { let f = pair(heap, p.value).set; return f, f.tag() == 5; }
            if p.attrs & attr_writable() == 0 { return Value.Undefined, false; }
            break;
        }
        proto = prototype(heap, proto.id());
    }
    return Value.Undefined, if as_element(heap, o, i) { put_element(heap, o, i, value) } else { add_own(heap, o, k, value, attrs_data()) };
}
// [[DefineOwnProperty]] by object kind: ordinary objects (define_own), an
// array's elements as data properties and its named properties, a closure's
// own properties (light-cljs-805); an array's `length`, its elements with
// other descriptors, other kinds and a function's keys function_key refuses
// fail UnsupportedOperand.
fn define_property(heap: Heap&, object: Value, k: Value, value: Value, attrs: u8, half: i64 = 0) -> bool {
    let o: i64 = object.id(); let kind = node(heap, o).kind;
    if kind == 0 || kind == 4 || kind == 13 || kind == 20 || kind == 21 { return define_own(heap, o, k, value, attrs, half); }
    if kind == 1 && !key_is(heap, k, "length") {
        let i = index(heap, k);
        if heap.fault != Fault.None { return false; }
        if i < 0 { return define_own(heap, o, k, value, attrs, half); }
        if half == 0 && attrs == attrs_data() { return set_element(heap, object, i, value); }
    }
    if kind == 2 {
        if !function_key(heap, o, k) || lazy_change(heap, o) { return false; }
        return define_own(heap, o, k, value, attrs, half);
    }
    if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; }
    return false;
}
// [[Delete]] by object kind: ordinary objects (delete_own); an array's
// element becomes a hole (a dictionary array's record is removed), its
// `length` is non-configurable, and a named property goes as an ordinary
// object's does, as does a closure's (light-cljs-805). Other kinds, and a
// function's keys function_key refuses, fail UnsupportedOperand.
fn delete_property(heap: Heap&, object: Value, k: Value) -> bool {
    let o: i64 = object.id();
    match node(heap, o).kind {
        4 => { return delete_own(heap, o, k); },
        0, 13, 20, 21 => {
            let i = element_key(heap, k);
            if heap.fault != Fault.None { return false; }
            if object_element(heap, o, i).tag() != -1 { if !charge(heap) { return false; } set_slot(heap, node(heap, o).start + i, hole()); return true; }
            return delete_own(heap, o, k);
        },
        1 => {
            if key_is(heap, k, "length") { return false; }
            let i = index(heap, k);
            if heap.fault != Fault.None { return false; }
            if i < 0 || node(heap, o).initialized { return delete_own(heap, o, k); }
            if !charge(heap) { return false; }
            if i < node(heap, o).count && i < node(heap, o).length { set_slot(heap, node(heap, o).start + i, hole()); }
            return true;
        },
        2 => {
            if lazy_function(heap, o) && k.tag() == 4 {
                if lazy_prototype(heap, o) && key_is(heap, k, "prototype") { return false; }
                if (key_is(heap, k, "length") || key_is(heap, k, "name")) && lazy_change(heap, o) { return false; }
            }
            return function_key(heap, o, k) && delete_own(heap, o, k);
        },
        _ => { heap.fault = Fault.UnsupportedOperand; return false; },
    }
}
// [[OwnPropertyKeys]] (OrdinaryOwnPropertyKeys) of an ordinary object or an
// array, appended to out: array-index keys in ascending numeric order, then
// the other strings in property creation order, then symbols in creation
// order. An array's are its element indices, then `length`; an ordinary
// object's elements (light-cljs-804) are index keys like its index-keyed
// shape keys; a closure's are its own properties in creation order, `length`,
// `name` and `prototype` first (light-cljs-806), except a class constructor's
// or a generator's. Other kinds fail UnsupportedOperand. Index keys of elements are
// interned strings the call creates. own_property_keys gives each key's
// attribute bits and array index (-1 for a name or symbol) too.
struct OwnKey { key: Value, attrs: u8 = 0, index: i64 = -1 }
fn own_property_keys(heap: Heap&, object: Value, out: OwnKey[>..]&) -> bool {
    let o: i64 = object.id(); let kind = node(heap, o).kind;
    if kind != 0 && kind != 1 && kind != 2 && kind != 4 && kind != 13 && kind != 20 && kind != 21 || kind == 2 && unsupported_prototype(heap, o) { heap.fault = Fault.UnsupportedOperand; return false; }
    // A lazy closure's are `length`, `name` and, for an ordinary function,
    // `prototype`, not yet made (light-cljs-806).
    if kind == 2 && lazy_function(heap, o) {
        for name in ["length", "name", "prototype"] {
            if name == "prototype" && !has_prototype(heap, o) { break; }
            let k = key_named(heap, name);
            if heap.fault != Fault.None { return false; }
            out.push(OwnKey { key: k, attrs: if name == "prototype" { attr_writable() } else { attr_configurable() } });
        }
        return charge(heap, 3);
    }
    var keys: OwnKey[>..] = [];
    // An element run's scan costs a work unit a slot, holes included.
    if (kind == 0 || !node(heap, o).initialized) && !charge(heap, if kind == 0 { node(heap, o).count as i64 } else { min(node(heap, o).length, node(heap, o).count) as i64 }) { return false; }
    if kind == 1 && !node(heap, o).initialized {
        let end = min(node(heap, o).length, node(heap, o).count);
        for i in end { if slot(heap, node(heap, o).start + i).tag() != -1 { keys.push(OwnKey { key: index_key(heap, i), attrs: attrs_data(), index: i }); } }
    } else if kind == 0 {
        for i in node(heap, o).count { if slot(heap, node(heap, o).start + i).tag() != -1 { keys.push(OwnKey { key: index_key(heap, i), attrs: attrs_data(), index: i }); } }
    }
    // Shape keys or records: a dense array's are its named properties
    // (light-cljs-805), a dictionary array's its elements and names.
    if node(heap, o).initialized {
        for i in node(heap, o).key_count { let p = dict_at(heap, o, i); keys.push(OwnKey { key: p.key, attrs: p.attrs, index: -2 }); }
    } else {
        let base = keys.len; keys.resize(base + node(heap, o).key_count, OwnKey { key: Value.Undefined, index: -2 });
        var s: i64 = node(heap, o).shape;
        while s != 0 { let e .= shape_row(heap, s); keys[base + e.count - 1].key = e.key; keys[base + e.count - 1].attrs = e.attrs; s = e.parent; }
    }
    if heap.fault != Fault.None || !charge(heap, keys.len) { return false; }
    // A shape key or record (index -2 until decoded) may be an index too.
    for i in keys.len { if keys[i].index == -2 { keys[i].index = index(heap, keys[i].key); } }
    if heap.fault != Fault.None { return false; }
    var indices: OwnKey[>..] = [];
    for e in keys { if e.index >= 0 { indices.push(e); } }
    stable_sort(indices) { a, b => a.index < b.index };
    out.append(indices);
    if kind == 1 { let length = text(heap, "length"); out.push(OwnKey { key: intern(heap, length), attrs: attr_writable() }); }
    for e in keys { if e.key.tag() == 4 && e.index < 0 { out.push(e); } }
    for e in keys { if e.key.tag() == 6 { out.push(e); } }
    return heap.fault == Fault.None;
}
fn own_keys(heap: Heap&, object: Value, out: Value[>..]&) -> bool {
    var keys: OwnKey[>..] = [];
    if !own_property_keys(heap, object, keys) { return false; }
    for e in keys { out.push(e.key); }
    return true;
}
// [[SetPrototypeOf]] of an ordinary object (OrdinarySetPrototypeOf) to Null
// or an object: false, changing nothing, when the object is on that
// prototype's chain (a cycle).
fn set_prototype(heap: Heap&, o: i64, proto: Value) -> bool {
    var p = proto;
    while p.tag() == 5 { if !charge(heap) { return false; } if p.id() == o { return false; } p = prototype(heap, p.id()); }
    node_mut(heap, o).proto = proto; return true;
}
// Own data property by kind for Goose callers (compiler services, form
// operations): presence separately, since an own value can be undefined. An
// accessor property fails UnsupportedOperand: its getter can run only from
// the dispatch loop.
fn own_property(heap: Heap&, object: Value, k: Value) -> Value, bool {
    let key = intern(heap, k);
    if heap.fault != Fault.None { return Value.Undefined, false; }
    let p = own_entry(heap, object, key);
    if p.found && is_accessor(p.value) { heap.fault = Fault.UnsupportedOperand; return Value.Undefined, false; }
    return p.value, p.found;
}
// [[Get]] of a string key on any value, without running JavaScript: the
// value and, for an accessor with a getter, the getter its caller must run
// with `object` as receiver. A string primitive has its `length` and
// indices; other primitives' prototypes are not implemented
// (UnsupportedOperand), and undefined and null are a TypeError.
fn read(heap: Heap&, object: Value, k: Value) -> Value, Value {
    if !valid(heap, object) || !key_valid(heap, k) { return Value.Undefined, Value.Undefined; }
    if k.tag() != 4 && k.tag() != 6 { heap.fault = Fault.InvalidState; return Value.Undefined, Value.Undefined; }
    if object.tag() == 4 {
        if key_is(heap, k, "length") { return number(str_len(heap, object)), Value.Undefined; }
        let i = index(heap, k); if i >= 0 { if i >= str_len(heap, object) { return Value.Undefined, Value.Undefined; } return unit_string(heap, object, i), Value.Undefined; }
        heap.fault = Fault.UnsupportedOperand; return Value.Undefined, Value.Undefined;
    }
    if object.tag() != 5 { heap.fault = if object.tag() <= 1 { Fault.TypeError } else { Fault.UnsupportedOperand }; return Value.Undefined, Value.Undefined; }
    let key = intern(heap, k);
    if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
    return lookup(heap, object, key);
}
// [[Get]] for Goose callers: an accessor's getter fails UnsupportedOperand,
// since only the dispatch loop can run it (get_property).
fn get(heap: Heap&, object: Value, k: Value) -> Value {
    let v, getter = read(heap, object, k);
    if getter.tag() != 0 && heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; }
    return v;
}
// [[Set]] of a string key: the setter to run (see assign) and whether the
// assignment was accepted. Only objects have properties to set: a primitive
// is a TypeError (undefined, null) or not implemented yet.
fn write(heap: Heap&, object: Value, k: Value, value: Value) -> Value, bool {
    if !valid(heap, object) || !key_valid(heap, k) || !valid(heap, value) { return Value.Undefined, false; }
    if object.tag() != 5 || k.tag() != 4 && k.tag() != 6 { heap.fault = if object.tag() == 5 { Fault.UnsupportedOperand } else { Fault.TypeError }; return Value.Undefined, false; }
    let key = intern(heap, k);
    if heap.fault != Fault.None { return Value.Undefined, false; }
    return assign(heap, object, key, value);
}
// [[Set]] for Goose callers: a setter fails UnsupportedOperand and a refused
// assignment is a TypeError.
fn put(heap: Heap&, object: Value, k: Value, value: Value) -> bool {
    let setter, ok = write(heap, object, k, value);
    if heap.fault != Fault.None { return false; }
    if setter.tag() != 0 { heap.fault = Fault.UnsupportedOperand; return false; }
    if !ok { heap.fault = Fault.TypeError; }
    return ok;
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
        // Each record's array index, -1 for a named property (light-cljs-805),
        // which stays; the preflight decodes them all, so mutation below
        // cannot fail.
        var indices: i64[>..] = [];
        for r in count {
            if !charge(heap, 2 + str_len(heap, dict_at(heap, o, r).key)) { return false; }
            indices.push(index(heap, dict_at(heap, o, r).key));
            if heap.fault != Fault.None { return false; }
        }
        // Compact the kept records in order; the cut-off rows are cleared.
        var kept: i64 = 0;
        for r in count { if indices[r] < length { set_dict_at(heap, o, kept, dict_at(heap, o, r)); kept += 1; } }
        for r in kept..count { set_dict_at(heap, o, r, Property {}); }
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
        // A generator an exception or cancellation leaves is completed.
        match f.native { MapSeq => { node_mut(job.heap, f.input.id()).initialized = false; }, Resume => { complete_generator(job.heap, f.input.id()); }, _ => {} }
    }
}
// An error object (light-cljs-788, realm.goose error_object): a node of kind
// 13, an ordinary object with its type's prototype (TypeError.prototype for
// a TypeError fault, InternalError.prototype for a stack overflow) and an
// own `message`, whose `function` is error_code of the catchable fault it was
// raised for, or 0 for one JavaScript made (Error(...)). Uncaught, a fault's
// error reports that fault again; JavaScript's is a thrown value.
fn error_code(fault: Fault) -> u32 { return match fault { TypeError => 1, ReferenceError => 2, HostEffect => 3, StackLimit => 4, RangeError => 5, _ => 0 }; }
fn error_fault(code: i64) -> Fault { return match code { 1 => Fault.TypeError, 2 => Fault.ReferenceError, 3 => Fault.HostEffect, 4 => Fault.StackLimit, 5 => Fault.RangeError, _ => Fault.InvalidState }; }
// The JavaScript error name of a catchable fault, as the Pants engine and
// QuickJS name it (a call-depth or value-stack overflow is QuickJS's
// InternalError "stack overflow").
fn error_name(fault: Fault) -> const u8[:] { return match fault { TypeError => "TypeError", ReferenceError => "ReferenceError", RangeError => "RangeError", StackLimit => "InternalError", _ => "Error" }; }
fn catchable(fault: Fault) -> bool { return error_code(fault) != 0; }
// Whether an exception raised now would reach a catch or finally block.
fn catches(job: const Job&) -> bool {
    for h in job.handlers { if h.kind == HandlerKind.Catch || h.kind == HandlerKind.Finally || h.kind == HandlerKind.Async { return true; } }
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
// is raised as an error object of its type with its message (Job.detail, or
// fault_message's wording); every other fault ends the job, keeping
// Job.detail for its report (the built-in step it did not implement).
fn fail(job: Job&, fault: Fault, frame: Frame) {
    job.heap.fault = Fault.None;
    if fault == Fault.UnsupportedOperand && job.detail.len == 0 && job.heap.pending.tag() != 0 { pending_detail(job); }
    job.heap.pending = Value.Undefined;
    var reported = fault;
    if catchable(fault) && catches(job) {
        let message = fault_message(job, fault, frame);
        let error = if job.heap.fault == Fault.None { error_object(job.heap, fault_type(fault), error_code(fault), message) } else { Value.Undefined };
        job.detail.clear();
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
// A built-in step or pending property an UnsupportedOperand did not
// implement follows, its spaces as dashes (Job.detail):
// `Fault.UnsupportedOperand:Object:-primitive-wrapper-objects`.
fn describe_fault(out: u8[>..]&, code: const Code&, job: const Job&) {
    format(out, job.fault);
    if job.fault == Fault.Unsupported && job.fault_opcode >= 0 { out.push(':'); unsupported_operation(out, code, job); }
    if job.fault != Fault.Unsupported && job.detail.len > 0 { out.push(':'); for u in job.detail { out.push(if u == 32 { 45 } else if u > 126 { 63 } else { u as u8 }); } }
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
        if h.kind != HandlerKind.Catch && h.kind != HandlerKind.Finally && h.kind != HandlerKind.Async { continue; }
        if !unwind_to(job, h) { terminate(job, Fault.InvalidState, frame); return; }
        // An async function's boundary: its promise is rejected and it
        // returns the promise (continuations.goose).
        if h.kind == HandlerKind.Async { async_settle(job, h, value, true, frame); return; }
        if h.kind == HandlerKind.Catch { job.operands.push(value); }
        else { job.handlers.push(Handler { frame: h.frame, pc: h.pc, operands: h.operands, locals: h.locals, kind: HandlerKind.Throw, value: value }); }
        return;
    }
    uncaught(job, value, frame);
}
// An exception no handler caught ends the job: an error object a fault
// raised reports that fault, any other value is a thrown result.
fn uncaught(job: Job&, value: Value, frame: Frame) {
    if value.tag() == 5 && node(job.heap, value.id()).kind == 13 && node(job.heap, value.id()).function != 0 { terminate(job, error_fault(node(job.heap, value.id()).function), frame); return; }
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
        // An async function returns its promise, resolved with the value.
        if h.frame == top && h.kind == HandlerKind.Async { async_settle(job, h, value, false, frame_at(job, top, at)); return; }
    }
    let lb = job.frames[top].local_base; let ob = job.frames[top].operand_base;
    job.frames.pop(); job.locals.resize(lb); job.operands.resize(ob);
    deliver(job, value);
}
// Deliver a finished frame's value to the frame below, or, with none left,
// end the entry (continuations.goose, entry_done).
inline fn deliver(job: Job&, value: Value) { if job.frames.len == 0 { entry_done(job, value); } else { job.operands.push(value); } }
// Whether the innermost handler entry belongs to frame `top` and is `kind`.
fn own(job: const Job&, top: i64, kind: HandlerKind) -> bool {
    return job.handlers.len > 0 && job.handlers[job.handlers.len - 1].frame == top && job.handlers[job.handlers.len - 1].kind == kind;
}
fn pending(kind: HandlerKind) -> bool { return kind != HandlerKind.Catch && kind != HandlerKind.Finally && kind != HandlerKind.Async; }
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
    job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear(); job.root_receiver = Value.Undefined; job.gensym_prefix.clear(); job.gensym_next = 0; job.effect_args = [Value.Undefined; 8]; job.effect_count = 0; job.detail.clear();
    job.effect_frame = Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined };
    job.queue.clear(); job.queue_head = 0; job.draining = false; job.entry = Value.Undefined; job.rejections.clear();
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
    if n.kind < 5 || m.kind < 5 || n.kind == 13 || m.kind == 13 || n.kind == 14 || m.kind == 14 || n.kind >= 20 || m.kind >= 20 { return x == y; }
    // An atom is equal only to itself; two references to one cell are one atom.
    if n.kind >= 16 || m.kind >= 16 { return x == y || n.kind == 18 && m.kind == 18 && n.function == m.function; }
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
        // cljRealized of a delay (light-cljs-808; a lazy sequence stays a
        // boundary here).
        49 => { return delay_realized(heap, a); },
        _ => { heap.fault = Fault.NativeBoundary; return Value.Undefined; },
    }
}
// A bare built-in function object of code `id` (realm.goose BUILTINS), for
// compiler services and tests; the realm and boot_runtime make their own.
fn builtin(heap: Heap&, id: i64) -> Value { return allocate(heap, Node { kind: 4, function: id as u32 }); }
fn install_field(heap: Heap&, object: Value, name: const u8[:], value: Value) -> bool { let k = text(heap, name); return put(heap, object, k, value); }
// The runtime leaves that run no JavaScript, by code (realm.goose
// RUNTIME_LEAVES; call_builtin dispatches the realm's).
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
        // The setter returned: drop its value, deliver the assignment's.
        Setter => { job.operands.pop(); finish_native(job, frame, frame.output); },
        CopyData => { copy_step(job, frame); },
        ArrayCallbacks a => { array_callbacks_step(job, frame, a.which); },
        Swap s => { swap_step(job, frame, s.which); },
        Force x => { force_step(job, frame, x.which); },
        Resume => { resume_step(job, frame); },
        None => {},
    }
    if heap.fault != Fault.None { fail(job, heap.fault, frame); }
}
// Run an accessor's setter for an assignment whose result is `result`
// (Native.Setter): the native frame, then the setter's frame above it with
// the object as receiver and the value as its argument.
fn call_setter(job: Job&, setter: Value, object: Value, value: Value, result: Value) {
    let heap .= job.heap;
    if setter.tag() != 5 || node(heap, setter.id()).kind != 2 { heap.fault = Fault.UnsupportedOperand; return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 1, local_base: job.locals.len, operand_base: job.operands.len, callee: setter, receiver: object, output: result, native: Native.Setter });
    if !enter(job, setter, object, [value]) { job.frames.pop(); }
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
        if frame.callee.tag() == 5 && node(heap, frame.callee.id()).kind == 4 { builtin_callback(job, frame.callee, nil(), arg, frame); }
        else { enter(job, frame.callee, nil(), [arg]); }
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
        if frame.callee.tag() == 5 && node(heap, frame.callee.id()).kind == 4 { builtin_callback(job, frame.callee, Value.Undefined, arg, frame); }
        else { enter(job, frame.callee, Value.Undefined, [arg]); }
    }
}
// A native frame's callback that is a built-in function: gensym in a scope,
// a host effect saved at the frame, or a plain built-in whose result the
// frame's next step pops.
fn builtin_callback(job: Job&, callee: Value, receiver: Value, arg: Value, frame: Frame) {
    let id: i64 = node(job.heap, callee.id()).function; let b = builtin_of(job.heap, callee.id());
    if b == Builtin.Gensym && job.gensym_next > 0 { job.operands.push(gensym(job, [arg])); }
    else if forces(job.heap, b, [arg]) { start_force(job, b, arg, job.operands.len); }
    else if effect_builtin(b) { request_effect(job, id, [arg], frame); }
    else { let v = call_builtin(job, id, receiver, [arg]); job.operands.push(v); }
}
// Pop a finished native frame and deliver its result to the frame below.
fn finish_native(job: Job&, frame: Frame, result: Value) {
    job.frames.pop(); job.operands.resize(frame.operand_base);
    deliver(job, result);
}
// Whether a binary operator's operand is an ordinary object, which converts
// through ToPrimitive before the operator runs.
fn ordinary(heap: const Heap&, v: Value) -> bool { return v.tag() == 5 && (node(heap, v.id()).kind == 0 || node(heap, v.id()).kind == 13 || node(heap, v.id()).kind == 20 || node(heap, v.id()).kind == 21); }
// Start the resumable ToPrimitive (hint default) of a binary operator `op`
// whose operands x and y include an ordinary object (light-cljs-786): a
// native frame converts them, left to right, and then runs the operator. The
// frame below has already consumed the operands.
fn to_primitive(job: Job&, op: i64, x: Value, y: Value) {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: Value.Undefined, input: x, output: y, native: Native.ToPrimitive { op: op as u8, method: 0 } });
}
// The interned key of these bytes, made if there is none. A key of Latin-1
// text is a Latin-1 header (the encoding is canonical), so this compares the
// bytes without building a string.
fn key_named(heap: Heap&, name: const u8[:]) -> Value {
    if !charge(heap, name.len + 1) { return Value.Undefined; }
    var h: u32 = 2166136261; for b in name { h = (h ^ b) * 16777619; }
    if heap.keys.len > 0 {
        let mask = heap.keys.len - 1; var i = h & mask;
        while heap.keys[i].row != no_row() {
            let e = heap.keys[i]; let s = header(heap, e.row);
            if e.hash == h && !s.wide && s.len == name.len && narrow_run(heap, s) == name { return Value.String { s: e.row }; }
            i = (i + 1) & mask;
        }
    }
    let k = text(heap, name);
    return if heap.fault == Fault.None { intern(heap, k) } else { Value.Undefined };
}
// One step of OrdinaryToPrimitive with hint default (number order: valueOf,
// then toString), the Pants engine's Convert native (rust/src/vm/native.rs).
// The operand being converted is index 0 (input) or 1 (output); an operand
// that is not an ordinary object is left as it is for the operator. A
// callable method, own or inherited ([[Get]], light-cljs-787), is called with
// the object as receiver; a primitive result replaces the operand, an object
// result tries the next method, and a missing or non-callable one is
// skipped. A method behind an accessor comes from its getter, run with the
// object as receiver (light-cljs-805): pc 2 awaits its result, the method. A
// method the realm's pending %Object.prototype% would supply fails
// UnsupportedOperand (on_intrinsic), as does a built-in method or getter; an
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
    var m = Value.Undefined;
    if frame.pc == 2 { m = job.operands.pop(); job.frames[at].pc = 0; }
    else {
        let k = key_named(heap, if method == 0 { "valueOf" } else { "toString" });
        if heap.fault != Fault.None { return; }
        let value, getter = lookup(heap, v, k);
        if heap.fault != Fault.None { return; }
        if getter.tag() == 5 {
            if node(heap, getter.id()).kind != 2 { heap.fault = Fault.UnsupportedOperand; return; }
            job.frames[at].pc = 2; enter(job, getter, v, []); return;
        }
        m = value;
    }
    // A plain built-in method (Object.prototype.valueOf and toString,
    // Error.prototype.toString) runs here; its result is the next step's.
    if m.tag() == 5 && node(heap, m.id()).kind == 4 {
        let b = builtin_of(heap, m.id());
        if effect_builtin(b) || b == Builtin.Mapv { heap.fault = Fault.UnsupportedOperand; return; }
        let r = call_builtin(job, node(heap, m.id()).function, v, []);
        if heap.fault == Fault.None { job.frames[at].pc = 1; job.operands.push(r); }
        return;
    }
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
    var used: i64 = 0;
    while job.status == tiny::Status.Ready && !job.effect_pending && job.frames.len > 0 && job.frames[job.frames.len - 1].function >= 0 && (allowance < 0 || used < allowance) {
        // Instructions only grow while a job runs: a function loaded from a
        // cell appends its code (cells.goose decode_callable), which a frame
        // entered after the load reaches through this view.
        let program = code.ins[..];
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
                    else if object_hit(job, object, i) { job.operands.pop(); job.operands.pop(); job.operands.push(slot(heap, node(heap, object.id()).start + i)); }
                    else {
                        settle(job, ran, used); job.frames[top].pc = pc; job.operands.pop();
                        if i >= 0 && dense(job, object) { job.operands.pop(); let e, found = own_element(heap, object, i); job.operands.push(e); }
                        else { let k = key(heap, v); get_property(job, k); }
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
                },
                SetNamed s => {
                    settle(job, ran, used); job.frames[top].pc = pc; let value = job.operands.pop(); set_property(job, value, heap.constants[f.constants + s.k], false, s.strict || f.strict);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                DefineNamed d => {
                    settle(job, ran, used); job.frames[top].pc = pc; let value = job.operands.pop(); define_field(job, heap.constants[f.constants + d.k], value, d.half);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                SetIndexed s => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let v = job.operands.pop(); let object = job.operands[job.operands.len - 1]; let i = number_index(v);
                    if i >= 0 && dense(job, object) && writable(job, object.id()) { job.operands.pop(); if valid(heap, value) { set_element(heap, object, i, value); } job.operands.push(value); }
                    else if object_hit(job, object, i) && writable(job, object.id()) { job.operands.pop(); if valid(heap, value) { set_slot(heap, node(heap, object.id()).start + i, value); } job.operands.push(value); }
                    else { let k = key(heap, v); set_property(job, value, k, false, s.strict || f.strict); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                DefineIndexed d => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let v = job.operands.pop(); let k = key(heap, v);
                    if heap.fault == Fault.None { define_field(job, k, value, d.half); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                SetProto => {
                    settle(job, ran, used); job.frames[top].pc = pc; set_proto(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Delete d => {
                    settle(job, ran, used); job.frames[top].pc = pc; delete_op(job, d.strict || f.strict);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                In => {
                    settle(job, ran, used); job.frames[top].pc = pc; in_op(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Append => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let object = job.operands[job.operands.len - 1];
                    if dense(job, object) && writable(job, object.id()) { let end = node(heap, object.id()).length; if valid(heap, value) { set_element(heap, object, end, value); } }
                    else {
                        let k = if valid(heap, object) && object.tag() == 5 && node(heap, object.id()).kind == 1 { key(heap, number(node(heap, object.id()).length)) } else { heap.fault = Fault.TypeError; Value.Undefined };
                        set_property(job, value, k, true, true);
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
                    var v, found = own_property(heap, heap.global, k);
                    // A compiler job's global names are the V8 build's; one
                    // the image lacks is a boundary, never a ReferenceError. A
                    // name the global object inherits (Object.prototype's) is
                    // found there; a pending one fails by name.
                    if !found && heap.fault == Fault.None {
                        if job.protected_nodes > 0 { heap.fault = Fault.NativeBoundary; }
                        else if has_property(heap, heap.global, intern(heap, k)) { v = get(heap, heap.global, k); }
                        else if g.throws && heap.fault == Fault.None { not_defined(job, k); }
                    }
                    job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                TypeOf => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let v = job.operands.pop(); job.operands.push(type_name(heap, v));
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
                ForInStart => {
                    settle(job, ran, used); job.frames[top].pc = pc; for_in_start(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                ForInNext => {
                    settle(job, ran, used); job.frames[top].pc = pc; for_in_next(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                OwnKeys => {
                    settle(job, ran, used); job.frames[top].pc = pc; own_keys_op(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                IsEnumerable => {
                    settle(job, ran, used); job.frames[top].pc = pc; is_enumerable(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // A CopyData native frame copies the keys (or nothing is
                // pushed and the frame continues in place).
                CopyData => {
                    settle(job, ran, used); job.frames[top].pc = pc; copy_data(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                ToPropertyKey => {
                    settle(job, ran, used); job.frames[top].pc = pc; to_property_key(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                RequireCoercible => { if job.operands[job.operands.len - 1].tag() <= 1 { heap.fault = Fault.TypeError; break; } },
                // Generators and async functions (continuations.goose): each
                // detaches, ends or settles the frame, so the loop leaves it.
                GeneratorStart => {},
                AsyncStart => { settle(job, ran, used); job.frames[top].pc = pc; async_start(job, top); if heap.fault != Fault.None { break; } left = fuel(job, allowance, used); },
                GeneratorInitialYield => { settle(job, ran, used); job.frames[top].pc = pc; initial_yield(job, top, at); break; },
                GeneratorYield => { settle(job, ran, used); job.frames[top].pc = pc; generator_yield(job, top, at); break; },
                Await => { settle(job, ran, used); job.frames[top].pc = pc; await_value(job, top, at); break; },
                Unsupported => { heap.fault = Fault.Unsupported; break; },
                Construct c => {
                    if !constructible(heap, job.operands[job.operands.len - c.argc - 1]) { heap.fault = Fault.Unsupported; break; }
                    settle(job, ran, used);
                    if !invoke(job, top, at, c.argc, false) { break; }
                    left = fuel(job, allowance, used);
                },
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
        } else if forces(heap, BUILTINS[id].which, args) { let d = args[0]; start_force(job, BUILTINS[id].which, d, base);
        } else if native_builtin(BUILTINS[id].which) { native_start(job, BUILTINS[id].which, receiver, args, base);
        } else if id == 5 && job.gensym_next > 0 { let value = gensym(job, args); job.operands.resize(base); job.operands.push(value); }
        else if id >= 5 && id <= 8 { request_effect(job, id, args, frame_at(job, top, at)); job.operands.resize(base); }
        else { let value = call_builtin(job, id, receiver, args); job.operands.resize(base); job.operands.push(value); }
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
    } else if valid(heap, object) && object.tag() == 5 && (node(heap, object.id()).kind == 4 || node(heap, object.id()).kind == 2 && node(heap, object.id()).key_count == 0 && !node(heap, object.id()).initialized) && key_is(heap, k, "call") {
        job.operands.push(get(heap, object, k));
    } else if object.tag() == 5 && !realm_intrinsic(object.id()) && object.id() < job.protected_nodes {
        let value, found = own_property(heap, object, k);
        let optional = object == heap.global && (key_is(heap, k, "__lightCompilerRootReceiver") || key_is(heap, k, "__lightCompilerRootContext"));
        if !found && !optional { heap.fault = Fault.NativeBoundary; } job.operands.push(value);
    } else {
        // An accessor's getter runs as a frame entered directly: its return
        // value is the read's result for this frame (light-cljs-787).
        prepare_function(job, object, k, false);
        let value, getter = read(heap, object, k);
        if heap.fault != Fault.None || getter.tag() != 5 { job.operands.push(value); }
        else if node(heap, getter.id()).kind != 2 { heap.fault = Fault.UnsupportedOperand; job.operands.push(value); }
        else { enter(job, getter, object, []); }
    }
}
// A ReferenceError for global name k: "k is not defined".
fn not_defined(job: Job&, k: Value) {
    job.detail.clear(); append_utf16(job.heap, k, job.detail);
    for c in " is not defined" { job.detail.push(c); }
    job.heap.fault = Fault.ReferenceError;
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
// Before a job's operation on key k of `object` (light-cljs-805,
// light-cljs-806): a lazy closure's properties (lazy_function) are made
// (materialize_function) on a read of `prototype` from an ordinary function
// with a lazy one (lazy_prototype), on the function or inherited from it
// (the first object of the chain that has the key or is such a function),
// or, `adding`, before an assignment, definition or delete of any key on a
// lazy closure, so that `length`, `name` and `prototype` come first in
// creation order (the Pants engine's function.rs). Reads of `length` and
// `name`, `in`, and a delete of a lazy `prototype` answer without making
// them (own_entry, has_property, delete_property). Only a function the job
// allocated is changed: making another's would write a row the reset does
// not restore, so a read fails UnsupportedOperand (a read has no JavaScript
// failure for it), as the Pants engine does for a published function; a
// write has already failed NativeBoundary. Goose callers' reads meet a lazy
// prototype in function_key and their changes to a lazy closure in
// lazy_change, which fail by name.
fn prepare_function(job: Job&, object: Value, k: Value, adding: bool) {
    let heap .= job.heap;
    if heap.fault != Fault.None || object.tag() != 5 || object.id() >= heap.nodes.len { return; }
    var at = object;
    if adding { if !lazy_function(heap, at.id()) { return; } }
    else {
        if !key_is(heap, k, "prototype") { return; }
        let key = intern(heap, k);
        loop {
            if heap.fault != Fault.None { return; }
            if lazy_prototype(heap, at.id()) { break; }
            if own_entry(heap, at, key).found || heap.fault != Fault.None { return; }
            let proto = prototype(heap, at.id());
            if proto.tag() != 5 || !charge(heap) { return; }
            at = proto;
        }
    }
    if !writable(job, at.id()) { heap.fault = Fault.UnsupportedOperand; return; }
    materialize_function(heap, at.id());
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
// An unprotected ordinary object other than the compiler roots object with
// an element i (light-cljs-804): element opcodes read and write that element
// directly, without building its key string.
inline fn object_hit(job: const Job&, object: Value, i: i64) -> bool {
    let heap .= job.heap;
    if i < 0 || object.tag() != 5 || object.id() >= heap.nodes.len || object.id() < job.protected_nodes || job.root_receiver.tag() == 5 && object == job.root_receiver { return false; }
    let n .= node(heap, object.id());
    return n.kind == 0 && i < n.count && slot(heap, n.start + i).tag() != -1;
}
// [[Set]] from an assignment (light-cljs-787): chain, the expression yields
// the object (88), else the value. A setter runs through a Setter native
// frame; a refused assignment is a TypeError in strict code and does nothing
// in sloppy code.
fn set_property(job: Job&, value: Value, k: Value, chain: bool, strict: bool) {
    let heap .= job.heap;
    let object = job.operands.pop();
    let result = if chain { object } else { value };
    if object.tag() == 5 && !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; job.operands.push(result); return; }
    prepare_function(job, object, k, true);
    let setter, ok = write(heap, object, k, value);
    if heap.fault == Fault.None && setter.tag() == 5 { call_setter(job, setter, object, value, result); return; }
    if heap.fault == Fault.None { if ok { record(job, object, k, value); } else if strict { heap.fault = Fault.TypeError; } }
    job.operands.push(result);
}
// DefineField, DefineGetter and DefineSetter (81, 103, 104; key k) and
// DefineElem, DefineGetterElem and DefineSetterElem (130, 133, 134; k the
// converted key): obj v → obj, [[DefineOwnProperty]] of an object literal's
// own property, a data property (half 0) or an accessor's getter (1) or
// setter (2), merged with the other half (light-cljs-787). A refused
// definition is a TypeError (CreateDataPropertyOrThrow). Data definitions
// enter the write log as assignments did.
fn define_field(job: Job&, k: Value, value: Value, half: u8) {
    let heap .= job.heap;
    let object = job.operands[job.operands.len - 1];
    if !valid(heap, object) || !key_valid(heap, k) || !valid(heap, value) { return; }
    if object.tag() != 5 || k.tag() != 4 && k.tag() != 6 { heap.fault = if object.tag() == 5 { Fault.UnsupportedOperand } else { Fault.TypeError }; return; }
    if !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; return; }
    prepare_function(job, object, k, true);
    let key = intern(heap, k);
    if heap.fault != Fault.None { return; }
    let ok = define_property(heap, object, key, value, if half == 0 { attrs_data() } else { attrs_accessor() }, half);
    if heap.fault != Fault.None { return; }
    if !ok { heap.fault = Fault.TypeError; } else if half == 0 { record(job, object, k, value); }
}
// SetProto (95): obj v → obj. An object or null v becomes the literal's
// prototype ([[SetPrototypeOf]]; a cycle is a TypeError); any other v is
// ignored.
fn set_proto(job: Job&) {
    let heap .= job.heap;
    let v = job.operands.pop(); let object = job.operands[job.operands.len - 1];
    if !valid(heap, object) || !valid(heap, v) || v.tag() != 5 && v.tag() != 1 { return; }
    if object.tag() != 5 || node(heap, object.id()).kind != 0 { heap.fault = Fault.UnsupportedOperand; return; }
    if !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; return; }
    if !set_prototype(heap, object.id(), v) && heap.fault == Fault.None { heap.fault = Fault.TypeError; }
}
// Delete (97) and DeleteStrict (153): o k → b, [[Delete]] of the own
// property (delete_property). Undefined or null o is a TypeError; a string's
// `length` and indices below it are non-configurable, and other primitives
// have no own properties. A non-configurable property stays: false in
// sloppy code, a TypeError in strict code.
fn delete_op(job: Job&, strict: bool) {
    let heap .= job.heap;
    let kv = job.operands.pop(); let object = job.operands.pop();
    if !valid(heap, object) || !key_valid(heap, kv) { return; }
    if object.tag() <= 1 { heap.fault = Fault.TypeError; return; }
    let k = key(heap, kv);
    if heap.fault != Fault.None { return; }
    var ok = true;
    if object.tag() == 4 { let i = index(heap, k); ok = !(key_is(heap, k, "length") || i >= 0 && i < str_len(heap, object)); }
    else if object.tag() == 5 {
        if !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; return; }
        let key = intern(heap, k);
        if heap.fault != Fault.None { return; }
        // A lazy closure's `length` or `name` is made before it goes.
        if key.tag() == 4 && (key_is(heap, key, "length") || key_is(heap, key, "name")) { prepare_function(job, object, key, true); }
        ok = delete_property(heap, object, key);
    }
    if heap.fault != Fault.None { return; }
    if !ok && strict { heap.fault = Fault.TypeError; return; }
    job.operands.push(boolean(ok));
}
// In (98): k o → bool, `k in o` ([[HasProperty]]). A non-object o is a
// TypeError; an object k needs ToPrimitive, not implemented here
// (UnsupportedOperand).
fn in_op(job: Job&) {
    let heap .= job.heap;
    let object = job.operands.pop(); let kv = job.operands.pop();
    if !valid(heap, object) || !key_valid(heap, kv) { return; }
    if object.tag() != 5 { heap.fault = Fault.TypeError; return; }
    let k = key(heap, kv);
    if heap.fault != Fault.None { return; }
    let key = intern(heap, k);
    if heap.fault != Fault.None { return; }
    let found = has_property(heap, object, key);
    if heap.fault == Fault.None { job.operands.push(boolean(found)); }
}

// ---- Key enumeration (light-cljs-804; the Pants engine's vm/keys.rs and
// vm/native/copy_properties.rs, QuickJS's build_for_in_iterator and
// js_for_in_next).
//
// A key list is a node of kind 16 a job allocates and only its own frames
// hold (a for-in loop's scratch local, a CopyData native frame's callee), so
// JavaScript never reaches it and the reset truncates it with the job's rows.
// Its records heap.properties[keys..keys + key_count), in a run of
// key_capacity rows, are own keys with the attribute bits they had when
// listed (value Undefined). `value` is the object whose keys the walk is on
// (Undefined: a primitive's, or none); `start` its first record, the records
// before it being the keys of the objects a for-in walk has left; `length`
// the next record to visit.
//
// Enumerating an object the job may not read whole fails NativeBoundary: the
// compiler roots object and protected rows answer missing names by demand,
// so their keys are not a list.
fn listable(job: Job&, object: Value) -> bool {
    if !realm_intrinsic(object.id()) && object.id() < job.protected_nodes || job.root_receiver.tag() == 5 && object == job.root_receiver { job.heap.fault = Fault.NativeBoundary; return false; }
    return true;
}
// A primitive string's own keys: its indices, enumerable (and, with
// `length`, its non-enumerable `length`).
fn string_keys(heap: Heap&, s: Value, length: bool, out: OwnKey[>..]&) -> bool {
    let n = str_len(heap, s);
    if !charge(heap, n) { return false; }
    for i in n { let k = index_key(heap, i); if heap.fault != Fault.None { return false; } out.push(OwnKey { key: k, attrs: attrs_data(), index: i }); }
    if length { let k = key_named(heap, "length"); if heap.fault != Fault.None { return false; } out.push(OwnKey { key: k, attrs: 0 }); }
    return true;
}
// Append the string keys (or, with `symbols`, every key) of `keys` to key
// list o's records; a full run moves to a larger one at the table's end, as
// dict_append's does. A rejection changes nothing.
fn list_append(heap: Heap&, o: i64, keys: const OwnKey[:], symbols: bool) -> bool {
    var count: i64 = 0; for e in keys { if symbols || e.key.tag() == 4 { count += 1; } }
    let used: i64 = node(heap, o).key_count;
    if used + count > node(heap, o).key_capacity {
        let start = node(heap, o).keys; let capacity = max(4, 2 * (used + count));
        let at = add_properties(heap, capacity);
        if at < 0 { return false; }
        for i in used { set_property_row(heap, at + i, property_row(heap, start + i)); set_property_row(heap, start + i, Property {}); }
        node_mut(heap, o).keys = at as u32; node_mut(heap, o).key_capacity = capacity as u32;
    }
    var i = used;
    for e in keys { if symbols || e.key.tag() == 4 { set_property_row(heap, node(heap, o).keys + i, Property { key: e.key, attrs: e.attrs }); i += 1; } }
    node_mut(heap, o).key_count = i as u32;
    return true;
}
fn key_list(heap: Heap&, object: Value, keys: const OwnKey[:], symbols: bool) -> Value {
    let list = allocate(heap, Node { kind: 16, value: object });
    if list.tag() != 5 || !list_append(heap, list.id(), keys, symbols) { return Value.Undefined; }
    return list;
}
// ForInStart (99): v → it, a key list of v's own string keys, enumerable or
// not, in own_keys order (a non-enumerable key is never visited but hides the
// name further up the chain); a string's indices; none for any other
// primitive.
fn for_in_start(job: Job&) {
    let heap .= job.heap;
    let v = job.operands.pop();
    if !valid(heap, v) { return; }
    var keys: OwnKey[>..] = []; var walked = Value.Undefined;
    if v.tag() == 5 { if !listable(job, v) || !own_property_keys(heap, v, keys) { return; } walked = v; }
    else if v.tag() == 4 && !string_keys(heap, v, false, keys) { return; }
    let list = key_list(heap, walked, keys, false);
    if heap.fault == Fault.None { job.operands.push(list); }
}
// Whether key k is among the first `start` records of key list o: the names
// of the objects a for-in walk has left. One work unit a record.
fn hidden(heap: Heap&, o: i64, k: Value) -> bool {
    let start: i64 = node(heap, o).start;
    if !charge(heap, start) { return false; }
    let w = key_word(k); let keys: i64 = node(heap, o).keys;
    for i in start { if key_word(property_row(heap, keys + i).key) == w { return true; } }
    return false;
}
// ForInNext (100): it → key more. The next record that was enumerable when
// listed, is not hidden by an object the walk has left, and is still an own
// enumerable property of the object it was listed from (a key deleted or
// made non-enumerable before it is reached is skipped; keys added after the
// listing are not visited), and true; or undefined and false when the walk
// ends. When an object's keys are used up the walk lists its prototype's,
// read then, so a prototype changed before the walk reaches it counts. The
// walk ends at Null and at the realm's pending %Object.prototype% (Empty),
// which has no enumerable property (light-cljs-788 builds it). One work unit
// a record visited.
fn for_in_next(job: Job&) {
    let heap .= job.heap;
    let it = job.operands.pop();
    if !valid(heap, it) { return; }
    if it.tag() != 5 || node(heap, it.id()).kind != 16 { heap.fault = Fault.InvalidState; return; }
    if !writable(job, it.id()) { heap.fault = Fault.NativeBoundary; return; }
    let o: i64 = it.id();
    loop {
        if !charge(heap) { return; }
        let next: i64 = node(heap, o).length;
        let walked = node(heap, o).value;
        if next < node(heap, o).key_count {
            let r = property_row(heap, node(heap, o).keys + next);
            node_mut(heap, o).length = (next + 1) as u32;
            if r.attrs & attr_enumerable() == 0 || hidden(heap, o, r.key) { if heap.fault != Fault.None { return; } continue; }
            if walked.tag() == 5 {
                let e = enumerable_entry(heap, walked, r.key);
                if heap.fault != Fault.None { return; }
                if !e.found || e.attrs & attr_enumerable() == 0 { continue; }
            }
            job.operands.push(r.key); job.operands.push(boolean(true)); return;
        }
        let proto = if walked.tag() == 5 { prototype(heap, walked.id()) } else { Value.Undefined };
        if proto.tag() != 5 {
            node_mut(heap, o).value = Value.Undefined;
            job.operands.push(Value.Undefined); job.operands.push(boolean(false)); return;
        }
        var keys: OwnKey[>..] = [];
        if !listable(job, proto) || !own_property_keys(heap, proto, keys) { return; }
        let used: i64 = node(heap, o).key_count;
        if !list_append(heap, o, keys, false) { return; }
        node_mut(heap, o).value = proto; node_mut(heap, o).start = used as u32; node_mut(heap, o).length = used as u32;
    }
}
// OwnKeys (128): source → keys, a new array of the source's own keys,
// symbols included, before object rest copies them: an index as a number
// (as ToPropertyKey gives it), a name as a string. A string's are its indices
// and `length`; undefined and null are a TypeError; other primitives have
// none. Symbols have no values yet, so a symbol key fails UnsupportedOperand.
fn own_keys_op(job: Job&) {
    let heap .= job.heap;
    let source = job.operands.pop();
    if !valid(heap, source) { return; }
    if source.tag() <= 1 { heap.fault = Fault.TypeError; return; }
    var keys: OwnKey[>..] = [];
    if source.tag() == 5 { if !listable(job, source) || !own_property_keys(heap, source, keys) { return; } }
    else if source.tag() == 4 && !string_keys(heap, source, true, keys) { return; }
    let array = allocate(heap, Node { kind: 1 });
    if array.tag() != 5 { return; }
    for e, i in keys {
        if e.key.tag() != 4 { heap.fault = Fault.UnsupportedOperand; return; }
        if !set_element(heap, array, i, if e.index >= 0 { number(e.index) } else { e.key }) { return; }
    }
    job.operands.push(array);
}
// IsEnumerable (129): source key → whether the source has an own enumerable
// property `key` (already converted by ToPropertyKey): its current
// attributes, not a snapshot's. A string's indices are enumerable; other
// primitives have no own properties.
fn is_enumerable(job: Job&) {
    let heap .= job.heap;
    let kv = job.operands.pop(); let source = job.operands.pop();
    if !valid(heap, source) || !key_valid(heap, kv) { return; }
    var result = false;
    if source.tag() == 5 || source.tag() == 4 {
        if source.tag() == 5 && !listable(job, source) { return; }
        let k = key(heap, kv);
        if heap.fault != Fault.None { return; }
        if source.tag() == 4 { let i = index(heap, k); result = i >= 0 && i < str_len(heap, source); }
        else {
            let ik = intern(heap, k);
            if heap.fault != Fault.None { return; }
            let e = enumerable_entry(heap, source, ik);
            result = e.found && e.attrs & attr_enumerable() != 0;
        }
    }
    if heap.fault == Fault.None { job.operands.push(boolean(result)); }
}
// CopyDataProperties (180): target source → target, an object spread. An
// undefined or null source, or one with no own keys, copies nothing; a
// CopyData native frame copies the rest, one key a step: each key still an
// own enumerable property of the source when reached is read ([[Get]], a
// getter running as a frame above) and defined on the target
// (CreateDataProperty).
fn copy_data(job: Job&) {
    let heap .= job.heap;
    let source = job.operands.pop(); let target = job.operands[job.operands.len - 1];
    if !valid(heap, source) || !valid(heap, target) || source.tag() <= 1 { return; }
    if target.tag() != 5 || node(heap, target.id()).kind != 0 { heap.fault = Fault.UnsupportedOperand; return; }
    if !writable(job, target.id()) { heap.fault = Fault.NativeBoundary; return; }
    var keys: OwnKey[>..] = []; var walked = Value.Undefined;
    if source.tag() == 5 { if !listable(job, source) || !own_property_keys(heap, source, keys) { return; } walked = source; }
    else if source.tag() == 4 && !string_keys(heap, source, false, keys) { return; }
    if keys.len == 0 { return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    let list = key_list(heap, walked, keys, true);
    if heap.fault != Fault.None { return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len - 1, callee: list, receiver: Value.Undefined, input: source, output: target, native: Native.CopyData });
}
// Define a copied key on the spread's target; a refusal is a TypeError
// (CreateDataPropertyOrThrow). Data definitions enter the write log as
// define_field's do.
fn copy_define(job: Job&, target: Value, k: Value, value: Value) {
    if !define_property(job.heap, target, k, value, attrs_data()) { if job.heap.fault == Fault.None { job.heap.fault = Fault.TypeError; } return; }
    record(job, target, k, value);
}
fn copy_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1; let list: i64 = frame.callee.id();
    if frame.pc == 1 {
        let value = job.operands.pop(); job.frames[at].pc = 0;
        copy_define(job, frame.output, property_row(heap, node(heap, list).keys + frame.index - 1).key, value);
        return;
    }
    if frame.index >= node(heap, list).key_count { finish_native(job, frame, frame.output); return; }
    let k = property_row(heap, node(heap, list).keys + frame.index).key;
    job.frames[at].index += 1;
    if frame.input.tag() == 4 { let i = index(heap, k); if heap.fault == Fault.None { copy_define(job, frame.output, k, unit_string(heap, frame.input, i)); } return; }
    let e = enumerable_entry(heap, frame.input, k);
    if heap.fault != Fault.None || !e.found || e.attrs & attr_enumerable() == 0 { return; }
    if !is_accessor(e.value) { copy_define(job, frame.output, k, e.value); return; }
    let getter = pair(heap, e.value).get;
    if getter.tag() != 5 { copy_define(job, frame.output, k, Value.Undefined); return; }
    if node(heap, getter.id()).kind != 2 { heap.fault = Fault.UnsupportedOperand; return; }
    job.frames[at].pc = 1; enter(job, getter, frame.input, []);
}
// ToPropertyKey (93): o k → o k', the key converted once for a read and a
// later write of o[k] or an object pattern's key: an array index as a
// number, any other name as a string. Undefined or null o is a TypeError; a
// key needing ToPrimitive (an object) or number-to-string conversion fails
// UnsupportedOperand, as key() does.
fn to_property_key(job: Job&) {
    let heap .= job.heap;
    let kv = job.operands.pop(); let base = job.operands[job.operands.len - 1];
    if !valid(heap, base) || !key_valid(heap, kv) { return; }
    if base.tag() <= 1 { heap.fault = Fault.TypeError; return; }
    let k = key(heap, kv);
    if heap.fault != Fault.None { return; }
    let i = index(heap, k);
    if heap.fault == Fault.None { job.operands.push(if i >= 0 { number(i) } else { k }); }
}
fn claim(job: Job&) -> bool { if job.claimed || job.effect_pending || job.status != tiny::Status.Ready { return false; } job.claimed = true; return true; }
fn run_claimed(job: Job&, budget: i64) -> tiny::Outcome {
    if !job.claimed { return tiny::Outcome.Busy; }
    defer { job.claimed = false; }
    var used: i64 = 0;
    while job.status == tiny::Status.Ready && !job.effect_pending && (budget < 0 || used < budget) {
        // No frame left once the entry finished: the next promise job runs
        // (continuations.goose).
        if job.frames.len == 0 && job.draining { drain_step(job); used += 1; }
        else if job.frames.len == 0 || job.frames[job.frames.len - 1].function < 0 { native_step(job); used += 1; }
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
        6 => { out.append("symbol"); },
        _ => { out.append("invalid:tag"); },
    }
}
