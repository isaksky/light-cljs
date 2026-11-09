import std;
import binary;
import math;
import .program;
import .vm;
import .export_schema;
import .realm;
import .array_callbacks;
import .array_methods;
import .iterators;
import .proxy;
import .cells;
import .continuations;
import .number_text;
import .reclaim;
import .function_calls;
import .runtime_compile;
import .templates;
import .descriptors;
import .integrity;
import .prototypes;
import .object_statics;
import .symbols;

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
// relocation. Symbol keeps a well-known symbol index below symbol_floor()
// or, at and above it, a symbol row (node_kind::JsSymbol, light-cljs-885);
// BigInt keeps no payload (constants only: `Const` fails Unsupported on
// it). Empty marks an array
// hole; it never leaves an array run (`valid` refuses it elsewhere). As an
// object header's prototype, Empty stands for the realm's %Object.prototype%,
// which does not exist yet (light-cljs-787; see Node). Accessor is internal
// too: an accessor property's slot, naming its row of `heap.accessors`; it
// never leaves a property slot or record.
// Test a value's variant with the is_* predicates below. tag() orders the
// variants for range tests (`v.tag() <= value_tag::Null` is undefined, null
// or internal); it reuses the GSP constant tags (constant::) and adds the
// Goose-private Object, Empty and Accessor.
enum Value { Undefined, Null, Bool { v: bool }, Number { v: f64 }, String { s: u32 }, Symbol { s: u32 }, BigInt { b: u32 }, Object { o: u32 }, Empty, Accessor { a: u32 } }
// A string header. `wide` runs are UTF-16 units; a heap header that is not
// wide is a Latin-1 run in heap.latin1, and a heap string is wide only when
// some unit is above 255. Code and wire strings (Code, bridge graphs,
// artifacts) are always wide. `key` marks the interned property key of its
// text (light-cljs-787, intern): it is set only on a header row interning
// created, never on an existing row, so image rows never change.
struct Str { start: u32, len: u32, wide: bool = true, key: bool = false }
let value_tag::Accessor = -2;
let value_tag::Empty = -1;
let value_tag::Undefined = constant::Undefined;
let value_tag::Null = constant::Null;
let value_tag::Bool = constant::Bool;
let value_tag::Number = constant::Number;
let value_tag::String = constant::String;
let value_tag::Object = 5;
let value_tag::Symbol = constant::Symbol;
let value_tag::BigInt = constant::BigInt;
inline fn tag(v: Value) -> i64 { return match v { Undefined => value_tag::Undefined, Null => value_tag::Null, Bool => value_tag::Bool, Number => value_tag::Number, String => value_tag::String, Object => value_tag::Object, Symbol => value_tag::Symbol, BigInt => value_tag::BigInt, Empty => value_tag::Empty, Accessor => value_tag::Accessor }; }
inline fn is_undefined(v: Value) -> bool { return match v { Undefined => true, _ => false }; }
inline fn is_null(v: Value) -> bool { return match v { Null => true, _ => false }; }
inline fn is_bool(v: Value) -> bool { return match v { Bool => true, _ => false }; }
inline fn is_number(v: Value) -> bool { return match v { Number => true, _ => false }; }
inline fn is_string(v: Value) -> bool { return match v { String => true, _ => false }; }
inline fn is_object(v: Value) -> bool { return match v { Object => true, _ => false }; }
inline fn is_symbol(v: Value) -> bool { return match v { Symbol => true, _ => false }; }
inline fn is_bigint(v: Value) -> bool { return match v { BigInt => true, _ => false }; }
// An array hole (Empty).
inline fn is_hole(v: Value) -> bool { return match v { Empty => true, _ => false }; }
inline fn is_accessor(v: Value) -> bool { return match v { Accessor => true, _ => false }; }
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
// Symbol values (light-cljs-885; docs/javascript-coverage.md, decision 2):
// a Symbol payload below symbol_floor() is a well-known symbol's index
// (export_well_known_symbol, the realm's Symbol properties, the same in
// every program); one at or above it is symbol_floor() plus the node row
// of a symbol a job made (node_kind::JsSymbol). A symbol row is a plain
// u32 row like an object's: truncated with the job, checked by snapshots
// (snapshot::value), moved by in-job reclamation (reclaim.goose relink)
// and carried by saved continuations, which keep every row they reach.
inline fn symbol_floor() -> i64 { return 64; }
// The symbol row of v, or -1 (not a symbol, or a well-known one).
inline fn symbol_node(v: Value) -> i64 { return match v { Symbol x => if x.s >= symbol_floor() { x.s - symbol_floor() } else { -1 }, _ => -1 }; }
// The symbol whose row is `row`.
inline fn symbol_at(row: i64) -> Value { return Value.Symbol { s: (row + symbol_floor()) as u32 }; }
// Whether Symbol payload s names a symbol in `nodes`: a well-known index,
// or a symbol row.
fn symbol_payload(nodes: const Node[:], s: i64) -> bool {
    if s < symbol_floor() { return export_well_known_symbol(s).len > 0; }
    let r = s - symbol_floor();
    return r < nodes.len && nodes[r].kind == node_kind::JsSymbol;
}
// Whether v is a symbol row Symbol.for registered.
fn registered_symbol(heap: const Heap&, v: Value) -> bool { let r = symbol_node(v); return r >= 0 && r < heap.nodes.len && node(heap, r).initialized; }
// A symbol's description as text: a well-known symbol's `Symbol.name`, a
// symbol row's description string; false, with nothing appended, for a
// symbol whose description is undefined.
fn append_description(heap: const Heap&, v: Value, out: u16[>..]&) -> bool {
    let r = symbol_node(v);
    if r < 0 { for c in "Symbol." { out.push(c); } for c in export_well_known_symbol(symbol_index(v)) { out.push(c); } return true; }
    let d = node(heap, r).value;
    if !d.is_string() { return false; }
    append_utf16(heap, d, out); return true;
}
// SymbolDescriptiveString(v): "Symbol(" description ")".
fn append_descriptive(heap: const Heap&, v: Value, out: u16[>..]&) { for c in "Symbol(" { out.push(c); } append_description(heap, v, out); out.push(41); }
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
fn str_len(heap: const Heap&, v: Value) -> i64 { return if !v.is_string() { 0 } else { header(heap, v.sid()).len }; }
// The run of a wide (narrow) string; a header naming a run past its table
// fails by that table's name.
fn wide_run(heap: const Heap&, s: Str) -> const u16[:] {
    if !s.wide || s.start > heap.units.len || s.len > heap.units.len - s.start { abort("run is not in table units"); }
    return heap.units[s.start..(s.start + s.len)];
}
fn narrow_run(heap: const Heap&, s: Str) -> const u8[:] {
    if s.wide || s.start > latin1_len(heap) || s.len > latin1_len(heap) - s.start { abort("run is not in table latin1"); }
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
        Number n => Plain { tag: value_tag::Number, number: n.v }, Bool b => Plain { tag: value_tag::Bool, payload: if b.v { 1 } else { 0 } },
        String s => Plain { tag: value_tag::String, payload: s.s }, Object o => Plain { tag: value_tag::Object, payload: o.o },
        Symbol s => Plain { tag: value_tag::Symbol, payload: s.s }, BigInt b => Plain { tag: value_tag::BigInt, payload: b.b }, Accessor a => Plain { tag: value_tag::Accessor, payload: a.a }, _ => Plain { tag: v.tag() },
    };
}
fn plain_bytes(v: Value, out: u8[>..]&) { out.append(bytes_of([plain(v)])); }
// Bit-exact value identity: NaN equals itself, -0 differs from +0.
fn same_bits(a: Value, b: Value) -> bool { return bytes_of([plain(a)]) == bytes_of([plain(b)]); }
fn plain_bytes(n: Node, out: u8[>..]&) {
    out.append(bytes_of([n.kind, n.function, n.start, n.count, n.keys, n.key_count, n.key_capacity, n.length, n.origin, n.source, n.span_start, n.span_end, n.shape])); out.append(bytes_of([n.initialized, n.immutable, n.non_extensible]));
    plain_bytes(n.value, out); plain_bytes(n.namespace_value, out); plain_bytes(n.metadata, out); plain_bytes(n.proto, out);
    for v in n.slots { plain_bytes(v, out); }
}
fn native_code(n: Native) -> i64 { return match n { None => 0, Mapv => 1, MapSeq => 2, Setter => 4, CopyData => 5, Convert c => 3 + (match c.hint { Default => 0, Number => 1, String => 2 }) * 8 + (if c.finish { 4 } else { 0 }) + (c.count as i64) * 32 + (c.method as i64) * 1024, ArrayCallbacks a => -1 - (code(a.which) as i64), Swap s => -1001 - (code(s.which) as i64), Force x => -2001 - (code(x.which) as i64), Resume r => 3001 + (r.kind as i64), HasInstance => 6, ArrayMethod m => -3001 - (code(m.which) as i64), GetIterator => 7, BuiltinIterator m => -4001 - (code(m.which) as i64), ForIn => 8, Proxy p => 5001 + (p.method as i64), Forward w => 6001 + (if w.construct { 1 } else { 0 }), Bind => 7001, FunctionText => 8001, DynamicFunction => 9001, ErrorConstruct e => -5001 - (code(e.which) as i64), ErrorText => -6001, Descriptor d => -7001 - (code(d.which) as i64), Statics s => -8001 - (code(s.which) as i64), Construct => 10001 }; }
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
// has_text, text_start and text_end: the function's source text
// ([[SourceText]], Function.prototype.toString; light-cljs-853), a byte
// range of Code.source_bytes (program::source_text) on character
// boundaries (both 0 without text: a
// script or a synthesized function).
// simple: the parameter list is simple (no default, rest or pattern; export
// flag bit 19, light-cljs-854): sloppy code with one gives its arguments
// object a `callee` data property, any other the %ThrowTypeError% accessor.
// eval: the root function of eval code compiled at run time
// (light-cljs-856, runtime_compile.goose), whose var and function
// declarations make configurable global properties (global_op); never set
// by a loaded image.
struct Function { start: u32, count: u32, constants: u32, children: u32, params: u32, locals: u32, captures: u32, strict: bool, kind: i64 = 0, generator: bool = false, asynchronous: bool = false, name_start: u32 = 0, name_len: u32 = 0, length: u32 = 0, has_text: bool = false, text_start: u32 = 0, text_end: u32 = 0, simple: bool = false, eval: bool = false }
// direct_eval: the image marks the row as the call of a direct-eval site
// (descriptor bit 24, light-cljs-823); only the four call opcodes carry it.
struct Op { code: i64, arg: i64, offset: i64, start: i64, end: i64, depth: i64 = -1, direct_eval: bool = false }
// A direct-eval site (light-cljs-858; GSP version 6, program::Layout): the
// row of its marked call in Code.ops, its capture sources
// Code.site_sources[sources..sources + count] in the eval closure's capture
// order, and its scope as the opaque record
// Code.site_records[record..record + length], which only the run-time
// compiler reads (runtime_compile.goose, start_direct_eval). Code.sites is
// in ascending `op` order.
struct Site { op: u32, sources: u32, count: u32, record: u32, length: u32 }
// One capture source: kind an eval_source:: name, index the caller's local
// or capture (0 for the other kinds), role an eval_role:: name, cell whether
// the value is a binding cell.
struct SiteSource { kind: u8, role: u8, cell: bool, index: u32 }
// One decoded instruction per validated `Op` row (light-cljs-785; decision 1
// of docs/javascript-coverage.md): a fixed-mode enum dispatched by `match` in
// `execute`. Payloads are the row's validated operand; jump targets are absolute
// instruction indices. `Op` rows remain the validation and provenance record
// (source spans, byte offsets, the opcode a fault names). An opcode this VM
// does not implement decodes to `Unsupported` and fails by name when it runs.
enum Ins {
    Const { k: u32 }, Undefined, Load { slot: u32 }, Store { slot: u32 }, Pop, Dup, Dup2, Dup3, Binary { op: u8 }, Not,
    Jump { to: u32 }, JumpUnless { to: u32 }, Return, NewObject { bare: bool }, NewArray, Try { to: u32 }, Throw, EndTry,
    Unary { op: u8 }, Numeric2, Closure { child: u32 }, Call { argc: u32 }, CallMethod { argc: u32 },
    Arguments, MappedArguments, Rest { skip: u32 }, Callee, Capture { slot: u32 }, NewCell { immutable: bool },
    InitCell, ReadCell, WriteCell, GetNamed { k: u32 }, GetIndexed, SetNamed { k: u32, strict: bool }, DefineNamed { k: u32, half: u8 },
    SetIndexed { strict: bool }, DefineIndexed { half: u8 }, SetProto, Delete { strict: bool }, In, Append, ResolveThis, This { sloppy: bool }, Global, GetGlobal { k: u32, throws: bool },
    Literal { kind: u8, count: u32 }, Form { op: u32 }, ToString, Unsupported { opcode: u8 },
    // new f(...) (94; light-cljs-831, construct): an ordinary function's
    // [[Construct]] and the realm's constructors. NewTarget (GetNewTarget,
    // 154) reads the frame's new.target.
    Construct { argc: u32 }, NewTarget,
    // Finally blocks and completion records (light-cljs-786): the Pants
    // opcodes, with their stack contracts in rust/src/vm/instr.rs. Pending
    // is PendingThrow, PendingException or PendingCancel.
    PushFinally { to: u32 }, PopFinally, EndFinally, CallFinally, DropCompletion, Pending { what: u8 },
    // Key enumeration (light-cljs-804): for-in (ForInStart, ForInNext),
    // object rest (ToPropertyKey, RequireCoercible, OwnKeys, IsEnumerable)
    // and spread (CopyDataProperties).
    ForInStart, ForInNext, OwnKeys, IsEnumerable, CopyData, ToPropertyKey, RequireCoercible,
    // Typeof (light-cljs-788). ToObject (176, a `with` statement's object;
    // light-cljs-832): v → ToObject(v), boxing a primitive.
    TypeOf, ToObject,
    // v target → bool, `v instanceof target` (96; light-cljs-834, instanceof_op).
    Instanceof,
    // The iteration protocol (light-cljs-843): GetIterator (116) iterable →
    // record, an object with own `iterator` and `next` (get_iterator_op);
    // RequireObject (117) v → v, a TypeError unless v is an object.
    GetIterator, RequireObject,
    // AppendHole (89; light-cljs-844) arr → arr: an array literal's elided
    // element, the length one more and no element (a hole).
    AppendHole,
    // Dynamic environments (light-cljs-855; rust/src/frontend/lower.rs
    // environment_lookup): IsCell (174) v → bool, whether v is a binding
    // cell (a sloppy eval's variable environment) rather than a `with`
    // object; IsObject (175) v → bool, whether v is a JavaScript object (an
    // @@unscopables value).
    IsCell, IsObject,
    // SetComputedName (135) key f → key f: a computed-key literal
    // function's `name` (set_computed_name), `k` the prefix constant.
    ComputedName { k: u32 },
    // Generators and async functions (light-cljs-790, continuations.goose):
    // GeneratorStart only marks code; GeneratorInitialYield, GeneratorYield,
    // AsyncStart and Await detach and settle activations.
    // GeneratorYieldStar and AsyncGeneratorStart stay Unsupported.
    GeneratorStart, GeneratorInitialYield, GeneratorYield, AsyncStart, Await,
    // The Call, CallMethod, CallSpread or CallMethodSpread of a direct-eval
    // site (light-cljs-823): when its callee is the realm's %eval% it is a
    // direct eval in the caller's scope (light-cljs-858, start_direct_eval);
    // any other callee is an ordinary call, spread or not. SloppyThis
    // (CheckSloppyThis, 146) v → this: eval code's read of a sloppy caller's
    // receiver, an object unchanged, undefined or null the global object,
    // any other primitive boxed.
    DirectEval { argc: u32, method: bool, spread: bool }, SloppyThis,
    // Spread arguments (light-cljs-850; rust/src/vm/dispatch.rs
    // spread_arguments and call_spread): CheckSpread (CheckSpreadArguments)
    // args → args, a RangeError above 65,535 packed elements; CallSpread
    // (CallSpread, CallMethodSpread) f args → result and this f args →
    // result; ConstructSpread f args → result. The packed array is the
    // compiler's private array: its elements replace it on the operand stack
    // and the call runs as Call, CallMethod or Construct (spread_unpack).
    CheckSpread, CallSpread { method: bool }, ConstructSpread,
    // Script declarations and global names (light-cljs-824, global_op), `op`
    // the opcode and `k` its name constant: PutGlobal, PutGlobalStrict,
    // DeclareGlobalVar, DeclareGlobalFunction, DeclareGlobalLet,
    // DeclareGlobalConst, InitGlobalLexical, CheckGlobalVar,
    // CheckGlobalFunction, DeleteGlobal, DeclareGlobalConstValue and
    // CheckGlobalLexicalConflict. ThrowError: the compiler's static throw of
    // ThrowKind `kind` (a throw_kind:: selector).
    GlobalOp { op: u8, k: u32 }, ThrowError { kind: u8 },
    // TemplateObject (111; light-cljs-862, templates.goose): `count` cooked
    // and raw string pairs → the site's template object.
    TemplateObject { count: u32 },
    // DefineClassMethod, DefineClassGetter and DefineClassSetter (147-149;
    // light-cljs-886): obj k f → obj, a class element on its home object
    // (the prototype or the constructor), `half` a data method (0), getter
    // (1) or setter (2) (define_field's class attributes).
    DefineClass { half: u8 },
    // Derived classes (light-cljs-887; the Pants engine's class_step):
    // CheckHeritage (155) parent → parent, a TypeError unless null or a
    // constructor; InitHeritage (156) class parent proto → class, both
    // inheritance links; SuperConstructor (GetSuperConstructor, 157) f →
    // f's [[Prototype]]; ConstructSuper (158, ConstructSuperSpread 159
    // with `spread`) parent newTarget a1..an → result, the parent's
    // [[Construct]] with the shared newTarget; BindThis (160) cell result →
    // result, a derived constructor's `this` initialized once.
    // ForwardArguments (162) → args, the frame's arguments in a packed
    // array, decodes to Rest { skip: 0 }: no observable iteration.
    // ResolveThis (161) v → v reads a derived constructor's `this` cell.
    CheckHeritage, InitHeritage, SuperConstructor, ConstructSuper { argc: u32, spread: bool }, BindThis,
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
    // Direct-eval sites (light-cljs-858; Site).
    sites: Site[>..] = [], site_sources: SiteSource[>..] = [], site_records: u8[>..] = [],
}
// Unsupported: an exported opcode (or constant kind) this VM does not
// implement ran; Job.fault_opcode names it. Like every fault except the
// TypeError, ReferenceError, RangeError, SyntaxError, HostEffect and StackLimit throws
// (catchable(), light-cljs-786), JavaScript cannot catch it. SyntaxError is
// a script declaration's run-time check (light-cljs-824, global_op). Cancelled: the job's consumer
// cancelled it (cancel()); its finally blocks ran first. UnhandledRejection
// and Unsettled end a job whose promise jobs have drained (light-cljs-790,
// continuations.goose): a promise was rejected with no reaction, the
// result its thrown value; or the entry returned a promise that can never
// settle, since nothing is left to run.
enum Fault { None, InvalidState, StaleID, MemoryLimit, WorkLimit, InstructionLimit, StackLimit, TypeError, ReferenceError, ConstAssignment, UnsupportedOperand, NativeBoundary, DependencyNeed, EffectContract, HostEffect, Unsupported, Cancelled, RangeError, UnhandledRejection, Unsettled, SyntaxError }
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
// `conts` the continuation table's rows (light-cljs-790); `code` the
// instruction rows run-time compilation adds in one job (light-cljs-856,
// Job.compiled_ops).
struct Limits { nodes: i64 = 100000, values: i64 = 1048576, units: i64 = 1048576, properties: i64 = 1048576, work: i64 = 10000000, steps: i64 = 1000000, frames: i64 = 10000, stack: i64 = 1048576, shapes: i64 = 1048576, conts: i64 = 1048576, code: i64 = 1048576 }
// All heap tables append until session teardown; IDs are never recycled.
// kind: a node_kind:: name below (Goose-private; light-cljs-838). The
// numbers are stored in snapshots and artifacts, so a new kind takes the
// next number and none is renumbered; ordered tests (`kind <
// node_kind::List`) rely on the grouping. Captures and binding cell values
// never reference frame scratch. Property chains are indices, not Goose
// references.
let node_kind::None = -1;      // not an object (a sentinel, never stored)
let node_kind::Object = 0;     // an ordinary object
let node_kind::Array = 1;
let node_kind::Closure = 2;
let node_kind::Binding = 3;    // a closure's captured binding cell
let node_kind::Builtin = 4;
let node_kind::List = 5;       // List .. Set: collection forms
let node_kind::Vector = 6;
// A map's or set's `length` is its traversal order, a map_order:: name
// below (light-cljs-877).
let node_kind::Map = 7;
let node_kind::Set = 8;
let node_kind::Symbol = 9;
let node_kind::Keyword = 10;
let node_kind::Scalar = 11;    // a scalar form occurrence
// A concatenated sequence (form_concat, a rest), read as a list.
let node_kind::Concat = 12;
let node_kind::Error = 13;     // an opaque caught runtime error
// An arguments object (light-cljs-854): an ordinary object with an element
// run whose class is Arguments (Object.prototype.toString); see "Arguments
// objects".
let node_kind::Arguments = 14;
// A finite lazy (deferred) map: value=callback, namespace_value=input,
// start/count=reserved memo range, length=completed callbacks.
let node_kind::LazySeq = 15;
// A key list: a for-in walk's or an object spread's own keys, job-local;
// see "Key enumeration" and proxy.goose.
let node_kind::KeyList = 16;
// A job's own atom or volatile: value=state, length=flavor (0 atom or 1
// volatile).
let node_kind::Atom = 17;
// A process-wide cell's reference: function=cell, length=flavor (0 atom,
// 1 volatile or 2 delay, whose value is its thunk when the job holds it);
// cells.goose.
let node_kind::Cell = 18;
// A job's own delay: value=thunk, pending while truthy, namespace_value=the
// value it realized (light-cljs-808).
let node_kind::Delay = 19;
// A generator object: function=its Activation row in heap.conts or
// no_row(), length=its state (light-cljs-790; continuations.goose).
let node_kind::Generator = 20;
// A promise: function=its Promise row in heap.conts.
let node_kind::Promise = 21;
// A primitive wrapper object (light-cljs-832): value=the primitive.
let node_kind::Wrapper = 22;
// A built-in iterator object (iterators.goose).
let node_kind::Iterator = 23;
// A proxy: value=target, namespace_value=handler (proxy.goose).
let node_kind::Proxy = 24;
// A bound function exotic object (light-cljs-852; function_calls.goose):
// value=its target (an earlier row, callable), namespace_value=the bound
// `this`, start/count=the bound arguments, a shared run of heap.values like
// a closure's captures; proto=the target's [[GetPrototypeOf]] when bound
// (Empty: %Function.prototype%). Its own properties are a built-in
// function's (a shape or records): `length` and `name` from bind, then any
// the job adds. It has no `prototype`. In a cell or a publication it
// follows the closure rule: a function with own properties is a mutable
// JavaScript object, refused, and a bound function always has `length`
// and `name`; a worker snapshot carries it like any row.
let node_kind::BoundFunction = 25;
// A mapped arguments object's map (light-cljs-860; see "Mapped arguments
// objects"): start/count=a run of one value per mapped index, the
// node_kind::MappedBinding cell aliasing that index, or Undefined once
// it is unmapped; value=that arguments object, which names it in
// namespace_value.
let node_kind::ArgumentMap = 26;
// A parameter's binding cell while it aliases an index of a mapped
// arguments object: value=the arguments object, length=the index,
// namespace_value=Null or, once a redefinition moved the property out of
// the element run, its interned key. Its value is that object's own
// property; unmapping turns it into an ordinary node_kind::Binding holding
// the value it had.
let node_kind::MappedBinding = 27;
// A symbol a job made (light-cljs-885; symbols.goose), named by the Symbol
// values symbol_at gives, never by an Object value: value=its description,
// a string or undefined; initialized=Symbol.for registered it, its
// description then being its key; namespace_value=the symbol registered
// before it, or Null (the chain Heap.registry starts). It has no
// properties, prototype or element run, and nothing writes it.
let node_kind::JsSymbol = 28;
// A map's or set's traversal order (its node's `length`; light-cljs-877),
// as the V8 build's runtime (src/js_runtime.js) has it: an array map or a
// hash map or set, the Pants engine's FormMap::Positional and Hashed
// (collections/form.rs). Insertion: a map or set of up to 8 entries, and a
// constant literal or array map of any size (LiteralMap, ArrayMap),
// printed in insertion order. Hash: a hash-map literal's map (CljZipMap,
// NewFormMap) of any size, a set literal of more than 8 members
// (NewFormSet), a compiler form's map of more than 16 operands or set of
// more than 8 distinct members (the form bridge, decoded_hash_ordered and
// merge_keys), and a map or set an assoc or conj
// took past 8 entries; it stays Hash through every update and prints in
// ClojureScript's hash order (form_hash, legacy_order_key). The values run
// stays in insertion order either way, so lookups and equality do not look
// at it.
let map_order::Insertion = 0;
let map_order::Hash = 1;
// An array's attribute flags, bits of its node's `function` (light-cljs-881;
// "Property descriptors" below), 0 for an array no definition changed, so
// the element fast paths (dense) see only arrays without them. ReadOnlyLength:
// its `length` is not writable (ArraySetLength). Attributed: it is a
// dictionary array that may hold an element record other than a plain
// writable, enumerable, configurable data property (a non-writable or
// non-configurable element, or an accessor), so element writes take the
// property model's checks.
// Sealed and Frozen (light-cljs-882, integrity.goose): a dense array that
// SetIntegrityLevel sealed or froze, every element of its run
// non-configurable and, Frozen (which sets Sealed too), non-writable, so its
// elements stay in the run; only a non-extensible array has them, since a
// new element could not share them. A move to dictionary mode turns them
// into its records' attributes (array_to_dictionary). A template object and
// its `raw` (templates.goose) are frozen arrays.
let array_flag::ReadOnlyLength: u32 = 1;
let array_flag::Attributed: u32 = 2;
let array_flag::Sealed: u32 = 4;
let array_flag::Frozen: u32 = 8;
inline fn array_flags(n: const Node&) -> u32 { return if n.kind == node_kind::Array { n.function } else { 0 }; }
inline fn read_only_length(heap: const Heap&, o: i64) -> bool { return array_flags(node(heap, o)) & array_flag::ReadOnlyLength != 0; }
// The attributes of a dense array's elements (array_flag).
inline fn element_attrs(n: const Node&) -> u8 {
    let f = array_flags(n);
    return if f & array_flag::Frozen != 0 { attr_enumerable() } else if f & array_flag::Sealed != 0 { attr_writable() | attr_enumerable() } else { attrs_data() };
}
// Whether node o is a frozen array (array_flag::Frozen).
checkfree inline fn frozen_array(heap: const Heap&, o: i64) -> bool { let n .= node(heap, o); return n.kind == node_kind::Array && n.function & array_flag::Frozen != 0; }
// A sequence's (node_kind::Concat) flavor, its node's `length`
// (light-cljs-877): an ordinary sequence, or a realized integer range
// (realm.goose core_range), which nth indexes as the V8 build's
// IntegerRange does, flooring a fractional index inside its count; its
// rest is one too.
let seq_flavor::Sequence = 0;
let seq_flavor::IntegerRange = 1;
inline fn integer_range(n: Node) -> bool { return n.kind == node_kind::Concat && n.length == seq_flavor::IntegerRange; }
inline fn hash_ordered(n: Node) -> bool { return (n.kind == node_kind::Map || n.kind == node_kind::Set) && n.length == map_order::Hash; }
// Errors, generators, promises, wrappers and iterators are ordinary objects
// with internal slots: their own properties are an Object's, without an
// element run.
// An arguments object is an ordinary object with its element run.
// Closure immutable marks fnApplyNull's immutable receiver convention.
// non_extensible is a JavaScript object's [[Extensible]] cleared
// (light-cljs-882, integrity.goose): set only on the kinds with own
// properties (snapshot.goose named), never cleared, and only by the job
// that allocated the node (or the realm, for %ThrowTypeError%). A
// non-extensible object refuses a new property and another prototype.
// Errors preserve object identity/truthiness;
// property access/coercion and form transport reject unsupported inspection.
// Metadata/origin belong to the occurrence, independently of value equality.
// Links (function, start, keys, source) and counts are plain u32 rows;
// source is no_row() for a node without provenance. origin is the compiler
// occurrence the node came from, not a VM row, and stays -1 when absent.
//
// Own properties (light-cljs-787; decision 2 of docs/javascript-coverage.md).
// An ordinary object (node_kind::Object) keeps its key_count properties in one of two
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
// UnsupportedOperand (see on_intrinsic). An array (node_kind::Array) and a closure
// (node_kind::Closure) keep named properties the same way (light-cljs-805): a dense
// array and a closure in shape mode (their `start` and `count` are the
// elements or the captures), a dictionary array's names among its index
// records. Other kinds keep no own properties.
struct Node { kind: i64, function: u32 = 0, start: u32 = 0, count: u32 = 0, keys: u32 = 0, key_count: u32 = 0, key_capacity: u32 = 0, length: u32 = 0, initialized: bool = false, immutable: bool = false, non_extensible: bool = false, value: Value = Value.Undefined, namespace_value: Value = Value.Null, metadata: Value = Value.Undefined, origin: i64 = -1, source: u32 = 4294967295, span_start: u32 = 0, span_end: u32 = 0, shape: u32 = 0, proto: Value = Value.Empty, slots: Value[2] = [Value.Undefined; 2] }
// A form source row: its text in the owner's source bytes (`identity` -1),
// or, from the compiler (GFB2, light-cljs-817), the compiler's identity for
// the source and its byte length, with no text (`start` 0). Goose never
// interprets a source's text: it range-checks spans against `count` and
// carries rows on; the compiler checks an identity row's spans against its
// own source, UTF-8 boundaries included.
struct SourceRange { start: u32, count: u32, identity: i64 = -1 }
fn has_text(r: SourceRange) -> bool { return r.identity < 0; }
// The largest identity a graph's source entry carries (a signed 32-bit word).
fn max_identity() -> i64 { return 2147483647; }
// Whether a row's text, if it has any, lies inside `bytes` source bytes, and
// an identity row has no text and an identity a graph can carry.
fn source_row_ok(r: SourceRange, bytes: i64) -> bool {
    if !has_text(r) { return r.start == 0 && r.identity <= max_identity(); }
    return r.start <= bytes && r.count <= bytes - r.start;
}
// `r` for text appended at offset `bytes`; an identity row has none to move.
fn shifted_source(r: SourceRange, bytes: i64) -> SourceRange { return if has_text(r) { SourceRange { start: (bytes + r.start) as u32, count: r.count } } else { r }; }
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
    pending: Value = Value.Undefined,
    // proxy_method: the proxy internal method (proxy.goose, proxy_method::)
    // an UnsupportedOperand reached, for its report; fail() takes and
    // clears it (-1: none).
    proxy_method: i64   = -1,
    fault:        Fault = Fault.None, global: Value = Value.Undefined, call_method: Value = Value.Undefined, boot_count: i64 = 0, initializer_count: i64 = 0, producer_count: i64 = 0,
    live_jobs:    i64   = 0,
    // Reclamation inside a job (light-cljs-847, reclaim.goose): each table's
    // margin below its limit while a job runs; a collection asked for at the
    // next safe point; and the collections this heap ran (the JOBS line).
    // None of it is part of a snapshot.
    slack: Slack = Slack {}, reclaim: bool = false, reclaims: i64 = 0,
    // The Latin-1 table's last latin1_spare bytes are a collection's
    // leftovers, not rows: the table's length is latin1.len - latin1_spare,
    // and add_latin1 writes over them first. (A collection cannot shrink a
    // byte array it reaches through Job.heap: Goose's shrink check would
    // then refuse every caller of run that holds a byte view.)
    latin1_spare: i64 = 0,
    // The Symbol.for registry (light-cljs-885; symbols.goose): the newest
    // registered symbol, whose row links the one registered before it
    // (node_kind::JsSymbol), or Null. A state field, so the worker reset
    // and snapshots restore it (snapshot::Header) and reclamation keeps
    // and moves the symbols it reaches; there is no lookup structure
    // outside the rows.
    registry: Value = Value.Null,
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
//   Convert      ToPrimitive with `hint` of `count` consecutive operands of
//                the frame below (light-cljs-828; the Pants engine's
//                NativeState::Convert): operand i is
//                operands[operand_base - argument_count + i], index the
//                one being converted, left to right. `method` 0 is
//                GetMethod(@@toPrimitive), 1 and 2 OrdinaryToPrimitive's
//                methods in the hint's order; pc 1 awaits a method's
//                result, pc 2 a method from its getter (light-cljs-805).
//                A primitive replaces its object in place (and, when
//                `finish`, its ToNumber, or under hint string its
//                ToString (String(o), light-cljs-832), before the next
//                operand converts); then the frame pops and the operation below,
//                whose pc was left at its own step, runs again on
//                primitives. Nothing it completed runs twice.
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
//   Resume       a generator's next, return or throw (resume_kind::, continuations.goose)
//                (light-cljs-790; continuations.goose): input the
//                generator, its activation attached above; pc 1 awaits the
//                value it yields or returns.
//   HasInstance  InstanceofOperator and OrdinaryHasInstance (light-cljs-834;
//                instanceof_op): receiver the target C, input the value V;
//                pc 1 awaits C[@@hasInstance] from its getter, pc 2 a user
//                @@hasInstance method's result (ToBoolean) or a
//                native-frame built-in's, pc 3 C.prototype from its getter,
//                pc 4 V converted by a Convert frame for the built-in
//                method callee (Number or String). Its result replaces the
//                operation's operands at operand_base.
//   ArrayMethod  Array.prototype.push, join and toString (light-cljs-842;
//                array_methods.goose, which gives their fields and phases),
//                its Builtin the payload.
//   BuiltinIterator  %ArrayIteratorPrototype%.next and
//                String.prototype[@@iterator] (light-cljs-844;
//                iterators.goose, which gives their fields and phases),
//                its Builtin the payload.
//   GetIterator  GetIterator(v, sync) (light-cljs-843; get_iterator_op):
//                receiver the iterable v, input the iterator once called;
//                pc 1 awaits v[@@iterator] from its getter, pc 2 the
//                iterator from the method (a user function's frame or a
//                native-frame built-in's), pc 3 the iterator's `next` from
//                its getter. Its result, the iterator record, goes where
//                the operation's operand was (operand_base).
//   ForIn        ForInNext over a proxy, or a walk that reached one
//                (light-cljs-845; for_in_walk): input the key list; pc 1
//                awaits the walked proxy's keys, pc 2 the descriptor of the
//                key just reached, pc 3 the walk's next object. Its results,
//                the key and whether there is one, go where the key list
//                was (operand_base).
//   Proxy        a proxy's [[GetPrototypeOf]], [[OwnPropertyKeys]] or
//                [[GetOwnProperty]] (light-cljs-845; proxy.goose, which
//                gives its fields and phases), its proxy_method the payload.
//   Forward      Function.prototype.call and apply, Reflect.apply and
//                Reflect.construct (light-cljs-851; function_calls.goose,
//                which gives its fields and phases): `construct` once
//                Reflect.construct has resolved its target.
//                A bound function's call or construction runs as one too
//                (light-cljs-852), `construct` for the latter.
//   Bind         Function.prototype.bind (light-cljs-852; function_calls.goose,
//                which gives its fields and phases).
//   FunctionText Function.prototype.toString of a function without source
//                text (light-cljs-853; function_calls.goose, which gives its
//                fields and phase).
//   DynamicFunction  the Function constructor (light-cljs-857;
//                runtime_compile.goose, which gives its fields and phase).
//   ErrorConstruct  Error(message, options) and the seven NativeError
//                constructors when a step runs JavaScript (light-cljs-870;
//                realm.goose, which gives its fields and phases), its
//                Builtin the payload.
//   ErrorText    Error.prototype.toString when a read or conversion runs
//                JavaScript (light-cljs-870; realm.goose, which gives its
//                fields and phases).
//   Statics      the Object statics that expose own keys, Reflect.ownKeys
//                and has, and toString's @@toStringTag getter
//                (light-cljs-884; object_statics.goose, which gives their
//                fields and phases), its Builtin the payload.
//   Construct    [[Construct]] of an ordinary function or base class whose
//                newTarget's `prototype` is an accessor (light-cljs-886;
//                construct): callee the constructor, input newTarget, the
//                constructor and its arguments at operand_base; pc 0
//                awaits the getter's value, then the constructor's frame
//                replaces this one.
// ToPrimitive's preferred type (ECMAScript 7.1.1): the string a
// @@toPrimitive method receives and OrdinaryToPrimitive's method order.
enum Hint { Default, Number, String }
enum Native { None, Mapv, MapSeq, Setter, CopyData, Convert { hint: Hint, finish: bool, count: u8, method: u8 }, ArrayCallbacks { which: Builtin }, Swap { which: Builtin }, Force { which: Builtin }, Resume { kind: u8 }, HasInstance, ArrayMethod { which: Builtin }, GetIterator, BuiltinIterator { which: Builtin }, ForIn, Proxy { method: u8 }, Forward { construct: bool }, Bind, FunctionText, DynamicFunction, ErrorConstruct { which: Builtin }, ErrorText, Descriptor { which: Builtin }, Statics { which: Builtin }, Construct }
// Actual arguments occupy bounded job-local scratch after declared locals.
// Copies live across yields/helper suspension and disappear on frame unwind.
// A bytecode frame's `input` is its new.target (light-cljs-831): undefined
// in a call; in a [[Construct]] the constructor, with `receiver` the new
// object that replaces a non-object return value (return_value). Being frame
// data, the pending substitution survives budget cuts, helper waits and
// saved continuations with the frame.
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
// detached activation, a generator's frame or a suspended job's whole
// stack, saves its frames' handlers (Cont.Saved).
enum HandlerKind { Catch, Finally, Normal, Return, Throw, Cancel, Jump, Async }
struct Handler { frame: i64, pc: i64, operands: i64, locals: i64, kind: HandlerKind = HandlerKind.Catch, value: Value = Value.Undefined }
root struct Job {
    heap:     Heap&, frames: Frame[>..<] = [], locals: Value[>..<] = [], operands: Value[>..<] = [],
    writes:   Write[>..]                 = [], status: tiny::Status = tiny::Status.Ready, result: Value = Value.Undefined,
    handlers: Handler[>..<]              = [],
    fault:    Fault                      = Fault.None, fault_pc: i64 = -1, fault_function: i64 = -1, fault_opcode: i64 = -1, thrown: bool = false,
    // The script's global lexical record (light-cljs-824): a dictionary-mode
    // node the job allocated by its first DeclareGlobalLet, DeclareGlobalConst
    // or DeclareGlobalConstValue, one record per script `let`, `const` or
    // `class` binding, its value the binding's cell and its attributes
    // writable for `let` (global_op); Undefined while the job has none.
    // JavaScript never sees the node. The reset drops it with the rows.
    lexicals:        Value = Value.Undefined,
    steps:           i64   = 0, claimed: bool = false, registered: bool = false,
    effects_enabled: bool  = false, effect_pending: bool = false, effect_kind: i64 = -1,
    helpers_enabled: bool  = false,
    protected_nodes: i64   = 0,
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
    // The heap's code, writable, where the job may add to it (null
    // elsewhere, where such an addition fails by name): a function loaded
    // from a cell installs its code there in a macro job (light-cljs-809),
    // and run-time compilation appends its functions (light-cljs-856,
    // runtime_compile.goose), compiled_ops instructions so far, in a macro
    // job and in a definition's or helper's initializer (light-cljs-863),
    // whose prepared definition or callable publication carries the code
    // on. The worker's reset cuts both away (snapshot::truncate).
    writable_code: Code? = null, compiled_ops: i64 = 0,
    // Promise jobs (light-cljs-790; continuations.goose): the job's FIFO of
    // reactions, `queue_head` its next; `draining` once the entry has
    // finished with `entry`, its value; and the promises rejected while no
    // reaction was registered on them. All of it is the job's, saved with
    // its continuation and cleared by reset.
    queue: Task[>..] = [], queue_head: i64 = 0, draining: bool = false, entry: Value = Value.Undefined, rejections: u32[>..] = [],
    // The tables' lengths when the job started or resumed (light-cljs-847,
    // reclaim.goose): a collection inside the job moves only rows above
    // them. reclaimable: start or resume_stack armed the job's margins.
    floor: Tops = Tops {}, reclaimable: bool = false,
    // The host reads the write log's values (execute.goose's typed
    // outcome): a collection keeps them; otherwise they are weak.
    logged_values: bool = false,
    // The template objects of the tagged-template sites the job evaluated
    // (light-cljs-862; templates.goose, TemplateSite): job-local, saved with
    // its continuation, a reclamation root, cleared by reset.
    templates: TemplateSite[>..] = [],
}
// A row a job holds that names a process-wide cell (a cell reference) or
// has a process-wide identity (a value loaded from or stored in a cell, or
// a node of a publication it installed). count > 1: the rows row..row +
// count, a publication's, whose identities are identity..identity + count.
struct CellRow { row: i64, cell: i64 = -1, identity: i64 = 0, count: i64 = 1 }
inline fn number(n: f64) -> Value { return Value.Number { v: n }; }
inline fn boolean(b: bool) -> Value { return Value.Bool { v: b }; }
// Whether v is a JavaScript object: an object row of any kind JavaScript
// can hold (functions, arrays, proxies, wrappers, forms ...), not a binding
// cell or a key list, which only compiled code and native frames see.
inline fn javascript_object(heap: const Heap&, v: Value) -> bool { return v.is_object() && !internal_kind(node(heap, v.id()).kind); }
// The kinds only compiled code and native frames see: binding cells (a
// mapped parameter's too), key lists and arguments maps.
inline fn internal_kind(kind: i64) -> bool { return kind == node_kind::Binding || kind == node_kind::KeyList || kind == node_kind::MappedBinding || kind == node_kind::ArgumentMap; }
// A code constant's units: Code keeps its own string headers over its units.
fn code_chars(code: const Code&, v: Value) -> const u16[:] { let s = code.strings[v.sid()]; return code.units[s.start..(s.start + s.len)]; }
fn reject(code: Code&, error: tiny::LoadError, pc: i64) -> bool { code.error = error; code.error_pc = pc; return false; }
// Every opcode's stack effect and control flow come from the generated
// registry table (export_stack_effect, light-cljs-783); there is no
// hand-written opcode list. `captures` is the child function's capture count,
// read only when the effect's multiplied count is that (count::Captures).
fn effect_word(e: i64, op: Op, captures: i64) -> i64, i64 {
    if e < 0 { return -1, -1; }
    let n = match (e >> 24) & 15 { count::Operand => op.arg, count::Captures => captures, count::FormArity => export_form_arity(op.arg), _ => 0 };
    if n < 0 { return -1, -1; }
    return (e & 255) + ((e >> 16) & 255) * n, (e >> 8) & 255;
}
fn stack_effect(code: const Code&, f: Function, op: Op) -> i64, i64 {
    let e = export_stack_effect(op.code);
    let captures = if e >= 0 && ((e >> 24) & 15) == count::Captures { code.functions[code.children[f.children + op.arg]].captures } else { 0 };
    return effect_word(e, op, captures);
}
// Flow (flow::): Next, Jump, Branch, Catch (the target starts with the
// exception pushed) or End. Branching opcodes carry a byte-offset target
// operand (operand::Target).
fn flow(code: i64) -> i64 { return export_stack_effect(code) >> 28; }
fn branches(code: i64) -> bool { return ((export_descriptor(code) >> 8) & 255) == operand::Target; }
fn target(code: const Code&, f: Function, offset: i64) -> i64 {
    var lo = f.start; var hi = f.start + f.count;
    while lo < hi { let mid = lo + (hi - lo) / 2; if code.ops[mid].offset < offset { lo = mid + 1; } else { hi = mid; } }
    return lo;
}
fn instruction(op: Op) -> Ins {
    let a = op.arg as u32;
    return match op.code {
        opcode::Const => Ins.Const { k: a }, opcode::Undefined => Ins.Undefined, opcode::GetLocal => Ins.Load { slot: a }, opcode::SetLocal => Ins.Store { slot: a },
        opcode::Pop => Ins.Pop, opcode::Dup => Ins.Dup, opcode::Dup2 => Ins.Dup2, opcode::Dup3 => Ins.Dup3,
        opcode::Add, opcode::Sub, opcode::Mul, opcode::Lt, opcode::StrictEq, opcode::Div, opcode::Rem, opcode::Gt, opcode::Le, opcode::Ge, opcode::LooseEq, opcode::Plus, opcode::CompareLt, opcode::CompareGt, opcode::CompareLe, opcode::CompareGe => Ins.Binary { op: op.code as u8 },
        opcode::BitAnd, opcode::BitOr, opcode::BitXor, opcode::Shl, opcode::Sar, opcode::UShr, opcode::Pow => Ins.Binary { op: op.code as u8 },
        opcode::Not => Ins.Not, opcode::Jump => Ins.Jump { to: a }, opcode::JumpIfFalse => Ins.JumpUnless { to: a }, opcode::Return => Ins.Return,
        opcode::NewObject => Ins.NewObject { bare: true }, opcode::NewObjectLiteral => Ins.NewObject { bare: false }, opcode::NewArray => Ins.NewArray, opcode::PushCatch => Ins.Try { to: a }, opcode::Throw => Ins.Throw, opcode::PopCatch => Ins.EndTry,
        opcode::Neg, opcode::ToNumber, opcode::ToNumeric, opcode::Inc, opcode::Dec, opcode::BitNot => Ins.Unary { op: op.code as u8 }, opcode::ToNumberPair => Ins.Numeric2,
        // Method, an object literal's or a class's method, getter or
        // setter: obj c1..cn → obj f, a closure like Function's; its home
        // object only serves `super`, whose opcodes are not implemented
        // (light-cljs-787): light-cljs-888 stores it (see
        // docs/javascript-coverage.md, light-cljs-886).
        opcode::Function, opcode::Method => Ins.Closure { child: a }, opcode::Construct => Ins.Construct { argc: a },
        opcode::Call => if op.direct_eval { Ins.DirectEval { argc: a, method: false, spread: false } } else { Ins.Call { argc: a } },
        opcode::CallMethod => if op.direct_eval { Ins.DirectEval { argc: a, method: true, spread: false } } else { Ins.CallMethod { argc: a } },
        opcode::CallSpread, opcode::CallMethodSpread => if op.direct_eval { Ins.DirectEval { argc: 0, method: op.code == opcode::CallMethodSpread, spread: true } } else { Ins.CallSpread { method: op.code == opcode::CallMethodSpread } },
        opcode::ConstructSpread => Ins.ConstructSpread, opcode::CheckSpreadArguments => Ins.CheckSpread,
        opcode::NewArguments => Ins.Arguments, opcode::NewMappedArguments => Ins.MappedArguments, opcode::Rest => Ins.Rest { skip: a }, opcode::CurrentFunction => Ins.Callee, opcode::GetCapture => Ins.Capture { slot: a },
        opcode::NewCell, opcode::NewSealableCell => Ins.NewCell { immutable: false }, opcode::NewFixedCell => Ins.NewCell { immutable: true },
        opcode::CellInit => Ins.InitCell, opcode::CellGet => Ins.ReadCell, opcode::CellSet => Ins.WriteCell,
        opcode::GetField => Ins.GetNamed { k: a }, opcode::GetElem => Ins.GetIndexed,
        opcode::PutField => Ins.SetNamed { k: a, strict: false }, opcode::PutFieldStrict => Ins.SetNamed { k: a, strict: true }, opcode::PutElem => Ins.SetIndexed { strict: false }, opcode::PutElemStrict => Ins.SetIndexed { strict: true }, opcode::AppendElement => Ins.Append,
        opcode::DefineField => Ins.DefineNamed { k: a, half: 0 }, opcode::DefineGetter => Ins.DefineNamed { k: a, half: 1 }, opcode::DefineSetter => Ins.DefineNamed { k: a, half: 2 },
        opcode::DefineElem => Ins.DefineIndexed { half: 0 }, opcode::DefineGetterElem => Ins.DefineIndexed { half: 1 }, opcode::DefineSetterElem => Ins.DefineIndexed { half: 2 },
        opcode::SetProto => Ins.SetProto, opcode::Delete => Ins.Delete { strict: false }, opcode::DeleteStrict => Ins.Delete { strict: true }, opcode::In => Ins.In,
        opcode::ResolveThis => Ins.ResolveThis, opcode::GetThis => Ins.This { sloppy: true }, opcode::GetRawThis => Ins.This { sloppy: false }, opcode::GetGlobalObject => Ins.Global,
        opcode::GetGlobal => Ins.GetGlobal { k: a, throws: true }, opcode::GetGlobalOrUndefined => Ins.GetGlobal { k: a, throws: false },
        opcode::PutGlobal, opcode::DeclareGlobalVar, opcode::DeclareGlobalFunction, opcode::DeclareGlobalLet, opcode::DeclareGlobalConst, opcode::InitGlobalLexical, opcode::CheckGlobalVar, opcode::CheckGlobalFunction, opcode::DeleteGlobal, opcode::DeclareGlobalConstValue, opcode::CheckGlobalLexicalConflict, opcode::PutGlobalStrict => Ins.GlobalOp { op: op.code as u8, k: a },
        opcode::ThrowError => Ins.ThrowError { kind: if op.arg >= 0 && op.arg < export_selector_count(opcode::ThrowError) { op.arg as u8 } else { 255 } },
        opcode::NewFormList => Ins.Literal { kind: node_kind::List, count: a }, opcode::NewFormVector => Ins.Literal { kind: node_kind::Vector, count: a },
        opcode::NewFormMap => Ins.Literal { kind: node_kind::Map, count: a * 2 }, opcode::NewFormSet => Ins.Literal { kind: node_kind::Set, count: a },
        opcode::FormOp => Ins.Form { op: a },
        opcode::PushFinally => Ins.PushFinally { to: a }, opcode::PopFinally => Ins.PopFinally, opcode::EndFinally => Ins.EndFinally, opcode::CallFinally => Ins.CallFinally, opcode::DropCompletion => Ins.DropCompletion,
        opcode::PendingThrow, opcode::PendingException, opcode::PendingCancel => Ins.Pending { what: op.code as u8 },
        opcode::Typeof => Ins.TypeOf, opcode::ToString => Ins.ToString, opcode::ForInStart => Ins.ForInStart, opcode::ForInNext => Ins.ForInNext, opcode::OwnKeys => Ins.OwnKeys, opcode::IsEnumerable => Ins.IsEnumerable, opcode::CopyDataProperties => Ins.CopyData, opcode::ToPropertyKey => Ins.ToPropertyKey, opcode::RequireCoercible => Ins.RequireCoercible,
        opcode::GeneratorStart => Ins.GeneratorStart, opcode::GeneratorInitialYield => Ins.GeneratorInitialYield, opcode::GeneratorYield => Ins.GeneratorYield, opcode::AsyncStart => Ins.AsyncStart, opcode::Await => Ins.Await,
        opcode::GetNewTarget => Ins.NewTarget, opcode::ToObject => Ins.ToObject, opcode::Instanceof => Ins.Instanceof,
        opcode::GetIterator => Ins.GetIterator, opcode::RequireObject => Ins.RequireObject, opcode::AppendHole => Ins.AppendHole,
        opcode::IsCell => Ins.IsCell, opcode::IsObject => Ins.IsObject, opcode::CheckSloppyThis => Ins.SloppyThis, opcode::SetComputedName => Ins.ComputedName { k: a },
        opcode::TemplateObject => Ins.TemplateObject { count: a },
        opcode::DefineClassMethod => Ins.DefineClass { half: 0 }, opcode::DefineClassGetter => Ins.DefineClass { half: 1 }, opcode::DefineClassSetter => Ins.DefineClass { half: 2 },
        opcode::CheckHeritage => Ins.CheckHeritage, opcode::InitHeritage => Ins.InitHeritage, opcode::GetSuperConstructor => Ins.SuperConstructor,
        opcode::ConstructSuper => Ins.ConstructSuper { argc: a, spread: false }, opcode::ConstructSuperSpread => Ins.ConstructSuper { argc: 0, spread: true },
        opcode::BindThis => Ins.BindThis, opcode::ForwardArguments => Ins.Rest { skip: 0 },
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
        let at = l.functions + i * function_row::Bytes; let flags = program::word(bytes, at + 48);
        code.functions.push(Function { start: program::word(bytes, at) as u32, count: program::word(bytes, at + 4) as u32, constants: program::word(bytes, at + 8) as u32, children: program::word(bytes, at + 16) as u32, params: program::word(bytes, at + 24) as u32, locals: program::word(bytes, at + 28) as u32, captures: program::word(bytes, at + 32) as u32, strict: (flags & 1) == 1, kind: (flags >> 8) & 255, generator: ((flags >> 16) & 1) == 1, asynchronous: ((flags >> 17) & 1) == 1, name_start: program::word(bytes, at + 36) as u32, name_len: program::word(bytes, at + 40) as u32, length: program::word(bytes, at + 44) as u32, has_text: (flags & function_row::SourceText) != 0, text_start: program::word(bytes, at + 52) as u32, text_end: program::word(bytes, at + 56) as u32, simple: (flags & function_row::SimpleParameters) != 0 });
    }
    for i in program::word(bytes, 16) {
        let at = l.constants + i * 12; let tag = program::word(bytes, at);
        var value: Value = Value.Undefined;
        // Symbol keeps its well-known index. No BigInt operation is
        // implemented, so its validated digits stay in the image; `Const`
        // fails Unsupported before either value reaches a JavaScript value.
        match tag {
            constant::Number => { let n, ok = binary::read_f64le(bytes, at + 4); assert(ok); value = number(n); },
            constant::Symbol => { value = Value.Symbol { s: program::word(bytes, at + 4) as u32 }; },
            constant::BigInt => { value = Value.BigInt { b: 0 }; },
            constant::Null => { value = Value.Null; },
            constant::Bool => { value = boolean(program::word(bytes, at + 4) != 0); },
            constant::String => { code.strings.push(Str { start: program::word(bytes, at + 4) as u32, len: program::word(bytes, at + 8) as u32 }); value = Value.String { s: (code.strings.len - 1) as u32 }; },
            _ => {},
        }
        code.constants.push(value);
    }
    for i in program::word(bytes, 20) { code.children.push(program::word(bytes, l.children + i * 4) as u32); }
    for i in program::word(bytes, 24) {
        let at = l.instructions + i * 20;
        code.ops.push(Op { code: program::word(bytes, at) & 255, arg: program::word(bytes, at + 4), offset: program::word(bytes, at + 16), start: program::word(bytes, at + 8), end: program::word(bytes, at + 12), direct_eval: (program::word(bytes, at) >> 24) == 1 });
    }
    for i in program::word(bytes, 28) { let at = l.units + i * 2; code.units.push(bytes[at] | ((bytes[at + 1] as u16) << 8)); }
    for i in program::word(bytes, l.tail) {
        let at = l.sites + i * 20;
        code.sites.push(Site { op: program::word(bytes, at) as u32, sources: program::word(bytes, at + 4) as u32, count: program::word(bytes, at + 8) as u32, record: program::word(bytes, at + 12) as u32, length: program::word(bytes, at + 16) as u32 });
    }
    for i in program::word(bytes, l.tail + 4) {
        let at = l.sources + i * 12; let role = program::word(bytes, at + 8);
        code.site_sources.push(SiteSource { kind: program::word(bytes, at) as u8, role: (role & 255) as u8, cell: (role & eval_role::Cell) != 0, index: program::word(bytes, at + 4) as u32 });
    }
    code.site_records.append(bytes[l.records..l.end]);
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
            if next_flow == flow::End { continue; }
            for edge in (if next_flow == flow::Branch || next_flow == flow::Catch { 2 } else { 1 }) {
                let next = if next_flow != flow::Next && edge == 0 { op.arg } else { pc + 1 };
                let expected = after + (if next_flow == flow::Catch && edge == 0 { 1 } else { 0 });
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
    for f in other.functions { var n = f; n.start += ops as u32; n.constants += constants as u32; n.children += children as u32; n.name_start += units as u32; if n.has_text { n.text_start += bytes as u32; n.text_end += bytes as u32; } out.functions.push(n); }
    for op in other.ops { var n = op; n.start += bytes; n.end += bytes; if branches(n.code) { n.arg += ops; } out.ops.push(n); }
    let strings = out.strings.len;
    for s in other.strings { out.strings.push(Str { start: s.start + units as u32, len: s.len }); }
    for v in other.constants { out.constants.push(if v.is_string() { Value.String { s: (v.sid() + strings) as u32 } } else { v }); }
    for id in other.children { out.children.push((functions + id) as u32); }
    // Sites stay in ascending op order: the appended ops are above.
    let sources = out.site_sources.len; let records = out.site_records.len;
    for s in other.sites { var n = s; n.op += ops as u32; n.sources += sources as u32; n.record += records as u32; out.sites.push(n); }
    out.site_sources.append(other.site_sources); out.site_records.append(other.site_records);
    out.units.append(other.units); out.source_bytes.append(other.source_bytes); decode(out); out.valid = true;
    return functions;
}
// The direct-eval site of instruction row `op` (Code.sites, ascending), or -1.
fn site_at(code: const Code&, op: i64) -> i64 {
    var lo: i64 = 0; var hi = code.sites.len;
    while lo < hi { let mid = lo + (hi - lo) / 2; if code.sites[mid].op < op { lo = mid + 1; } else { hi = mid; } }
    return if lo < code.sites.len && code.sites[lo].op == op { lo } else { -1 };
}
fn memory(heap: Heap&) -> bool { heap.fault = Fault.MemoryLimit; return false; }
// The heap's table lengths, as the image tops a restore or reset sets.
fn tops(heap: const Heap&) -> Tops { return Tops { nodes: heap.nodes.len, values: heap.values.len, properties: heap.properties.len, units: heap.units.len, strings: heap.strings.len, latin1: latin1_len(heap), shapes: heap.shapes.len, accessors: heap.accessors.len, conts: heap.conts.len }; }
// The Latin-1 table's rows: its bytes but a collection's leftovers (Heap.latin1_spare).
inline fn latin1_len(heap: const Heap&) -> i64 { return heap.latin1.len - heap.latin1_spare; }
fn valid(heap: Heap&, v: Value) -> bool {
    if heap.fault != Fault.None { return false; }
    return match v {
        // A well-known symbol (the realm's Symbol properties) or a symbol row.
        Symbol x => if symbol_payload(heap.nodes, x.s) { true } else { heap.fault = if x.s - symbol_floor() >= heap.nodes.len { Fault.StaleID } else { Fault.InvalidState }; false },
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
// their tables have no job limit. While a job runs, the check compares
// against the limit less the table's margin (Heap.slack); past that, the
// cold path asks for a collection inside the job (reclaim.goose, exhausted).
fn room(heap: Heap&, length: i64, top: i64, limit: i64, slack: i64&, count: i64) -> bool {
    if count < 0 || count > limit - slack - (length - top) { return exhausted(heap, length - top, limit, slack, count); }
    return true;
}
// `nodes` then `extra` copies of `fill`; returns the first row, or -1.
fn add_nodes(heap: Heap&, nodes: const Node[:], extra: i64 = 0, fill: Node = Node { kind: node_kind::Object }) -> i64 {
    if extra < 0 || !room(heap, heap.nodes.len, heap.image.nodes, heap.limits.nodes, heap.slack.nodes, nodes.len + extra) { return -1; }
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
    if extra < 0 || !room(heap, heap.values.len, heap.image.values, heap.limits.values, heap.slack.values, values.len + extra) { return -1; }
    let at = heap.values.len; heap.values.append(values); for i in extra { heap.values.push(fill); }
    return at;
}
// Whether `count` more value rows fit, for preflights that must fail before
// any other table changes.
fn values_fit(heap: const Heap&, count: i64) -> bool { return count >= 0 && count <= heap.limits.values - (heap.values.len - heap.image.values); }
fn nodes_fit(heap: const Heap&, count: i64) -> bool { return count >= 0 && count <= heap.limits.nodes - (heap.nodes.len - heap.image.nodes); }
// Code-unit rows the heap added above its image, in both unit tables: the
// unit limit counts them together.
fn unit_rows(heap: const Heap&) -> i64 { return heap.units.len - heap.image.units + latin1_len(heap) - heap.image.latin1; }
fn units_fit(heap: const Heap&, count: i64) -> bool { return count >= 0 && count <= heap.limits.units - unit_rows(heap); }
// Take node, value, code-unit and string rows of the heap's headroom
// without adding them (light-cljs-820: the entries a narrowed argument
// graph left out, bridge::Omitted). The limits count rows above the image
// tops, so lowered tops count these as rows the job added, in every check,
// until a restore, extend or reset sets the tops again (snapshot), as it
// does for the rows the job did add.
fn reserve_rows(heap: Heap&, nodes: i64, values: i64, units: i64, strings: i64) {
    assert(nodes >= 0 && values >= 0 && units >= 0 && strings >= 0);
    heap.image.nodes -= nodes; heap.image.values -= values; heap.image.latin1 -= units; heap.image.strings -= strings;
}
fn add_units(heap: Heap&, units: const u16[:]) -> i64 {
    if !room(heap, unit_rows(heap), 0, heap.limits.units, heap.slack.units, units.len) { return -1; }
    let at = heap.units.len; heap.units.append(units); return at;
}
fn add_latin1(heap: Heap&, bytes: const u8[:]) -> i64 {
    if !room(heap, unit_rows(heap), 0, heap.limits.units, heap.slack.units, bytes.len) { return -1; }
    if heap.latin1_spare > 0 { return reuse_latin1(heap, bytes); }
    let at = heap.latin1.len; heap.latin1.append(bytes); return at;
}
// The string header table's one allocation function, over a run already in
// its unit table (a view, a copied image range or new units): `wide` names
// heap.units, otherwise heap.latin1. Callers keep the encoding canonical.
// Header rows share the unit limit: an empty string still takes one.
fn string_ref(heap: Heap&, start: i64, len: i64, wide: bool) -> Value {
    let length = if wide { heap.units.len } else { latin1_len(heap) };
    if start < 0 || len < 0 || start > length || len > length - start { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if !room(heap, heap.strings.len, heap.image.strings, heap.limits.units, heap.slack.strings, 1) { return Value.Undefined; }
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
    for v in values { heap.constants.push(if v.is_string() { intern(heap, v) } else { v }); }
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
    if k.is_symbol() { return k; }
    if !k.is_string() { if heap.fault == Fault.None { heap.fault = Fault.InvalidState; } return Value.Undefined; }
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
    if !c.is_string() { return Value.Undefined; }
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
    return match a {
        Undefined, Null => true, Bool => a.flag() == b.flag(), Number => a.num() == b.num(),
        String => { if !charge(heap, str_len(heap, a) + str_len(heap, b)) { return false; } same_text(heap, a, b) },
        Object => a == b || same_keyword(heap, a.id(), b.id()),
        _ => a == b,
    };
}
// Whether object rows a and b are one keyword (light-cljs-878). The V8
// build interns keyword literals and the keywords of imported forms: one
// object per namespace and name, whichever unit, job, publication or cell
// it came from (a fresh one only from cljs.core/keyword, which fails
// NativeBoundary here). Every keyword row this VM makes is of that kind
// (form_op::Keyword, the form bridge, publications, cell values), and a
// keyword row is immutable, with no own properties or metadata, so its
// namespace and name are its identity: ===, Object.is and identical?
// compare two keyword rows by them, and nothing JavaScript does can tell
// two such rows apart. There is no intern index, so no table or job state
// has to follow the worker reset, reclamation, snapshots or continuations
// (decisions 2 and 3), and a keyword arriving in a publication or a cell
// needs no remapping. A runtime keyword constructor added here must mark
// its keywords and keep them out of this rule.
fn same_keyword(heap: Heap&, a: i64, b: i64) -> bool {
    if node(heap, a).kind != node_kind::Keyword || node(heap, b).kind != node_kind::Keyword { return false; }
    let an = node(heap, a).value; let bn = node(heap, b).value; let ans = node(heap, a).namespace_value; let bns = node(heap, b).namespace_value;
    if !charge(heap, str_len(heap, an) + str_len(heap, ans)) { return false; }
    if !same_text(heap, an, bn) || ans.is_string() != bns.is_string() { return false; }
    return !ans.is_string() || same_text(heap, ans, bns);
}
// ECMAScript ToNumber of a primitive (light-cljs-827; the Pants engine's
// vm/dispatch.rs to_number): undefined NaN, null +0, a boolean 0 or 1, a
// string by StringToNumber. No user code runs (decision 5). A symbol is a
// TypeError, a BigInt fails Unsupported (the BigInt area, light-cljs-771)
// and an object UnsupportedOperand (ToPrimitive runs before this in the
// callers that have one). A scalar form (node_kind::Scalar) converts as its primitive.
fn numeric(heap: Heap&, v: Value) -> f64 {
    if !valid(heap, v) { return 0; }
    let x = form_value(heap, v);
    return match x {
        Undefined => 0.0 / 0.0, Null => 0.0, Bool => if x.flag() { 1.0 } else { 0.0 }, Number => x.num(), String => string_to_number(heap, x),
        Symbol => { heap.fault = Fault.TypeError; 0.0 }, BigInt => { heap.fault = Fault.Unsupported; 0.0 }, _ => { heap.fault = Fault.UnsupportedOperand; 0.0 },
    };
}
// StrWhiteSpaceChar (ECMAScript 7.1.4.1.1; the Pants engine's
// value/string.rs is_str_white_space): WhiteSpace (TAB, VT, FF, U+FEFF and
// the Zs characters) or a LineTerminator. All are BMP, so one unit decides.
fn str_white_space(u: u16) -> bool {
    return u >= 0x09 && u <= 0x0D || u == 0x20 || u == 0xA0 || u == 0x1680 || u >= 0x2000 && u <= 0x200A || u == 0x2028 || u == 0x2029 || u == 0x202F || u == 0x205F || u == 0x3000 || u == 0xFEFF;
}
// StringToNumber of a string value: StrWhiteSpace trimmed from both ends,
// then number::parse_literal (number_text.goose) of what remains, which must
// be ASCII (a unit above 127 is NaN). The scan is charged to the work
// budget, one unit a code unit.
fn string_to_number(heap: Heap&, s: Value) -> f64 {
    let n = str_len(heap, s);
    if !charge(heap, n) { return 0; }
    var start: i64 = 0; while start < n && str_white_space(unit(heap, s, start)) { start += 1; }
    var end = n; while end > start && str_white_space(unit(heap, s, end - 1)) { end -= 1; }
    let h = header(heap, s.sid());
    if !h.wide {
        let run = narrow_run(heap, h)[start..end];
        for c in run { if c > 127 { return 0.0 / 0.0; } }
        return number::parse_literal(run);
    }
    var text: u8[>..] = [];
    var i = start; while i < end { let u = unit(heap, s, i); if u > 127 { return 0.0 / 0.0; } text.push(u as u8); i += 1; }
    return number::parse_literal(text);
}
// A property key (light-cljs-787): a string or a symbol, whose values are
// valid (light-cljs-885).
inline fn key_valid(heap: Heap&, k: Value) -> bool { return valid(heap, k); }
// ToPropertyKey of a primitive (light-cljs-826; the Pants engine's
// vm/property.rs to_property_key): a string or symbol is its own key and any
// other primitive its ToString, so `-0` gives "0", 1.5 "1.5", true "true" and
// null "null"; an index number takes the integer spelling directly. An
// object key converts before this, in the dispatch loop's Convert frame
// (light-cljs-828); one that reaches it fails UnsupportedOperand.
fn key(heap: Heap&, value: Value) -> Value {
    if !key_valid(heap, value) { return Value.Undefined; }
    if value.is_string() || value.is_symbol() { return value; }
    if value.is_number() && value.num() >= 0 && value.num() < 4294967295.0 && value.num() == (value.num() as i64) { let bytes = str(value.num() as i64); return text(heap, bytes); }
    if value.is_object() { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
    return to_string(heap, value);
}
// ECMAScript ToString of a primitive other than a string, as the Latin-1
// bytes it appends to out (light-cljs-826; the Pants engine's
// vm/dispatch.rs to_js_string): "undefined", "null", "true" or "false", and
// Number::toString in radix 10 (number_text.goose). No user code runs
// (decision 5). A symbol is a TypeError; a BigInt fails Unsupported (the
// BigInt area, light-cljs-771) and an object UnsupportedOperand (its
// ToPrimitive runs before this, in the callers that have one).
fn primitive_text(heap: Heap&, v: Value, out: u8[>..]&) -> bool {
    match v {
        Undefined => { out.append("undefined"); },
        Null => { out.append("null"); },
        Bool b => { out.append(if b.v { "true" } else { "false" }); },
        Number n => { number::append_decimal(out, n.v); },
        Symbol => { heap.fault = Fault.TypeError; return false; },
        BigInt => { heap.fault = Fault.Unsupported; return false; },
        Object => { heap.fault = Fault.UnsupportedOperand; return false; },
        _ => { heap.fault = Fault.InvalidState; return false; },
    }
    return true;
}
// ToString of a primitive as a string value: a string itself, otherwise a
// new Latin-1 string of primitive_text's bytes, charged to the unit limit.
fn to_string(heap: Heap&, v: Value) -> Value {
    if !valid(heap, v) { return Value.Undefined; }
    if v.is_string() { return v; }
    var bytes: u8[>..] = [];
    if !primitive_text(heap, v, bytes) { return Value.Undefined; }
    return text(heap, bytes);
}
// `+` with a string operand: the ToString of both operands, concatenated.
// A string side keeps its units; any other primitive converts to ASCII
// (primitive_text). The result is Latin-1 unless a string side is wide.
fn concat_mixed(heap: Heap&, a: Value, b: Value) -> Value {
    if !valid(heap, a) || !valid(heap, b) { return Value.Undefined; }
    var left: u8[>..] = []; var right: u8[>..] = [];
    if !a.is_string() && !primitive_text(heap, a, left) { return Value.Undefined; }
    if !b.is_string() && !primitive_text(heap, b, right) { return Value.Undefined; }
    let n = (if a.is_string() { str_len(heap, a) } else { left.len }) + (if b.is_string() { str_len(heap, b) } else { right.len });
    if !units_fit(heap, n) { memory(heap); return Value.Undefined; }
    if a.is_string() && header(heap, a.sid()).wide || b.is_string() && header(heap, b.sid()).wide {
        var units: u16[>..] = [];
        if a.is_string() { append_utf16(heap, a, units); } else { for c in left { units.push(c); } }
        if b.is_string() { append_utf16(heap, b, units); } else { for c in right { units.push(c); } }
        return string(heap, units);
    }
    var bytes: u8[>..] = [];
    if a.is_string() { bytes.append(narrow_run(heap, header(heap, a.sid()))); } else { bytes.append(left); }
    if b.is_string() { bytes.append(narrow_run(heap, header(heap, b.sid()))); } else { bytes.append(right); }
    return text(heap, bytes);
}
fn key_is(heap: Heap&, value: Value, bytes: const u8[:]) -> bool {
    if !value.is_string() || str_len(heap, value) != bytes.len || !charge(heap, bytes.len) { return false; }
    let s = header(heap, value.sid());
    if !s.wide { return narrow_run(heap, s) == bytes; }
    let units = wide_run(heap, s);
    for byte, i in bytes { if units[i] != byte { return false; } } return true;
}
fn index(heap: Heap&, k: Value) -> i64 {
    if !k.is_string() { return -1; }
    // An array index is decimal digits: always Latin-1.
    let s = header(heap, k.sid());
    if s.wide || s.len == 0 || s.len > 10 { return -1; }
    if !charge(heap, s.len) { return -1; }
    let digits = narrow_run(heap, s);
    var n: i64 = 0;
    for c, i in digits { if c < 48 || c > 57 || (i == 0 && c == 48 && digits.len > 1) { return -1; } n = n * 10 + (c - 48); }
    return if n < 4294967295 { n } else { -1 };
}
// Arrays (light-cljs-785). A dense array (node_kind::Array) keeps its elements in its
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
    return if v.is_number() && v.num() >= 0 && v.num() < 4294967295.0 && v.num() == (v.num() as i64) { v.num() as i64 } else { -1 };
}
// The interned key of array index i.
fn index_key(heap: Heap&, i: i64) -> Value { let k = key(heap, number(i)); return if heap.fault == Fault.None { intern(heap, k) } else { Value.Undefined }; }
fn own_element(heap: Heap&, array: Value, i: i64) -> Value, bool {
    if node(heap, array.id()).initialized {
        let k = index_key(heap, i);
        if heap.fault != Fault.None { return Value.Undefined, false; }
        let p = find_own(heap, array.id(), k);
        // An accessor element (light-cljs-881) has no value here: the
        // callers' general steps read it through [[Get]].
        if p.found && is_accessor(p.value) { return Value.Undefined, false; }
        return p.value, p.found;
    }
    // Field reads: a whole Node copy would dominate an element access.
    if i >= node(heap, array.id()).length || i >= node(heap, array.id()).count { return Value.Undefined, false; }
    let v = slot(heap, node(heap, array.id()).start + i);
    if v.is_hole() { return Value.Undefined, false; }
    return v, true;
}
// More than doubling the run would leave most of its slots holes.
fn sparse_write(count: i64, i: i64) -> bool { return i >= count && i > 2 * count + 1024; }
fn set_element(heap: Heap&, array: Value, i: i64, value: Value) -> bool {
    // A built-in's CreateDataPropertyOrThrow into a frozen array, or of a
    // new element of a non-extensible one (light-cljs-882); assignment and
    // definition refuse these before they get here.
    if node(heap, array.id()).non_extensible {
        let v, found = own_element(heap, array, i);
        if heap.fault != Fault.None { return false; }
        if !found || frozen_array(heap, array.id()) { heap.fault = Fault.TypeError; return false; }
    }
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
    if !k.is_string() { return -1; }
    let s = header(heap, k.sid());
    if s.wide || s.len == 0 || s.len > 10 { return -1; }
    let c = narrow_run(heap, s)[0];
    if c < 48 || c > 57 { return -1; }
    return index(heap, k);
}
// Whether objects of this kind keep their index-keyed data properties in an
// element run as an ordinary object does: ordinary and arguments objects
// (light-cljs-854). Arrays have their own element rules.
checkfree inline fn element_kind(kind: i64) -> bool { return kind == node_kind::Object || kind == node_kind::Arguments; }
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
    // A sealed or frozen array's elements keep their attributes as records.
    let attrs = element_attrs(node(heap, o)); let integrity = node(heap, o).function & (array_flag::Sealed | array_flag::Frozen);
    let named: i64 = node(heap, o).key_count;
    var records: i64 = 0; for i in end { if !slot(heap, start + i).is_hole() { records += 1; } }
    let capacity = max(4, records + named);
    let run = add_properties(heap, capacity);
    if run < 0 { return false; }
    // Records are built aside: the header changes only after every failure point.
    var built: Property[>..] = [];
    for i in end {
        let v = slot(heap, start + i);
        if v.is_hole() { continue; }
        let k = index_key(heap, i);
        if heap.fault != Fault.None { return false; }
        built.push(Property { key: k, value: v, attrs: attrs });
    }
    let base = built.len; built.resize(base + named, Property {});
    var s: i64 = node(heap, o).shape;
    while s != 0 { let e = shape_row(heap, s); built[base + e.count - 1] = Property { key: e.key, value: slot_value(heap, o, e.count - 1), attrs: e.attrs }; s = e.parent; }
    if !charge(heap, count + named) { return false; }
    for row in start..(start + count) { set_slot(heap, row, Value.Undefined); }
    for i in named { set_slot_value(heap, o, i, Value.Undefined); }
    node_mut(heap, o).keys = run as u32; node_mut(heap, o).key_count = built.len as u32; node_mut(heap, o).key_capacity = capacity as u32; node_mut(heap, o).initialized = true; node_mut(heap, o).count = 0; node_mut(heap, o).shape = 0;
    if integrity != 0 { node_mut(heap, o).function = (node(heap, o).function & ~integrity) | array_flag::Attributed; }
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
// `match`, where the exotic kinds (arrays and their `length`, functions'
// `call`) override the ordinary ones; a kind whose
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
// What a definition (define_property, define_own) gives: a data property
// (Data), an accessor's getter or setter merged with an existing pair's other
// function (Getter, Setter; an object literal's DefineGetter and
// DefineSetter), or a complete property ValidateAndApplyPropertyDescriptor
// has already checked (Complete, light-cljs-881; define_descriptor).
let half::Data: i64 = 0;
let half::Getter: i64 = 1;
let half::Setter: i64 = 2;
let half::Complete: i64 = 3;
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
// Shape and accessor rows count together against limits.shapes (Limits).
fn shape_room(heap: Heap&, count: i64) -> bool {
    return room(heap, heap.shapes.len + heap.accessors.len, heap.image.shapes + heap.image.accessors, heap.limits.shapes, heap.slack.shapes, count);
}
// The one allocation point of the property table: `count` cleared rows at
// its end, charged to the rows the heap has added above its image
// (limits.properties counts allocated rows, spare capacity included, not live
// records; property_rows is their running total for snapshot validation).
// Returns the run's first row, or -1 with MemoryLimit.
fn add_properties(heap: Heap&, count: i64) -> i64 {
    if !room(heap, heap.properties.len, heap.image.properties, heap.limits.properties, heap.slack.properties, count) { return -1; }
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
    if node(heap, o).kind == node_kind::Array { return array_to_dictionary(heap, object(o)); }
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
// SetIntegrityLevel's attributes (light-cljs-882, integrity.goose): attrs
// not configurable and, `frozen`, a data property's not writable either.
inline fn restricted(attrs: u8, accessor: bool, frozen: bool) -> u8 {
    let a = attrs & ~attr_configurable();
    return if frozen && !accessor { a & ~attr_writable() } else { a };
}
// Give every property of shape-mode object o restricted attributes: a new
// chain of transitions from the empty shape with the same keys in the same
// order, so each value keeps its slot and objects sealed or frozen alike
// share shapes. A rejection leaves the object as it was.
fn restrict_shape(heap: Heap&, o: i64, frozen: bool) -> bool {
    var chain: Shape[>..] = [];
    var s:     i64        = node(heap, o).shape;
    while s != 0 { chain.push(shape_row(heap, s)); s = shape_row(heap, s).parent; }
    if !charge(heap, chain.len) { return false; }
    var at: i64 = 0; var i = chain.len;
    while i > 0 {
        i -= 1;
        let e = chain[i];
        at = transition(heap, at, e.key, restricted(e.attrs, is_accessor(slot_value(heap, o, e.count - 1)), frozen));
        if at < 0 { return false; }
    }
    node_mut(heap, o).shape = at as u32;
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
    if a.is_number() && b.is_number() && a.num() != a.num() && b.num() != b.num() { return true; }
    if a.is_string() && b.is_string() { return same_text(heap, a, b); }
    return same_bits(a, b) || a.is_object() && b.is_object() && a.id() < heap.nodes.len && b.id() < heap.nodes.len && same_keyword(heap, a.id(), b.id());
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
// a hole only once that property exists. With `half` 3 (complete, from
// Object.defineProperty and its relatives, light-cljs-881) `value` is the
// property's whole new state, a data value or an Accessor naming its pair,
// which define_descriptor has already validated against the current one.
fn define_own(heap: Heap&, o: i64, k: Value, value: Value, attrs: u8, half: i64 = 0) -> bool {
    let i = if element_kind(node(heap, o).kind) { element_key(heap, k) } else { -1 };
    if heap.fault != Fault.None { return false; }
    let data = (half == half::Data || half == half::Complete && !is_accessor(value)) && attrs == attrs_data();
    let merge = half == half::Getter || half == half::Setter;
    if i >= 0 && !object_element(heap, o, i).is_hole() {
        if data { set_slot(heap, node(heap, o).start + i, value); return true; }
        var v = value;
        if merge {
            let a = add_accessor(heap, if half == half::Getter { Pair { get: value } } else { Pair { set: value } });
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
    if merge {
        var fns = if p.found && is_accessor(p.value) { pair(heap, p.value) } else { Pair {} };
        if half == half::Getter { fns.get = value; } else { fns.set = value; }
        // A non-configurable accessor accepts only its own function again,
        // with the same attributes (SameValue of the half), changing nothing.
        if p.found && p.attrs & attr_configurable() == 0 {
            let old = pair(heap, p.value);
            return is_accessor(p.value) && attrs == p.attrs && same_bits(if half == half::Getter { old.get } else { old.set }, value);
        }
        let a = add_accessor(heap, fns);
        if a < 0 { return false; }
        v = Value.Accessor { a: a as u32 };
    }
    if !p.found { return add_own(heap, o, k, v, attrs); }
    if half != half::Complete && p.attrs & attr_configurable() == 0 {
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
// engine's rust/src/vm/function.rs). A closure (node_kind::Closure) keeps its properties
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
// `constructor`. A class constructor (constructor kind 2 to 4,
// light-cljs-886) makes all three when it is made (closure), never lazily:
// its `prototype` is a new ordinary object with that `constructor`, and is
// neither writable, enumerable nor configurable. Not implemented, failing
// UnsupportedOperand by name: an async generator's `prototype`, and
// therefore its key list. A computed-key literal function's name
// (SetComputedName, light-cljs-855) makes its properties at once, with
// that name (set_computed_name). Built-ins
// (node_kind::Builtin) have their own `length` and `name` from the realm (light-cljs-788).
fn constructor_kind(heap: const Heap&, o: i64) -> i64 { return heap.code.functions[node(heap, o).function].kind; }
fn lazy_function(heap: const Heap&, o: i64) -> bool {
    let n .= node(heap, o);
    return n.kind == node_kind::Closure && n.key_count == 0 && !n.initialized;
}
// Whether closure o's code gives it an own `prototype`: an ordinary
// function's, a class constructor's or a (synchronous) generator's.
fn has_prototype(heap: const Heap&, o: i64) -> bool { let f .= heap.code.functions[node(heap, o).function]; return f.kind != constructor::None || f.generator && !f.asynchronous; }
fn lazy_prototype(heap: const Heap&, o: i64) -> bool { return lazy_function(heap, o) && has_prototype(heap, o); }
// Whether closure o's `prototype` is one this VM does not make (an async
// generator's).
fn unsupported_prototype(heap: const Heap&, o: i64) -> bool { let f .= heap.code.functions[node(heap, o).function]; return f.generator && f.asynchronous; }
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
    if !k.is_string() { return true; }
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
    let n = function_name(heap, o);
    return heap.fault == Fault.None && materialize_named(heap, o, n);
}
// SetComputedName (light-cljs-855; rust/src/vm/dispatch.rs
// computed_name_step): key f → key f, where f is the fresh closure of a
// literal's computed-key method, getter or setter and key its property key
// (ToPropertyKey's: a string, a symbol or an index). Its `name` is the
// constant prefix ("", "get " or "set ") and the key's spelling, a
// well-known symbol's bracketed description (`[Symbol.unscopables]`); its
// properties are made now, with that name. A class constructor, made with
// its properties (light-cljs-886), is named by a computed key of the
// literal holding the class (`{ [k]: class {} }`) before any element is
// defined: its `name` value is replaced.
fn set_computed_name(job: Job&, prefix: Value) {
    let heap .= job.heap;
    let f = job.operands[job.operands.len - 1]; let k = job.operands[job.operands.len - 2];
    if !f.is_object() || !valid(heap, f) || !prefix.is_string() { heap.fault = Fault.InvalidState; return; }
    let is_class = node(heap, f.id()).kind == node_kind::Closure && constructor_kind(heap, f.id()) >= constructor::BaseClass && !lazy_function(heap, f.id());
    if !is_class && !lazy_function(heap, f.id()) { heap.fault = Fault.InvalidState; return; }
    var units: u16[>..] = [];
    append_utf16(heap, prefix, units);
    // A symbol's description in brackets, nothing for an undefined one.
    if k.is_symbol() { var d: u16[>..] = []; if append_description(heap, k, d) { units.push(91); units.append(d[..]); units.push(93); } }
    else if k.is_string() { append_utf16(heap, k, units); }
    else if k.is_number() { let s = to_string(heap, k); if heap.fault != Fault.None { return; } append_utf16(heap, s, units); }
    else { heap.fault = Fault.InvalidState; return; }
    if !units_fit(heap, units.len) { memory(heap); return; }
    let n = string(heap, units);
    if heap.fault != Fault.None { return; }
    if !is_class { materialize_named(heap, f.id(), n); return; }
    let name = key_named(heap, "name");
    if heap.fault != Fault.None { return; }
    let p = find_own(heap, f.id(), name);
    if heap.fault != Fault.None { return; }
    if !p.found || is_accessor(p.value) { heap.fault = Fault.InvalidState; return; }
    write_own(heap, f.id(), p.index, n);
}
fn materialize_named(heap: Heap&, o: i64, n: Value) -> bool {
    let length = key_named(heap, "length"); let name = key_named(heap, "name");
    if heap.fault != Fault.None || !add_own(heap, o, length, function_length(heap, o), attr_configurable()) || !add_own(heap, o, name, n, attr_configurable()) { return false; }
    if !has_prototype(heap, o) { return true; }
    let constructor = key_named(heap, "constructor"); let prototype = key_named(heap, "prototype");
    if heap.fault != Fault.None { return false; }
    // An ordinary function's or a class's; otherwise a generator's.
    let kind = constructor_kind(heap, o); let ordinary = kind != constructor::None;
    let p = allocate(heap, if ordinary { Node { kind: node_kind::Object } } else { Node { kind: node_kind::Object, proto: object(generator_prototype_row()) } });
    if heap.fault != Fault.None || ordinary && !add_own(heap, p.id(), constructor, object(o), attr_writable() | attr_configurable()) { return false; }
    return add_own(heap, o, prototype, p, if kind >= constructor::BaseClass { 0 } else { attr_writable() });
}
// Primitive wrapper objects (light-cljs-832; decision 2): node_kind::Wrapper, whose
// `value` is the number, string or boolean it wraps ([[NumberData]],
// [[StringData]], [[BooleanData]]) and whose own properties are an ordinary
// object's, without an element run. A String object is exotic
// (ECMAScript 10.4.3): its `length` (non-writable, non-enumerable,
// non-configurable) and each index below it (an enumerable, non-writable,
// non-configurable one-unit string) are own properties read from the
// string, never stored. string_key: whether interned key k is one of them,
// and which (-1 for `length`, else the index).
fn string_key(heap: Heap&, o: i64, k: Value) -> bool, i64 {
    let s = node(heap, o).value;
    if !s.is_string() || !k.is_string() { return false, 0; }
    if key_is(heap, k, "length") { return true, -1; }
    let i = index(heap, k);
    return i >= 0 && i < str_len(heap, s), i;
}
// [[GetOwnProperty]] by object kind (interned key k): ordinary and arguments
// objects their element runs and shapes or records; arrays their writable, non-enumerable, non-configurable
// `length`, their elements and their named properties (light-cljs-805: shape
// keys of a dense array, records of a dictionary array); closures their own properties (see
// above) and, failing those, `call` (Function.prototype's, until the realm),
// built-ins only `call`; primitive wrappers (node_kind::Wrapper) their ordinary
// properties and, for a String object, its exotic `length` and indices
// first (string_key). Every other kind fails UnsupportedOperand: it is
// not implemented yet.
fn own_entry(heap: Heap&, object: Value, k: Value) -> Own {
    let o: i64 = object.id();
    match node(heap, o).kind {
        node_kind::Wrapper => {
            let exotic, i = string_key(heap, o, k);
            if heap.fault != Fault.None { return Own {}; }
            if !exotic { return find_own(heap, o, k); }
            let s = node(heap, o).value;
            if i < 0 { return Own { found: true, value: number(str_len(heap, s)), attrs: 0 }; }
            return Own { found: true, index: i, value: unit_string(heap, s, i), attrs: attr_enumerable() };
        },
        node_kind::Object, node_kind::Arguments, node_kind::Error, node_kind::Generator, node_kind::Promise, node_kind::Iterator => {
            if node(heap, o).count > 0 {
                let i = element_key(heap, k);
                if heap.fault != Fault.None { return Own {}; }
                let v = object_element(heap, o, i);
                if !v.is_hole() { return Own { found: true, index: i, value: v, attrs: attrs_data(), element: true }; }
            }
            return find_own(heap, o, k);
        },
        node_kind::Array => {
            if key_is(heap, k, "length") { return Own { found: true, value: number(node(heap, o).length), attrs: if read_only_length(heap, o) { 0 } else { attr_writable() } }; }
            let i = index(heap, k);
            if heap.fault != Fault.None { return Own {}; }
            if i < 0 || node(heap, o).initialized { return find_own(heap, o, k); }
            // A sealed or frozen array's elements (array_flag) are not configurable.
            let v, found = own_element(heap, object, i);
            return Own { found: found, index: i, value: v, attrs: element_attrs(node(heap, o)) };
        },
        node_kind::Closure => {
            // A lazy closure's `length` and `name` come from its code row.
            if k.is_string() && lazy_function(heap, o) {
                if key_is(heap, k, "length") { return Own { found: true, value: function_length(heap, o), attrs: attr_configurable() }; }
                if key_is(heap, k, "name") { return Own { found: true, value: function_name(heap, o), attrs: attr_configurable() }; }
            }
            if !function_key(heap, o, k) { return Own {}; }
            return find_own(heap, o, k);
        },
        node_kind::Builtin, node_kind::BoundFunction => { return find_own(heap, o, k); },
        // A proxy's runs in a native frame (proxy.goose), not here.
        node_kind::Proxy => { proxy_fault(heap, proxy_method::GetOwnProperty); return Own {}; },
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
        if heap.fault != Fault.None { proxy_relabel(heap, at, proxy_method::Get); return Value.Undefined, Value.Undefined; }
        if p.found {
            if is_pending(p.value) { pending_fault(heap, k); return Value.Undefined, Value.Undefined; }
            return if is_accessor(p.value) { Value.Undefined } else { p.value }, if is_accessor(p.value) { pair(heap, p.value).get } else { Value.Undefined };
        }
        let proto = prototype(heap, at.id());
        if !proto.is_object() { return Value.Undefined, Value.Undefined; }
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
        if heap.fault != Fault.None { proxy_relabel(heap, at, proxy_method::HasProperty); return false; }
        if p.found { return true; }
        let proto = prototype(heap, at.id());
        if !proto.is_object() { return false; }
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
// kinds, a function's keys function_key refuses, and the realm's pending
// properties (light-cljs-788) fail UnsupportedOperand.
fn assign(heap: Heap&, object: Value, k: Value, value: Value) -> Value, bool {
    let o: i64 = object.id(); let kind = node(heap, o).kind;
    match kind {
        node_kind::Object, node_kind::Arguments, node_kind::Builtin, node_kind::BoundFunction, node_kind::Error, node_kind::Generator, node_kind::Promise, node_kind::Iterator => {},
        node_kind::Array => {
            // A non-writable `length` (light-cljs-881) refuses the write; a
            // name goes on below, where its own non-writable property or
            // the array's non-extensibility refuses it.
            if key_is(heap, k, "length") { return Value.Undefined, !read_only_length(heap, o) && array_length(heap, object, value); }
            let i = index(heap, k);
            if heap.fault != Fault.None { return Value.Undefined, false; }
            if i >= 0 && node(heap, o).function == 0 && !node(heap, o).non_extensible && node(heap, o).proto.is_hole() { return Value.Undefined, set_element(heap, object, i, value); }
            // An array a definition or an integrity level changed
            // (array_flag) or made non-extensible (light-cljs-882): its
            // element record or flags decide as an own property does, a
            // new element is refused, and with a non-writable `length` an
            // index at or past it is refused (ArrayDefineOwnProperty). An
            // array with a prototype other than the realm's
            // (light-cljs-883) asks its chain about a missing element first.
            if i >= 0 {
                if !node(heap, o).initialized {
                    let v, found = own_element(heap, object, i);
                    if heap.fault != Fault.None { return Value.Undefined, false; }
                    if found { return Value.Undefined, !frozen_array(heap, o) && set_element(heap, object, i, value); }
                } else {
                    let own = find_own(heap, o, k);
                    if heap.fault != Fault.None { return Value.Undefined, false; }
                    if own.found {
                        if is_accessor(own.value) { let f = pair(heap, own.value).set; return f, f.is_object(); }
                        if own.attrs & attr_writable() == 0 { return Value.Undefined, false; }
                        write_own(heap, o, own.index, value); return Value.Undefined, true;
                    }
                }
                if !node(heap, o).proto.is_hole() {
                    let decided, f, ok = inherited_assign(heap, o, k);
                    if decided { return f, ok; }
                }
                if node(heap, o).non_extensible { return Value.Undefined, false; }
                if read_only_length(heap, o) && i >= node(heap, o).length { return Value.Undefined, false; }
                return Value.Undefined, set_element(heap, object, i, value);
            }
        },
        node_kind::Closure => {
            if !function_key(heap, o, k) || lazy_change(heap, o) { return Value.Undefined, false; }
        },
        // A String object's `length` and indices are not writable.
        node_kind::Wrapper => {
            let exotic, i = string_key(heap, o, k);
            if heap.fault != Fault.None || exotic { return Value.Undefined, false; }
        },
        node_kind::Proxy => { proxy_fault(heap, proxy_method::Set); return Value.Undefined, false; },
        _ => { heap.fault = Fault.UnsupportedOperand; return Value.Undefined, false; },
    }
    // An element is a writable data property (light-cljs-804); only an
    // ordinary object (or arguments object) keeps index keys outside its
    // shape or records here.
    let i = if element_kind(kind) { element_key(heap, k) } else { -1 };
    if heap.fault != Fault.None { return Value.Undefined, false; }
    if !object_element(heap, o, i).is_hole() { set_slot(heap, node(heap, o).start + i, value); return Value.Undefined, true; }
    let own = find_own(heap, o, k);
    if heap.fault != Fault.None { return Value.Undefined, false; }
    if own.found {
        if is_pending(own.value) { pending_fault(heap, k); return Value.Undefined, false; }
        if is_accessor(own.value) { let f = pair(heap, own.value).set; return f, f.is_object(); }
        if own.attrs & attr_writable() == 0 { return Value.Undefined, false; }
        write_own(heap, o, own.index, value); return Value.Undefined, true;
    }
    let decided, f, ok = inherited_assign(heap, o, k);
    if decided { return f, ok; }
    // A non-extensible object (light-cljs-882) refuses a new property.
    if node(heap, o).non_extensible { return Value.Undefined, false; }
    return Value.Undefined, if as_element(heap, o, i) { put_element(heap, o, i, value) } else { add_own(heap, o, k, value, attrs_data()) };
}
// assign's walk of node o's prototype chain for key k, which o lacks: true
// and assign's answer when an inherited property decides (its setter, or a
// refusal: a non-writable data property or an accessor without a setter) or
// the walk faults (a pending property fails by name); false when the
// receiver is to get a new own data property.
inline fn inherited_assign(heap: Heap&, o: i64, k: Value) -> bool, Value, bool {
    var proto = prototype(heap, o);
    while proto.is_object() {
        if !charge(heap) { return true, Value.Undefined, false; }
        let p = own_entry(heap, proto, k);
        if heap.fault != Fault.None { proxy_relabel(heap, proto, proxy_method::Set); return true, Value.Undefined, false; }
        if p.found {
            if is_pending(p.value) { pending_fault(heap, k); return true, Value.Undefined, false; }
            if is_accessor(p.value) { let f = pair(heap, p.value).set; return true, f, f.is_object(); }
            if p.attrs & attr_writable() == 0 { return true, Value.Undefined, false; }
            return false, Value.Undefined, true;
        }
        proto = prototype(heap, proto.id());
    }
    return false, Value.Undefined, true;
}
// [[DefineOwnProperty]] by object kind: ordinary objects (define_own), an
// array's elements and named properties, its `length` (ArraySetLength,
// light-cljs-881: only with `half` Complete, from define_descriptor, which
// has validated it), a closure's own properties (light-cljs-805). A plain
// data element of an array stays in its run; an element with other
// attributes or an accessor moves the array to dictionary mode
// (array_to_dictionary, as light-cljs-805 moves named properties) and marks
// it Attributed (array_flag); with a non-writable `length` an index at or
// past it is refused (ArrayDefineOwnProperty). Other kinds and a function's
// keys function_key refuses fail UnsupportedOperand.
fn define_property(heap: Heap&, object: Value, k: Value, value: Value, attrs: u8, half: i64 = 0) -> bool {
    let o: i64 = object.id(); let kind = node(heap, o).kind;
    // A non-extensible object (light-cljs-882) takes no new property.
    if node(heap, o).non_extensible { let own = own_entry(heap, object, k); if heap.fault != Fault.None || !own.found { return false; } }
    // A mapped index (light-cljs-860, ECMAScript 10.4.4.2): the definition
    // writes through; an accessor or a non-writable one then unmaps it, the
    // parameter keeping its old value or the value just defined.
    if kind == node_kind::Arguments && node(heap, o).namespace_value.is_object() {
        let i = element_key(heap, k);
        if heap.fault != Fault.None { return false; }
        let c = mapped_cell_of(heap, o, i);
        if c >= 0 {
            let before = mapped_value(heap, o, i, c);
            if heap.fault != Fault.None || !define_own(heap, o, k, value, attrs, half) { return false; }
            let accessor = half == half::Getter || half == half::Setter || half == half::Complete && is_accessor(value);
            if accessor { unmap(heap, o, i, c, before); } else if attrs & attr_writable() == 0 { unmap(heap, o, i, c, value); } else if object_element(heap, o, i).is_hole() { node_mut(heap, c).namespace_value = k; }
            return true;
        }
    }
    if kind == node_kind::Object || kind == node_kind::Arguments || kind == node_kind::Builtin || kind == node_kind::BoundFunction || kind == node_kind::Error || kind == node_kind::Generator || kind == node_kind::Promise || kind == node_kind::Iterator { return define_own(heap, o, k, value, attrs, half); }
    if kind == node_kind::Array {
        if key_is(heap, k, "length") {
            if half != half::Complete || !value.is_number() { if heap.fault == Fault.None { heap.fault = Fault.InvalidState; } return false; }
            return define_array_length(heap, object, value.num() as i64, attrs & attr_writable() != 0);
        }
        let i = index(heap, k);
        if heap.fault != Fault.None { return false; }
        if i < 0 { return define_own(heap, o, k, value, attrs, half); }
        if read_only_length(heap, o) && i >= node(heap, o).length { return false; }
        // An array's length stays within the value limit (set_element's rule).
        if i >= heap.limits.values { return memory(heap); }
        let data = half == half::Data || half == half::Complete && !is_accessor(value);
        let plain = data && attrs == attrs_data();
        // A dense element with the run's attributes stays in the run; a
        // sealed or frozen array's (light-cljs-882) only from a validated
        // definition, and a frozen one's changes nothing (define_descriptor
        // has refused any change).
        if data && attrs == element_attrs(node(heap, o)) && !node(heap, o).initialized && (plain || half == half::Complete) {
            if frozen_array(heap, o) { let v, found = own_element(heap, object, i); return heap.fault == Fault.None && found && same_value(heap, v, value); }
            return set_element(heap, object, i, value);
        }
        if !node(heap, o).initialized && !array_to_dictionary(heap, object) { return false; }
        if !define_own(heap, o, k, value, attrs, half) { return false; }
        if !plain { node_mut(heap, o).function = node(heap, o).function | array_flag::Attributed; }
        if i >= node(heap, o).length { node_mut(heap, o).length = (i + 1) as u32; }
        return true;
    }
    if kind == node_kind::Closure {
        if !function_key(heap, o, k) || lazy_change(heap, o) { return false; }
        return define_own(heap, o, k, value, attrs, half);
    }
    if kind == node_kind::Wrapper {
        // A String object's `length` and indices are non-configurable and
        // non-writable: only a definition that changes nothing is accepted
        // (ValidateAndApplyPropertyDescriptor).
        let exotic, i = string_key(heap, o, k);
        if heap.fault != Fault.None { return false; }
        if !exotic { return define_own(heap, o, k, value, attrs, half); }
        let p = own_entry(heap, object, k);
        return heap.fault == Fault.None && (half == half::Data || half == half::Complete && !is_accessor(value)) && attrs == p.attrs && same_value(heap, p.value, value);
    }
    if kind == node_kind::Proxy { proxy_fault(heap, proxy_method::DefineOwnProperty); return false; }
    if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; }
    return false;
}
// ArraySetLength (ECMAScript 10.4.2.4) of a validated `length` definition
// (define_descriptor): the array's length becomes `length` (shortening
// drops its elements from the end, stopping just past the last
// non-configurable one, which refuses the definition), and then, unless
// `writable`, its `length` is read-only (array_flag::ReadOnlyLength).
fn define_array_length(heap: Heap&, object: Value, length: i64, writable: bool) -> bool {
    let o: i64 = object.id();
    var ok = true;
    if length != node(heap, o).length {
        ok = array_length(heap, object, number(length));
        if heap.fault != Fault.None { return false; }
    }
    if !writable { node_mut(heap, o).function = node(heap, o).function | array_flag::ReadOnlyLength; }
    return ok;
}
// [[Delete]] by object kind: ordinary objects (delete_own); an array's
// element becomes a hole (a dictionary array's record is removed), its
// `length` is non-configurable, and a named property goes as an ordinary
// object's does, as does a closure's (light-cljs-805). Other kinds, and a
// function's keys function_key refuses, fail UnsupportedOperand.
fn delete_property(heap: Heap&, object: Value, k: Value) -> bool {
    let o: i64 = object.id();
    match node(heap, o).kind {
        node_kind::Builtin, node_kind::BoundFunction => { return delete_own(heap, o, k); },
        node_kind::Object, node_kind::Arguments, node_kind::Error, node_kind::Generator, node_kind::Promise, node_kind::Iterator => {
            let i = element_key(heap, k);
            if heap.fault != Fault.None { return false; }
            // A deleted mapped index is unmapped (light-cljs-860, ECMAScript
            // 10.4.4.5); its parameter keeps the value.
            let c = mapped_cell_of(heap, o, i);
            let before = if c >= 0 { mapped_value(heap, o, i, c) } else { Value.Undefined };
            if heap.fault != Fault.None { return false; }
            var deleted = false;
            if !object_element(heap, o, i).is_hole() { if !charge(heap) { return false; } set_slot(heap, node(heap, o).start + i, hole()); deleted = true; }
            else { deleted = delete_own(heap, o, k); }
            if deleted && c >= 0 && heap.fault == Fault.None { unmap(heap, o, i, c, before); }
            return deleted;
        },
        node_kind::Array => {
            if key_is(heap, k, "length") { return false; }
            let i = index(heap, k);
            if heap.fault != Fault.None { return false; }
            if i < 0 || node(heap, o).initialized { return delete_own(heap, o, k); }
            // A sealed or frozen array's elements are not configurable (array_flag).
            if node(heap, o).function & array_flag::Sealed != 0 { let v, found = own_element(heap, object, i); return heap.fault == Fault.None && !found; }
            if !charge(heap) { return false; }
            if i < node(heap, o).count && i < node(heap, o).length { set_slot(heap, node(heap, o).start + i, hole()); }
            return true;
        },
        node_kind::Closure => {
            if lazy_function(heap, o) && k.is_string() {
                if lazy_prototype(heap, o) && key_is(heap, k, "prototype") { return false; }
                if (key_is(heap, k, "length") || key_is(heap, k, "name")) && lazy_change(heap, o) { return false; }
            }
            return function_key(heap, o, k) && delete_own(heap, o, k);
        },
        // A String object's `length` and indices are non-configurable.
        node_kind::Wrapper => {
            let exotic, i = string_key(heap, o, k);
            return heap.fault == Fault.None && !exotic && delete_own(heap, o, k);
        },
        node_kind::Proxy => { proxy_fault(heap, proxy_method::Delete); return false; },
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
// or a generator's; a String object's indices first and then `length`
// (light-cljs-832). Other kinds fail UnsupportedOperand. Index keys of elements are
// interned strings the call creates. own_property_keys gives each key's
// attribute bits and array index (-1 for a name or symbol) too.
struct OwnKey { key: Value, attrs: u8 = 0, index: i64 = -1 }
fn own_property_keys(heap: Heap&, object: Value, out: OwnKey[>..]&) -> bool {
    let o: i64 = object.id(); let kind = node(heap, o).kind;
    if kind != node_kind::Object && kind != node_kind::Array && kind != node_kind::Closure && kind != node_kind::Builtin && kind != node_kind::BoundFunction && kind != node_kind::Error && kind != node_kind::Generator && kind != node_kind::Promise && kind != node_kind::Wrapper && kind != node_kind::Iterator && kind != node_kind::Arguments || kind == node_kind::Closure && unsupported_prototype(heap, o) {
        // A proxy's runs in a native frame (proxy.goose), not here.
        if kind == node_kind::Proxy { proxy_fault(heap, proxy_method::OwnPropertyKeys); } else { heap.fault = Fault.UnsupportedOperand; }
        return false;
    }
    // A lazy closure's are `length`, `name` and, for an ordinary function,
    // `prototype`, not yet made (light-cljs-806).
    if kind == node_kind::Closure && lazy_function(heap, o) {
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
    if (element_kind(kind) || !node(heap, o).initialized) && !charge(heap, if element_kind(kind) { node(heap, o).count as i64 } else { min(node(heap, o).length, node(heap, o).count) as i64 }) { return false; }
    if kind == node_kind::Array && !node(heap, o).initialized {
        let end = min(node(heap, o).length, node(heap, o).count);
        let attrs = element_attrs(node(heap, o));
        for i in end { if !slot(heap, node(heap, o).start + i).is_hole() { keys.push(OwnKey { key: index_key(heap, i), attrs: attrs, index: i }); } }
    } else if element_kind(kind) {
        for i in node(heap, o).count { if !slot(heap, node(heap, o).start + i).is_hole() { keys.push(OwnKey { key: index_key(heap, i), attrs: attrs_data(), index: i }); } }
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
    // A String object's indices come before its other index keys, and its
    // `length` before its other names (StringExoticObject [[OwnPropertyKeys]]).
    let wrapped = if kind == node_kind::Wrapper { node(heap, o).value } else { Value.Undefined };
    if wrapped.is_string() {
        let n = str_len(heap, wrapped);
        if !charge(heap, n) { return false; }
        for i in n { out.push(OwnKey { key: index_key(heap, i), attrs: attr_enumerable(), index: i }); }
    }
    out.append(indices);
    if kind == node_kind::Array || wrapped.is_string() { let length = text(heap, "length"); out.push(OwnKey { key: intern(heap, length), attrs: if kind == node_kind::Array && !read_only_length(heap, o) { attr_writable() } else { 0 } }); }
    for e in keys { if e.key.is_string() && e.index < 0 { out.push(e); } }
    for e in keys { if e.key.is_symbol() { out.push(e); } }
    return heap.fault == Fault.None;
}
fn own_keys(heap: Heap&, object: Value, out: Value[>..]&) -> bool {
    var keys: OwnKey[>..] = [];
    if !own_property_keys(heap, object, keys) { return false; }
    for e in keys { out.push(e.key); }
    return true;
}
// [[SetPrototypeOf]] (light-cljs-883; ECMAScript 10.1.2.1
// OrdinarySetPrototypeOf, 10.4.7.2 SetImmutablePrototype) of `object`, any
// JavaScript object kind, to Null or an object: true when the object has
// that prototype afterwards; false without a fault when it refuses another
// (prototype_refusal says why); false with a fault otherwise. The same
// prototype again is accepted with no write. %Object.prototype% is an
// immutable-prototype exotic object; a non-extensible object (light-cljs-882)
// keeps its prototype; a prototype whose chain reaches the object is
// refused, the walk stopping at a proxy (whose [[GetPrototypeOf]] could run
// JavaScript). A row the job did not allocate faults NativeBoundary before
// anything changes (decision 3). A proxy fails by name (light-cljs-891), as
// does a form, which has no [[Prototype]] here while the V8 build's runtime
// makes it an ordinary object. The kind does not change: an array stays an
// array, a function a function. A prototype equal to the kind's realm
// prototype is stored as Empty (kind_prototype), so an object set back to
// its default is the untouched object the dense-array paths expect.
fn set_prototype(job: Job&, object: Value, proto: Value) -> bool {
    let heap .= job.heap; let o: i64 = object.id(); let kind = node(heap, o).kind;
    if kind == node_kind::Proxy { proxy_fault(heap, proxy_method::SetPrototypeOf); return false; }
    if !integrity_kind(kind) { if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; } return false; }
    if same_bits(prototype(heap, o), proto) { return true; }
    if prototype_refusal(heap, o, proto) != refusal::None || heap.fault != Fault.None { return false; }
    // A realm row never changes its prototype, even in a job that may write
    // realm rows (the conformance and execute runners): dense relies on the
    // realm's prototype chains (decision 3).
    if !writable(job, o) || heap.global.is_object() && realm_row(o) { heap.fault = Fault.NativeBoundary; return false; }
    store_prototype(heap, o, proto); return true;
}
// OrdinarySetPrototypeOf of node o for Goose callers (tests, which own
// every row): the same prototype, a refusal and the Empty encoding as
// above, without the kind and ownership checks.
fn set_prototype(heap: Heap&, o: i64, proto: Value) -> bool {
    if same_bits(prototype(heap, o), proto) { return true; }
    if prototype_refusal(heap, o, proto) != refusal::None || heap.fault != Fault.None { return false; }
    store_prototype(heap, o, proto); return true;
}
inline fn store_prototype(heap: Heap&, o: i64, proto: Value) {
    node_mut(heap, o).proto = if heap.global.is_object() && same_bits(kind_prototype(heap, o), proto) { Value.Empty } else { proto };
}
// Why [[SetPrototypeOf]] of node o to another prototype is refused, or
// refusal::None (also on a fault: the walk charges a work unit a link).
let refusal::None: i64 = 0;
let refusal::Immutable: i64 = 1;
let refusal::NotExtensible: i64 = 2;
let refusal::Circular: i64 = 3;
fn prototype_refusal(heap: Heap&, o: i64, proto: Value) -> i64 {
    if heap.global.is_object() && o == object_prototype_row() { return refusal::Immutable; }
    if node(heap, o).non_extensible { return refusal::NotExtensible; }
    var p = proto;
    while p.is_object() { if !charge(heap) { return refusal::None; } if p.id() == o { return refusal::Circular; } p = prototype(heap, p.id()); }
    return refusal::None;
}
// The TypeError of a refused [[SetPrototypeOf]] of node o (the Pants
// engine's wording; QuickJS words the cycle "circular prototype chain").
fn prototype_type_error(job: Job&, o: i64, proto: Value) -> Value {
    let r = prototype_refusal(job.heap, o, proto);
    if r == refusal::Immutable { return type_error(job, "cannot set the prototype of an immutable prototype object"); }
    if r == refusal::NotExtensible { return type_error(job, "cannot set the prototype: the object is not extensible"); }
    return type_error(job, "cannot set the prototype: the prototype chain would be circular");
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
// with `object` as receiver. An object's lookup comes first. A primitive
// base (GetValue's ToObject, ECMAScript 6.2.5.5; QuickJS's
// JS_GetPrototypePrimitive) makes no wrapper (decision 2): a string first
// answers its own `length` and indices, then the lookup starts at the
// primitive's realm prototype (primitive_prototype), and a getter found
// there runs with the primitive itself as receiver. Undefined and null are
// a TypeError; a symbol's is %Symbol.prototype% (light-cljs-885), and a
// BigInt base has no prototype here yet (UnsupportedOperand, which
// get_property names).
fn read(heap: Heap&, object: Value, k: Value) -> Value, Value {
    if !valid(heap, object) || !key_valid(heap, k) { return Value.Undefined, Value.Undefined; }
    if !k.is_string() && !k.is_symbol() { heap.fault = Fault.InvalidState; return Value.Undefined, Value.Undefined; }
    if object.is_object() {
        let key = intern(heap, k);
        if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
        return lookup(heap, object, key);
    }
    if object.is_string() {
        if key_is(heap, k, "length") { return number(str_len(heap, object)), Value.Undefined; }
        let i = index(heap, k); if i >= 0 && i < str_len(heap, object) { return unit_string(heap, object, i), Value.Undefined; }
        if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
    }
    let proto = primitive_prototype(object);
    if !proto.is_object() { heap.fault = if object.tag() <= value_tag::Null { Fault.TypeError } else { Fault.UnsupportedOperand }; return Value.Undefined, Value.Undefined; }
    let key = intern(heap, k);
    if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
    return lookup(heap, proto, key);
}
// [[Get]] for Goose callers: an accessor's getter fails UnsupportedOperand,
// since only the dispatch loop can run it (get_property).
fn get(heap: Heap&, object: Value, k: Value) -> Value {
    let v, getter = read(heap, object, k);
    if !getter.is_undefined() && heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; }
    return v;
}
// [[Set]] of a string key: the setter to run (see assign) and whether the
// assignment was accepted. Undefined and null are a TypeError. A number,
// string or boolean base (PutValue's ToObject, ECMAScript 6.2.5.6) is the
// receiver of OrdinarySet from its realm prototype, without a wrapper: a
// setter found on the chain runs with the primitive as receiver; anything
// else refuses the write (a TypeError in strict code), since a primitive
// takes no own property and a string's `length` and indices are read-only.
// A symbol base is the same (light-cljs-885); a BigInt base fails
// UnsupportedOperand (set_property names it).
fn write(heap: Heap&, object: Value, k: Value, value: Value) -> Value, bool {
    if !valid(heap, object) || !key_valid(heap, k) || !valid(heap, value) { return Value.Undefined, false; }
    if !k.is_string() && !k.is_symbol() { heap.fault = if object.tag() <= value_tag::Null { Fault.TypeError } else { Fault.UnsupportedOperand }; return Value.Undefined, false; }
    if object.is_object() {
        let key = intern(heap, k);
        if heap.fault != Fault.None { return Value.Undefined, false; }
        return assign(heap, object, key, value);
    }
    let proto = primitive_prototype(object);
    if !proto.is_object() { heap.fault = if object.tag() <= value_tag::Null { Fault.TypeError } else { Fault.UnsupportedOperand }; return Value.Undefined, false; }
    if object.is_string() {
        if key_is(heap, k, "length") { return Value.Undefined, false; }
        let i = index(heap, k);
        if heap.fault != Fault.None || i >= 0 && i < str_len(heap, object) { return Value.Undefined, false; }
    }
    let key = intern(heap, k);
    var at = proto;
    while at.is_object() && heap.fault == Fault.None {
        let p = own_entry(heap, at, key);
        if heap.fault != Fault.None { break; }
        if p.found {
            if is_pending(p.value) { pending_fault(heap, key); break; }
            if is_accessor(p.value) { let f = pair(heap, p.value).set; return f, f.is_object(); }
            break;
        }
        if !charge(heap) { break; }
        at = prototype(heap, at.id());
    }
    return Value.Undefined, false;
}
// [[Set]] for Goose callers: a setter fails UnsupportedOperand and a refused
// assignment is a TypeError.
fn put(heap: Heap&, object: Value, k: Value, value: Value) -> bool {
    let setter, ok = write(heap, object, k, value);
    if heap.fault != Fault.None { return false; }
    if !setter.is_undefined() { heap.fault = Fault.UnsupportedOperand; return false; }
    if !ok { heap.fault = Fault.TypeError; }
    return ok;
}
// ArraySetLength (ECMAScript 10.4.2.4) of a primitive: its ToNumber
// (numeric: a string, boolean, null or undefined converts, a symbol is a
// TypeError), which must be a uint32 (ToUint32 of itself), else a
// RangeError (light-cljs-893; property_fault words it). An object value
// converts in a Convert frame before this (length_converts); one that
// reaches here fails UnsupportedOperand by name. A shorter length turns a
// dense array's cut-off slots back into holes, or drops a dictionary
// array's cut-off records from its run (their rows stay charged until
// teardown); the work is charged before anything changes, so a work
// rejection is atomic. A longer length writes nothing.
fn array_length(heap: Heap&, object: Value, value: Value) -> bool {
    let n: f64 = if value.is_number() { value.num() } else { numeric(heap, value) };
    if heap.fault != Fault.None { return false; }
    if !(n >= 0 && n <= 4294967295.0) || n != trunc(n) { heap.fault = Fault.RangeError; return false; }
    let length = n as i64;
    // A built-in's length update of an array whose `length` is read-only (a
    // frozen array's, light-cljs-882); assignment refuses it first.
    if read_only_length(heap, object.id()) { heap.fault = Fault.TypeError; return false; }
    if length > heap.limits.values { return memory(heap); }
    if node(heap, object.id()).initialized { return dictionary_length(heap, object, length); }
    let start = node(heap, object.id()).start; let end = min(node(heap, object.id()).length, node(heap, object.id()).count);
    var target = length;
    if length < end {
        if !charge(heap, end - length) { return false; }
        // A sealed array's elements are not configurable (array_flag): the
        // length stops just past the last one, as dictionary_length's does.
        if node(heap, object.id()).function & array_flag::Sealed != 0 { var last = end; while last > length && slot(heap, start + last - 1).is_hole() { last -= 1; } target = last; }
        for i in target..end { set_slot(heap, start + i, hole()); }
    }
    node_mut(heap, object.id()).length = target as u32; return target == length;
}
// Preflight the full walk before mutation so a work rejection is atomic.
// A non-configurable element at or past the new length (light-cljs-881;
// ArraySetLength step 17) stays: the length stops just past the last one,
// and the shortening reports false (a refused write) with no fault.
fn dictionary_length(heap: Heap&, object: Value, length: i64) -> bool {
    var target = length;
    if length < node(heap, object.id()).length {
        let o = object.id(); let count = node(heap, o).key_count;
        // Each record's array index, -1 for a named property (light-cljs-805),
        // which stays; the preflight decodes them all, so mutation below
        // cannot fail.
        var indices: i64[>..] = [];
        for r in count {
            if !charge(heap, 2 + str_len(heap, dict_at(heap, o, r).key)) { return false; }
            let i = index(heap, dict_at(heap, o, r).key);
            if heap.fault != Fault.None { return false; }
            indices.push(i);
            if i >= target && dict_at(heap, o, r).attrs & attr_configurable() == 0 { target = i + 1; }
        }
        // Compact the kept records in order; the cut-off rows are cleared.
        var kept: i64 = 0;
        for r in count { if indices[r] < target { set_dict_at(heap, o, kept, dict_at(heap, o, r)); kept += 1; } }
        for r in kept..count { set_dict_at(heap, o, r, Property {}); }
        node_mut(heap, o).key_count = kept as u32;
    }
    node_mut(heap, object.id()).length = target as u32; return target == length;
}
fn closure(heap: Heap&, function: i64, captures: const Value[:]) -> Value {
    if function < 0 || function >= heap.code.functions.len || captures.len != heap.code.functions[function].captures { heap.fault = Fault.InvalidState; return Value.Undefined; }
    if !values_fit(heap, captures.len) || !charge(heap, captures.len) { if heap.fault == Fault.None { memory(heap); } return Value.Undefined; }
    for v in captures { if !valid(heap, v) { return Value.Undefined; } }
    let start = add_values(heap, captures);
    let f = allocate(heap, Node { kind: node_kind::Closure, function: function as u32, start: start as u32, count: captures.len as u32 });
    // A class constructor's `length`, `name` and `prototype` are made with
    // it (light-cljs-886): a class is never a lazy closure.
    if heap.fault == Fault.None && heap.code.functions[function].kind >= constructor::BaseClass { materialize_function(heap, f.id()); }
    return f;
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
fn error_code(fault: Fault) -> u32 { return match fault { TypeError => 1, ReferenceError => 2, HostEffect => 3, StackLimit => 4, RangeError => 5, SyntaxError => 6, _ => 0 }; }
fn error_fault(code: i64) -> Fault { return match code { 1 => Fault.TypeError, 2 => Fault.ReferenceError, 3 => Fault.HostEffect, 4 => Fault.StackLimit, 5 => Fault.RangeError, 6 => Fault.SyntaxError, _ => Fault.InvalidState }; }
// The JavaScript error name of a catchable fault, as the Pants engine and
// QuickJS name it (a call-depth or value-stack overflow is QuickJS's
// InternalError "stack overflow").
fn error_name(fault: Fault) -> const u8[:] { return match fault { TypeError => "TypeError", ReferenceError => "ReferenceError", RangeError => "RangeError", SyntaxError => "SyntaxError", StackLimit => "InternalError", _ => "Error" }; }
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
    if fault == Fault.UnsupportedOperand && job.detail.len == 0 && !job.heap.pending.is_undefined() { pending_detail(job); }
    if fault == Fault.UnsupportedOperand && job.detail.len == 0 && job.heap.proxy_method >= 0 { proxy_detail(job); }
    job.heap.pending = Value.Undefined; job.heap.proxy_method = -1;
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
    // Printable ASCII, a control character as `?` too, so the text stays
    // one protocol line; bounded as describe_uncaught's (a key in the
    // message can be long).
    if job.fault != Fault.Unsupported && job.detail.len > 0 {
        out.push(':');
        for u, i in job.detail { if i >= description_limit() { out.append("..."); return; } out.push(if u == 32 { 45 } else if u > 126 || u < 32 { 63 } else { u as u8 }); }
    }
}
// Just the operation: `construct`, `const(BigInt)`.
fn unsupported_operation(out: u8[>..]&, code: const Code&, job: const Job&) {
    if job.fault != Fault.Unsupported || job.fault_opcode < 0 { return; }
    out.append(export_opcode_name(job.fault_opcode));
    if job.fault_opcode == opcode::Const {
        let c = code.constants[code.functions[job.fault_function].constants + code.ops[job.fault_pc].arg];
        out.append(if c.is_symbol() { "(Symbol)" } else if c.is_bigint() { "(BigInt)" } else { "" });
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
// The fault's error keeps the message it was raised with (error_object keeps
// it in the node's value), whatever JavaScript did to its properties since:
// Job.detail carries it to the report (describe_uncaught).
fn uncaught(job: Job&, value: Value, frame: Frame) {
    if value.is_object() && node(job.heap, value.id()).kind == node_kind::Error && node(job.heap, value.id()).function != 0 {
        let original = node(job.heap, value.id()).value;
        job.detail.clear(); if original.is_string() { append_utf16(job.heap, original, job.detail); }
        terminate(job, error_fault(node(job.heap, value.id()).function), frame); return;
    }
    job.result = value; job.thrown = true; terminate(job, Fault.HostEffect, frame);
}
// The longest description describe_uncaught reports, in bytes before its
// "..." (the Pants engine bounds engine messages, bbp-268, bbp-286).
fn description_limit() -> i64 { return 1024; }
// What a failed job threw, as the V8 build's diagnostic and the Pants
// engine's "uncaught exception: " text describe it (exceptions.rs
// uncaught_error_objects_report_name_and_message and
// unhandled_failures_report_the_source_position_and_call_chain), appended to
// `out`; false, appending nothing, when the job did not end with an uncaught
// exception. A thrown primitive is printed as JavaScript prints it; an error
// object (an Error, or an object inheriting %Error.prototype%, as
// cljs.core's ExceptionInfo does) as Error.prototype.toString prints it from
// data properties; one whose name or message is an accessor or an object,
// and every other object, as "an object". A catchable fault no handler
// caught is its error's name and message (an error a fault raised keeps its
// original kind and message when rethrown: uncaught). Nothing runs
// JavaScript and nothing recurses; the job's work count and fault are left
// as they were. The text is UTF-8 on one line (escape_description).
fn describe_uncaught(out: u8[>..]&, job: Job&) -> bool {
    if job.status != tiny::Status.Failed { return false; }
    var units: u16[>..] = [];
    if job.thrown && job.fault == Fault.HostEffect {
        let heap .= job.heap;
        let fault = heap.fault; let work = heap.work; heap.fault = Fault.None; heap.work = 0;
        describe_value(heap, job.result, units);
        heap.fault = fault; heap.work = work;
    } else if !job.thrown && catchable(job.fault) {
        for c in error_name(job.fault) { units.push(c); }
        var message: u16[>..] = [];
        if job.detail.len > 0 { message.append(job.detail[..]); }
        else {
            let code .= job.heap.code;
            let op = if job.fault_function >= 0 && job.fault_pc >= 0 && job.fault_pc < code.ops.len { code.ops[job.fault_pc].code } else { -1 };
            for c in fault_words(job.fault, op) { message.push(c); }
        }
        if message.len > 0 { units.push(58); units.push(32); units.append(message[..]); }
    } else { return false; }
    escape_description(out, units[..]);
    return true;
}
// A thrown value's description (describe_uncaught) in UTF-16 units.
fn describe_value(heap: Heap&, v: Value, out: u16[>..]&) {
    if !v.is_object() { primitive_text(heap, v, out); return; }
    if !error_like(heap, v) { for c in "an object" { out.push(c); } return; }
    var name: u16[>..] = []; var message: u16[>..] = [];
    if !error_field(heap, v, "name", "Error", name) || !error_field(heap, v, "message", "", message) { heap.fault = Fault.None; for c in "an object" { out.push(c); } return; }
    // Error.prototype.toString: name alone, message alone, or both.
    out.append(name[..]);
    if name.len > 0 && message.len > 0 { out.push(58); out.push(32); }
    out.append(message[..]);
}
// ToString of primitive v as JavaScript prints it, without running
// JavaScript: a symbol (only the well-known ones exist) as
// Symbol(Symbol.iterator); a BigInt has no digits here yet.
fn primitive_text(heap: const Heap&, v: Value, out: u16[>..]&) {
    var bytes: u8[>..] = [];
    match v {
        Undefined => { bytes.append("undefined"); },
        Null => { bytes.append("null"); },
        Bool b => { bytes.append(if b.v { "true" } else { "false" }); },
        Number n => { number::append_decimal(bytes, n.v); },
        String => { append_utf16(heap, v, out); return; },
        Symbol => { append_descriptive(heap, v, out); return; },
        BigInt => { bytes.append("a BigInt"); },
        _ => { bytes.append("an object"); },
    }
    for c in bytes { out.push(c); }
}
// Whether object v is an error to describe by name and message: an Error
// object, or one with %Error.prototype% on its prototype chain (a realm
// heap's; a hand-made test heap has no realm). Follows at most 10,000
// prototypes; a proxy or a kind without a prototype ends the walk.
fn error_like(heap: Heap&, v: Value) -> bool {
    if node(heap, v.id()).kind == node_kind::Error { return true; }
    if !heap.global.is_object() { return false; }
    var at = v;
    for i in 10000 {
        if node(heap, at.id()).kind == node_kind::Proxy { return false; }
        let p = prototype(heap, at.id());
        if !p.is_object() { return false; }
        if p.id() == error_prototype_row(0) { return true; }
        at = p;
    }
    return false;
}
// Data property `field` of error v, own or inherited, as text (ToString of a
// primitive; `fallback` when absent or undefined), into out: false for an
// accessor, an object value, a symbol, a proxy on the chain or any other
// lookup that would need JavaScript (Pants engine native/error.rs
// diagnostic). Leaves heap.fault set on a failed lookup; the caller clears it.
fn error_field(heap: Heap&, v: Value, field: const u8[:], fallback: const u8[:], out: u16[>..]&) -> bool {
    let k = find_key(heap, field);
    if k.is_undefined() { for c in fallback { out.push(c); } return true; }
    var at = v;
    for i in 10000 {
        if node(heap, at.id()).kind == node_kind::Proxy { return false; }
        let p = own_entry(heap, at, k);
        if heap.fault != Fault.None { return false; }
        if p.found {
            if is_accessor(p.value) || p.value.is_object() || p.value.is_symbol() || p.value.is_hole() { return false; }
            if p.value.is_undefined() { for c in fallback { out.push(c); } } else { primitive_text(heap, p.value, out); }
            return true;
        }
        let proto = prototype(heap, at.id());
        if !proto.is_object() { for c in fallback { out.push(c); } return true; }
        at = proto;
    }
    return false;
}
// A description as one line of UTF-8 for the runner's protocol: printable
// ASCII as is, a line break, tab or other control character as \n, \r, \t or
// \u00XX, a surrogate pair as its code point and a lone surrogate as U+FFFD
// (JavaScript's ToString shows it so), cut at a code point after
// description_limit() bytes with "...".
fn escape_description(out: u8[>..]&, units: const u16[:]) {
    let start = out.len; var i: i64 = 0;
    while i < units.len {
        if out.len - start >= description_limit() { out.append("..."); return; }
        let u: i64 = units[i]; i += 1;
        var c = u;
        if u >= 55296 && u <= 56319 && i < units.len && units[i] >= 56320 && units[i] <= 57343 { let low: i64 = units[i]; c = 65536 + ((u - 55296) << 10) + (low - 56320); i += 1; }
        else if u >= 55296 && u <= 57343 { c = 65533; }
        if c == 10 { out.append("\\n"); }
        else if c == 13 { out.append("\\r"); }
        else if c == 9 { out.append("\\t"); }
        else if c < 32 || c == 127 { out.append("\\u00"); out.push(hex_digit(c >> 4)); out.push(hex_digit(c & 15)); }
        else if c < 128 { out.push(c as u8); }
        else if c < 2048 { out.push((192 | (c >> 6)) as u8); out.push((128 | (c & 63)) as u8); }
        else if c < 65536 { out.push((224 | (c >> 12)) as u8); out.push((128 | ((c >> 6) & 63)) as u8); out.push((128 | (c & 63)) as u8); }
        else { out.push((240 | (c >> 18)) as u8); out.push((128 | ((c >> 12) & 63)) as u8); out.push((128 | ((c >> 6) & 63)) as u8); out.push((128 | (c & 63)) as u8); }
    }
}
inline fn hex_digit(d: i64) -> u8 { return (if d < 10 { 48 + d } else { 87 + d }) as u8; }
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
    // A constructed frame (new.target set) gives its new object unless it
    // returned an object (OrdinaryCallEvaluateBody's [[Construct]] step).
    // A derived constructor's receiver is its `this` cell (light-cljs-887):
    // undefined gives the bound `this`, a ReferenceError if super() never
    // ran, and any other non-object is a TypeError, thrown past the frame.
    var result = value;
    if job.frames[top].input.is_object() && !value.is_object() {
        result = job.frames[top].receiver;
        if result.is_object() && node(job.heap, result.id()).kind == node_kind::Binding {
            if !value.is_undefined() { type_error(job, "derived class constructor must return an object or undefined"); }
            else if !node(job.heap, result.id()).initialized { reference_fault(job, "this is not initialized"); }
            else { result = node(job.heap, result.id()).value; }
            if job.heap.fault != Fault.None { fail(job, job.heap.fault, frame_at(job, top, at)); return; }
        }
    }
    job.frames.pop(); job.locals.resize(lb); job.operands.resize(ob);
    deliver(job, result);
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
    if what == opcode::PendingCancel { return boolean(own(job, top, HandlerKind.Cancel)); }
    let thrown = own(job, top, HandlerKind.Throw);
    if what == opcode::PendingThrow { return boolean(thrown); }
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
// `new_target` is the frame's new.target: the constructor in a
// [[Construct]] (construct), undefined in a call.
fn push_frame(job: Job&, callee: Value, receiver: Value, arguments: const Value[:], checked: bool, operands: i64, new_target: Value = Value.Undefined) -> bool {
    let heap .= job.heap;
    if !callee.is_object() || node(heap, callee.id()).kind != node_kind::Closure {
        // A callable proxy's [[Call]] fails by name (proxy.goose).
        if callee.is_object() && node(heap, callee.id()).kind == node_kind::Proxy { proxy_invoke(heap, callee, proxy_method::Call); return false; }
        heap.fault = Fault.TypeError; return false;
    }
    let id = node(heap, callee.id()).function; let f .= heap.code.functions[id];
    // A class constructor has no [[Call]]: calling it throws TypeError; only
    // construct enters it, with its new.target (light-cljs-886).
    if f.kind >= constructor::BaseClass && !new_target.is_object() { type_error(job, "class constructors must be invoked with 'new'"); return false; }
    if job.frames.len >= heap.limits.frames || f.locals + arguments.len > heap.limits.stack - job.locals.len - operands { heap.fault = Fault.StackLimit; return false; }
    if !charge(heap, f.locals + arguments.len) { return false; }
    if checked { for v in arguments { if !valid(heap, v) { return false; } } }
    let base = job.locals.len; job.locals.resize(base + f.locals, Value.Undefined);
    for value, i in arguments { if i < f.params { job.locals[base + i] = value; } }
    job.locals.append(arguments);
    job.frames.push(Frame { function: id, pc: f.start, local_base: base, operand_base: operands, callee: callee, receiver: receiver, argument_count: arguments.len, input: new_target });
    return true;
}
fn start(job: Job&, callee: Value, arguments: const Value[:], receiver: Value = Value.Undefined) -> bool {
    if job.registered || job.status != tiny::Status.Ready { return false; }
    assert(job.frames.len == 0 && job.locals.len == 0 && job.operands.len == 0 && job.steps == 0);
    arm(job);
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
    disarm(job);
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
    job.protected_nodes = 0; job.owned_nodes = 0; job.foreign.clear(); job.root_receiver = Value.Undefined; job.gensym_prefix.clear(); job.gensym_next = 0; job.effect_args = [Value.Undefined; 8]; job.effect_count = 0; job.detail.clear(); job.heap.proxy_method = -1;
    job.effect_frame = Frame { function: -1, pc: -1, local_base: 0, operand_base: 0, callee: Value.Undefined, receiver: Value.Undefined };
    job.queue.clear(); job.queue_head = 0; job.draining = false; job.entry = Value.Undefined; job.rejections.clear();
    job.lexicals = Value.Undefined; job.compiled_ops = 0; job.floor = Tops {}; job.reclaimable = false; job.logged_values = false; job.templates.clear();
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
    if op == opcode::StrictEq { return boolean(equal(heap, a, b)); }
    // IsLooselyEqual of primitives (light-cljs-827; the Pants engine's
    // vm/dispatch.rs loose_equals): null and undefined equal each other only;
    // one type compares strictly; numbers, strings and booleans of different
    // types compare their ToNumber values (a boolean first converts, then
    // the comparison repeats, which comes to the same); a symbol equals no
    // other type. An object beside another primitive converts before this
    // (the Convert frame, light-cljs-828); a light-cljs runtime value there
    // fails UnsupportedOperand.
    if op == opcode::LooseEq {
        if (a.is_undefined() || a.is_null()) && (b.is_undefined() || b.is_null()) { return boolean(true); }
        if a.tag() == b.tag() { return boolean(equal(heap, a, b)); }
        if a.tag() <= value_tag::Null || b.tag() <= value_tag::Null { return boolean(false); }
        if a.tag() <= value_tag::String && b.tag() <= value_tag::String { let x = numeric(heap, a); let y = numeric(heap, b); return if heap.fault == Fault.None { boolean(x == y) } else { Value.Undefined }; }
        if a.is_object() || b.is_object() { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
        if a.is_symbol() || b.is_symbol() { return if valid(heap, a) && valid(heap, b) { boolean(false) } else { Value.Undefined }; }
        heap.fault = Fault.UnsupportedOperand; return Value.Undefined;
    }
    if op == opcode::Plus && a.is_string() && b.is_string() {
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
    // After ToPrimitive (the caller's native frame for objects; a scalar
    // form is its primitive, as numeric() reads it), a string on either side
    // concatenates the ToString of both; otherwise numbers add.
    if op == opcode::Plus {
        let x = form_value(heap, a); let y = form_value(heap, b);
        if x.is_string() || y.is_string() { return concat_mixed(heap, x, y); }
    }
    // IsLessThan of primitives (light-cljs-827; the Pants engine's
    // vm/dispatch.rs is_less_than): two strings (a scalar form counting as
    // its primitive) compare by code units; any other pair compares the
    // ToNumber values below, where NaN makes every comparison false.
    if op == opcode::CompareLt || op == opcode::CompareGt || op == opcode::CompareLe || op == opcode::CompareGe {
        let s = form_value(heap, a); let t = form_value(heap, b);
        if s.is_string() && t.is_string() {
            if !charge(heap, str_len(heap, s) + str_len(heap, t)) { return Value.Undefined; }
            let p = str_len(heap, s); let q = str_len(heap, t);
            var c: i64 = 0; for i in min(p, q) { let x = unit(heap, s, i); let y = unit(heap, t, i); if x != y { c = if x < y { -1 } else { 1 }; break; } } if c == 0 { c = if p < q { -1 } else if p > q { 1 } else { 0 }; }
            return boolean(if op == opcode::CompareLt { c < 0 } else if op == opcode::CompareGt { c > 0 } else if op == opcode::CompareLe { c <= 0 } else { c >= 0 });
        }
    }
    let x = numeric(heap, a); let y = numeric(heap, b);
    if heap.fault != Fault.None { return Value.Undefined; }
    if op == opcode::BitAnd || op == opcode::BitOr || op == opcode::BitXor || op == opcode::Shl || op == opcode::Sar || op == opcode::UShr || op == opcode::Pow { return arithmetic(op, x, y); }
    return match tiny::number_binary(op, x, y) {
        Number n => number(n.value), Bool b => boolean(b.value), _ => { heap.fault = Fault.InvalidState; Value.Undefined },
    };
}

// Bounded persistent flat ranges. Updates copy only the changed container's
// child IDs; input occurrences and untouched subtrees retain exact identity.
fn nil() -> Value { return Value.Null; }
fn form_value(heap: Heap&, v: Value) -> Value {
    if !valid(heap, v) { return Value.Undefined; }
    return if v.is_object() && node(heap, v.id()).kind == node_kind::Scalar { node(heap, v.id()).value } else { v };
}
fn form_kind(heap: Heap&, v: Value) -> i64 {
    let x = form_value(heap, v);
    return if x.is_object() { node(heap, x.id()).kind } else { node_kind::None };
}
fn form_collection(heap: Heap&, kind: i64, children: const Value[:], template: Node = Node { kind: node_kind::Object }) -> Value {
    if kind < node_kind::List || kind > node_kind::Set || (kind == node_kind::Map && children.len % 2 != 0) { heap.fault = Fault.TypeError; return Value.Undefined; }
    if !values_fit(heap, children.len) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, children.len) { return Value.Undefined; }
    for child in children { if !valid(heap, child) { return Value.Undefined; } }
    var made = template; made.kind = kind; made.start = heap.values.len as u32; made.count = children.len as u32;
    add_values(heap, children); return allocate(heap, made);
}
// Ordinary constructors normalize equal keys/members. LiteralMap and the
// compiler occurrence bridge deliberately retain positional source operands.
fn form_construct(heap: Heap&, kind: i64, children: const Value[:], template: Node = Node { kind: node_kind::Object }) -> Value {
    if kind != node_kind::Map && kind != node_kind::Set { return form_collection(heap, kind, children, template); }
    let stride: i64 = if kind == node_kind::Map { 2 } else { 1 };
    if children.len % stride != 0 { heap.fault = Fault.TypeError; return Value.Undefined; }
    if !charge(heap, children.len) { return Value.Undefined; }
    var normalized: Value[>..] = [];
    for i in (children.len / stride) {
        let key = children[i * stride]; var found: i64 = -1;
        for j in (normalized.len / stride) { if form_equal(heap, normalized[j * stride], key) { found = j * stride; break; } }
        if heap.fault != Fault.None { return Value.Undefined; }
        if found >= 0 { if kind == node_kind::Map { normalized[found + 1] = children[i * stride + 1]; } }
        else { normalized.push(key); if kind == node_kind::Map { normalized.push(children[i * stride + 1]); } }
    }
    return form_collection(heap, kind, normalized, template);
}
// An empty collection literal of `kind` (light-cljs-878): the realm's one
// empty collection of that kind (realm.goose empty_form_row); a heap built
// without the realm makes a new one.
fn empty_literal(heap: Heap&, kind: i64) -> Value {
    if heap.global.is_object() { return object(empty_form_row(kind)); }
    return form_collection(heap, kind, []);
}
// form_construct, then the V8 build's traversal order (map_order): `always`
// for NewFormMap's map (the Pants engine's new_map) of any size and the
// runtime's mapFromArray of more than 16 operands (literalMap's `map`);
// otherwise a map or set past 8 entries, as NewFormSet's (new_set) and
// setFromArray's (`set`, conj by conj) promote; anything else stays
// insertion-ordered.
fn form_ordered(heap: Heap&, kind: i64, children: const Value[:], always: bool) -> Value {
    let v = form_construct(heap, kind, children);
    if heap.fault != Fault.None || kind != node_kind::Map && kind != node_kind::Set { return v; }
    let entries = form_count(heap, v);
    if always || entries > 8 { node_mut(heap, v.id()).length = map_order::Hash; }
    return v;
}

// ClojureScript's `hash` of the form values this VM has (light-cljs-877),
// the V8 build's (the Pants engine's collections/form.rs `hash`: the
// runtime's Murmur3 profile), in 32-bit unsigned words that wrap as its
// i32 arithmetic does: nil and undefined 0, booleans 1231 and 1237,
// numbers (number_hash), strings (hash_int of string_hash), keywords and
// symbols (hash_named), lists, sequences and vectors as ordered
// collections, maps and sets as unordered ones, every position of their
// value runs counted (equal reader keys included, as the Pants engine's
// positional storage counts them). Another object (an atom, a delay, a
// function, a JavaScript object) hashes by its identity there, which no
// Goose heap row reproduces: hashing one returns false with no fault, for
// the caller to fail by name; a lazy sequence too (its elements would run
// JavaScript). Each node is one unit of work; nesting past 128 is the
// catchable StackLimit, as form_equal's.
inline fn rotl(x: u32, n: u32) -> u32 { return (x << n) | (x >> (32 - n)); }
inline fn mix_k1(v: u32) -> u32 { return rotl(v * 3432918353, 15) * 461845907; }
inline fn mix_h1(h: u32, v: u32) -> u32 { return rotl(h ^ v, 13) * 5 + 3864292196; }
fn fmix(h0: u32, length: u32) -> u32 {
    var h = h0 ^ length;
    h = (h ^ (h >> 16)) * 2246822507;
    h = (h ^ (h >> 13)) * 3266489909;
    return h ^ (h >> 16);
}
inline fn hash_int(v: u32) -> u32 { return if v == 0 { 0 } else { fmix(mix_h1(0, mix_k1(v)), 4) }; }
inline fn mix_collection_hash(basis: u32, count: i64) -> u32 { return fmix(mix_h1(0, mix_k1(basis)), (count & 4294967295) as u32); }
// Java's `31 * h + c` over a string's units.
fn string_hash(heap: const Heap&, s: Value) -> u32 {
    var h: u32 = 0;
    if !s.is_string() { return h; }
    let r = header(heap, s.sid());
    if r.wide { for u in wide_run(heap, r) { h = h * 31 + u; } } else { for c in narrow_run(heap, r) { h = h * 31 + c; } }
    return h;
}
// Murmur3 of a name's units, two to a word.
fn hash_chars(heap: const Heap&, s: Value) -> u32 {
    let n = str_len(heap, s); var h: u32 = 0; var i: i64 = 0;
    while i + 1 < n { let a: u32 = unit(heap, s, i); let b: u32 = unit(heap, s, i + 1); h = mix_h1(h, mix_k1(a | (b << 16))); i += 2; }
    if i < n { let last: u32 = unit(heap, s, i); h = h ^ mix_k1(last); }
    return fmix(h, ((n * 2) & 4294967295) as u32);
}
// A symbol's hash (a keyword's adds 0x9e3779b9): the name's hash_chars
// mixed with its namespace's string_hash (0 without one); `seed >> 2` is
// i32's arithmetic shift.
fn hash_named(heap: const Heap&, name: Value, ns: Value) -> u32 {
    let seed = hash_chars(heap, name);
    let shifted = (seed >> 2) | (if seed >= 2147483648 { 3221225472 } else { 0 });
    let space = if ns.is_string() { string_hash(heap, ns) } else { 0 };
    return seed ^ (space + 2654435769 + (seed << 6) + shifted);
}
// A number's: NaN and the infinities fixed words, a safe integer its
// remainder by 2^31 - 1 (truncated, so -0 is 0), any other the two
// big-endian words of its little-endian bytes, xored.
fn number_hash(x: f64) -> u32 {
    if x != x { return 2146959360; }
    if x > 1.7976931348623157e308 { return 2146435072; }
    if x < -1.7976931348623157e308 { return 4293918720; }
    if x == trunc(x) && x <= 9007199254740991.0 && x >= -9007199254740991.0 {
        let k = x as i64; let r = k - (k / 2147483647) * 2147483647;
        return ((r + 4294967296) & 4294967295) as u32;
    }
    let one:  f64[1] = [x]; let b = bytes_of(one);
    let b0:   u32    = b[0]; let b1: u32 = b[1]; let b2: u32 = b[2]; let b3: u32 = b[3];
    let b4:   u32    = b[4]; let b5: u32 = b[5]; let b6: u32 = b[6]; let b7: u32 = b[7];
    let high: u32    = (b0 << 24) | (b1 << 16) | (b2 << 8) | b3;
    let low:  u32    = (b4 << 24) | (b5 << 16) | (b6 << 8) | b7;
    return high ^ low;
}
recursive fn form_hash(heap: Heap&, v: Value, depth: i64 = 0) -> u32, bool {
    if !charge(heap) { return 0, false; }
    if depth >= 128 { heap.fault = Fault.StackLimit; return 0, false; }
    let x = form_value(heap, v);
    if heap.fault != Fault.None { return 0, false; }
    match x {
        Undefined, Null => { return 0, true; },
        Bool => { return if x.flag() { 1231 } else { 1237 }, true; },
        Number => { return number_hash(x.num()), true; },
        String => { if !charge(heap, str_len(heap, x)) { return 0, false; } return hash_int(string_hash(heap, x)), true; },
        Object => {},
        _ => { return 0, false; },
    }
    let n = node(heap, x.id());
    if n.kind == node_kind::Keyword || n.kind == node_kind::Symbol {
        if !charge(heap, str_len(heap, n.value) + str_len(heap, n.namespace_value)) { return 0, false; }
        let h = hash_named(heap, n.value, n.namespace_value);
        return if n.kind == node_kind::Keyword { h + 2654435769 } else { h }, true;
    }
    if n.kind == node_kind::List || n.kind == node_kind::Vector || n.kind == node_kind::Concat {
        var h: u32 = 1;
        for i in n.count { let c, ok = form_hash(heap, slot(heap, n.start + i), depth + 1); if !ok { return 0, false; } h = h * 31 + c; }
        return mix_collection_hash(h, n.count), true;
    }
    if n.kind == node_kind::Map {
        var sum: u32 = 0;
        for i in (n.count / 2) {
            let k, kok = form_hash(heap, slot(heap, n.start + i * 2), depth + 1); if !kok { return 0, false; }
            let w, wok = form_hash(heap, slot(heap, n.start + i * 2 + 1), depth + 1); if !wok { return 0, false; }
            sum = sum + mix_collection_hash(31 * (31 + k) + w, 2);
        }
        return mix_collection_hash(sum, n.count / 2), true;
    }
    if n.kind == node_kind::Set {
        var sum: u32 = 0;
        for i in n.count { let k, ok = form_hash(heap, slot(heap, n.start + i), depth + 1); if !ok { return 0, false; } sum = sum + k; }
        return mix_collection_hash(sum, n.count), true;
    }
    return 0, false;
}
// The order key of a hash-ordered map's key or set's member with hash h
// (the Pants engine's legacy_order, ClojureScript's PersistentHashMap
// traversal): nil first, then the five-bit trie paths of h compared from
// the low bits, not numerically.
fn legacy_order_key(heap: Heap&, k: Value, h: u32) -> u64 {
    if form_value(heap, k).tag() <= value_tag::Null { return 0; }
    var key: u64 = 1;
    var rest = h;
    for i in 7 { let chunk: u64 = rest & 31; key = (key << 5) | chunk; rest = rest >> 5; }
    return key;
}
// A hash-ordered map's entry or set's member: its legacy_order_key and its
// position in the values run.
struct OrderKey { key: u64, index: i64 }
// The entries of map or set `n` (by position in its values run, a map's
// entry i at 2i) in its traversal order, as the V8 build's runtime walks
// it for printing, seq and its form bridge: insertion order, or for a
// hash-ordered one its keys sorted by legacy_order_key of form_hash, equal
// keys (a full hash collision) in insertion order. False when a key does
// not hash here (form_hash), with no fault, or on a work or stack fault.
fn traversal_order(heap: Heap&, n: Node, order: i64[>..]&) -> bool {
    let stride:  i64 = if n.kind == node_kind::Map { 2 } else { 1 };
    let entries: i64 = n.count / stride; let start: i64 = n.start;
    order.clear();
    if !hash_ordered(n) || entries < 2 { for i in entries { order.push(i); } return true; }
    var keys: OrderKey[>..] = [];
    for i in entries {
        let k = slot(heap, start + i * stride);
        let h, ok = form_hash(heap, k);
        if !ok || heap.fault != Fault.None { return false; }
        keys.push(OrderKey { key: legacy_order_key(heap, k, h), index: i });
    }
    if !charge(heap, entries) { return false; }
    sort(keys) { x, y => x.key < y.key || x.key == y.key && x.index < y.index };
    for o in keys { order.push(o.index); }
    return true;
}
fn form_count(heap: Heap&, v: Value) -> i64 {
    let x = form_value(heap, v);
    if x.tag() <= value_tag::Null { return 0; }
    if x.is_string() { return str_len(heap, x); }
    if x.is_object() { let n = node(heap, x.id()); if n.kind >= node_kind::List && n.kind <= node_kind::Set || n.kind == node_kind::Concat { return if n.kind == node_kind::Map { n.count / 2 } else { n.count }; } }
    heap.fault = Fault.TypeError; return 0;
}
fn form_nth(heap: Heap&, v: Value, i: i64, fallback: Value = Value.Null) -> Value {
    let x = form_value(heap, v);
    if x.tag() <= value_tag::Null { return fallback; }
    if x.is_string() { return if i >= 0 && i < str_len(heap, x) { unit_string(heap, x, i) } else { fallback }; }
    if x.is_object() { let n = node(heap, x.id()); if n.kind == node_kind::List || n.kind == node_kind::Vector || n.kind == node_kind::Concat { return if i >= 0 && i < n.count { slot(heap, n.start + i) } else { fallback }; } }
    heap.fault = Fault.TypeError; return Value.Undefined;
}
recursive fn form_equal(heap: Heap&, a: Value, b: Value, depth: i64 = 0) -> bool {
    if !charge(heap) { return false; }
    if depth >= 128 { heap.fault = Fault.StackLimit; return false; }
    if !valid(heap, a) || !valid(heap, b) { return false; }
    if a.is_object() && a == b { return true; }
    let x = form_value(heap, a); let y = form_value(heap, b);
    if x.tag() <= value_tag::Null && y.tag() <= value_tag::Null { return true; }
    if x.tag() != y.tag() { return false; }
    if !x.is_object() { return equal(heap, x, y); }
    let n = node(heap, x.id()); let m = node(heap, y.id());
    if n.kind == node_kind::LazySeq || m.kind == node_kind::LazySeq { heap.fault = Fault.NativeBoundary; return false; }
    if n.kind < node_kind::List || m.kind < node_kind::List || n.kind == node_kind::Error || m.kind == node_kind::Error || n.kind == node_kind::Arguments || m.kind == node_kind::Arguments || n.kind >= node_kind::Generator || m.kind >= node_kind::Generator { return x == y; }
    // An atom is equal only to itself; two references to one cell are one atom.
    if n.kind >= node_kind::KeyList || m.kind >= node_kind::KeyList { return x == y || n.kind == node_kind::Cell && m.kind == node_kind::Cell && n.function == m.function; }
    if n.kind != m.kind && !((n.kind == node_kind::List || n.kind == node_kind::Vector || n.kind == node_kind::Concat) && (m.kind == node_kind::List || m.kind == node_kind::Vector || m.kind == node_kind::Concat)) { return false; }
    if n.kind == node_kind::Symbol || n.kind == node_kind::Keyword { return equal(heap, n.value, m.value) && equal(heap, n.namespace_value, m.namespace_value); }
    if n.count != m.count { return false; }
    if n.kind == node_kind::Map || n.kind == node_kind::Set {
        let stride: i64 = if n.kind == node_kind::Map { 2 } else { 1 };
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
    if kind != node_kind::Vector && kind != node_kind::Map { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = node(heap, x.id()); var at: i64 = -1;
    if kind == node_kind::Vector {
        let i = form_value(heap, k);
        if !i.is_number() || !(i.num() >= 0 && i.num() <= n.count) || i.num() != (i.num() as i64) { heap.fault = Fault.TypeError; return Value.Undefined; }
        at = i.num() as i64;
    } else {
        for i in (n.count / 2) { if form_equal(heap, slot(heap, n.start + i * 2), k) { at = i * 2 + 1; break; } }
    }
    if !charge(heap, n.count + 2) { return Value.Undefined; }
    let replacing = at >= 0 && at < n.count;
    // A ninth entry promotes an insertion-ordered map to hash order, its
    // entries rebuilt as the V8 build's hash map takes them (the Pants
    // engine's map_assoc): equal keys (reader-distinct, equal once read)
    // keep the first key and the last value. Metadata stays.
    if kind == node_kind::Map && !replacing && n.count / 2 >= 8 && n.length == map_order::Insertion {
        if !values_fit(heap, n.count + 2) { memory(heap); return Value.Undefined; }
        var pairs: Value[>..] = [];
        pairs.append(run(heap, n.start, n.count)); pairs.push(k); pairs.push(value);
        var promoted = n; promoted.origin = -1; promoted.length = map_order::Hash;
        return form_construct(heap, node_kind::Map, pairs, promoted);
    }
    let count = n.count + (if replacing { 0 } else if kind == node_kind::Map { 2 } else { 1 });
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
        if kind == node_kind::Map && !valid(heap, k) { return Value.Undefined; } if !valid(heap, value) { return Value.Undefined; }
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
        if kind == node_kind::Map { add_values(heap, [k, value]); } else { add_values(heap, [value]); }
    }
    return result;
}
// conj onto nil, a list or a sequence (Concat) prepends and makes a list;
// onto a vector or a set appends, a set's member already there giving the
// set itself (light-cljs-873, as cljs.core's).
fn form_conj(heap: Heap&, v: Value, value: Value) -> Value {
    let x = form_value(heap, v); let found = if x.tag() <= value_tag::Null { node_kind::List } else { form_kind(heap, x) };
    let kind = if found == node_kind::Concat { node_kind::List } else { found };
    if kind != node_kind::List && kind != node_kind::Vector && kind != node_kind::Set { heap.fault = Fault.TypeError; return Value.Undefined; }
    let n = if x.tag() <= value_tag::Null { Node { kind: node_kind::List } } else { node(heap, x.id()) };
    if kind == node_kind::Set {
        for i in n.count { if !charge(heap) { return Value.Undefined; } if form_equal(heap, slot(heap, n.start + i), value) { return v; } if heap.fault != Fault.None { return Value.Undefined; } }
        // A ninth member promotes an insertion-ordered set to hash order,
        // its members rebuilt as the V8 build's hash set takes them (the
        // Pants engine's set_add): of equal members the first stays.
        if n.count >= 8 && n.length == map_order::Insertion {
            if !values_fit(heap, n.count + 1) { memory(heap); return Value.Undefined; }
            var members: Value[>..] = [];
            members.append(run(heap, n.start, n.count)); members.push(value);
            var promoted = n; promoted.origin = -1; promoted.length = map_order::Hash;
            return form_construct(heap, node_kind::Set, members, promoted);
        }
    }
    if !charge(heap, n.count + 1) { return Value.Undefined; }
    let count = n.count + 1;
    if !values_fit(heap, count) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, count) { return Value.Undefined; }
    if kind == node_kind::List && !valid(heap, value) { return Value.Undefined; }
    for child in run(heap, n.start, n.count) { if !valid(heap, child) { return Value.Undefined; } }
    if kind != node_kind::List && !valid(heap, value) { return Value.Undefined; }
    // A sequence's conj is a new list, without the sequence's metadata.
    var made = if found == node_kind::Concat { Node { kind: node_kind::List } } else { n }; made.kind = kind; made.origin = -1; made.start = heap.values.len as u32; made.count = count;
    let result = allocate(heap, made); if heap.fault != Fault.None { return Value.Undefined; }
    if kind == node_kind::List { add_values(heap, [value]); }
    add_values(heap, run(heap, n.start, n.count));
    if kind != node_kind::List { add_values(heap, [value]); }
    return result;
}
fn form_get(heap: Heap&, v: Value, k: Value, fallback: Value) -> Value {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if x.tag() <= value_tag::Null { return fallback; }
    if kind == node_kind::Vector || x.is_string() {
        let i = form_value(heap, k); if !i.is_number() || !(i.num() >= 0 && i.num() < form_count(heap, x)) || i.num() != (i.num() as i64) { return fallback; }
        return form_nth(heap, x, i.num() as i64, fallback);
    }
    // As the V8 build's cljGet (the Pants engine's clj::get): a set's
    // stored member, and `fallback` for a value that is no map, set, vector
    // or string (light-cljs-872). A lazy sequence stays a boundary (form_op).
    if kind == node_kind::LazySeq { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    if kind == node_kind::Set {
        let s = node(heap, x.id());
        for i in s.count { let v = slot(heap, s.start + i); if form_equal(heap, v, k) { return v; } if heap.fault != Fault.None { return Value.Undefined; } }
        return fallback;
    }
    if kind != node_kind::Map { return fallback; }
    let n = node(heap, x.id());
    for i in (n.count / 2) { if form_equal(heap, slot(heap, n.start + i * 2), k) { return slot(heap, n.start + i * 2 + 1); } }
    return fallback;
}
fn form_contains(heap: Heap&, v: Value, k: Value) -> bool {
    let x = form_value(heap, v); let kind = form_kind(heap, x);
    if x.tag() <= value_tag::Null { return false; }
    if kind == node_kind::Vector || x.is_string() {
        let i = form_value(heap, k);
        return i.is_number() && i.num() >= 0 && i.num() < form_count(heap, x) && i.num() == trunc(i.num());
    }
    if kind == node_kind::Map || kind == node_kind::Set {
        let n = node(heap, x.id()); let stride: i64 = if kind == node_kind::Map { 2 } else { 1 };
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
        if x.tag() <= value_tag::Null { continue; }
        let kind = form_kind(heap, x);
        if kind != node_kind::List && kind != node_kind::Vector && kind != node_kind::Concat { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
        let n = node(heap, x.id());
        if !values_fit(heap, count + n.count) { memory(heap); return Value.Undefined; }
        if !charge(heap, n.count * 2) { return Value.Undefined; }
        for child in run(heap, n.start, n.count) { if !valid(heap, child) { return Value.Undefined; } }
        count += n.count;
    }
    let start = heap.values.len;
    let result = allocate(heap, Node { kind: node_kind::Concat, start: start as u32, count: count as u32 });
    if heap.fault != Fault.None { return Value.Undefined; }
    // Preflight above accounted both validation and copying. Append only
    // after every failure point; the inputs are immutable stable ranges.
    for coll in colls {
        let x = form_value(heap, coll); if x.tag() <= value_tag::Null { continue; }
        let n = node(heap, x.id()); add_values(heap, run(heap, n.start, n.count));
    }
    return result;
}
fn form_sequence(heap: Heap&, v: Value, skip: i64, empty_nil: bool) -> Value {
    let x = form_value(heap, v); let count = form_count(heap, x);
    if heap.fault != Fault.None { return Value.Undefined; }
    if count <= skip { return if empty_nil { nil() } else { form_collection(heap, node_kind::List, []) }; }
    let kind = form_kind(heap, x);
    if kind != node_kind::List && kind != node_kind::Vector && kind != node_kind::Concat { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    // An integer range's seq is itself and its rest an integer range, as
    // the V8 build's are.
    let n = node(heap, x.id());
    if integer_range(n) && skip == 0 { return x; }
    return allocate(heap, Node { kind: node_kind::Concat, start: (n.start + skip) as u32, count: (n.count - skip) as u32, length: if integer_range(n) { seq_flavor::IntegerRange } else { seq_flavor::Sequence } });
}
// cljs.core's errors from the light-cljs form operations (light-cljs-873):
// what the V8 build's runtime (src/js_runtime.js nth, count, seq, assoc,
// conj, mapConj and a vector's call) throws for the same operands, a
// js/Error raised here as a catchable HostEffect fault whose message
// core_error words on the fault path. Boundary is an operand that runtime
// accepts and this one does not implement (a JavaScript array, an object's
// iteration or text); it fails NativeBoundary, uncatchably. Classifying
// reads no row the operation would not, and allocates nothing.
enum CoreError { None, Boundary, OutOfBounds, NthIndex, NthType, CountType, NotSeqable, AssocType, VectorKey, ConjType, InvokeKey, MapEntry }
fn core_failed(heap: Heap&, e: CoreError) -> bool {
    if heap.fault != Fault.None { return true; }
    if e == CoreError.None { return false; }
    heap.fault = if e == CoreError.Boundary { Fault.NativeBoundary } else { Fault.HostEffect };
    return true;
}
// Whether nth finds index i (a number) in x, a vector, string, list or
// sequence: a vector's in [0, count) and a string's in (-1, length),
// truncated; a list is walked, so only an integer in [0, count) is found.
fn nth_found(heap: Heap&, x: Value, i: f64) -> bool {
    if x.is_string() { return i > -1 && i < str_len(heap, x); }
    let n = node(heap, x.id());
    if n.kind == node_kind::Vector || integer_range(n) { return i >= 0 && i < n.count; }
    return i >= 0 && i < n.count && i == trunc(i);
}
fn nth_error(heap: Heap&, coll: Value, index: Value, fallback: bool) -> CoreError {
    if !form_value(heap, index).is_number() { return CoreError.NthIndex; }
    let x = form_value(heap, coll); if x.tag() <= value_tag::Null { return CoreError.None; }
    let kind = form_kind(heap, x);
    if x.is_string() || kind == node_kind::List || kind == node_kind::Vector || kind == node_kind::Concat { return if fallback || nth_found(heap, x, form_value(heap, index).num()) { CoreError.None } else { CoreError.OutOfBounds }; }
    return if kind == node_kind::Array { CoreError.Boundary } else { CoreError.NthType };
}
fn count_error(heap: Heap&, coll: Value) -> CoreError {
    let x = form_value(heap, coll); let kind = form_kind(heap, x);
    if x.tag() <= value_tag::Null || x.is_string() || kind >= node_kind::List && kind <= node_kind::Set || kind == node_kind::Concat { return CoreError.None; }
    return if kind == node_kind::Array { CoreError.Boundary } else { CoreError.CountType };
}
// seq's: a number, boolean, keyword or symbol is not seqable, with its
// text in the message; any other object fails by name (an iterable one the
// runtime seqs, another's text is its toString's).
fn seq_error(heap: Heap&, coll: Value) -> CoreError {
    let x = form_value(heap, coll); let kind = form_kind(heap, x);
    if x.tag() <= value_tag::Null || x.is_string() || kind >= node_kind::List && kind <= node_kind::Set || kind == node_kind::Concat || kind == node_kind::LazySeq { return CoreError.None; }
    return if x.is_number() || x.is_bool() || kind == node_kind::Symbol || kind == node_kind::Keyword { CoreError.NotSeqable } else { CoreError.Boundary };
}
// assoc's: nil and a map take any key; a vector a number in [0, count],
// a fractional one inside it (which the runtime's trie lookup resolves)
// failing by name.
fn assoc_error(heap: Heap&, coll: Value, k: Value) -> CoreError {
    let x = form_value(heap, coll); let kind = form_kind(heap, x);
    if x.tag() <= value_tag::Null || kind == node_kind::Map { return CoreError.None; }
    if kind != node_kind::Vector { return CoreError.AssocType; }
    let i = form_value(heap, k); let n = node(heap, x.id()).count;
    if !i.is_number() { return CoreError.VectorKey; }
    if !(i.num() >= 0 && i.num() <= n) { return CoreError.OutOfBounds; }
    return if i.num() == trunc(i.num()) { CoreError.None } else if i.num() < n { CoreError.Boundary } else { CoreError.OutOfBounds };
}
// conj's, and the value its message names: nil, a list, sequence, vector
// or set takes any item; a map takes nil, a vector (its first two items
// the entry, so one of fewer than two is out of bounds) or a seqable of
// such vectors.
fn conj_error(heap: Heap&, coll: Value, item: Value) -> CoreError, Value {
    let x = form_value(heap, coll); let kind = form_kind(heap, x);
    if x.tag() <= value_tag::Null || kind >= node_kind::List && kind <= node_kind::Vector || kind == node_kind::Set || kind == node_kind::Concat { return CoreError.None, Value.Undefined; }
    if kind != node_kind::Map { return CoreError.ConjType, Value.Undefined; }
    let y = form_value(heap, item); let entries = form_kind(heap, y);
    // A hash-ordered map's entries go in the runtime's own iteration order
    // (the Pants engine's trie order, not the printed one), which this VM
    // does not model: into an insertion-ordered map, where it decides the
    // result's order, two or more of them are a boundary.
    if entries == node_kind::Map && hash_ordered(node(heap, y.id())) && node(heap, y.id()).count > 2 && !hash_ordered(node(heap, x.id())) { return CoreError.Boundary, Value.Undefined; }
    if y.tag() <= value_tag::Null || entries == node_kind::Map { return CoreError.None, Value.Undefined; }
    if entries == node_kind::Vector { return if node(heap, y.id()).count < 2 { CoreError.OutOfBounds } else { CoreError.None }, Value.Undefined; }
    if y.is_string() { return if str_len(heap, y) == 0 { CoreError.None } else { CoreError.MapEntry }, Value.Undefined; }
    let e = seq_error(heap, y); if e != CoreError.None { return e, y; }
    // A lazy sequence's entries are not demanded here, and a set's are in
    // the runtime's iteration order (which entry fails first, which of two
    // equal keys wins), which this one does not model.
    if entries == node_kind::LazySeq || entries == node_kind::Set && node(heap, y.id()).count > 0 { return CoreError.Boundary, Value.Undefined; }
    let n = node(heap, y.id());
    for i in n.count {
        let entry = form_value(heap, slot(heap, n.start + i));
        if form_kind(heap, entry) != node_kind::Vector { return CoreError.MapEntry, Value.Undefined; }
        if node(heap, entry.id()).count < 2 { return CoreError.OutOfBounds, Value.Undefined; }
    }
    return CoreError.None, Value.Undefined;
}
// A vector's call: a number index, found as nth finds it.
fn invoke_error(heap: Heap&, v: Value, index: Value) -> CoreError {
    let i = form_value(heap, index);
    if !i.is_number() { return CoreError.InvokeKey; }
    return if nth_found(heap, form_value(heap, v), i.num()) { CoreError.None } else { CoreError.OutOfBounds };
}
// conj onto a map of an item conj_error admitted: each entry assoc'd in
// order.
fn map_conj(heap: Heap&, m: Value, item: Value) -> Value {
    let y = form_value(heap, item); let kind = form_kind(heap, y);
    if y.tag() <= value_tag::Null || y.is_string() { return m; }
    let n = node(heap, y.id());
    if kind == node_kind::Vector { return form_assoc(heap, m, slot(heap, n.start), slot(heap, n.start + 1)); }
    var out = m;
    if kind == node_kind::Map {
        for i in (n.count / 2) { out = form_assoc(heap, out, slot(heap, n.start + i * 2), slot(heap, n.start + i * 2 + 1)); if heap.fault != Fault.None { return Value.Undefined; } }
        return out;
    }
    for i in n.count {
        let entry = node(heap, form_value(heap, slot(heap, n.start + i)).id());
        out = form_assoc(heap, out, slot(heap, entry.start), slot(heap, entry.start + 1)); if heap.fault != Fault.None { return Value.Undefined; }
    }
    return out;
}
// cljs.core's "v is not ISeqable" for v, a number, boolean, keyword or
// symbol (seq_error's NotSeqable): its text is String(v), a keyword's
// :ns/name and a symbol's ns/name.
fn not_seqable(job: Job&, v: Value) {
    let heap .= job.heap; let x = form_value(heap, v);
    job.detail.clear();
    if x.is_number() { var digits: u8[>..] = []; number::append_decimal(digits, x.num()); for c in digits { job.detail.push(c); } }
    else if x.is_bool() { for c in (if x.flag() { "true" } else { "false" }) { job.detail.push(c); } }
    else {
        let n = node(heap, x.id());
        if n.kind == node_kind::Keyword { job.detail.push(58); }
        if n.namespace_value.is_string() { append_utf16(heap, n.namespace_value, job.detail); job.detail.push(47); }
        append_utf16(heap, n.value, job.detail);
    }
    for c in " is not ISeqable" { job.detail.push(c); }
    heap.fault = Fault.HostEffect;
}
// Whether v is a source a runtime leaf that seqs it (mapv, into, vec,
// sequence, get-in's path) can take; otherwise its error is raised, as
// seq_error classifies it.
fn seq_source(job: Job&, v: Value) -> bool {
    let e = seq_error(job.heap, v);
    if job.heap.fault != Fault.None { return false; }
    if e == CoreError.NotSeqable { not_seqable(job, v); return false; }
    if e == CoreError.Boundary { job.heap.fault = Fault.NativeBoundary; return false; }
    return true;
}
// The message of a CoreError the form operation `op` raised on the operands
// at `base` (the fault path, before they are popped).
fn core_error(job: Job&, op: i64, base: i64) {
    let heap .= job.heap; let args = job.operands[base..];
    heap.fault = Fault.None;
    var e = CoreError.None; var culprit = args[0];
    match op {
        form_op::CljNth, form_op::CljNthOr => { e = nth_error(heap, args[0], args[1], op == form_op::CljNthOr); },
        form_op::CljCount => { e = count_error(heap, args[0]); },
        form_op::CljSeq, form_op::CljRest, form_op::CljNext, form_op::CljFirst => { e = seq_error(heap, args[0]); },
        form_op::CljAssoc => { e = assoc_error(heap, args[0], args[1]); },
        form_op::CljConj => { let c, v = conj_error(heap, args[0], args[1]); e = c; culprit = v; },
        form_op::CljInvoke => { e = invoke_error(heap, args[0], args[1]); },
        _ => {},
    }
    if heap.fault != Fault.None { return; }
    if e == CoreError.NotSeqable { not_seqable(job, culprit); return; }
    core_raise(job, e);
}
// Raise CoreError e, one whose message is fixed text.
fn core_raise(job: Job&, e: CoreError) -> Value {
    let words: const u8[:] = match e {
        OutOfBounds => "Index out of bounds", NthIndex => "Index argument to nth must be a number", NthType => "nth not supported on this type",
        CountType => "count not supported on this type", AssocType => "assoc not supported on this type", VectorKey => "Vector key must be a number",
        ConjType => "conj not supported on this type", InvokeKey => "Key must be integer", MapEntry => "Map conj requires vector entries",
        _ => "",
    };
    // Only a raised CoreError reaches here.
    if words.len == 0 { job.heap.fault = Fault.InvalidState; return Value.Undefined; }
    job.detail.clear(); for c in words { job.detail.push(c); }
    job.heap.fault = Fault.HostEffect;
    return Value.Undefined;
}
fn named_supported(heap: Heap&, ns: Value, name: Value) -> bool {
    let n = utf16(heap, name);
    if !charge(heap, n.len + str_len(heap, ns)) { return false; }
    if ns.is_string() { for i in str_len(heap, ns) { if unit(heap, ns, i) == 47 { return false; } } if str_len(heap, ns) == 0 && n.len == 0 { return false; } }
    else { for c in n { if c == 47 && n.len != 1 { return false; } } }
    return true;
}
fn form_op(heap: Heap&, op: i64, args: const Value[:]) -> Value {
    for v in args {
        if !valid(heap, v) { return Value.Undefined; }
        // Partial/lazy protocol operations are outside this full-demand
        // target slice. Never inspect an uninitialized memo as a form.
        if v.is_object() && node(heap, v.id()).kind == node_kind::LazySeq { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    }
    let a = args[0];
    match op {
        form_op::Keyword, form_op::Symbol => {
            if (a.tag() > value_tag::Null && !a.is_string()) || !args[1].is_string() { heap.fault = Fault.TypeError; return Value.Undefined; }
            if !named_supported(heap, a, args[1]) { if heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; } return Value.Undefined; }
            return allocate(heap, Node { kind: if op == form_op::Keyword { node_kind::Keyword } else { node_kind::Symbol }, value: args[1], namespace_value: if a.tag() <= value_tag::Null { nil() } else { a } });
        },
        form_op::Kind => { let kind = form_kind(heap, a); return match kind { node_kind::List, node_kind::Concat => text(heap, "list"), node_kind::Vector => text(heap, "vector"), node_kind::Map => text(heap, "map"), node_kind::Set => text(heap, "set"), node_kind::Symbol => text(heap, "symbol"), node_kind::Keyword => text(heap, "keyword"), _ => nil() }; },
        form_op::Count => { return number(form_count(heap, a)); },
        form_op::Nth => { let i = form_value(heap, args[1]); if !i.is_number() || !(i.num() >= -2147483648.0 && i.num() <= 2147483647.0) || i.num() != (i.num() as i64) || i.num() < 0 || i.num() >= form_count(heap, a) { heap.fault = Fault.TypeError; return Value.Undefined; } return form_nth(heap, a, i.num() as i64); },
        form_op::CljCount => { if core_failed(heap, count_error(heap, a)) { return Value.Undefined; } return number(form_count(heap, a)); },
        form_op::CljNth, form_op::CljNthOr => {
            if core_failed(heap, nth_error(heap, a, args[1], op == form_op::CljNthOr)) { return Value.Undefined; }
            let x = form_value(heap, a); let i = form_value(heap, args[1]).num();
            let fallback = if op == form_op::CljNthOr { args[2] } else { nil() };
            if x.tag() <= value_tag::Null || !nth_found(heap, x, i) { return fallback; }
            return form_nth(heap, x, trunc(i) as i64);
        },
        form_op::Get, form_op::CljGet => { return form_get(heap, a, args[1], args[2]); },
        form_op::CljGet2 => { return form_get(heap, a, args[1], nil()); },
        form_op::CljContains => { return boolean(form_contains(heap, a, args[1])); },
        form_op::CljConcat => { return form_concat(heap, args); },
        form_op::CljConcatAll => {
            let x = form_value(heap, a); if x.tag() <= value_tag::Null { return form_concat(heap, []); }
            let kind = form_kind(heap, x);
            if kind != node_kind::List && kind != node_kind::Vector && kind != node_kind::Concat { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
            let n = node(heap, x.id()); return form_concat(heap, run(heap, n.start, n.count));
        },
        form_op::Assoc => { return form_assoc(heap, a, args[1], args[2]); },
        form_op::CljAssoc => {
            if core_failed(heap, assoc_error(heap, a, args[1])) { return Value.Undefined; }
            // assoc of nil is a one-entry map, as cljs.core's.
            if form_value(heap, a).tag() <= value_tag::Null { return form_collection(heap, node_kind::Map, [args[1], args[2]]); }
            return form_assoc(heap, a, args[1], args[2]);
        },
        form_op::Conj => { return form_conj(heap, a, args[1]); },
        form_op::CljConj => {
            let e, culprit = conj_error(heap, a, args[1]);
            if core_failed(heap, e) { return Value.Undefined; }
            if form_kind(heap, a) == node_kind::Map { return map_conj(heap, a, args[1]); }
            return form_conj(heap, a, args[1]);
        },
        form_op::Equals => { return boolean(form_equal(heap, a, args[1])); },
        form_op::Meta, form_op::CljMeta => { return if a.is_object() && !node(heap, a.id()).metadata.is_undefined() { node(heap, a.id()).metadata } else { nil() }; },
        form_op::WithMeta, form_op::CljWithMeta => { let kind = form_kind(heap, a); let meta = form_value(heap, args[1]); if !(kind >= node_kind::List && kind <= node_kind::Symbol || kind == node_kind::Concat) || (meta.tag() > value_tag::Null && form_kind(heap, meta) != node_kind::Map) { heap.fault = Fault.TypeError; return Value.Undefined; } var n = node(heap, a.id()); n.metadata = if meta.tag() <= value_tag::Null { Value.Undefined } else { args[1] }; n.origin = -1; return allocate(heap, n); },
        form_op::Value => { return form_value(heap, a); },
        form_op::CljTruth => { let v = form_value(heap, a); return boolean(v.tag() > value_tag::Null && !(v.is_bool() && !v.flag())); },
        form_op::CljSeq, form_op::CljRest, form_op::CljNext => {
            if core_failed(heap, seq_error(heap, a)) { return Value.Undefined; }
            return form_sequence(heap, a, if op == form_op::CljSeq { 0 } else { 1 }, op != form_op::CljRest);
        },
        form_op::CljFirst => {
            if core_failed(heap, seq_error(heap, a)) { return Value.Undefined; }
            // A map's first entry and a set's first member are in the
            // runtime's iteration order, which this one does not model.
            let kind = form_kind(heap, a);
            if kind == node_kind::Map || kind == node_kind::Set { if form_count(heap, a) == 0 { return nil(); } heap.fault = Fault.NativeBoundary; return Value.Undefined; }
            return form_nth(heap, a, 0);
        },
        form_op::LiteralMap => { let v = form_value(heap, a); let kind = form_kind(heap, v); if kind != node_kind::List && kind != node_kind::Vector { heap.fault = Fault.TypeError; return Value.Undefined; } let n = node(heap, v.id()); if n.count == 0 { return empty_literal(heap, node_kind::Map); } return form_collection(heap, node_kind::Map, run(heap, n.start, n.count)); },
        // A map literal with entries in a native macro unit (light-cljs-872):
        // insertion-ordered, equal keys keeping the first key and the last
        // value (the Pants engine's array_map_from_sequence).
        form_op::ArrayMap => { let v = form_value(heap, a); let kind = form_kind(heap, v); if kind != node_kind::List && kind != node_kind::Vector { heap.fault = Fault.TypeError; return Value.Undefined; } let n = node(heap, v.id()); return form_construct(heap, node_kind::Map, run(heap, n.start, n.count)); },
        // A hash-map literal (light-cljs-877): its keys, then its values, as
        // two lists, made a map as the Pants engine's clj::zip_map makes it
        // (new_map: equal keys keep the first key and the last value, hash
        // order at any size).
        form_op::CljZipMap => {
            let ks = form_value(heap, a); let vs = form_value(heap, args[1]);
            let kk = form_kind(heap, ks); let vk = form_kind(heap, vs);
            if kk != node_kind::List && kk != node_kind::Vector || vk != node_kind::List && vk != node_kind::Vector { heap.fault = Fault.TypeError; return Value.Undefined; }
            let kn = node(heap, ks.id()); let vn = node(heap, vs.id());
            if kn.count != vn.count { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
            if !charge(heap, kn.count * 2) { return Value.Undefined; }
            var pairs: Value[>..] = [];
            for i in kn.count { pairs.push(slot(heap, kn.start + i)); pairs.push(slot(heap, vn.start + i)); }
            return form_ordered(heap, node_kind::Map, pairs, true);
        },
        form_op::ReaderCollection => { if form_kind(heap, a) != node_kind::Map && form_kind(heap, a) != node_kind::Set { heap.fault = Fault.TypeError; return Value.Undefined; } return a; },
        form_op::CljCallable => { let kind = form_kind(heap, a); return boolean(kind == node_kind::Vector || kind == node_kind::Map || kind == node_kind::Set || kind == node_kind::Symbol || kind == node_kind::Keyword); },
        form_op::CljCallMember => {
            let value = form_value(heap, a);
            if value.is_object() && (node(heap, value.id()).kind == node_kind::Closure || node(heap, value.id()).kind == node_kind::Builtin || node(heap, value.id()).kind == node_kind::BoundFunction) { return heap.call_method; }
            heap.fault = Fault.NativeBoundary; return Value.Undefined;
        },
        form_op::CljInvoke => {
            let arity = args[3]; let kind = form_kind(heap, a);
            // A vector's call takes one index and ignores any other
            // argument, as the runtime's (light-cljs-873).
            if kind == node_kind::Vector {
                if core_failed(heap, invoke_error(heap, a, args[1])) { return Value.Undefined; }
                return form_nth(heap, a, trunc(form_value(heap, args[1]).num()) as i64);
            }
            if !arity.is_number() || (arity.num() != 1 && arity.num() != 2) { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
            if kind == node_kind::Symbol || kind == node_kind::Keyword { return form_get(heap, args[1], a, args[2]); }
            if kind == node_kind::Map { return form_get(heap, a, args[1], args[2]); }
            if kind == node_kind::Set {
                let n = node(heap, a.id());
                for i in n.count { if !charge(heap) { return Value.Undefined; } let v = slot(heap, n.start + i); if form_equal(heap, v, args[1]) { return v; } if heap.fault != Fault.None { return Value.Undefined; } }
                return args[2];
            }
            heap.fault = Fault.NativeBoundary; return Value.Undefined;
        },
        form_op::CljReadRoot => {
            if !a.is_object() || node(heap, a.id()).kind != node_kind::Object || args[1].tag() > value_tag::Null { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
            if !args[2].is_string() { heap.fault = Fault.TypeError; return Value.Undefined; }
            let value, found = own_property(heap, a, args[2]);
            // A compiler root read is a dependency request, even if the JS
            // value would otherwise be undefined. No live-job parking/replay
            // is supported: prerequisites must be resolved before admission.
            if !found && heap.fault == Fault.None { heap.fault = Fault.DependencyNeed; } return value;
        },
        form_op::FnApplyNull => {
            if !a.is_object() || node(heap, a.id()).kind != node_kind::Closure { heap.fault = Fault.TypeError; return Value.Undefined; }
            var row = node(heap, a.id()); row.immutable = true;
            return allocate(heap, row);
        },
        form_op::FnApplyReceiver => { return if a.is_object() && node(heap, a.id()).kind == node_kind::Closure && node(heap, a.id()).immutable { nil() } else { a }; },
        // cljRealized of a delay (light-cljs-808; a lazy sequence stays a
        // boundary here).
        form_op::CljRealized => { return delay_realized(heap, a); },
        _ => { heap.fault = Fault.NativeBoundary; return Value.Undefined; },
    }
}
// A bare built-in function object of code `id` (realm.goose BUILTINS), for
// compiler services and tests; the realm and boot_runtime make their own.
fn builtin(heap: Heap&, id: i64) -> Value { return allocate(heap, Node { kind: node_kind::Builtin, function: id as u32 }); }
fn install_field(heap: Heap&, object: Value, name: const u8[:], value: Value) -> bool { let k = text(heap, name); return put(heap, object, k, value); }
// The runtime leaves that run no JavaScript, by code (realm.goose
// RUNTIME_LEAVES; call_builtin dispatches the realm's).
fn native_value(heap: Heap&, id: i64, args: const Value[:]) -> Value {
    if id == 18 { return form_construct(heap, node_kind::List, args); }
    if id == 19 && args.len == 2 { return form_op(heap, form_op::Symbol, args); }
    if id == 17 && args.len == 2 {
        // The generated target maps a nonchunked finite rest list. Vectors,
        // transducers, multiple sources and nested lazy inputs have different
        // demand/chunk semantics and remain explicit boundaries.
        let input = args[1]; let kind = form_kind(heap, input);
        if input.tag() > value_tag::Null && kind != node_kind::List && kind != node_kind::Concat { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
        let count = form_count(heap, input);
        if heap.fault != Fault.None { return Value.Undefined; }
        if !values_fit(heap, count) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
        if !charge(heap, count) { return Value.Undefined; }
        let start = add_values(heap, [], count);
        return allocate(heap, Node { kind: node_kind::LazySeq, start: start as u32, count: count as u32, value: args[0], namespace_value: input });
    }
    if id == 16 && args.len == 1 {
        let kind = form_kind(heap, args[0]); if heap.fault != Fault.None { return Value.Undefined; }
        if kind == node_kind::List || kind == node_kind::Concat || kind == node_kind::LazySeq { return args[0]; }
        return form_sequence(heap, args[0], 0, false);
    }
    if id == 15 && args.len == 2 {
        let object = args[0]; let skip = args[1];
        if !valid(heap, object) || !object.is_object() || node(heap, object.id()).kind != node_kind::Arguments || !skip.is_number() || !(skip.num() >= 0 && skip.num() <= 65535) || skip.num() != (skip.num() as i64) { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
        // An arguments object as NewArguments made it (its template's shape,
        // `length` its argument count) is read from its run; any other
        // through its `length` and elements (light-cljs-854).
        let o: i64 = object.id(); let n .= node(heap, o);
        let length = n.slots[0];
        if heap.global.is_object() && !n.initialized && (n.shape == node(heap, arguments_template_row(false)).shape || n.shape == node(heap, arguments_template_row(true)).shape) && length.is_number() && length.num() == n.count {
            let count: i64 = n.count; let offset = min(count, skip.num() as i64); var whole = true;
            for v in run(heap, n.start + offset, count - offset) { if v.is_hole() { whole = false; } }
            if whole { return form_collection(heap, node_kind::List, run(heap, n.start + offset, count - offset)); }
        }
        let k = key_named(heap, "length");
        let len = if heap.fault == Fault.None { get(heap, object, k) } else { Value.Undefined };
        if heap.fault != Fault.None { return Value.Undefined; }
        // ToLength, as Array.prototype.slice.call(args, start) gives it; an
        // object length (ToPrimitive, a frame) fails by name, a symbol is a
        // TypeError.
        if len.is_object() || len.is_bigint() { heap.fault = Fault.UnsupportedOperand; return Value.Undefined; }
        if len.is_symbol() { heap.fault = Fault.TypeError; return Value.Undefined; }
        let n = numeric(heap, len);
        if heap.fault != Fault.None { return Value.Undefined; }
        let end: i64 = if !(n > 0) { 0 } else { min(trunc(n), max_safe_integer()) as i64 };
        let first = min(end, skip.num() as i64);
        if !values_fit(heap, end - first) { memory(heap); return Value.Undefined; }
        var values: Value[>..] = [];
        for i in first..end {
            let e = object_element(heap, o, i);
            if !e.is_hole() { values.push(e); continue; }
            let ik = index_key(heap, i);
            let v = if heap.fault == Fault.None { get(heap, object, ik) } else { Value.Undefined };
            if heap.fault != Fault.None { return Value.Undefined; }
            values.push(v);
        }
        return form_collection(heap, node_kind::List, values);
    }
    if id >= 10 && id <= 13 && args.len == 1 {
        let array = args[0];
        if !valid(heap, array) || !array.is_object() || node(heap, array.id()).kind != node_kind::Array { heap.fault = Fault.TypeError; return Value.Undefined; }
        let count = node(heap, array.id()).length;
        if !values_fit(heap, count) { memory(heap); return Value.Undefined; }
        if !charge(heap, count) { return Value.Undefined; }
        var values: Value[>..] = [];
        for i in count { let v, found = own_element(heap, array, i); values.push(v); }
        return form_ordered(heap, if id == 10 { node_kind::Vector } else if id == 11 { node_kind::List } else if id == 12 { node_kind::Map } else { node_kind::Set }, values, id == 12 && values.len > 16);
    }
    if id == 0 && args.len == 1 { return args[0]; }
    // inc is the runtime's `x => x + 1`: a string concatenates "1" (it does
    // not convert by ToNumber), as `+` does.
    if id == 1 && args.len == 1 { return binary_value(heap, opcode::Plus, args[0], number(1)); }
    if id == 3 && args.len == 1 {
        let kind = form_kind(heap, args[0]); if kind == node_kind::Vector { return args[0]; }
        if kind == node_kind::List || kind == node_kind::Concat { let n = node(heap, args[0].id()); return form_collection(heap, node_kind::Vector, run(heap, n.start, n.count)); }
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
        let kind = if a.is_object() { node(heap, a.id()).kind } else { node_kind::None };
        if a.is_string() { append_utf16(heap, a, units); }
        else if kind == node_kind::Symbol || kind == node_kind::Keyword {
            let n = node(heap, a.id()); if kind == node_kind::Keyword { units.push(58); }
            if n.namespace_value.is_string() { append_utf16(heap, n.namespace_value, units); units.push(47); }
            append_utf16(heap, n.value, units);
        } else { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    }
    for b in job.gensym_prefix { units.push(b); }
    for b in str(job.gensym_next) { units.push(b); }
    // V8 reads a name with a slash as a namespace and a name; not supported here.
    for u in units { if u == 47 { heap.fault = Fault.NativeBoundary; return Value.Undefined; } }
    if !charge(heap, units.len) { return Value.Undefined; }
    let name = allocate(heap, Node { kind: node_kind::Symbol, value: string(heap, units), namespace_value: nil() });
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
    if !valid(heap, value) || !value.is_object() || node(heap, value.id()).kind != node_kind::LazySeq { return false; }
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
        if nested && value.is_object() {
            let n = node(job.heap, value.id());
            if n.kind == node_kind::List || n.kind == node_kind::Vector || n.kind == node_kind::Concat {
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
        Convert c => { convert_step(job, frame, c.hint, c.finish, c.count, c.method); },
        // The setter returned: drop its value, deliver the assignment's; or
        // an array length's conversion finished (length_step).
        Setter => { if frame.pc == setter_pc::Length { length_step(job, frame); } else { job.operands.pop(); finish_native(job, frame, frame.output); } },
        CopyData => { copy_step(job, frame); },
        ArrayCallbacks a => { array_callbacks_step(job, frame, a.which); },
        Swap s => { swap_step(job, frame, s.which); },
        Force x => { force_step(job, frame, x.which); },
        Resume => { resume_step(job, frame); },
        HasInstance => { has_instance_step(job, frame); },
        GetIterator => { get_iterator_step(job, frame); },
        ArrayMethod m => { array_method_step(job, frame, m.which); },
        BuiltinIterator m => { iterator_step(job, frame, m.which); },
        ForIn => { for_in_step(job, frame); },
        Proxy p => { proxy_step(job, frame, p.method); },
        Forward w => { forward_step(job, frame, w.construct); },
        Bind => { bind_step(job, frame); },
        FunctionText => { function_text_step(job, frame); },
        DynamicFunction => { dynamic_function_step(job, frame); },
        ErrorConstruct e => { error_construct_step(job, frame, e.which); },
        ErrorText => { error_text_step(job, frame); },
        Descriptor d => { descriptor_step(job, frame, d.which); },
        Statics s => { statics_step(job, frame, s.which); },
        Construct => { construct_step(job, frame); },
        None => {},
    }
    if heap.fault != Fault.None { fail(job, heap.fault, frame); }
}
// Run an accessor's setter for an assignment whose result is `result`
// (Native.Setter): the native frame, then the setter's frame above it with
// the object as receiver and the value as its argument.
fn call_setter(job: Job&, setter: Value, object: Value, value: Value, result: Value) {
    let heap .= job.heap;
    // %ThrowTypeError% (Function.prototype.caller and .arguments) throws in place.
    if is_throw_type_error(heap, setter) { throw_type_error(job, object, 1); return; }
    if !setter.is_object() { heap.fault = Fault.UnsupportedOperand; return; }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: setter_pc::Setter, local_base: job.locals.len, operand_base: job.operands.len, callee: setter, receiver: object, output: result, native: Native.Setter });
    if node(heap, setter.id()).kind == node_kind::Closure { if !enter(job, setter, object, [value]) { job.frames.pop(); } return; }
    // Any other setter (a built-in, a bound function; light-cljs-881) runs
    // on the general call path; its result arrives for the Setter frame.
    if !valid(heap, object) || !valid(heap, value) { job.frames.pop(); return; }
    call_accessor(job, setter, object, [value]);
}
// A Setter frame's pc: a setter is running above it, or (Length) a Convert
// frame converting an array's new `length` (start_length).
let setter_pc::Setter = 1;
let setter_pc::Length = 2;
// Whether [[Set]] of key k to `value` on `object` writes an array's
// `length` with an object value, which converts first (ArraySetLength's
// ToNumber, ECMAScript 10.4.2.4; light-cljs-893): ToPrimitive with hint
// number runs in a Convert frame above the operation. A read-only length
// refuses the write before any conversion (OrdinarySet), and a primitive
// converts in array_length. The value's tag is tested first, so a write of
// a primitive pays nothing more.
fn length_converts(heap: Heap&, object: Value, k: Value, value: Value) -> bool {
    if !value.is_object() || !object.is_object() || object.id() >= heap.nodes.len || node(heap, object.id()).kind != node_kind::Array || read_only_length(heap, object.id()) || !convertible(heap, value) { return false; }
    return key_is(heap, k, "length");
}
// An assignment's array `length` write of an object (length_converts): a
// Setter frame at pc Length holding the array, key k, the assignment's
// result and whether it is strict, with two copies of the value converting
// above it, in order (ToPrimitive with hint number, then ToNumber, each):
// ArraySetLength's ToUint32 and ToNumber both convert the original value
// (steps 3 and 4), as QuickJS does. length_step compares and writes.
fn start_length(job: Job&, object: Value, k: Value, value: Value, result: Value, strict: bool) {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: setter_pc::Length, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: object, input: k, output: result, index: if strict { 1 } else { 0 }, native: Native.Setter });
    job.operands.push(value); job.operands.push(value);
    start_convert(job, Hint.Number, true, 2, 2);
}
// ArraySetLength steps 3 to 5 once both conversions of an object length
// are numbers (on the operand top, the first below): the first's ToUint32,
// which must equal the second, else a RangeError, checked before the
// length's writability (light-cljs-893).
fn converted_length(job: Job&) -> Value {
    let second = job.operands.pop(); let first = job.operands.pop();
    if !first.is_number() || !second.is_number() { job.heap.fault = Fault.InvalidState; return Value.Undefined; }
    let u = to_uint32(first.num()) as f64;
    if u != second.num() { job.detail.clear(); for c in "invalid array length" { job.detail.push(c); } job.heap.fault = Fault.RangeError; return Value.Undefined; }
    return number(u);
}
// Whether a converted length n leaves `object`'s read-only length as it is:
// ArraySetLength's OrdinaryDefineOwnProperty accepts an unchanged value
// (a conversion may have made the length read-only).
fn length_kept(heap: Heap&, object: Value, n: Value) -> bool {
    return read_only_length(heap, object.id()) && (node(heap, object.id()).length as f64) == n.num();
}
// The converted length arrived: [[Set]] it as set_property does, then
// deliver the assignment's result. The conversion may have changed the
// array (frozen it, grown it); the write sees it as it is now.
fn length_step(job: Job&, frame: Frame) {
    let heap .= job.heap;
    let n = converted_length(job);
    if heap.fault != Fault.None { return; }
    let object = frame.receiver; let k = frame.input;
    if length_kept(heap, object, n) { finish_native(job, frame, frame.output); return; }
    let setter, ok = write(heap, object, k, n);
    if heap.fault != Fault.None { property_fault(job, object, k, true, true); return; }
    if setter.is_object() { heap.fault = Fault.InvalidState; return; }
    if ok { record(job, object, k, n); } else if frame.index != 0 { refused_write(job, object, k); if heap.fault == Fault.None { heap.fault = Fault.TypeError; } return; }
    finish_native(job, frame, frame.output);
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
        node_mut(heap, frame.input.id()).kind = node_kind::Concat;
        node_mut(heap, frame.input.id()).initialized = false;
        node_mut(heap, frame.input.id()).value = Value.Undefined; node_mut(heap, frame.input.id()).namespace_value = nil();
        job.frames.pop();
    } else {
        if job.operands.len >= heap.limits.stack - job.locals.len { heap.fault = Fault.StackLimit; return; }
        let arg = form_nth(heap, row.namespace_value, frame.index);
        job.frames[job.frames.len - 1].pc = 1;
        if frame.callee.is_object() && node(heap, frame.callee.id()).kind == node_kind::Builtin { builtin_callback(job, frame.callee, nil(), arg, frame); }
        else if is_bound(heap, frame.callee) { start_bound(job, frame.callee, nil(), [arg], job.operands.len); }
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
        // A callback that is no object is not a function (light-cljs-873).
        if !frame.callee.is_object() { type_error(job, "not a function"); return; }
        let arg = form_nth(heap, frame.input, frame.index);
        job.frames[job.frames.len - 1].index += 1; job.frames[job.frames.len - 1].pc = 1;
        if frame.callee.is_object() && node(heap, frame.callee.id()).kind == node_kind::Builtin { builtin_callback(job, frame.callee, Value.Undefined, arg, frame); }
        else if is_bound(heap, frame.callee) { start_bound(job, frame.callee, Value.Undefined, [arg], job.operands.len); }
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
    else if forwarding(b) { native_start(job, b, receiver, [arg], job.operands.len); }
    else if conditional_native(b) { start_conditional_native(job, b, receiver, [arg], job.operands.len); }
    else { let v = call_builtin(job, id, receiver, [arg]); job.operands.push(v); }
}
// Pop a finished native frame and deliver its result to the frame below.
fn finish_native(job: Job&, frame: Frame, result: Value) {
    job.frames.pop(); job.operands.resize(frame.operand_base);
    deliver(job, result);
}
// Whether an operand converts through ToPrimitive (light-cljs-828): an object
// with a prototype chain, ordinary objects, arrays, closures, built-in
// functions, errors, generators, promises and primitive wrappers (the kinds
// lookup reads). The light-cljs runtime kinds (forms, cells, cursors, key
// lists) do not: an operator on one fails by name as before, and a scalar
// form (node_kind::Scalar) is its primitive.
fn convertible(heap: const Heap&, v: Value) -> bool {
    if !v.is_object() { return false; }
    let kind = node(heap, v.id()).kind;
    return kind == node_kind::Object || kind == node_kind::Array || kind == node_kind::Closure || kind == node_kind::Builtin || kind == node_kind::BoundFunction || kind == node_kind::Error || kind == node_kind::Generator || kind == node_kind::Promise || kind == node_kind::Wrapper || kind == node_kind::Iterator || kind == node_kind::Proxy || kind == node_kind::Arguments;
}
// Whether call or construction of f with arguments `args` converts the
// first argument first, and with which hint (light-cljs-832): Number(o) and
// String(o) of an object run its ToPrimitive (hint number; hint string, then
// ToString) in a Convert frame, and the call runs again on the primitive.
// Symbol(o) and Symbol.for(o) convert o as String(o) does (light-cljs-885),
// but only when called: a construction (`constructing`) of either throws
// before it converts anything.
fn argument_converts(heap: const Heap&, f: Value, args: const Value[:], constructing: bool = false) -> bool, Hint {
    if args.len == 0 || !convertible(heap, args[0]) || !f.is_object() || f.id() >= heap.nodes.len || node(heap, f.id()).kind != node_kind::Builtin { return false, Hint.Default; }
    let b = builtin_of(heap, f.id());
    let symbol = !constructing && (b == Builtin.SymbolFunction || b == Builtin.SymbolFor);
    return b == Builtin.Number || b == Builtin.String || symbol, if b == Builtin.Number { Hint.Number } else { Hint.String };
}
// Start the resumable ToPrimitive with `hint` of the `count` operands whose
// first lies `depth` below the operand top (Native.Convert): a native frame
// above the operation's, which converts them in place, left to right, and
// pops, so the operation, its pc left at its own step, runs again on
// primitives. With `finish` each primitive also takes ToNumber before the
// next operand converts (ToNumberPair, the unary operators, a length), or
// under hint string ToString (String(o) and new String(o), light-cljs-832).
fn start_convert(job: Job&, hint: Hint, finish: bool, count: i64, depth: i64) {
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return; }
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: Value.Undefined, argument_count: depth, native: Native.Convert { hint: hint, finish: finish, count: count as u8, method: 0 } });
}
// Whether a binary operator's operands convert first: `+` and the
// relational operators convert both, in source order (binary_hint);
// IsLooselyEqual only an object beside a primitive other than undefined and
// null (two objects compare by identity, and an object never equals
// undefined or null).
fn binary_converts(heap: const Heap&, op: i64, x: Value, y: Value) -> bool {
    if op == opcode::Plus || relational(op) { return convertible(heap, x) || convertible(heap, y); }
    if op == opcode::LooseEq { return convertible(heap, x) && !y.is_object() && y.tag() > value_tag::Null || convertible(heap, y) && !x.is_object() && x.tag() > value_tag::Null; }
    return false;
}
inline fn relational(op: i64) -> bool { return op == opcode::CompareLt || op == opcode::CompareGt || op == opcode::CompareLe || op == opcode::CompareGe; }
// The hint of a binary operator's conversion: number for the relational
// operators, default for `+` and `==`.
inline fn binary_hint(op: i64) -> Hint { return if relational(op) { Hint.Number } else { Hint.Default }; }
// Whether key operand k of a property operation on `base` converts first
// (ToPropertyKey of an object, hint string): an undefined or null base is
// the operation's TypeError before the key's conversion, so it does not.
inline fn key_converts(heap: const Heap&, base: Value, k: Value) -> bool { return convertible(heap, k) && base.tag() > value_tag::Null; }
// The interned key of these bytes, made if there is none. A key of Latin-1
// text is a Latin-1 header (the encoding is canonical), so this compares the
// bytes without building a string.
fn key_named(heap: Heap&, name: const u8[:]) -> Value {
    if !charge(heap, name.len + 1) { return Value.Undefined; }
    let found = find_key(heap, name);
    if !found.is_undefined() { return found; }
    let k = text(heap, name);
    return if heap.fault == Fault.None { intern(heap, k) } else { Value.Undefined };
}
// The interned key spelled `name`, or Undefined when there is none: a
// lookup that charges no work and makes nothing (describe_uncaught).
fn find_key(heap: const Heap&, name: const u8[:]) -> Value {
    var h: u32 = 2166136261; for b in name { h = (h ^ b) * 16777619; }
    if heap.keys.len > 0 {
        let mask = heap.keys.len - 1; var i = h & mask;
        while heap.keys[i].row != no_row() {
            let e = heap.keys[i]; let s = header(heap, e.row);
            if e.hash == h && !s.wide && s.len == name.len && narrow_run(heap, s) == name { return Value.String { s: e.row }; }
            i = (i + 1) & mask;
        }
    }
    return Value.Undefined;
}
// After binary_value of operator op on a and b failed with a TypeError, a
// symbol's conversion, its message in Job.detail: ToString when op is `+`
// with a string operand (a scalar form counting as its primitive, as there),
// otherwise ToNumber. Reads no table row through valid(), which refuses
// every value once a fault is set.
fn symbol_detail(job: Job&, op: i64, a: Value, b: Value) {
    if job.heap.fault != Fault.TypeError || job.detail.len > 0 { return; }
    let words: const u8[:] = if op == opcode::Plus && (holds_string(job.heap, a) || holds_string(job.heap, b)) { "Cannot convert a Symbol value to a string" } else { "Cannot convert a Symbol value to a number" };
    for c in words { job.detail.push(c); }
}
fn holds_string(heap: const Heap&, v: Value) -> bool {
    return v.is_string() || v.is_object() && v.id() < heap.nodes.len && heap.nodes[v.id()].kind == node_kind::Scalar && heap.nodes[v.id()].value.is_string();
}
// One step of ToPrimitive (ECMAScript 7.1.1; the Pants engine's Convert,
// rust/src/vm/native.rs) for the operand at `index` (see Native.Convert):
// GetMethod(input, @@toPrimitive) first, called with the hint's string and
// the object as receiver, an object result a TypeError; then
// OrdinaryToPrimitive, valueOf then toString (toString first for hint
// string), a callable method's primitive result replacing the operand, an
// object result or a missing or non-callable method trying the next; an
// object neither method converts is a TypeError. A method, own or inherited
// ([[Get]], light-cljs-787), behind an accessor comes from its getter, run
// with the object as receiver (pc 2). A user method or getter runs as a
// frame above this one (decision 4); a plain built-in method (the realm's
// Object.prototype.valueOf and toString, Error.prototype.toString) runs
// here and its result is the next step's; an Array.prototype built-in that
// runs as a native frame (Array.prototype.toString, light-cljs-842) runs as
// one above this one. A method the realm's pending properties would supply
// fails by name, as does a built-in method that needs a
// native frame or a host effect. Operands that need no conversion are left for
// the operation, except that with `numeric` each takes ToNumber in turn.
fn convert_step(job: Job&, frame: Frame, hint: Hint, finish: bool, count: u8, method: u8) {
    let heap .= job.heap; let at = job.frames.len - 1;
    if frame.index >= count {
        // Every operand is primitive: the operation below runs again.
        job.frames.pop(); job.operands.resize(frame.operand_base);
        return;
    }
    let slot = frame.operand_base - frame.argument_count + frame.index;
    if frame.pc == 1 {
        // A scalar form (node_kind::Scalar: a primitive with its provenance, as a
        // helper's value arrives) is its primitive.
        let result = form_value(heap, job.operands.pop());
        job.frames[at].pc = 0;
        if result.is_object() {
            if method == 0 { type_error(job, "cannot convert object to primitive value"); return; }
            job.frames[at].native = Native.Convert { hint: hint, finish: finish, count: count, method: method + 1 };
            return;
        }
        converted(job, at, slot, result, hint, finish, count);
        return;
    }
    let v = job.operands[slot];
    if !convertible(heap, v) { converted(job, at, slot, v, hint, finish, count); return; }
    if method >= 3 { type_error(job, "cannot convert object to primitive value"); return; }
    var m = Value.Undefined;
    if frame.pc == 2 { m = job.operands.pop(); job.frames[at].pc = 0; }
    else {
        let k: Value = if method == 0 { Value.Symbol { s: well_known::ToPrimitive } } else { key_named(heap, if (method == 1) == (hint == Hint.String) { "toString" } else { "valueOf" }) };
        if heap.fault != Fault.None { return; }
        let value, getter = lookup(heap, v, k);
        if heap.fault != Fault.None { return; }
        if getter.is_object() {
            job.frames[at].pc = 2;
            if node(heap, getter.id()).kind == node_kind::Closure { enter(job, getter, v, []); } else { call_accessor(job, getter, v, []); }
            return;
        }
        m = value;
    }
    let next = Native.Convert { hint: hint, finish: finish, count: count, method: method + 1 };
    if method == 0 {
        // GetMethod: undefined or null is no method; anything else must be
        // callable.
        if m.tag() <= value_tag::Null { job.frames[at].native = next; return; }
        if !callable(heap, m) { type_error(job, "Symbol.toPrimitive is not a function"); return; }
        let words: const u8[:] = match hint { Default => "default", Number => "number", String => "string" };
        let h = key_named(heap, words);
        if heap.fault != Fault.None { return; }
        conversion_call(job, at, m, v, [h]);
        return;
    }
    if callable(heap, m) { conversion_call(job, at, m, v, []); return; }
    job.frames[at].native = next;
}
// Call conversion method m with receiver v: its result arrives at pc 1.
fn conversion_call(job: Job&, at: i64, m: Value, v: Value, args: const Value[:]) {
    let heap .= job.heap;
    job.frames[at].pc = 1;
    if node(heap, m.id()).kind == node_kind::Closure { enter(job, m, v, args); return; }
    if is_bound(heap, m) { start_bound(job, m, v, args, job.operands.len); return; }
    let b = builtin_of(heap, m.id());
    // An Array.prototype native frame (Array.prototype.toString, light-cljs-842)
    // runs above this one; its result arrives at pc 1.
    if array_native(b) || forwarding(b) { native_start(job, b, v, args, job.operands.len); return; }
    // Function.prototype.toString (light-cljs-853): its FunctionText frame, if it needs one.
    if b == Builtin.FunctionPrototypeToString { start_function_text(job, v, job.operands.len); return; }
    // Error.prototype.toString (light-cljs-870): its ErrorText frame, if it needs one.
    if conditional_native(b) { start_conditional_native(job, b, v, args, job.operands.len); return; }
    if effect_builtin(b) || native_builtin(b) || b == Builtin.Mapv || b == Builtin.MapSeq || forces(heap, b, args) { unsupported_step(job, b, "as a ToPrimitive method"); return; }
    let r = call_builtin(job, node(heap, m.id()).function, v, args);
    if heap.fault == Fault.None { job.operands.push(r); }
}
// Operand `slot` of the frame at `at` is primitive value p: it replaces the
// operand (with `finish`, as its ToString under hint string and otherwise
// its ToNumber, a symbol's TypeError named here, since this frame names no
// instruction) and the next operand converts.
fn converted(job: Job&, at: i64, slot: i64, p: Value, hint: Hint, finish: bool, count: u8) {
    let heap .= job.heap;
    if finish && hint == Hint.String && !p.is_string() {
        let s = to_string(heap, p);
        if heap.fault == Fault.TypeError && job.detail.len == 0 { for ch in "Cannot convert a Symbol value to a string" { job.detail.push(ch); } }
        if heap.fault != Fault.None { return; }
        job.operands[slot] = s;
    } else if finish && hint != Hint.String && !p.is_number() {
        let n = numeric(heap, p);
        if heap.fault == Fault.TypeError && job.detail.len == 0 { for ch in "Cannot convert a Symbol value to a number" { job.detail.push(ch); } }
        if heap.fault != Fault.None { return; }
        job.operands[slot] = number(n);
    } else { job.operands[slot] = p; }
    job.frames[at].index += 1;
    job.frames[at].native = Native.Convert { hint: hint, finish: finish, count: count, method: 0 };
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
// A collection asked for (reclaim.goose) spends the fuel at once, so the
// loop leaves for run_claimed's safe point after the instruction that asked.
inline fn fuel(job: const Job&, allowance: i64, used: i64) -> i64 {
    if job.heap.reclaim { return 0; }
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
// ToUint32 of a Number (ECMAScript 7.1.7; the Pants engine's
// vm/dispatch.rs to_int32 reads the same bits signed): NaN and the
// infinities are 0, anything else truncates toward zero, modulo 2^32.
inline fn to_uint32(x: f64) -> i64 {
    if !(x - x == 0) { return 0; }
    var m = trunc(x) % 4294967296.0; if m < 0 { m += 4294967296.0; }
    return m as i64;
}
// ToInt32: ToUint32's bits read as a two's-complement 32-bit integer.
inline fn to_int32(x: f64) -> i64 { return signed32(to_uint32(x)); }
inline fn signed32(u: i64) -> i64 { return if u >= 2147483648 { u - 4294967296 } else { u }; }
// `&`, `|`, `^`, `<<`, `>>` and `>>>` of two Numbers (the Pants engine's
// BitAnd ... UShr arm): ToInt32 of both (ToUint32 for `>>>`'s left), the
// shift count ToUint32(y) & 31. Every result is an integer in int32 range
// (uint32 for `>>>`), exact as a Number and never -0.
inline fn bitwise(op: i64, x: f64, y: f64) -> f64 {
    let c = to_uint32(y) & 31;
    let r: i64 = match op {
        opcode::BitAnd => to_int32(x) & to_int32(y), opcode::BitOr => to_int32(x) | to_int32(y), opcode::BitXor => to_int32(x) ^ to_int32(y),
        opcode::Shl => signed32((to_uint32(x) << c) & 4294967295), opcode::Sar => to_int32(x) >> c, _ => to_uint32(x) >> c,
    };
    return r;
}
// Number::exponentiate (`x ** y`; the Pants engine's exponentiate and
// QuickJS js_pow): C's pow except that a non-finite exponent with base 1 or
// -1 is NaN, where pow gives 1.
inline fn exponentiate(x: f64, y: f64) -> f64 { return if !(y - y == 0) && (x == 1 || x == -1) { 0.0 / 0.0 } else { pow(x, y) }; }
// A unary operator of a Number: `-`, `++`, `--`, `~` (ToInt32, then the
// bits inverted) and the conversions, which keep it.
inline fn unary_number(op: i64, n: f64) -> f64 { return if op == opcode::Neg { -n } else if op == opcode::Inc { n + 1 } else if op == opcode::Dec { n - 1 } else if op == opcode::BitNot { -1 - to_int32(n) } else { n }; }
// Numbers on both sides of a binary operator: what binary_value computes for
// them, without its coercions (no charge, no fault).
inline fn arithmetic(op: i64, x: f64, y: f64) -> Value {
    return match op {
        opcode::Add, opcode::Plus => number(x + y), opcode::Sub => number(x - y), opcode::Mul => number(x * y), opcode::Div => number(x / y), opcode::Rem => number(x % y),
        opcode::BitAnd, opcode::BitOr, opcode::BitXor, opcode::Shl, opcode::Sar, opcode::UShr => number(bitwise(op, x, y)), opcode::Pow => number(exponentiate(x, y)),
        opcode::StrictEq, opcode::LooseEq => boolean(x == y), opcode::Gt, opcode::CompareGt => boolean(x > y), opcode::Le, opcode::CompareLe => boolean(x <= y), opcode::Ge, opcode::CompareGe => boolean(x >= y), _ => boolean(x < y),
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
                if allowance >= 0 && used >= allowance || heap.reclaim { job.frames[top].pc = pc; return used; }
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
                    // A Symbol constant is a well-known symbol, the value
                    // Symbol's properties hold (an @@unscopables read in a
                    // `with` body, light-cljs-855); a BigInt constant has no
                    // VM value yet.
                    if v.is_bigint() { heap.fault = Fault.Unsupported; break; }
                    job.operands.push(v);
                },
                Undefined => { job.operands.push(Value.Undefined); },
                Load l => { job.operands.push(job.locals[lb + l.slot]); },
                Store s => { job.locals[lb + s.slot] = job.operands.pop(); },
                Pop => { job.operands.pop(); },
                Dup => { let v = job.operands[job.operands.len - 1]; job.operands.push(v); },
                // a b → a b a b: a computed reference read before it is written.
                Dup2 => { let n = job.operands.len; let a = job.operands[n - 2]; let b = job.operands[n - 1]; job.operands.push(a); job.operands.push(b); },
                // a b c → a b c a b c: the super-reference form (its own opcodes stay Unsupported).
                Dup3 => { let n = job.operands.len; let a = job.operands[n - 3]; let b = job.operands[n - 2]; let c = job.operands[n - 1]; job.operands.push(a); job.operands.push(b); job.operands.push(c); },
                Binary b => {
                    let y = job.operands.pop(); let x = job.operands.pop();
                    if x.is_number() && y.is_number() { job.operands.push(arithmetic(b.op, x.num(), y.num())); }
                    else if (x.is_object() || y.is_object()) && binary_converts(heap, b.op, x, y) {
                        // An object operand: the operands go back and a
                        // Convert frame replaces them with primitives, then
                        // this instruction runs again (light-cljs-828).
                        job.operands.push(x); job.operands.push(y);
                        settle(job, ran, used); job.frames[top].pc = at; start_convert(job, binary_hint(b.op), false, 2, 2); in_place(job, top, at); break;
                    } else {
                        settle(job, ran, used); job.frames[top].pc = pc;
                        let v = binary_value(heap, b.op, x, y); job.operands.push(v);
                        if heap.fault == Fault.TypeError && b.op == opcode::Plus { symbol_detail(job, b.op, x, y); }
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
                // NewObject (16) makes a null-prototype object, the private
                // binding object of a sloppy eval's variable environment
                // (rust/src/vm/dispatch.rs, light-cljs-855); NewObjectLiteral an
                // ordinary one.
                NewObject n => {
                    settle(job, ran, used); job.frames[top].pc = pc; let v = allocate(heap, if n.bare { Node { kind: node_kind::Object, proto: Value.Null } } else { Node { kind: node_kind::Object } }); job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                NewArray => {
                    settle(job, ran, used); job.frames[top].pc = pc; let v = allocate(heap, Node { kind: node_kind::Array }); job.operands.push(v);
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
                // A number converts in place; any other operand runs ToNumber
                // (numeric), which may charge work (StringToNumber), so the
                // counts settle first and the fuel is recomputed after.
                Unary u => {
                    let v = job.operands.pop();
                    if v.is_number() { let n = v.num(); job.operands.push(number(unary_number(u.op, n))); }
                    else if convertible(heap, v) {
                        // ToPrimitive with hint number, then ToNumber, in a
                        // Convert frame; this instruction runs again.
                        job.operands.push(v);
                        settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.Number, true, 1, 1); in_place(job, top, at); break;
                    } else {
                        settle(job, ran, used); job.frames[top].pc = pc;
                        let n = numeric(heap, v);
                        job.operands.push(number(unary_number(u.op, n)));
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
                },
                Numeric2 => {
                    let b = job.operands.pop(); let a = job.operands.pop();
                    if a.is_number() && b.is_number() { job.operands.push(a); job.operands.push(b); }
                    else if convertible(heap, a) || convertible(heap, b) {
                        // The left operand's ToPrimitive and ToNumber finish
                        // before the right's start (a Convert frame with
                        // `numeric`); this instruction runs again on numbers.
                        job.operands.push(a); job.operands.push(b);
                        settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.Number, true, 2, 2); in_place(job, top, at); break;
                    } else {
                        settle(job, ran, used); job.frames[top].pc = pc;
                        let x = numeric(heap, a); let y = numeric(heap, b);
                        job.operands.push(number(x)); job.operands.push(number(y));
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
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
                MappedArguments => {
                    settle(job, ran, used); job.frames[top].pc = pc; let here = job.frames[top]; mapped_arguments(job, here, f);
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
                    settle(job, ran, used); job.frames[top].pc = pc; let v = allocate(heap, Node { kind: node_kind::Binding, immutable: c.immutable }); job.operands.push(v);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                InitCell => { cell(job, 48); if heap.fault != Fault.None { break; } },
                ReadCell => { cell(job, 49); if heap.fault != Fault.None { break; } },
                WriteCell => { cell(job, 50); if heap.fault != Fault.None { break; } },
                GetNamed g => {
                    settle(job, ran, used); job.frames[top].pc = pc; get_property(job, heap.constants[f.constants + g.k], true);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                GetIndexed => {
                    let v = job.operands[job.operands.len - 1]; let object = job.operands[job.operands.len - 2]; let i = number_index(v);
                    // A dense run read neither charges nor faults.
                    if i >= 0 && dense(job, object) && !node(heap, object.id()).initialized { job.operands.pop(); job.operands.pop(); let e, found = own_element(heap, object, i); job.operands.push(e); }
                    else if object_hit(job, object, i) { job.operands.pop(); job.operands.pop(); job.operands.push(slot(heap, node(heap, object.id()).start + i)); }
                    // An object key: ToPropertyKey (ToPrimitive with hint
                    // string) in a Convert frame, then this instruction runs
                    // again (light-cljs-828), as in the arms below.
                    else if key_converts(heap, object, v) { settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.String, false, 1, 1); in_place(job, top, at); break; }
                    else {
                        settle(job, ran, used); job.frames[top].pc = pc; job.operands.pop();
                        if i >= 0 && dense(job, object) { job.operands.pop(); let e, found = own_element(heap, object, i); job.operands.push(e); }
                        else if object.tag() <= value_tag::Null { job.operands.pop(); base_error(job, object, Value.Undefined, false, false); job.operands.push(Value.Undefined); }
                        else { let k = key(heap, v); get_property(job, k, false); }
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
                    if key_converts(heap, job.operands[job.operands.len - 3], job.operands[job.operands.len - 2]) { settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.String, false, 1, 2); in_place(job, top, at); break; }
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let v = job.operands.pop(); let object = job.operands[job.operands.len - 1]; let i = number_index(v);
                    if i >= 0 && dense(job, object) && writable(job, object.id()) { job.operands.pop(); if valid(heap, value) { set_element(heap, object, i, value); } job.operands.push(value); }
                    else if object_hit(job, object, i) && writable(job, object.id()) { job.operands.pop(); if valid(heap, value) { set_slot(heap, node(heap, object.id()).start + i, value); } job.operands.push(value); }
                    else { let k = key(heap, v); set_property(job, value, k, false, s.strict || f.strict); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                DefineIndexed d => {
                    if key_converts(heap, job.operands[job.operands.len - 3], job.operands[job.operands.len - 2]) { settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.String, false, 1, 2); in_place(job, top, at); break; }
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let v = job.operands.pop(); let k = key(heap, v);
                    if heap.fault == Fault.None { define_field(job, k, value, d.half); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // The key is ToPropertyKey's already (a string, symbol or
                // number), so nothing converts here.
                DefineClass d => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let v = job.operands.pop(); let k = key(heap, v);
                    if heap.fault == Fault.None { define_field(job, k, value, d.half, true); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                SetProto => {
                    settle(job, ran, used); job.frames[top].pc = pc; set_proto(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Delete d => {
                    if key_converts(heap, job.operands[job.operands.len - 2], job.operands[job.operands.len - 1]) { settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.String, false, 1, 1); in_place(job, top, at); break; }
                    settle(job, ran, used); job.frames[top].pc = pc; delete_op(job, d.strict || f.strict);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                In => {
                    // `k in o` checks o (the top) before it converts k.
                    if job.operands[job.operands.len - 1].is_object() && convertible(heap, job.operands[job.operands.len - 2]) { settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.String, false, 1, 2); in_place(job, top, at); break; }
                    settle(job, ran, used); job.frames[top].pc = pc; in_op(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // A user @@hasInstance method or a getter runs above a
                // HasInstance native frame, which delivers the result.
                Instanceof => {
                    settle(job, ran, used); job.frames[top].pc = pc; instanceof_op(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // A getter or the @@iterator method runs above a GetIterator
                // native frame, which delivers the record.
                GetIterator => {
                    settle(job, ran, used); job.frames[top].pc = pc; get_iterator_op(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // An iterator method's result (a scalar form is its primitive).
                RequireObject => {
                    if !form_value(heap, job.operands[job.operands.len - 1]).is_object() { type_error(job, "iterator must return an object"); break; }
                },
                // An elided element: a dense array the job may write grows
                // by one, its new slot a hole (array_length), for the
                // instruction's own work unit.
                // A dynamic environment is a binding cell or a `with`
                // object (rust/src/vm/dispatch.rs IsCell and IsObject).
                IsCell => { let v = job.operands.pop(); job.operands.push(boolean(v.is_object() && node(heap, v.id()).kind == node_kind::Binding)); },
                IsObject => { let v = job.operands.pop(); job.operands.push(boolean(javascript_object(heap, v))); },
                ComputedName c => {
                    settle(job, ran, used); job.frames[top].pc = pc; set_computed_name(job, heap.constants[f.constants + c.k]);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                AppendHole => {
                    let object = job.operands[job.operands.len - 1];
                    if !dense(job, object) { heap.fault = Fault.TypeError; break; }
                    if !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; break; }
                    if !array_length(heap, object, number((node(heap, object.id()).length as f64) + 1)) { break; }
                },
                Append => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let value = job.operands.pop(); let object = job.operands[job.operands.len - 1];
                    if dense(job, object) && writable(job, object.id()) { let end = node(heap, object.id()).length; if valid(heap, value) { set_element(heap, object, end, value); } }
                    else {
                        let k = if valid(heap, object) && object.is_object() && node(heap, object.id()).kind == node_kind::Array { key(heap, number(node(heap, object.id()).length)) } else { heap.fault = Fault.TypeError; Value.Undefined };
                        set_property(job, value, k, true, true);
                    }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // A derived constructor's `this` is its frame's binding cell
                // (construct, light-cljs-887), read here: a ReferenceError
                // before super() has bound it. Any other receiver is itself.
                ResolveThis => {
                    let value = job.operands[job.operands.len - 1];
                    if value.is_object() {
                        let kind = node(heap, value.id()).kind;
                        if kind == node_kind::Binding { if !resolve_this(job, value) { break; } }
                        else if kind == node_kind::MappedBinding { heap.fault = Fault.UnsupportedOperand; break; }
                    }
                },
                This t => {
                    let receiver = job.frames[top].receiver;
                    if receiver.is_object() || !t.sloppy || f.strict { job.operands.push(receiver); }
                    else if receiver.is_undefined() || receiver.is_null() { job.operands.push(heap.global); }
                    else {
                        // OrdinaryCallBindThis: sloppy code sees a primitive
                        // receiver boxed (ToObject, light-cljs-832). The frame
                        // keeps the wrapper, so every `this` of the call is
                        // the same object; strict code keeps the primitive.
                        settle(job, ran, used); job.frames[top].pc = pc;
                        let boxed = to_object(job, receiver);
                        if heap.fault == Fault.None { job.frames[top].receiver = boxed; }
                        job.operands.push(boxed);
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
                },
                Global => { job.operands.push(heap.global); },
                GetGlobal g => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let k = heap.constants[f.constants + g.k];
                    // A compiler job's global names are the V8 build's; one
                    // the image lacks is a boundary, never a ReferenceError.
                    // Its scripts declare no bindings (global_op). Any other
                    // job resolves the name (get_global).
                    if job.protected_nodes > 0 {
                        let v, found = own_property(heap, heap.global, k);
                        if !found && heap.fault == Fault.None { heap.fault = Fault.NativeBoundary; }
                        job.operands.push(v);
                    } else { get_global(job, k, g.throws); }
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                TypeOf => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let v = job.operands.pop(); job.operands.push(type_name(heap, v));
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                // ToString (an untagged template's substitution): a string
                // stays as it is; another primitive converts (to_string). An
                // object runs ToPrimitive with hint string in a Convert frame
                // and this instruction runs again (light-cljs-828); a
                // light-cljs runtime value fails by name.
                ToString => {
                    let v = job.operands[job.operands.len - 1];
                    if !v.is_string() {
                        settle(job, ran, used);
                        if convertible(heap, v) { job.frames[top].pc = at; start_convert(job, Hint.String, false, 1, 1); in_place(job, top, at); break; }
                        job.frames[top].pc = pc;
                        if v.is_object() && valid(heap, v) { unsupported_detail(job, "ToString of a light-cljs runtime value"); }
                        else { job.operands.pop(); job.operands.push(to_string(heap, v)); }
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
                },
                Literal l => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let base = job.operands.len - (l.count);
                    let value = if l.count == 0 && l.kind != node_kind::Map { empty_literal(heap, l.kind) } else { form_ordered(heap, l.kind, job.operands[base..], l.kind == node_kind::Map) }; job.operands.resize(base); job.operands.push(value);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                Form x => {
                    settle(job, ran, used);
                    let op = x.op; let base = job.operands.len - export_form_arity(op);
                    if op == form_op::Count || op == form_op::CljCount || op == form_op::CljConcat || op == form_op::CljConcatAll {
                        // A demand starts before the step counts (see invoke).
                        let demanded = demand_maps(job, job.operands[(job.operands.len - code.effects[at].pop)..], op == form_op::CljConcatAll);
                        if demanded || heap.fault != Fault.None { job.steps -= 1; job.frames[top].pc = at; if !demanded { fail(job, heap.fault, frame_at(job, top, at)); } break; }
                    }
                    job.frames[top].pc = pc;
                    // The live compiler adapter opts into a demand at the exact saved
                    // intrinsic-call continuation. Sealed-plan users retain the
                    // explicit DependencyNeed failure from form_op below.
                    var waiting = false;
                    if op == form_op::CljReadRoot && job.helpers_enabled {
                        let args = job.operands[base..];
                        if valid(heap, args[0]) && args[0].is_object() && node(heap, args[0].id()).kind == node_kind::Object && args[1].tag() <= value_tag::Null && args[2].is_string() {
                            let value, found = own_property(heap, args[0], args[2]);
                            // The answer is written into the receiver: only the
                            // compiler roots object or a row the job allocated
                            // (light-cljs-795), never a retained image row.
                            if !found && heap.fault == Fault.None && !(args[0] == job.root_receiver || writable(job, args[0].id())) { heap.fault = Fault.NativeBoundary; }
                            if !found && heap.fault == Fault.None { request_effect(job, 9, args, frame_at(job, top, at)); waiting = true; }
                        }
                    }
                    if waiting { job.operands.resize(base); }
                    else {
                        let value = form_op(heap, op, job.operands[base..]);
                        if heap.fault == Fault.HostEffect { core_error(job, op, base); }
                        job.operands.resize(base); job.operands.push(value);
                    }
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
                    if key_converts(heap, job.operands[job.operands.len - 2], job.operands[job.operands.len - 1]) { settle(job, ran, used); job.frames[top].pc = at; start_convert(job, Hint.String, false, 1, 1); in_place(job, top, at); break; }
                    settle(job, ran, used); job.frames[top].pc = pc; to_property_key(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                RequireCoercible => { if job.operands[job.operands.len - 1].tag() <= value_tag::Null { type_error(job, "cannot convert to object"); break; } },
                // Generators and async functions (continuations.goose): each
                // detaches, ends or settles the frame, so the loop leaves it.
                GeneratorStart => {},
                AsyncStart => { settle(job, ran, used); job.frames[top].pc = pc; async_start(job, top); if heap.fault != Fault.None { break; } left = fuel(job, allowance, used); },
                GeneratorInitialYield => { settle(job, ran, used); job.frames[top].pc = pc; initial_yield(job, top, at); break; },
                GeneratorYield => { settle(job, ran, used); job.frames[top].pc = pc; generator_yield(job, top, at); break; },
                Await => { settle(job, ran, used); job.frames[top].pc = pc; await_value(job, top, at); break; },
                Unsupported => { heap.fault = Fault.Unsupported; break; },
                GlobalOp x => {
                    settle(job, ran, used); job.frames[top].pc = pc; global_op(job, x.op, heap.constants[f.constants + x.k], f.strict, f.eval);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                ThrowError t => { throw_error(job, t.kind, at); break; },
                TemplateObject t => {
                    settle(job, ran, used); job.frames[top].pc = pc; template_object(job, at, t.count);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                DirectEval c => {
                    // The callee sits below the arguments (a spread call's one
                    // packed array), above a method's receiver.
                    // Only the realm's %eval% itself makes a direct eval: a
                    // Proxy around it, or a bound copy, is an ordinary call.
                    let below = if c.spread { 1 } else { c.argc as i64 };
                    let direct = realm_eval(heap, job.operands[job.operands.len - below - 1]);
                    settle(job, ran, used);
                    let argc: i64 = if c.spread { spread_unpack(job, top, at, c.method, false) } else { c.argc };
                    if argc < 0 { if heap.fault == Fault.None { in_place(job, top, at); } break; }
                    if direct {
                        job.frames[top].pc = at + 1;
                        start_direct_eval(job, top, at, argc, c.method);
                        if !in_place(job, top, at) { break; }
                    } else if !invoke(job, top, at, argc, c.method) { break; }
                    left = fuel(job, allowance, used);
                },
                SloppyThis => {
                    let v = job.operands[job.operands.len - 1];
                    if v.is_undefined() || v.is_null() { job.operands[job.operands.len - 1] = heap.global; }
                    else if !v.is_object() {
                        // A primitive receiver is boxed, as This boxes a
                        // sloppy function's own (OrdinaryCallBindThis).
                        settle(job, ran, used); job.frames[top].pc = pc;
                        let boxed = to_object(job, v);
                        if heap.fault == Fault.None { job.operands[job.operands.len - 1] = boxed; }
                        if !in_place(job, top, at) { break; }
                        left = fuel(job, allowance, used);
                    }
                },
                CheckSpread => {
                    let a = job.operands[job.operands.len - 1];
                    if !a.is_object() || node(heap, a.id()).kind != node_kind::Array { heap.fault = Fault.TypeError; break; }
                    if node(heap, a.id()).length > 65535 { heap.fault = Fault.RangeError; break; }
                },
                CallSpread s => {
                    settle(job, ran, used);
                    let argc = spread_unpack(job, top, at, s.method, false);
                    if argc < 0 { if heap.fault == Fault.None { in_place(job, top, at); } break; }
                    if !invoke(job, top, at, argc, s.method) { break; }
                    left = fuel(job, allowance, used);
                },
                ConstructSpread => {
                    settle(job, ran, used);
                    let argc = spread_unpack(job, top, at, false, true);
                    if argc < 0 { if heap.fault == Fault.None { in_place(job, top, at); } break; }
                    if !construct_at(job, top, at, argc) { break; }
                    left = fuel(job, allowance, used);
                },
                // A Delay constructs as its call (light-cljs-808, with its
                // map demand); every other callee takes construct.
                Construct c => {
                    settle(job, ran, used);
                    if !construct_at(job, top, at, c.argc) { break; }
                    left = fuel(job, allowance, used);
                },
                NewTarget => { job.operands.push(job.frames[top].input); },
                // Derived classes (light-cljs-887; see Ins).
                CheckHeritage => {
                    let p = job.operands[job.operands.len - 1];
                    if !p.is_null() && !is_constructor(heap, p) { type_error(job, "parent class must be a constructor"); break; }
                },
                InitHeritage => {
                    settle(job, ran, used); job.frames[top].pc = pc; init_heritage(job);
                    if !in_place(job, top, at) { break; }
                    left = fuel(job, allowance, used);
                },
                SuperConstructor => {
                    let g = job.operands.pop();
                    if !g.is_object() { heap.fault = Fault.InvalidState; break; }
                    job.operands.push(prototype(heap, g.id()));
                },
                ConstructSuper c => {
                    settle(job, ran, used);
                    var argc: i64 = c.argc;
                    if c.spread {
                        argc = spread_unpack(job, top, at, false, true, true);
                        if argc < 0 { if heap.fault == Fault.None { in_place(job, top, at); } break; }
                    }
                    if !construct_super(job, top, at, argc) { break; }
                    left = fuel(job, allowance, used);
                },
                BindThis => { if !bind_this(job) { break; } },
                ToObject => {
                    settle(job, ran, used); job.frames[top].pc = pc;
                    let v = job.operands.pop(); job.operands.push(to_object(job, v));
                    if !in_place(job, top, at) { break; }
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
// Construct and ConstructSpread at `at`, with the instruction's counts
// settled and f a1..an the top argc + 1 operands. Returns whether the frame
// continues in place.
inline fn construct_at(job: Job&, top: i64, at: i64, argc: i64) -> bool {
    let heap .= job.heap;
    let callee = job.operands[job.operands.len - argc - 1];
    if constructible(heap, callee) { return invoke(job, top, at, argc, false); }
    // new Number(o), new String(o): the construction runs again on the
    // primitive (light-cljs-832).
    let converts, hint = argument_converts(heap, callee, job.operands[(job.operands.len - argc)..], true);
    if converts { job.frames[top].pc = at; start_convert(job, hint, hint == Hint.String, 1, argc); in_place(job, top, at); return false; }
    job.frames[top].pc = at + 1; construct(job, argc);
    return in_place(job, top, at);
}
// A spread call's arguments (light-cljs-850; the Pants engine's
// spread_arguments and call_spread): the packed array on the operand top,
// the compiler's private array built by AppendElement and checked by
// CheckSpread, is replaced by its elements, read straight from its element
// run with no observable property access, and the call then runs as Call,
// CallMethod or Construct of that many arguments (the same frame entry,
// receiver and new.target, built-ins included). Elements that would pass
// the value limit are a catchable StackLimit before anything changes.
// A built-in callee that would run the instruction again from `at` first
// (invoke's map demand, a Number or String conversion of its first
// argument) does so with the packed array still in place, so the call
// stays one instruction: a map demand realizes the map in its own row, and
// a conversion converts a copy of the first argument pushed above the
// array, which the next run writes back into the array (a primitive on top
// marks it: the packed array is always an Array). Returns the argument
// count, or -1 when the call does not run now (heap.fault set, or a demand
// or conversion started with the pc at `at`). A super(...a) call
// (`parent`, ConstructSuper with spread, light-cljs-887) has the parent
// constructor and newTarget below the array; the parent decides.
fn spread_unpack(job: Job&, top: i64, at: i64, method: bool, constructs: bool, parent: bool = false) -> i64 {
    let heap .= job.heap;
    let last = job.operands[job.operands.len - 1];
    let converted = !last.is_object() || node(heap, last.id()).kind != node_kind::Array;
    let primitive = if converted { job.operands.pop() } else { Value.Undefined };
    let array = job.operands[job.operands.len - 1];
    if !array.is_object() || node(heap, array.id()).kind != node_kind::Array { heap.fault = Fault.InvalidState; return -1; }
    let base = job.operands.len - 1 - (if method || parent { 2 } else { 1 });
    var callee = job.operands[base + (if method { 1 } else { 0 })];
    // Function.prototype.call forwards its receiver as callee (call).
    var skip: i64 = 0;
    if !constructs && callee.is_object() && node(heap, callee.id()).kind == node_kind::Builtin && builtin_of(heap, callee.id()) == Builtin.FunctionPrototypeCall {
        callee = if method { job.operands[base] } else { Value.Undefined }; skip = 1;
    }
    let count = node(heap, array.id()).length as i64;
    // The limit comes first, as in the Pants engine: no conversion or
    // demand runs for a call that cannot enter.
    if count - 1 > heap.limits.stack - job.locals.len - job.operands.len { heap.fault = Fault.StackLimit; return -1; }
    if converted {
        if !set_element(heap, array, skip, primitive) { return -1; }
    } else if callee.is_object() && node(heap, callee.id()).kind == node_kind::Builtin && !(constructs && constructible(heap, callee)) && count > skip {
        let b = builtin_of(heap, callee.id());
        let first, _ = own_element(heap, array, skip);
        if heap.fault != Fault.None { return -1; }
        // Symbol(o) and Symbol.for(o) convert o as String(o) does, but only
        // when called (light-cljs-885; argument_converts).
        let symbol = !constructs && (b == Builtin.SymbolFunction || b == Builtin.SymbolFor);
        if (b == Builtin.Number || b == Builtin.String || symbol) && convertible(heap, first) {
            if job.operands.len >= heap.limits.stack - job.locals.len { heap.fault = Fault.StackLimit; return -1; }
            job.operands.push(first); job.frames[top].pc = at;
            start_convert(job, if b == Builtin.Number { Hint.Number } else { Hint.String }, b != Builtin.Number, 1, 1);
            return -1;
        }
        if !constructs && (b == Builtin.Vec || b == Builtin.Into) {
            for i in skip..count {
                let v, _ = own_element(heap, array, i);
                if heap.fault != Fault.None { return -1; }
                if demand_map(job, v) { job.steps -= 1; job.frames[top].pc = at; return -1; }
                if heap.fault != Fault.None { return -1; }
            }
        }
    }
    if !charge(heap, count) { return -1; }
    job.operands.pop();
    for i in count {
        let v, _ = own_element(heap, array, i);
        if heap.fault != Fault.None { return -1; }
        job.operands.push(v);
    }
    return count;
}
// new f(a1, ..., an) (Construct, light-cljs-831; the Pants engine's
// vm/dispatch.rs construct_target, QuickJS's JS_CallConstructorInternal),
// f and its arguments the top argc + 1 operands, every argument already
// evaluated and none used before f is known to be a constructor. An ordinary
// function (constructor kind Ordinary) or base class (BaseClass, light-cljs-886) gets OrdinaryCreateFromConstructor's
// object, a job row inheriting F.prototype (made on first observation, as a
// read makes it) or, when that is not an object, %Object.prototype%; F's
// frame runs with it as `this` and F as new.target, and return_value
// substitutes it for a non-object return value. A derived class
// (light-cljs-887) runs with an unbound `this` cell instead, which super()
// binds. Object, Array and the Error constructors construct as their call
// (call_builtin), which is their [[Construct]] when new.target is the
// constructor itself, as it is for `new`: their prototypes are the realm's;
// another newTarget (a subclass's, Reflect.construct's) gives its
// `prototype` (construct_builtin). Function constructs as its call
// does, in its DynamicFunction frame (light-cljs-857). Number, String and Boolean
// make a wrapper object of their conversion (construct_wrapper,
// light-cljs-832; an object argument converted first, in the dispatch
// loop's Convert frame). Not constructors, a catchable
// TypeError: primitives, objects without [[Call]], the light-cljs runtime
// kinds, arrows, methods, accessors, generator and async functions, and the
// realm's other built-ins (Symbol's [[Construct]] throws). Failing by name
// (decision 1): the light-cljs runtime's leaves other than Delay, which the V8 build
// constructs as ordinary JavaScript functions.
fn construct(job: Job&, argc: i64, new_target: Value = Value.Empty) {
    let heap .= job.heap;
    let base = job.operands.len - argc - 1;
    let f = job.operands[base];
    // new.target: F itself for `new`, Reflect.construct's newTarget
    // (light-cljs-851), whose `prototype` the new object takes.
    let nt = if new_target.is_hole() { f } else { new_target };
    let kind: i64 = if f.is_object() { node(heap, f.id()).kind } else { node_kind::None };
    if kind == node_kind::Closure {
        let code_kind = constructor_kind(heap, f.id());
        // A base class constructs as an ordinary function does (its body
        // first runs the instance-field initializer, compiled into it). A
        // derived class (light-cljs-887) makes no object: its frame's
        // receiver is a new uninitialized binding cell, its `this`, which
        // super() binds (BindThis) to the parent's object and ResolveThis
        // reads; arrows and eval code capture the cell. It is frame state
        // (a job row the frame names), so it survives waits, saved
        // continuations and collections as any receiver does.
        if code_kind > constructor::BaseClass {
            let cell = allocate(heap, Node { kind: node_kind::Binding });
            if heap.fault != Fault.None { return; }
            if push_frame(job, f, cell, job.operands[(base + 1)..], false, base, nt) { job.operands.resize(base); }
            return;
        }
        if code_kind == constructor::None { heap.fault = Fault.TypeError; return; }
        // A `prototype` getter on newTarget (a bound function's or an
        // object's on its chain, Reflect.construct's) runs as a frame above
        // a Construct native frame, which then enters the constructor
        // (construct_step).
        let proto, getter = closure_prototype(job, nt);
        if heap.fault != Fault.None { return; }
        if getter.is_object() {
            if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
            job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: f, receiver: Value.Undefined, input: nt, native: Native.Construct });
            call_accessor(job, getter, nt, []);
            return;
        }
        enter_constructor(job, f, nt, proto, base);
        return;
    }
    // A bound function's [[Construct]] runs in a Forward frame, newTarget
    // the bound function or Reflect.construct's (light-cljs-852).
    if kind == node_kind::BoundFunction {
        if !is_constructor(heap, f) { heap.fault = Fault.TypeError; return; }
        start_bound(job, f, nt, job.operands[(base + 1)..], base, true);
        return;
    }
    if kind != node_kind::Builtin { if kind == node_kind::Proxy { proxy_invoke(heap, f, proxy_method::Construct); } else { heap.fault = Fault.TypeError; } return; }
    let b = builtin_of(heap, f.id());
    if !constructor_builtin(b) || b == Builtin.SymbolFunction { heap.fault = Fault.TypeError; return; }
    // A built-in that makes its object from newTarget's `prototype`
    // (GetPrototypeFromConstructor, light-cljs-887: a class extending it, or
    // Reflect.construct) reads it first; a getter there runs as a frame
    // above a Construct native frame, which then constructs (construct_step).
    // ExceptionInfo always reads it, as an ordinary function does; the
    // Function constructor reads it after compiling (runtime_compile.goose).
    // Number and String convert their argument first (ToNumeric and
    // ToString precede OrdinaryCreateFromConstructor, so a Symbol's
    // TypeError comes before any getter); the primitive replaces the
    // argument, which construct_builtin's conversion leaves as it is.
    if (b == Builtin.Number || b == Builtin.String) && argc > 0 && !same_value(heap, nt, f) {
        let v = wrapper_primitive(job, b, job.operands[(base + 1)..]);
        if heap.fault != Fault.None { return; }
        job.operands[base + 1] = v;
    }
    if b == Builtin.ExceptionInfo || b != Builtin.Proxy && b != Builtin.Function && !same_value(heap, nt, f) {
        let proto, getter = closure_prototype(job, nt);
        if heap.fault != Fault.None { return; }
        if getter.is_object() {
            if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
            job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: f, receiver: Value.Undefined, input: nt, native: Native.Construct });
            call_accessor(job, getter, nt, []);
            return;
        }
        construct_builtin(job, b, f, nt, base, proto);
        return;
    }
    construct_builtin(job, b, f, nt, base, Value.Empty);
}
// The [[Construct]] of constructor built-in b (function object f) with
// newTarget nt, f and its arguments the operands from `base`, its object's
// prototype `proto` (Empty: the realm's for b, as when nt is f or nt's
// `prototype` is not an object).
fn construct_builtin(job: Job&, b: Builtin, f: Value, nt: Value, base: i64, proto: Value) {
    let heap .= job.heap;
    var value = Value.Undefined;
    match b {
        // new Proxy(target, handler) (light-cljs-845; proxy.goose).
        Proxy => { value = construct_proxy(job, job.operands[(base + 1)..]); },
        // OrdinaryCreateFromConstructor(newTarget, %Object.prototype%): a
        // newTarget other than Object makes an ordinary object, whatever
        // the argument.
        Object => {
            if same_value(heap, nt, f) { value = call_builtin(job, node(heap, f.id()).function, Value.Undefined, job.operands[(base + 1)..]); }
            else { value = allocate(heap, Node { kind: node_kind::Object, proto: proto }); }
        },
        // The Function constructor's DynamicFunction frame (light-cljs-857;
        // runtime_compile.goose) delivers the function at `base` and applies
        // a newTarget other than Function.
        Function => { start_dynamic_function(job, job.operands[(base + 1)..], base, if same_value(heap, nt, f) { Value.Undefined } else { nt }); return; },
        // The error constructors (light-cljs-870; realm.goose start_error)
        // take newTarget's `prototype` first, then run their steps, as an
        // ErrorConstruct frame when one runs JavaScript; the error goes at `base`.
        Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError => {
            start_error(job, b, job.operands[(base + 1)..], base, proto);
            return;
        },
        // An array keeps its exotic `length` with another prototype.
        Array => { value = call_builtin(job, node(heap, f.id()).function, Value.Undefined, job.operands[(base + 1)..]); },
        Number, String, Boolean => { value = construct_wrapper(job, b, job.operands[(base + 1)..]); },
        // new ExceptionInfo(...) (light-cljs-875; realm.goose
        // construct_exception_info) with newTarget's `prototype`.
        ExceptionInfo => { value = construct_exception_info(job, nt, job.operands[(base + 1)..], true, proto); },
        _ => { unsupported_step(job, b, "construct"); return; },
    }
    if heap.fault != Fault.None { return; }
    if proto.is_object() && b != Builtin.ExceptionInfo && b != Builtin.Object && value.is_object() { node_mut(heap, value.id()).proto = proto; }
    job.operands.resize(base); job.operands.push(value);
}
// GetPrototypeFromConstructor(nt) for construct: the prototype (Empty, the
// constructor's realm default, when not an object), or the getter of an
// accessor `prototype`, which construct runs (light-cljs-886; for
// built-ins, light-cljs-887).
fn closure_prototype(job: Job&, nt: Value) -> Value, Value {
    let heap .= job.heap;
    if node(heap, nt.id()).kind == node_kind::Closure && lazy_prototype(heap, nt.id()) && !writable(job, nt.id()) { unsupported_detail(job, "construct: a function whose lazy prototype is in a row this job did not allocate"); return Value.Undefined, Value.Undefined; }
    let k = key_named(heap, "prototype");
    if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
    prepare_function(job, nt, k, false);
    if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
    let v, getter = lookup(heap, nt, k);
    if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; }
    return if v.is_object() { v } else { Value.Empty }, getter;
}
// OrdinaryCreateFromConstructor's object with prototype `proto`, then
// constructor f's frame with it as `this` and nt as new.target; f and its
// arguments are the operands from `base`.
fn enter_constructor(job: Job&, f: Value, nt: Value, proto: Value, base: i64) {
    let heap .= job.heap;
    let o = allocate(heap, Node { kind: node_kind::Object, proto: proto });
    if heap.fault != Fault.None { return; }
    if push_frame(job, f, o, job.operands[(base + 1)..], false, base, nt) { job.operands.resize(base); }
}
// The Construct frame's step (see Native): the getter's value on the
// operand top is newTarget's prototype, the constructor's realm default
// when it is not an object; the constructor's frame (a closure's) or its
// construction (a built-in's, light-cljs-887) replaces this one.
fn construct_step(job: Job&, frame: Frame) {
    let heap .= job.heap;
    if frame.pc != 0 || job.operands.len < frame.operand_base + 2 || !same_bits(job.operands[frame.operand_base], frame.callee) || !frame.callee.is_object() { heap.fault = Fault.InvalidState; return; }
    let p = job.operands.pop();
    if !valid(heap, p) { return; }
    job.frames.pop();
    let proto = if p.is_object() { p } else { Value.Empty };
    if node(heap, frame.callee.id()).kind == node_kind::Builtin { construct_builtin(job, builtin_of(heap, frame.callee.id()), frame.callee, frame.input, frame.operand_base, proto); return; }
    enter_constructor(job, frame.callee, frame.input, proto, frame.operand_base);
}
// super(a1, ..., an) at `at` (ConstructSuper, light-cljs-887; the Pants
// engine's construct_target with a target): parent newTarget a1..an on the
// operand top, the parent's [[Construct]] with that newTarget, its result
// delivered to this frame. A parent that is not a constructor (a derived
// class of null has %Function.prototype%) is construct's TypeError. new
// Number(o) and new String(o) as the parent convert their first argument
// first, the instruction then running again. Returns whether the frame
// continues in place.
fn construct_super(job: Job&, top: i64, at: i64, argc: i64) -> bool {
    let heap .= job.heap;
    let first = job.operands.len - argc;
    let parent = job.operands[first - 2];
    let converts, hint = argument_converts(heap, parent, job.operands[first..], true);
    if converts { job.frames[top].pc = at; start_convert(job, hint, hint == Hint.String, 1, argc); in_place(job, top, at); return false; }
    let target = job.operands[first - 1];
    for i in (first - 1)..(job.operands.len - 1) { job.operands[i] = job.operands[i + 1]; }
    job.operands.pop();
    job.frames[top].pc = at + 1; construct(job, argc, target);
    return in_place(job, top, at);
}
// BindThis: cell result → result, the derived constructor's `this` bound
// to super()'s result; a second super() is a ReferenceError after its
// parent ran. Returns false with a fault.
fn bind_this(job: Job&) -> bool {
    let heap .= job.heap;
    let value = job.operands.pop(); let cell = job.operands.pop();
    if !cell.is_object() || node(heap, cell.id()).kind != node_kind::Binding { heap.fault = Fault.InvalidState; return false; }
    if !writable(job, cell.id()) { heap.fault = Fault.NativeBoundary; return false; }
    let row .= node_mut(heap, cell.id());
    if row.initialized { reference_fault(job, "this can be initialized only once"); return false; }
    row.value = value; row.initialized = true;
    job.operands.push(value);
    return true;
}
// ResolveThis of a derived constructor's `this` cell on the operand top:
// its value, or a ReferenceError before super() bound it.
fn resolve_this(job: Job&, cell: Value) -> bool {
    let row .= node(job.heap, cell.id());
    if !row.initialized { reference_fault(job, "this is not initialized"); return false; }
    job.operands[job.operands.len - 1] = row.value;
    return true;
}
// A ReferenceError with this message.
fn reference_fault(job: Job&, message: const u8[:]) {
    job.detail.clear(); for c in message { job.detail.push(c); }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.ReferenceError; }
}
// InitHeritage: class parent proto → class (the Pants engine's
// inherit_class). The class's [[Prototype]] becomes the parent (it stays
// %Function.prototype% for `extends null`) and its `prototype` object's
// the parent's `prototype`, read by the lowering's GetField (null for
// `extends null`); a value other than an object or null is a TypeError.
// Both rows are the class's, made by this job.
fn init_heritage(job: Job&) {
    let heap .= job.heap;
    let proto = job.operands.pop(); let parent = job.operands.pop(); let class = job.operands[job.operands.len - 1];
    if !class.is_object() || node(heap, class.id()).kind != node_kind::Closure || constructor_kind(heap, class.id()) <= constructor::BaseClass { heap.fault = Fault.InvalidState; return; }
    if !proto.is_object() && !proto.is_null() && !parent.is_null() { type_error(job, "parent prototype must be an object or null"); return; }
    let k = key_named(heap, "prototype");
    if heap.fault != Fault.None { return; }
    let own = find_own(heap, class.id(), k);
    if heap.fault != Fault.None { return; }
    if !own.found || !own.value.is_object() || !writable(job, class.id()) || !writable(job, own.value.id()) { heap.fault = Fault.InvalidState; return; }
    if parent.is_object() { node_mut(heap, class.id()).proto = parent; }
    node_mut(heap, own.value.id()).proto = if proto.is_object() { proto } else { Value.Null };
}
// GetPrototypeFromConstructor(newTarget) for construct, Empty (the kind's
// realm default) when it is not an object. Making a closure's lazy
// `prototype` writes its row (decision 3).
fn construct_prototype(job: Job&, nt: Value) -> Value {
    let heap .= job.heap;
    if node(heap, nt.id()).kind == node_kind::Closure && lazy_prototype(heap, nt.id()) && !writable(job, nt.id()) { unsupported_detail(job, "construct: a function whose lazy prototype is in a row this job did not allocate"); return Value.Undefined; }
    return prototype_from_constructor(job, nt, Value.Empty, "a constructor's prototype getter");
}
// GetPrototypeFromConstructor(F) for a new object of F's (light-cljs-831,
// light-cljs-790): F's own `prototype` (an ordinary or generator function's,
// made first if lazy, as a read makes it) when it is an object, else
// `fallback` (Empty: the kind's realm default). A getter there fails by name
// (`what`): an ordinary function's `prototype` is a non-configurable data
// property, so only an inherited one can be an accessor.
fn prototype_from_constructor(job: Job&, callee: Value, fallback: Value, what: const u8[:]) -> Value {
    let heap .= job.heap;
    let k = key_named(heap, "prototype");
    if heap.fault != Fault.None { return Value.Undefined; }
    prepare_function(job, callee, k, false);
    if heap.fault != Fault.None { return Value.Undefined; }
    let v, getter = lookup(heap, callee, k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if getter.is_object() { unsupported_detail(job, what); return Value.Undefined; }
    return if v.is_object() { v } else { fallback };
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
    if callee.is_object() && node(job.heap, callee.id()).kind == node_kind::Closure {
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
    if valid(heap, callee) && callee.is_object() && node(heap, callee.id()).kind == node_kind::Builtin && node(heap, callee.id()).function == 14 && method {
        callee = job.operands[base]; if first < job.operands.len { first += 1; }
    }
    if valid(heap, callee) && callee.is_object() && node(heap, callee.id()).kind == node_kind::Builtin {
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
    var kind: i64 = if valid(heap, callee) && callee.is_object() { node(heap, callee.id()).kind } else { node_kind::None };
    if kind == node_kind::Builtin && node(heap, callee.id()).function == 14 {
        callee = receiver; receiver = if first < job.operands.len { job.operands[first] } else { Value.Undefined };
        if first < job.operands.len { first += 1; }
        kind = if valid(heap, callee) && callee.is_object() { node(heap, callee.id()).kind } else { node_kind::None };
    }
    // A bound function's call runs in a Forward frame (light-cljs-852).
    if kind == node_kind::BoundFunction { start_bound(job, callee, receiver, job.operands[first..], base); return; }
    if kind == node_kind::Builtin {
        let id = node(heap, callee.id()).function;
        let args = job.operands[first..];
        let converts, hint = argument_converts(heap, callee, args);
        // Number(o), String(o): the call runs again on the primitive.
        if converts { job.frames[top].pc = at; start_convert(job, hint, hint == Hint.String, 1, args.len); return; }
        if id == 2 && args.len == 2 {
            let source = form_kind(heap, args[1]);
            if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; }
            // A non-seqable source is cljs.core's error (light-cljs-873); a
            // map's entries and a set's members are in the runtime's
            // iteration order, which this one does not model.
            else if !seq_source(job, args[1]) {}
            else if (source == node_kind::Map || source == node_kind::Set) && form_count(heap, args[1]) > 0 { heap.fault = Fault.NativeBoundary; }
            else {
                let input = args[1]; let callback = args[0]; let out = form_collection(heap, node_kind::Vector, []);
                form_count(heap, input);
                job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: callback, receiver: Value.Undefined, input: input, output: out, native: Native.Mapv });
                job.operands.resize(base);
            }
        } else if forces(heap, BUILTINS[id].which, args) { let d = args[0]; start_force(job, BUILTINS[id].which, d, base);
        } else if BUILTINS[id].which == Builtin.FunctionPrototypeToString { start_function_text(job, receiver, base);
        } else if conditional_native(BUILTINS[id].which) { start_conditional_native(job, BUILTINS[id].which, receiver, args, base);
        } else if native_builtin(BUILTINS[id].which) { native_start(job, BUILTINS[id].which, receiver, args, base);
        } else if id == 5 && job.gensym_next > 0 { let value = gensym(job, args); job.operands.resize(base); job.operands.push(value); }
        else if id >= 5 && id <= 8 { request_effect(job, id, args, frame_at(job, top, at)); job.operands.resize(base); }
        else { let value = call_builtin(job, id, receiver, args); job.operands.resize(base); job.operands.push(value); }
    } else if push_frame(job, callee, receiver, job.operands[first..], false, base) { job.operands.resize(base); }
}
// Arguments objects (light-cljs-854; ECMAScript 10.4.4.6
// CreateUnmappedArgumentsObject, QuickJS js_build_arguments). NewArguments
// makes an ordinary object of kind node_kind::Arguments (its class, for
// Object.prototype.toString) inheriting %Object.prototype%: its elements
// are the call's arguments, an element run, and its other own properties,
// in a shape, are `length` (writable, configurable), @@iterator =
// %Array.prototype.values% (writable, configurable) and `callee`. A sloppy
// function with a simple parameter list (Function.simple) gets `callee` =
// the function, a writable, configurable data property, as the mapped
// object's is (parameter aliasing is light-cljs-860's); every other gets
// the %ThrowTypeError% accessor pair, neither enumerable nor configurable.
// Every later read, write, delete, definition and enumeration is an
// ordinary object's. Creation copies the shape and the two inline slots
// of the realm's template (arguments_template_row), so it costs one node
// and one run of values: the arguments, then `callee`'s overflow slot.
// Without a realm the object gets `length`, and `callee` when mapped,
// one property at a time.
fn new_arguments(heap: Heap&, args: const Value[:], callee: Value, mapped: bool) -> Value {
    let argc = args.len;
    if !values_fit(heap, argc + 1) || !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    if !charge(heap, argc + 1) { return Value.Undefined; }
    if !heap.global.is_object() {
        let offset = add_values(heap, args);
        if offset < 0 { return Value.Undefined; }
        let o = allocate(heap, Node { kind: node_kind::Arguments, start: offset as u32, count: argc as u32 });
        if !o.is_object() { return Value.Undefined; }
        let length = key_named(heap, "length");
        if heap.fault != Fault.None || !add_own(heap, o.id(), length, number(argc), attr_writable() | attr_configurable()) { return Value.Undefined; }
        if mapped { let k = key_named(heap, "callee"); if heap.fault != Fault.None || !add_own(heap, o.id(), k, callee, attr_writable() | attr_configurable()) { return Value.Undefined; } }
        return o;
    }
    let t = arguments_template_row(mapped);
    let shape = node(heap, t).shape; let iterator = node(heap, t).slots[1]; let extra: i64 = overflow(node(heap, t));
    let offset = add_values(heap, args, extra, if mapped { callee } else { slot(heap, node(heap, t).keys) });
    if offset < 0 { return Value.Undefined; }
    return allocate(heap, Node { kind: node_kind::Arguments, start: offset as u32, count: argc as u32, keys: (offset + argc) as u32, key_count: (extra + inline_slots()) as u32, key_capacity: extra as u32, shape: shape, slots: [number(argc), iterator] });
}
// `arguments` (new_arguments) or a rest array after `skip` parameters.
fn arguments(job: Job&, frame: Frame, f: const Function&, rest: bool, skip_count: i64) {
    let heap .= job.heap;
    let start = frame.local_base + f.locals;
    let skip = if rest { min(skip_count, frame.argument_count) } else { 0 };
    let args = job.locals[(start + skip)..(start + frame.argument_count)];
    if !rest {
        let o = new_arguments(heap, args, frame.callee, !f.strict && f.simple);
        if heap.fault == Fault.None { job.operands.push(o); }
    } else {
        let array = allocate(heap, Node { kind: node_kind::Array });
        if array.is_object() { for value, i in args { if !valid(heap, value) || !set_element(heap, array, i, value) { break; } } }
        job.operands.push(array);
    }
}
// Cell opcodes: CellInit initializes, CellGet reads, CellSet writes.
fn cell(job: Job&, op: i64) {
    let heap .= job.heap;
    let value = if op == opcode::CellGet { Value.Undefined } else { job.operands.pop() }; let cell = job.operands.pop();
    if valid(heap, cell) && cell.is_object() && node(heap, cell.id()).kind == node_kind::Binding {
        // A closure environment the job did not allocate is read-only.
        if op != opcode::CellGet && !writable(job, cell.id()) { heap.fault = Fault.NativeBoundary; return; }
        // The write log keeps no cell writes (light-cljs-801): nothing reads them.
        let row .= node_mut(heap, cell.id());
        if op == opcode::CellGet || op == opcode::CellSet {
            if !row.initialized { heap.fault = Fault.ReferenceError; }
            else if op == opcode::CellSet && row.immutable { heap.fault = Fault.ConstAssignment; }
            else if op == opcode::CellGet { job.operands.push(row.value); }
            else { row.value = value; }
        } else if row.initialized && row.immutable { heap.fault = Fault.ConstAssignment; }
        else { row.value = value; row.initialized = true; }
    } else if valid(heap, cell) && cell.is_object() && node(heap, cell.id()).kind == node_kind::MappedBinding {
        mapped_cell(job, op, cell.id(), value);
    } else if heap.fault == Fault.None { heap.fault = Fault.TypeError; }
}
// Mapped arguments objects (light-cljs-860; ECMAScript 10.4.4.7
// CreateMappedArgumentsObject and 10.4.4.1-5, the arguments exotic
// object's internal methods; QuickJS js_build_mapped_arguments and
// JS_PROP_VARREF). Only the Goose lowering emits NewMappedArguments, for a
// sloppy function with a simple parameter list that can observe the
// aliasing (rust/src/frontend/lower.rs wants_mapped_arguments); every
// other function keeps NewArguments and plain parameter locals, at no cost.
// The object is an arguments object exactly as NewArguments makes it, its
// own property i the truth for the parameter it aliases: each parameter
// local becomes a cell, for i < argument count a node_kind::MappedBinding
// naming the object and i (its reads and writes go to the object's own
// property i, an element or, after a redefinition that kept the mapping, a
// shape key or record), past it an ordinary binding cell. The object's
// namespace_value names its node_kind::ArgumentMap, whose run lists the
// cell of each mapped index. [[Delete]] and a [[DefineOwnProperty]] of an
// accessor or of a non-writable data property unmap index i: its map entry
// becomes Undefined and the cell an ordinary binding holding the value the
// specification leaves it (the old value, or a defined value first written
// through). Every other operation on the object is an ordinary object's,
// so reads, writes, enumeration and fnRest's run need no case. A duplicate
// parameter name leaves its earlier occurrences' cells unread, so only
// the last occurrence aliases. All links are node rows: a snapshot carries
// and validates them (snapshot.goose), in-job reclamation remaps them and
// treats a cell's link to its object as weak: a cell whose object nothing
// else reaches is detached, as unmapping would leave it, and the object
// discarded (light-cljs-867; reclaim.goose, "Mapped parameter cells"),
// a detached generator or async activation keeps its cells as any local,
// and direct eval reads the parameters through the same cells (858's
// eval-site records name the parameter locals, now cells).
fn mapped_arguments(job: Job&, frame: Frame, f: const Function&) {
    let heap .= job.heap;
    let start = frame.local_base + f.locals; let base = frame.local_base;
    var args: Value[>..] = []; args.append(job.locals[start..(start + frame.argument_count)]);
    let o = new_mapped_arguments(heap, args[..], frame.callee, job.locals[base..(base + f.params)]);
    if heap.fault == Fault.None { job.operands.push(o); }
}
// The mapped arguments object of the call's arguments `args` and its
// parameter locals `params`, each of which becomes its cell.
fn new_mapped_arguments(heap: Heap&, args: const Value[:], callee: Value, params: Value[:]) -> Value {
    let argc = args.len; let mapped = min(argc, params.len);
    // One node and argc + 1 values for the object, one node and `mapped`
    // values for its map, a node a parameter: checked before any changes.
    if !nodes_fit(heap, params.len + 2) || !values_fit(heap, argc + 1 + mapped) { memory(heap); return Value.Undefined; }
    let o = new_arguments(heap, args, callee, true);
    if heap.fault != Fault.None || !charge(heap, mapped) { return Value.Undefined; }
    let run = add_values(heap, [], mapped);
    if run < 0 { return Value.Undefined; }
    let m = allocate(heap, Node { kind: node_kind::ArgumentMap, start: run as u32, count: mapped as u32, value: o });
    if !m.is_object() { return Value.Undefined; }
    for i in params.len {
        let c = if i < argc { allocate(heap, Node { kind: node_kind::MappedBinding, length: i as u32, initialized: true, value: o }) } else { allocate(heap, Node { kind: node_kind::Binding, initialized: true, value: params[i] }) };
        if !c.is_object() { return Value.Undefined; }
        if i < argc { set_slot(heap, run + i, c); }
        params[i] = c;
    }
    node_mut(heap, o.id()).namespace_value = m;
    return o;
}
// CellGet, CellSet or CellInit of mapped cell c: the arguments object's
// own property it aliases. A cell or object the job did not allocate is
// read-only.
fn mapped_cell(job: Job&, op: i64, c: i64, value: Value) {
    let heap .= job.heap;
    let o: i64 = node(heap, c).value.id(); let i: i64 = node(heap, c).length;
    if op != opcode::CellGet && (!writable(job, c) || !writable(job, o)) { heap.fault = Fault.NativeBoundary; return; }
    if op == opcode::CellGet { let v = mapped_value(heap, o, i, c); if heap.fault == Fault.None { job.operands.push(v); } return; }
    mapped_write(heap, o, i, c, value);
}
// Write mapped index i of arguments object o, whose cell is c: its own
// property, as mapped_value finds it.
fn mapped_write(heap: Heap&, o: i64, i: i64, c: i64, value: Value) {
    let v = object_element(heap, o, i);
    if !v.is_hole() { set_slot(heap, node(heap, o).start + i, value); return; }
    let k = node(heap, c).namespace_value;
    if !k.is_string() { heap.fault = Fault.InvalidState; return; }
    let own = find_own(heap, o, k);
    if heap.fault != Fault.None { return; }
    // A mapped property is a writable data property (unmapping keeps it so).
    if !own.found || is_accessor(own.value) || own.attrs & attr_writable() == 0 { heap.fault = Fault.InvalidState; return; }
    write_own(heap, o, own.index, value);
}
// The value of mapped index i of arguments object o, whose cell is c: its
// own property, an element or, once a redefinition moved it, the shape key
// or record of the interned key the cell keeps (its namespace_value), so a
// cell's reads and writes never allocate.
fn mapped_value(heap: Heap&, o: i64, i: i64, c: i64) -> Value {
    let v = object_element(heap, o, i);
    if !v.is_hole() { return v; }
    let k = node(heap, c).namespace_value;
    if !k.is_string() { heap.fault = Fault.InvalidState; return Value.Undefined; }
    let own = find_own(heap, o, k);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !own.found || is_accessor(own.value) { heap.fault = Fault.InvalidState; return Value.Undefined; }
    return own.value;
}
// The cell aliasing index i of arguments object o, or -1 when i is not
// mapped (or o is not a mapped arguments object).
fn mapped_cell_of(heap: const Heap&, o: i64, i: i64) -> i64 {
    let n .= node(heap, o);
    if n.kind != node_kind::Arguments || !n.namespace_value.is_object() || i < 0 { return -1; }
    let map .= node(heap, n.namespace_value.id());
    if i >= map.count { return -1; }
    let c = slot(heap, map.start + i);
    return if c.is_object() { c.id() } else { -1 };
}
// Unmap index i of arguments object o, whose cell is c: the cell becomes an
// ordinary binding holding `value`.
fn unmap(heap: Heap&, o: i64, i: i64, c: i64, value: Value) {
    set_slot(heap, node(heap, node(heap, o).namespace_value.id()).start + i, Value.Undefined);
    detach_cell(heap, c, value);
}
// Mapped cell c becomes an ordinary binding holding `value`; its object's
// map is the caller's (unmap clears its entry; in-job reclamation detaches
// the cells of an object it discards, reclaim.goose).
fn detach_cell(heap: Heap&, c: i64, value: Value) {
    let cell .= node_mut(heap, c); cell.kind = node_kind::Binding; cell.value = value; cell.length = 0; cell.namespace_value = Value.Null;
}
fn get_property(job: Job&, k: Value, named: bool) {
    let heap .= job.heap;
    let object = job.operands.pop();
    if job.root_receiver.is_object() && object == job.root_receiver {
        let value, found = own_property(heap, object, k);
        // Only the compiler root intrinsic may suspend and demand a
        // definition. A raw JS bypass must not observe stale undefined.
        if !found { heap.fault = Fault.DependencyNeed; } job.operands.push(value);
    } else if valid(heap, object) && object.is_object() && (node(heap, object.id()).kind == node_kind::Builtin || node(heap, object.id()).kind == node_kind::Closure && node(heap, object.id()).key_count == 0 && !node(heap, object.id()).initialized) && key_is(heap, k, "call") {
        job.operands.push(get(heap, object, k));
    } else if object.is_object() && !realm_intrinsic(object.id()) && object.id() < job.protected_nodes {
        let value, found = own_property(heap, object, k);
        let optional = object == heap.global && (key_is(heap, k, "__lightCompilerRootReceiver") || key_is(heap, k, "__lightCompilerRootContext"));
        if !found && !optional { heap.fault = Fault.NativeBoundary; } job.operands.push(value);
    } else {
        // An accessor's getter runs as a frame entered directly: its return
        // value is the read's result for this frame (light-cljs-787). On a
        // primitive base (read) the getter's receiver is the primitive.
        prepare_function(job, object, k, false);
        let value, getter = read(heap, object, k);
        if heap.fault != Fault.None { property_fault(job, object, k, named, false); job.operands.push(value); return; }
        if !getter.is_object() { job.operands.push(value); return; }
        // %ThrowTypeError% (Function.prototype.caller and .arguments) runs in place.
        let done, v = accessor_builtin(job, getter, object, 0);
        if done { job.operands.push(v); }
        else if node(heap, getter.id()).kind == node_kind::Closure { enter(job, getter, object, []); }
        // Any other getter (a built-in, a bound function; light-cljs-881)
        // runs on the general call path, its result this read's.
        else { call_accessor(job, getter, object, []); }
    }
}
// The message of a fault of [[Get]] or [[Set]] of key k on `base` (only
// on the fault path): an undefined or null base's TypeError (base_error),
// a Symbol or BigInt base's named gap (primitive_gap).
fn property_fault(job: Job&, base: Value, k: Value, named: bool, write: bool) {
    if job.heap.fault == Fault.UnsupportedOperand { primitive_gap(job, base); }
    else if job.heap.fault == Fault.TypeError && base.tag() <= value_tag::Null && job.detail.len == 0 { base_error(job, base, k, named, write); }
    // A property write's only RangeError: an array's invalid new length
    // (array_length).
    else if job.heap.fault == Fault.RangeError && job.detail.len == 0 { for c in "invalid array length" { job.detail.push(c); } }
}
// A property access on a BigInt base fails by name (decision 1):
// %BigInt.prototype% is the BigInt area's (light-cljs-771). (A symbol
// base reads %Symbol.prototype% since light-cljs-885.)
fn primitive_gap(job: Job&, base: Value) {
    if !base.is_symbol() && !base.is_bigint() { return; }
    job.detail.clear();
    for c in (if base.is_symbol() { "property access on a Symbol: %Symbol.prototype%" } else { "property access on a BigInt: %BigInt.prototype%" }) { job.detail.push(c); }
}
// A ReferenceError for global name k: "k is not defined".
fn not_defined(job: Job&, k: Value) {
    job.detail.clear(); append_utf16(job.heap, k, job.detail);
    for c in " is not defined" { job.detail.push(c); }
    job.heap.fault = Fault.ReferenceError;
}
// A catchable `fault` whose message names key or name k between `before`
// and `after` (append_key's spelling). Messages are built here, on the
// fault path, never by the instruction that may fault.
fn named_error(job: Job&, fault: Fault, before: const u8[:], k: Value, after: const u8[:]) {
    job.detail.clear(); for c in before { job.detail.push(c); }
    append_key(job.heap, k, job.detail);
    for c in after { job.detail.push(c); }
    job.heap.fault = fault;
}
// Property key or name k as QuickJS spells an atom in a message
// (JS_AtomGetStr): a string as its code units (an index key is already its
// digits, key()), a symbol as its description (`Symbol.iterator` for a
// well-known one), a number as its ToString. Shared by the property-access
// errors below and the callee names of not-a-function errors.
fn append_key(heap: Heap&, k: Value, out: u16[>..]&) {
    if k.is_string() { append_utf16(heap, k, out); return; }
    if k.is_symbol() { append_description(heap, k, out); return; }
    if k.is_number() { var digits: u8[>..] = []; number::append_decimal(digits, k.num()); for c in digits { out.push(c); } }
}
// The TypeError of a property read (`write` false) or write on an undefined
// or null base, in QuickJS's wording (JS_GetPropertyInternal,
// JS_GetPropertyValue and JS_SetPropertyInternal): "cannot read property
// 'k' of undefined", "cannot set property 'k' of null". A computed read
// (`named` false) names no key, as QuickJS's OP_get_array_el does not; a
// write names its key either way.
fn base_error(job: Job&, base: Value, k: Value, named: bool, write: bool) {
    job.detail.clear();
    for c in (if write { "cannot set property " } else { "cannot read property " }) { job.detail.push(c); }
    if named || write {
        job.detail.push(39);
        // QuickJS formats the key with %s, so it ends at an embedded NUL.
        let at = job.detail.len; append_key(job.heap, k, job.detail);
        var end = at; while end < job.detail.len && job.detail[end] != 0 { end += 1; }
        job.detail.resize(end);
        job.detail.push(39); job.detail.push(32);
    }
    for c in (if base.is_null() { "of null" } else { "of undefined" }) { job.detail.push(c); }
    job.heap.fault = Fault.TypeError;
}
// Script declarations and global name resolution (light-cljs-824): the
// global environment of GlobalDeclarationInstantiation, as the Pants engine's
// vm/dispatch.rs global_step and realm.rs declare_global_with and
// check_global_declaration implement it, and QuickJS's
// JS_CheckDefineGlobalVar, OP_get_var and OP_put_var. Its declarative record
// is Job.lexicals; its object record is the global object, read and written
// on every access, so accessors and later redefinitions are seen.
//
// A global object the job may not write (a compiler job's, Job.protected_nodes)
// faults every one of these NativeBoundary before it changes anything, as a
// property write into it does; a compiler job's script therefore never
// declares a binding, and its GetGlobal keeps the compiler's own path.
//
// The record is a dictionary-mode node the job allocates: one record per
// lexical name (interned key), its value the binding's cell (node_kind::Binding) and its
// attributes writable for `let` and none for `const` and `class`. Its rows
// are the job's, so a reused worker's reset drops it; a suspended compiler
// job keeps the row ID in its continuation (compiler_session.goose Saved).
fn lexical_entry(job: Job&, key: Value) -> Own {
    if !job.lexicals.is_object() { return Own {}; }
    return find_own(job.heap, job.lexicals.id(), key);
}
fn global_cell(heap: const Heap&, v: Value) -> bool { return v.is_object() && v.id() < heap.nodes.len && node(heap, v.id()).kind == node_kind::Binding; }
// GetGlobal and GetGlobalOrUndefined (`typeof x`) where the job
// may write the global object: a lexical binding first (uninitialized, a
// ReferenceError even under `typeof`), then the global object's own or
// inherited property, a getter running as a frame (get_property). A name
// neither has is a ReferenceError, or undefined for `typeof`.
fn get_global(job: Job&, k: Value, throws: bool) {
    let heap .= job.heap;
    let key = intern(heap, k);
    if heap.fault != Fault.None { return; }
    let lexical = lexical_entry(job, key);
    if heap.fault != Fault.None { return; }
    if lexical.found {
        if !global_cell(heap, lexical.value) { heap.fault = Fault.InvalidState; return; }
        let cell .= node(heap, lexical.value.id());
        if !cell.initialized { named_error(job, Fault.ReferenceError, "cannot access '", key, "' before initialization"); return; }
        job.operands.push(cell.value); return;
    }
    let own = own_entry(heap, heap.global, key);
    if heap.fault != Fault.None { return; }
    if own.found && !is_accessor(own.value) { job.operands.push(own.value); return; }
    if !own.found && !has_property(heap, heap.global, key) {
        if heap.fault != Fault.None { return; }
        if throws { not_defined(job, key); return; }
        job.operands.push(Value.Undefined); return;
    }
    job.operands.push(heap.global); get_property(job, key, true);
}
// Whether global property `own` refuses a function declaration over it
// (CanDeclareGlobalFunction): a non-configurable property that is not a
// writable, enumerable data property.
fn refuses_function(own: Own) -> bool {
    return own.found && own.attrs & attr_configurable() == 0 && (is_accessor(own.value) || own.attrs & attr_writable() == 0 || own.attrs & attr_enumerable() == 0);
}
// Register lexical binding `key` with `cell` (DeclareGlobalLet,
// DeclareGlobalConst, DeclareGlobalConstValue): a SyntaxError for a name the
// global object holds as a non-configurable property (HasRestrictedGlobalProperty)
// or the record already holds.
fn declare_lexical(job: Job&, key: Value, cell: Value, mutable: bool) {
    let heap .= job.heap;
    if !global_cell(heap, cell) { heap.fault = Fault.InvalidState; return; }
    let own = own_entry(heap, heap.global, key);
    if heap.fault != Fault.None { return; }
    if own.found && own.attrs & attr_configurable() == 0 { named_error(job, Fault.SyntaxError, "cannot declare global lexical '", key, "'"); return; }
    let prior = lexical_entry(job, key);
    if heap.fault != Fault.None { return; }
    if prior.found { named_error(job, Fault.SyntaxError, "global lexical '", key, "' is already declared"); return; }
    if !job.lexicals.is_object() {
        let record = allocate(heap, Node { kind: node_kind::Object, initialized: true, proto: Value.Null });
        if heap.fault != Fault.None { return; }
        job.lexicals = record;
    }
    if !dict_append(heap, job.lexicals.id(), Property { key: key, value: cell, attrs: if mutable { attr_writable() } else { 0 } }) && heap.fault == Fault.None { memory(heap); }
}
// One GlobalOp instruction (Ins.GlobalOp) with name constant k, in a
// function whose code is `strict` or not, and `eval` code (an indirect
// eval's script, Function.eval; light-cljs-856) or not; the stack contracts
// are rust/src/vm/instr.rs's. Eval code's declarations are the Pants
// engine's declare_global_with and check_global_declaration with `eval`
// (ECMAScript EvalDeclarationInstantiation, CreateGlobalVarBinding and
// CreateGlobalFunctionBinding with D true): its properties are configurable,
// and a refused declaration is a TypeError, not a SyntaxError.
fn global_op(job: Job&, op: u8, k: Value, strict: bool, eval: bool) {
    let heap .= job.heap;
    if !k.is_string() { heap.fault = Fault.InvalidState; return; }
    let g = heap.global;
    if !g.is_object() || !writable(job, g.id()) { heap.fault = Fault.NativeBoundary; return; }
    let key = intern(heap, k);
    if heap.fault != Fault.None { return; }
    match op {
        // PutGlobal, PutGlobalStrict: v → v. A lexical binding is assigned
        // (uninitialized, a ReferenceError; const, a TypeError); otherwise
        // the global object's [[Set]], a setter running in a Setter native
        // frame. Strict code assigning a name the global object lacks is a
        // ReferenceError; sloppy code creates the property (writable,
        // enumerable, configurable). A refused assignment is a TypeError in
        // strict code and nothing in sloppy code (set_property).
        opcode::PutGlobal, opcode::PutGlobalStrict => {
            let value = job.operands.pop();
            let strict_put = strict || op == opcode::PutGlobalStrict;
            let lexical = lexical_entry(job, key);
            if heap.fault != Fault.None { return; }
            if lexical.found {
                if !global_cell(heap, lexical.value) { heap.fault = Fault.InvalidState; return; }
                if !node(heap, lexical.value.id()).initialized { named_error(job, Fault.ReferenceError, "cannot access '", key, "' before initialization"); return; }
                if lexical.attrs & attr_writable() == 0 { named_error(job, Fault.TypeError, "assignment to constant variable '", key, "'"); return; }
                if !writable(job, lexical.value.id()) { heap.fault = Fault.NativeBoundary; return; }
                node_mut(heap, lexical.value.id()).value = value; job.operands.push(value); return;
            }
            if strict_put && !has_property(heap, g, key) { if heap.fault == Fault.None { not_defined(job, key); } return; }
            job.operands.push(g); set_property(job, value, key, false, strict_put);
        },
        // DeclareGlobalVar: a var the global object lacks as an own property
        // becomes one (writable, enumerable, configurable only in eval
        // code); an existing one is kept, whatever it is.
        opcode::DeclareGlobalVar => {
            let own = own_entry(heap, g, key);
            let attrs = attr_writable() | attr_enumerable() | (if eval { attr_configurable() } else { 0 });
            // A non-extensible global object takes no new var (CheckGlobalVar
            // refused it first, light-cljs-882).
            if heap.fault == Fault.None && !own.found && node(heap, g.id()).non_extensible { named_error(job, if eval { Fault.TypeError } else { Fault.SyntaxError }, "cannot declare global '", key, "'"); return; }
            if heap.fault == Fault.None && !own.found && !add_own(heap, g.id(), key, Value.Undefined, attrs) && heap.fault == Fault.None { memory(heap); }
        },
        // DeclareGlobalFunction: the property the function's value is put
        // into (PutGlobal follows): a new or configurable one becomes a
        // writable, enumerable undefined, configurable only in eval code;
        // a non-configurable writable enumerable data property is kept; any
        // other non-configurable property is a SyntaxError (eval code: a
        // TypeError).
        opcode::DeclareGlobalFunction => {
            let own = own_entry(heap, g, key);
            if heap.fault != Fault.None { return; }
            let refused = if eval { Fault.TypeError } else { Fault.SyntaxError };
            if refuses_function(own) { named_error(job, refused, "cannot declare global function '", key, "'"); return; }
            if own.found && own.attrs & attr_configurable() == 0 { return; }
            if !own.found && node(heap, g.id()).non_extensible { named_error(job, refused, "cannot declare global '", key, "'"); return; }
            let attrs = attr_writable() | attr_enumerable() | (if eval { attr_configurable() } else { 0 });
            let ok = if own.found { define_own(heap, g.id(), key, Value.Undefined, attrs) } else { add_own(heap, g.id(), key, Value.Undefined, attrs) };
            if !ok && heap.fault == Fault.None { named_error(job, refused, "cannot declare global '", key, "'"); }
        },
        // DeclareGlobalLet, DeclareGlobalConst: cell → cell, the script's
        // cell registered by name.
        opcode::DeclareGlobalLet, opcode::DeclareGlobalConst => { declare_lexical(job, key, job.operands[job.operands.len - 1], op == opcode::DeclareGlobalLet); },
        // InitGlobalLexical: value → (the registered cell initialized).
        opcode::InitGlobalLexical => {
            let value = job.operands.pop();
            let lexical = lexical_entry(job, key);
            if heap.fault != Fault.None { return; }
            if !lexical.found || !global_cell(heap, lexical.value) { heap.fault = Fault.InvalidState; return; }
            if !writable(job, lexical.value.id()) { heap.fault = Fault.NativeBoundary; return; }
            let cell .= node_mut(heap, lexical.value.id()); cell.value = value; cell.initialized = true;
        },
        // CheckGlobalVar, CheckGlobalFunction: before any declaration effect
        // (CanDeclareGlobalVar, CanDeclareGlobalFunction; the Pants engine's
        // check_global_declaration): a name the global object lacks needs it
        // extensible (light-cljs-882), and a function's existing property
        // must not refuse it.
        opcode::CheckGlobalVar, opcode::CheckGlobalFunction => {
            // A var on an extensible global object needs no lookup.
            if op == opcode::CheckGlobalVar && !node(heap, g.id()).non_extensible { return; }
            let own = own_entry(heap, g, key);
            if heap.fault != Fault.None { return; }
            if !own.found && node(heap, g.id()).non_extensible || op == opcode::CheckGlobalFunction && refuses_function(own) { named_error(job, if eval { Fault.TypeError } else { Fault.SyntaxError }, "cannot declare global '", key, "'"); }
        },
        // DeleteGlobal (sloppy `delete x`): → bool, false for a lexical
        // binding or a non-configurable property.
        opcode::DeleteGlobal => {
            let lexical = lexical_entry(job, key);
            if heap.fault != Fault.None { return; }
            let deleted = !lexical.found && delete_property(heap, g, key);
            if heap.fault == Fault.None { job.operands.push(boolean(deleted)); }
        },
        // DeclareGlobalConstValue: a const the script keeps as a value gets
        // its record's cell, uninitialized until InitGlobalLexical.
        opcode::DeclareGlobalConstValue => {
            let cell = allocate(heap, Node { kind: node_kind::Binding, immutable: true });
            if heap.fault == Fault.None { declare_lexical(job, key, cell, false); }
        },
        // CheckGlobalLexicalConflict: a var or function name the record holds.
        opcode::CheckGlobalLexicalConflict => {
            let lexical = lexical_entry(job, key);
            if heap.fault == Fault.None && lexical.found { named_error(job, Fault.SyntaxError, "global declaration conflicts with lexical '", key, "'"); }
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// Append UTF-8 `bytes` as UTF-16 units; false (out partly written) for a
// malformed sequence. A surrogate code point's three-byte sequence (an
// image source's lone surrogate, program::source_text) is that unit.
fn append_utf8(out: u16[>..]&, bytes: const u8[:]) -> bool {
    var i: i64 = 0;
    while i < bytes.len {
        let b = bytes[i] as u32;
        let n: i64 = if b < 128 { 1 } else if b >= 194 && b < 224 { 2 } else if b >= 224 && b < 240 { 3 } else if b >= 240 && b < 245 { 4 } else { 0 };
        if n == 0 || i + n > bytes.len { return false; }
        var cp: u32 = if n == 1 { b } else if n == 2 { b & 31 } else if n == 3 { b & 15 } else { b & 7 };
        for j in 1..n { let c = bytes[i + j] as u32; if (c & 192) != 128 { return false; } cp = (cp << 6) | (c & 63); }
        if cp >= 65536 { let s = cp - 65536; out.push((55296 + (s >> 10)) as u16); out.push((56320 + (s & 1023)) as u16); } else { out.push(cp as u16); }
        i += n;
    }
    return true;
}
// ThrowError: the compiler's static throw of ThrowKind `kind`, a
// catchable error with the Pants engine's message (throw_message), naming
// the binding by the instruction's source text, as throw_message does (the
// generic wording only for a span that is not a source range).
fn throw_error(job: Job&, kind: u8, at: i64) {
    let heap .= job.heap; let code .= heap.code;
    var name: u16[>..] = []; var named = false;
    if at >= 0 && at < code.ops.len {
        let op = code.ops[at];
        named = op.start >= 0 && op.start <= op.end && op.end <= code.source_bytes.len && append_utf8(name, code.source_bytes[op.start..op.end]);
    }
    job.detail.clear();
    let words: const u8[:] = match kind {
        throw_kind::Unresolvable => if named { "" } else { "reference to an undeclared name" },
        throw_kind::Uninitialized => if named { "cannot access '" } else { "binding accessed before initialization" },
        throw_kind::ConstAssignment => if named { "assignment to constant variable '" } else { "assignment to a constant variable" },
        throw_kind::DeleteSuper => "cannot delete a super property",
        throw_kind::YieldStarMissingThrow => "yield* delegate has no throw method",
        _ => "",
    };
    for c in words { job.detail.push(c); }
    if named && kind <= throw_kind::ConstAssignment {
        for c in name { job.detail.push(c); }
        let tail: const u8[:] = if kind == throw_kind::Unresolvable { " is not defined" } else if kind == throw_kind::Uninitialized { "' before initialization" } else { "'" };
        for c in tail { job.detail.push(c); }
    }
    heap.fault = match kind { throw_kind::Unresolvable, throw_kind::Uninitialized, throw_kind::DeleteSuper => Fault.ReferenceError, throw_kind::ConstAssignment, throw_kind::YieldStarMissingThrow => Fault.TypeError, _ => Fault.InvalidState };
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
    if heap.fault != Fault.None || !object.is_object() || object.id() >= heap.nodes.len { return; }
    var at = object;
    if adding { if !lazy_function(heap, at.id()) { return; } }
    else {
        if !key_is(heap, k, "prototype") { return; }
        let key = intern(heap, k);
        loop {
            if heap.fault != Fault.None { return; }
            if lazy_prototype(heap, at.id()) { break; }
            // The read's [[Get]] reaches a proxy (and fails by name there).
            if node(heap, at.id()).kind == node_kind::Proxy { return; }
            if own_entry(heap, at, key).found || heap.fault != Fault.None { return; }
            let proto = prototype(heap, at.id());
            if !proto.is_object() || !charge(heap) { return; }
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
// read and write its run directly, without building key strings. Its
// prototype is the realm's (Empty), whose chain the realm builds without
// elements and keeps (a compiler job cannot write realm rows, and no job
// changes a realm row's prototype); another prototype may hold elements a
// hole would inherit (light-cljs-883), so such an array takes the general
// path.
fn dense(job: Job&, object: Value) -> bool {
    let heap .= job.heap;
    if !object.is_object() || !valid(heap, object) || object.id() < job.protected_nodes || node(heap, object.id()).kind != node_kind::Array || node(heap, object.id()).non_extensible || node(heap, object.id()).function != 0 || !node(heap, object.id()).proto.is_hole() { return false; }
    return !(job.root_receiver.is_object() && object == job.root_receiver);
}
// An unprotected ordinary object other than the compiler roots object with
// an element i (light-cljs-804): element opcodes read and write that element
// directly, without building its key string.
inline fn object_hit(job: const Job&, object: Value, i: i64) -> bool {
    let heap .= job.heap;
    if i < 0 || !object.is_object() || object.id() >= heap.nodes.len || object.id() < job.protected_nodes || job.root_receiver.is_object() && object == job.root_receiver { return false; }
    let n .= node(heap, object.id());
    return element_kind(n.kind) && i < n.count && !slot(heap, n.start + i).is_hole();
}
// [[Set]] from an assignment (light-cljs-787): chain, the expression yields
// the object (88), else the value. A setter runs through a Setter native
// frame; a refused assignment is a TypeError in strict code and does nothing
// in sloppy code. On a primitive base (write) a setter's receiver is the
// primitive, and a refusal says why (the Pants engine's wording).
fn set_property(job: Job&, value: Value, k: Value, chain: bool, strict: bool) {
    let heap .= job.heap;
    let object = job.operands.pop();
    let result = if chain { object } else { value };
    if object.is_object() && !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; job.operands.push(result); return; }
    prepare_function(job, object, k, true);
    // OrdinarySet reads an inherited `prototype` too: a lazy one on the
    // chain (an object made by Object.create(F), light-cljs-883) is made first.
    prepare_function(job, object, k, false);
    if heap.fault == Fault.None && length_converts(heap, object, k, value) { start_length(job, object, k, value, result, strict); return; }
    let setter, ok = write(heap, object, k, value);
    if heap.fault == Fault.None && setter.is_object() { call_setter(job, setter, object, value, result); return; }
    if heap.fault == Fault.None { if ok { record(job, object, k, value); } else if strict { refused_write(job, object, k); } }
    else { property_fault(job, object, k, true, true); }
    job.operands.push(result);
}
// The TypeError of a strict assignment to key k of `object` that [[Set]]
// refused: for a number, string or boolean base, the read-only own property
// of its string or the own property a primitive cannot take.
fn refused_write(job: Job&, object: Value, k: Value) {
    let heap .= job.heap;
    let t = wrapper_type(object);
    // A new property of a non-extensible object (light-cljs-882; the Pants
    // engine's wording).
    if object.is_object() && (k.is_string() || k.is_symbol()) && node(heap, object.id()).non_extensible {
        let key = intern(heap, k);
        if heap.fault != Fault.None { return; }
        let own = own_entry(heap, object, key);
        if heap.fault != Fault.None { return; }
        if !own.found { named_error(job, Fault.TypeError, "cannot add property '", k, "': the object is not extensible"); return; }
    }
    if object.is_object() || t < 0 || !k.is_string() { heap.fault = Fault.TypeError; return; }
    let i = index(heap, k);
    if heap.fault != Fault.None { return; }
    if object.is_string() && (key_is(heap, k, "length") || i >= 0 && i < str_len(heap, object)) { named_error(job, Fault.TypeError, "cannot assign to read-only property '", k, "' of a string"); return; }
    named_error(job, Fault.TypeError, "cannot create property '", k, if t == 0 { "' on a number: primitive values have no own properties" } else if t == 1 { "' on a string: primitive values have no own properties" } else { "' on a boolean: primitive values have no own properties" });
}
// DefineField, DefineGetter and DefineSetter (81, 103, 104; key k) and
// DefineElem, DefineGetterElem and DefineSetterElem (130, 133, 134; k the
// converted key): obj v → obj, [[DefineOwnProperty]] of an object literal's
// own property, a data property (half 0) or an accessor's getter (1) or
// setter (2), merged with the other half (light-cljs-787). A refused
// definition is a TypeError (CreateDataPropertyOrThrow). Data definitions
// enter the write log as assignments did. `class_member`: a class element
// (DefineClass, light-cljs-886; the Pants engine's define_class_element),
// non-enumerable and configurable, a method writable too; a getter or
// setter merges with the other half as a literal's does. A refusal (a
// computed `static ['prototype']()`, say) is a TypeError there too.
fn define_field(job: Job&, k: Value, value: Value, half: u8, class_member: bool = false) {
    let heap .= job.heap;
    let object = job.operands[job.operands.len - 1];
    if !valid(heap, object) || !key_valid(heap, k) || !valid(heap, value) { return; }
    if !object.is_object() || !k.is_string() && !k.is_symbol() { heap.fault = if object.is_object() { Fault.UnsupportedOperand } else { Fault.TypeError }; return; }
    if !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; return; }
    prepare_function(job, object, k, true);
    let key = intern(heap, k);
    if heap.fault != Fault.None { return; }
    let attrs = if class_member { if half == 0 { attr_writable() | attr_configurable() } else { attr_configurable() } } else if half == 0 { attrs_data() } else { attrs_accessor() };
    let ok = define_property(heap, object, key, value, attrs, half);
    if heap.fault != Fault.None { return; }
    if !ok { heap.fault = Fault.TypeError; } else if half == 0 { record(job, object, k, value); }
}
// SetProto: obj v → obj. An object or null v becomes the literal's
// prototype ([[SetPrototypeOf]], set_prototype: the literal is the job's
// new extensible object, so nothing refuses it); any other v is ignored.
fn set_proto(job: Job&) {
    let heap .= job.heap;
    let v = job.operands.pop(); let object = job.operands[job.operands.len - 1];
    if !valid(heap, object) || !valid(heap, v) || !v.is_object() && !v.is_null() { return; }
    if !object.is_object() || node(heap, object.id()).kind != node_kind::Object { heap.fault = Fault.UnsupportedOperand; return; }
    if !set_prototype(job, object, v) && heap.fault == Fault.None { prototype_type_error(job, object.id(), v); }
}
// Delete and DeleteStrict: o k → b, [[Delete]] of the own
// property (delete_property). Undefined or null o is a TypeError; a string's
// `length` and indices below it are non-configurable, and other primitives
// have no own properties. A non-configurable property stays: false in
// sloppy code, a TypeError in strict code.
fn delete_op(job: Job&, strict: bool) {
    let heap .= job.heap;
    let kv = job.operands.pop(); let object = job.operands.pop();
    if !valid(heap, object) || !key_valid(heap, kv) { return; }
    if object.tag() <= value_tag::Null { type_error(job, "cannot convert to object"); return; }
    let k = key(heap, kv);
    if heap.fault != Fault.None { return; }
    var ok = true;
    if object.is_string() { let i = index(heap, k); ok = !(key_is(heap, k, "length") || i >= 0 && i < str_len(heap, object)); }
    else if object.is_object() {
        if !writable(job, object.id()) { heap.fault = Fault.NativeBoundary; return; }
        let key = intern(heap, k);
        if heap.fault != Fault.None { return; }
        // A lazy closure's `length` or `name` is made before it goes.
        if key.is_string() && (key_is(heap, key, "length") || key_is(heap, key, "name")) { prepare_function(job, object, key, true); }
        ok = delete_property(heap, object, key);
    }
    if heap.fault != Fault.None { return; }
    if !ok && strict { type_error(job, "could not delete property"); return; }
    job.operands.push(boolean(ok));
}
// In: k o → bool, `k in o` ([[HasProperty]]). A non-object o is a
// TypeError; an object k converts before this (the Convert frame,
// light-cljs-828), and a light-cljs runtime value fails UnsupportedOperand.
fn in_op(job: Job&) {
    let heap .= job.heap;
    let object = job.operands.pop(); let kv = job.operands.pop();
    if !valid(heap, object) || !key_valid(heap, kv) { return; }
    if !object.is_object() { heap.fault = Fault.TypeError; return; }
    let k = key(heap, kv);
    if heap.fault != Fault.None { return; }
    let key = intern(heap, k);
    if heap.fault != Fault.None { return; }
    let found = has_property(heap, object, key);
    if heap.fault == Fault.None { job.operands.push(boolean(found)); }
}

// ---- instanceof (light-cljs-834; ECMAScript 13.10.2 InstanceofOperator and
// 7.3.22 OrdinaryHasInstance, the Pants engine's native::start_instanceof
// and vm/prototype.rs instance_of_prototype, QuickJS's JS_IsInstanceOf and
// JS_OrdinaryIsInstanceOf).
//
// Instanceof: v target → bool. A non-object target is a TypeError; then
// GetMethod(target, @@hasInstance): a callable method is called with target
// as receiver and v as its argument, its result converted by ToBoolean; a
// non-callable one is a TypeError; with none (undefined or null) a
// non-callable target is a TypeError, and otherwise OrdinaryHasInstance.
// The realm's Function.prototype[@@hasInstance] is OrdinaryHasInstance with
// its receiver as C, so it runs here without a call.
//
// User code (a user @@hasInstance method, a getter of @@hasInstance or of
// `prototype`) runs as a frame above a HasInstance native frame (decision 4),
// which the operation's operands leave first; everything else completes in
// the instruction. A built-in method that converts its argument (a Convert
// frame) or runs as a native frame also runs above it, as does a getter that is no closure (call_accessor, light-cljs-881). Failing by name (decision 1): a
// built-in @@hasInstance method that needs a host effect or a map demand, an
// arguments object as the target, an async generator's `prototype` (the generators area),
// and a light-cljs runtime value (a form, cell or cursor, which the V8
// build's runtime makes instances of its own constructors) as V or on its
// prototype chain. Bound functions (light-cljs-764) and Proxy
// (light-cljs-766) do not exist in this VM yet; when they do, a bound C
// defers to its target and a Proxy's getPrototypeOf trap runs as a frame.
fn instanceof_op(job: Job&) {
    let heap .= job.heap;
    // A scalar form (a primitive with its provenance) is its primitive.
    let target = job.operands.pop(); let v = form_value(heap, job.operands.pop());
    if !valid(heap, target) || heap.fault != Fault.None { return; }
    if !target.is_object() { type_error(job, "invalid 'instanceof' right operand"); return; }
    if !convertible(heap, target) { unsupported_detail(job, "instanceof: a light-cljs runtime value as the right operand"); return; }
    let m, getter = lookup(heap, target, Value.Symbol { s: well_known::HasInstance });
    if heap.fault != Fault.None { return; }
    if getter.is_object() { has_instance_await(job, -1, 1, target, v, getter, target, []); return; }
    has_instance_method(job, -1, target, v, m);
}
// The JavaScript object kinds instanceof reads the prototype of: those
// ToPrimitive converts (convertible).
inline fn instance_kind(heap: const Heap&, v: Value) -> bool { return convertible(heap, v); }
// InstanceofOperator from step 3, with m = target[@@hasInstance]. `at` is
// the HasInstance frame's row, -1 while the instruction runs without one.
fn has_instance_method(job: Job&, at: i64, c: Value, v: Value, m: Value) {
    let heap .= job.heap;
    if m.tag() <= value_tag::Null {
        if !callable(heap, c) { type_error(job, "invalid 'instanceof' right operand"); return; }
        ordinary_has_instance(job, at, c, v); return;
    }
    if function_has_instance(heap, m) { ordinary_has_instance(job, at, c, v); return; }
    has_instance_call(job, at, c, v, m);
}
// Whether m is the realm's Function.prototype[@@hasInstance].
inline fn function_has_instance(heap: const Heap&, m: Value) -> bool { return m.is_object() && node(heap, m.id()).kind == node_kind::Builtin && builtin_of(heap, m.id()) == Builtin.FunctionPrototypeHasInstance; }
// Call(m, c, [v]) for InstanceofOperator, m not Function.prototype[@@hasInstance]:
// a user method's frame, a bound function's Forward frame (light-cljs-852)
// or a native-frame built-in's above the HasInstance frame, or a plain
// built-in run here.
fn has_instance_call(job: Job&, at: i64, c: Value, v: Value, m: Value) {
    let heap .= job.heap;
    if !callable(heap, m) { type_error(job, "Symbol.hasInstance is not a function"); return; }
    let kind = node(heap, m.id()).kind;
    if kind == node_kind::Closure { has_instance_await(job, at, 2, c, v, m, c, [v]); return; }
    if kind == node_kind::BoundFunction {
        if has_instance_frame(job, at, 2, c, v, Value.Undefined) < 0 { return; }
        start_bound(job, m, c, [v], job.operands.len);
        return;
    }
    let b = builtin_of(heap, m.id());
    let args: Value[1] = [v];
    if effect_builtin(b) || b == Builtin.Mapv || b == Builtin.MapSeq || forces(heap, b, args[..]) { unsupported_step(job, b, "as a Symbol.hasInstance method"); return; }
    // Number(o) and String(o) convert o in a Convert frame first (pc 4); a
    // native-frame built-in runs above the HasInstance frame (pc 2).
    let converts, hint = argument_converts(heap, m, args[..]);
    if converts || native_builtin(b) {
        let row = has_instance_frame(job, at, if converts { 4 } else { 2 }, c, v, m);
        if row < 0 { return; }
        if converts { job.operands.push(v); start_convert(job, hint, hint == Hint.String, 1, 1); }
        else { native_start(job, b, c, args[..], job.operands.len); }
        return;
    }
    let r = call_builtin(job, node(heap, m.id()).function, c, args[..]);
    if heap.fault != Fault.None { return; }
    has_instance_done(job, at, truthy(heap, form_value(heap, r)));
}
// OrdinaryHasInstance(C, O). An ordinary function whose `prototype` is
// still lazy (light-cljs-805) has a prototype no object can inherit yet,
// since making an object from it (construct, a read) makes it first: O is
// not an instance, and nothing is made.
fn ordinary_has_instance(job: Job&, at: i64, bound: Value, o: Value) {
    let heap .= job.heap;
    if !callable(heap, bound) { has_instance_done(job, at, false); return; }
    // Step 2 (light-cljs-852): for a bound function, InstanceofOperator(O,
    // its target): the target's @@hasInstance, which for the realm's
    // continues here with the target as C. Any other method is called from
    // the HasInstance frame's pc 1, as a getter's result would be.
    var c = bound;
    while is_bound(heap, c) {
        let target = node(heap, c.id()).value;
        let m, getter = lookup(heap, target, Value.Symbol { s: well_known::HasInstance });
        if heap.fault != Fault.None { return; }
        if getter.is_object() { has_instance_await(job, at, 1, target, o, getter, target, []); return; }
        if m.tag() > value_tag::Null && !function_has_instance(heap, m) {
            if has_instance_frame(job, at, 1, target, o, Value.Undefined) >= 0 { push_operand(job, m); }
            return;
        }
        if !charge(heap) { return; }
        c = target;
    }
    if !o.is_object() { has_instance_done(job, at, false); return; }
    if node(heap, c.id()).kind == node_kind::Closure {
        if unsupported_prototype(heap, c.id()) { unsupported_detail(job, "instanceof: an async generator's prototype"); return; }
        if lazy_prototype(heap, c.id()) { has_instance_done(job, at, false); return; }
    }
    let k = key_named(heap, "prototype");
    if heap.fault != Fault.None { return; }
    let p, getter = lookup(heap, c, k);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { has_instance_await(job, at, 3, c, o, getter, c, []); return; }
    has_instance_walk(job, at, o, p);
}
// OrdinaryHasInstance from step 5: P = C.prototype must be an object; then
// O's prototype chain, each kind's default prototype included, one work
// unit a link. A light-cljs runtime value as O (light-cljs-875) is an
// instance of its V8 build runtime's own constructors, none of which is an
// Error: it is no instance when P is the realm's %Error.prototype%, a
// native error type's or ExceptionInfo.prototype (error_prototype), and
// otherwise fails by name.
fn has_instance_walk(job: Job&, at: i64, o: Value, p: Value) {
    let heap .= job.heap;
    if !p.is_object() { type_error(job, "operand 'prototype' property is not an object"); return; }
    if !instance_kind(heap, o) {
        if error_prototype(heap, p.id()) { has_instance_done(job, at, false); return; }
        unsupported_detail(job, "instanceof: a light-cljs runtime value"); return;
    }
    var x = o;
    loop {
        // A proxy's [[GetPrototypeOf]] fails by name here (proxy.goose).
        if node(heap, x.id()).kind == node_kind::Proxy { proxy_fault(heap, proxy_method::GetPrototypeOf); return; }
        let q = prototype(heap, x.id());
        if !q.is_object() { has_instance_done(job, at, false); return; }
        if q.id() == p.id() { has_instance_done(job, at, true); return; }
        if !charge(heap) { return; }
        if !instance_kind(heap, q) { unsupported_detail(job, "instanceof: a light-cljs runtime value"); return; }
        x = q;
    }
}
// Run f (a user method or getter) with `receiver` and `args` above the
// HasInstance frame, pushed first when the instruction has none; its result
// arrives at `phase`. A getter that is no closure runs on the general call
// path (call_accessor, light-cljs-881).
fn has_instance_await(job: Job&, at: i64, phase: i64, c: Value, v: Value, f: Value, receiver: Value, args: const Value[:]) {
    let heap .= job.heap;
    let fresh = at < 0;
    if has_instance_frame(job, at, phase, c, v, Value.Undefined) < 0 { return; }
    if node(heap, f.id()).kind != node_kind::Closure {
        var copied: Value[>..] = []; copied.append(args);
        call_accessor(job, f, receiver, copied[..]);
        if heap.fault != Fault.None && fresh { job.frames.pop(); }
        return;
    }
    if !push_frame(job, f, receiver, args, false, job.operands.len) && fresh { job.frames.pop(); }
}
// The HasInstance frame, at row `at` or pushed when the instruction has
// none, awaiting `phase`, with `m` the built-in method a phase 4 calls; its
// row, or -1 at the frame limit.
fn has_instance_frame(job: Job&, at: i64, phase: i64, c: Value, v: Value, m: Value) -> i64 {
    if at >= 0 { job.frames[at].pc = phase; job.frames[at].callee = m; job.frames[at].receiver = c; return at; }
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return -1; }
    job.frames.push(Frame { function: -1, pc: phase, local_base: job.locals.len, operand_base: job.operands.len, callee: m, receiver: c, input: v, native: Native.HasInstance });
    return job.frames.len - 1;
}
// The operation's result: pushed by the instruction, or delivered by its
// finished HasInstance frame.
fn has_instance_done(job: Job&, at: i64, result: bool) {
    if at < 0 { job.operands.push(boolean(result)); return; }
    finish_native(job, job.frames[at], boolean(result));
}
// One step of a HasInstance frame (see Native): the awaited value arrived.
// A scalar form (a primitive with its provenance) is its primitive.
fn has_instance_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let r = form_value(heap, job.operands.pop());
    if heap.fault != Fault.None { return; }
    match frame.pc {
        1 => { has_instance_method(job, at, frame.receiver, frame.input, r); },
        2 => { has_instance_done(job, at, truthy(heap, r)); },
        3 => { has_instance_walk(job, at, frame.input, r); },
        // r is V converted for the built-in method frame.callee.
        4 => {
            let args: Value[1] = [r];
            let result = call_builtin(job, node(heap, frame.callee.id()).function, frame.receiver, args[..]);
            if heap.fault == Fault.None { has_instance_done(job, at, truthy(heap, form_value(heap, result))); }
        },
        _ => { heap.fault = Fault.InvalidState; },
    }
}
// Function.prototype[@@hasInstance](V) (light-cljs-834; QuickJS's
// js_function_hasInstance): OrdinaryHasInstance with the receiver as C,
// started from the dispatch loop's call (native_start) with the call's
// operands cut to `base`, so a `prototype` getter runs above its frame.
fn start_function_has_instance(job: Job&, receiver: Value, v: Value, base: i64) {
    job.operands.resize(base);
    ordinary_has_instance(job, -1, receiver, v);
}
// rt.core.isInstance(c, x) (light-cljs-875), the V8 build's runtime's
// (src/bbp_runtime.js): for the Delay leaf, whether x is a delay;
// otherwise `val(x) instanceof c`, as the instruction runs it
// (instanceof_op), except that the step after GetMethod(c, @@hasInstance)
// runs from a HasInstance frame's pc 1, so a built-in method's native frame
// starts from the dispatch loop, not from this one. The runtime's other
// cases, a multimethod type and a native struct type, are values this
// runtime has none of: as the right operand of instanceof such a light-cljs
// value fails by name.
fn start_is_instance(job: Job&, c: Value, x: Value, base: i64) {
    let heap .= job.heap;
    job.operands.resize(base);
    if is_builtin(heap, c, Builtin.Delay) {
        let args: Value[1] = [x];
        let r = call_builtin(job, code(Builtin.IsDelay), Value.Undefined, args[..]);
        if heap.fault == Fault.None { job.operands.push(r); }
        return;
    }
    let v = form_value(heap, x);
    if !valid(heap, c) || heap.fault != Fault.None { return; }
    if !c.is_object() { type_error(job, "invalid 'instanceof' right operand"); return; }
    if !convertible(heap, c) { unsupported_detail(job, "instanceof: a light-cljs runtime value as the right operand"); return; }
    let m, getter = lookup(heap, c, Value.Symbol { s: well_known::HasInstance });
    if heap.fault != Fault.None { return; }
    if getter.is_object() { has_instance_await(job, -1, 1, c, v, getter, c, []); return; }
    if has_instance_frame(job, -1, 1, c, v, Value.Undefined) >= 0 { push_operand(job, m); }
}

// ---- The iteration protocol (light-cljs-843; ECMAScript 7.4.3
// GetIterator and 7.4.2 GetIteratorFromMethod, the Pants engine's
// vm/native/iterator.rs State::get_iterator and State::with_method,
// QuickJS's JS_GetIterator).
//
// GetIterator: v → record. method = GetMethod(v, @@iterator), read through
// [[Get]] (a primitive's through its realm prototype); undefined, null or
// a value that is not callable is a TypeError; iterator = Call(method, v),
// which must be an object; next = Get(iterator, "next"), read once. The
// record is a new ordinary object, a job row, with own data properties
// `iterator` and `next`, which the Rust compiler's lowering reads into
// hidden frame locals (rust/src/frontend/lower.rs
// iterator_acquire_value_mode) and never lets JavaScript see. Stepping
// (`next`), IteratorClose (`return`) and RequireObject on their results are
// ordinary bytecode around this one operation.
//
// User code (a getter of @@iterator or `next`, a user @@iterator method)
// runs as a frame above a GetIterator native frame (decision 4), which the
// operation's operand leaves first; with a data property holding the realm's
// %IteratorPrototype%[@@iterator] or another built-in that runs to
// completion, everything completes in the instruction. A native-frame
// built-in method (a generator's next, Array.prototype.map) runs above the
// frame too. get_iterator and get_iterator_from_method are the entries
// native consumers reuse (Array.from, Promise.all, the Map and Set
// constructors, `yield*`): the record arrives on the operand stack, now or
// when the GetIterator frame they leave above their own finishes, so the
// consumer's next step pops it either way.
//
// A symbol is not iterable unless %Symbol.prototype% (light-cljs-885)
// gains an @@iterator, which a compiler job cannot write; its read goes
// through that prototype like any primitive's. Arrays, strings and arguments objects iterate
// with the realm's built-in iterators (light-cljs-844, iterators.goose).
//
// Failing by name (decision 1): a BigInt iterable (the BigInt area's), a
// light-cljs runtime value as the iterable or the iterator, a built-in
// getter, and a host-effect or map built-in as the method.
fn get_iterator_op(job: Job&) {
    let heap .= job.heap;
    let v = job.operands.pop();
    if !valid(heap, v) { return; }
    get_iterator(job, v);
}
// GetIterator(v, sync) for the frame at the top: the record goes on the
// operands, or a GetIterator frame above it delivers the record there.
fn get_iterator(job: Job&, iterable: Value) {
    let heap .= job.heap;
    // A scalar form (a primitive with its provenance) is its primitive.
    let v = form_value(heap, iterable);
    if heap.fault != Fault.None { return; }
    // GetMethod(v, @@iterator) of undefined or null is a property read's
    // TypeError, as QuickJS's JS_GetIterator words it.
    if v.tag() <= value_tag::Null { base_error(job, v, Value.Symbol { s: well_known::Iterator }, true, false); return; }
    if !iteration_object(job, v, "the iterable") { return; }
    let m, getter = read(heap, v, Value.Symbol { s: well_known::Iterator });
    if heap.fault == Fault.UnsupportedOperand { primitive_gap(job, v); }
    if heap.fault != Fault.None { return; }
    if getter.is_object() { get_iterator_await(job, -1, 1, v, Value.Undefined, getter, v); return; }
    get_iterator_method(job, -1, v, m);
}
// GetIteratorFromMethod(v, method) with an already-read method, for native
// consumers (the Pants engine's with_method); the record arrives as
// get_iterator's does.
fn get_iterator_from_method(job: Job&, v: Value, method: Value) { get_iterator_method(job, -1, v, method); }
// Whether object v (an iterable or an iterator) is one this VM iterates; a
// primitive is. Otherwise a named fault.
fn iteration_object(job: Job&, v: Value, what: const u8[:]) -> bool {
    let heap .= job.heap;
    if !v.is_object() { return true; }
    // The compiler roots object and protected rows answer only their own
    // names (get_property). A protected array is iterated: its @@iterator is
    // the realm's and its cursor only reads it (light-cljs-844).
    if job.root_receiver.is_object() && v == job.root_receiver || !realm_intrinsic(v.id()) && v.id() < job.protected_nodes && node(heap, v.id()).kind != node_kind::Array { heap.fault = Fault.NativeBoundary; return false; }
    if !convertible(heap, v) {
        job.detail.clear(); for c in "GetIterator: a light-cljs runtime value as " { job.detail.push(c); } for c in what { job.detail.push(c); }
        heap.fault = Fault.UnsupportedOperand; return false;
    }
    return true;
}
// GetIteratorFromMethod from its step 1, m = v[@@iterator]. `at` is the
// GetIterator frame's row, -1 while the operation runs without one.
fn get_iterator_method(job: Job&, at: i64, v: Value, method: Value) {
    let heap .= job.heap;
    var f = form_value(heap, method);
    if heap.fault != Fault.None { return; }
    if !callable(heap, f) { type_error(job, "value is not iterable"); return; }
    // Call(method, v). Function.prototype.call as the method forwards its
    // receiver v as the callee with an undefined `this`, as the dispatch
    // loop's call does; a second forward then finds no callee.
    var receiver = v;
    loop {
        if node(heap, f.id()).kind == node_kind::Closure { get_iterator_await(job, at, 2, v, Value.Undefined, f, receiver); return; }
        // A bound function runs in a Forward frame above (light-cljs-852).
        if is_bound(heap, f) {
            let row = get_iterator_frame(job, at, 2, v, Value.Undefined);
            if row >= 0 { start_bound(job, f, receiver, [], job.operands.len); }
            return;
        }
        if builtin_of(heap, f.id()) != Builtin.FunctionPrototypeCall { break; }
        f = receiver; receiver = Value.Undefined;
        if !callable(heap, f) { type_error(job, "not a function"); return; }
    }
    let b = builtin_of(heap, f.id());
    // %IteratorPrototype%[@@iterator] returns its receiver.
    if b == Builtin.IteratorPrototypeIterator { get_iterator_next(job, at, receiver); return; }
    if effect_builtin(b) || b == Builtin.Mapv || b == Builtin.MapSeq { unsupported_step(job, b, "as a Symbol.iterator method"); return; }
    // A string's own cursor (light-cljs-844) needs no frame.
    if b == Builtin.StringPrototypeIterator && receiver.is_string() { let it = string_cursor(job, b, receiver); if heap.fault == Fault.None { get_iterator_next(job, at, it); } return; }
    if native_builtin(b) {
        let row = get_iterator_frame(job, at, 2, v, Value.Undefined);
        if row >= 0 { native_start(job, b, receiver, [], job.operands.len); }
        return;
    }
    let it = call_builtin(job, node(heap, f.id()).function, receiver, []);
    if heap.fault != Fault.None { return; }
    get_iterator_next(job, at, it);
}
// GetIteratorFromMethod from step 3: the iterator must be an object; its
// `next`, read once.
fn get_iterator_next(job: Job&, at: i64, iterator: Value) {
    let heap .= job.heap;
    let it = form_value(heap, iterator);
    if heap.fault != Fault.None { return; }
    if !it.is_object() { type_error(job, "iterator must be an object"); return; }
    if !iteration_object(job, it, "the iterator") { return; }
    let k = key_named(heap, "next");
    if heap.fault != Fault.None { return; }
    let next, getter = lookup(heap, it, k);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { get_iterator_await(job, at, 3, Value.Undefined, it, getter, it); return; }
    get_iterator_done(job, at, it, next);
}
// The iterator record {iterator, next}: pushed by the operation, or
// delivered by its finished GetIterator frame.
fn get_iterator_done(job: Job&, at: i64, it: Value, next: Value) {
    let heap .= job.heap;
    let record = allocate(heap, Node { kind: node_kind::Object });
    if heap.fault != Fault.None || !realm_value(heap, record.id(), "iterator", it, attrs_data()) || !realm_value(heap, record.id(), "next", next, attrs_data()) { return; }
    if at >= 0 { finish_native(job, job.frames[at], record); return; }
    if job.operands.len >= heap.limits.stack - job.locals.len { heap.fault = Fault.StackLimit; return; }
    job.operands.push(record);
}
// Run f (a user method or getter) with `receiver` above the GetIterator
// frame, pushed first when the operation has none; its result arrives at
// `phase`. A getter that is no closure runs on the general call path
// (call_accessor, light-cljs-881).
fn get_iterator_await(job: Job&, at: i64, phase: i64, v: Value, it: Value, f: Value, receiver: Value) {
    let heap .= job.heap;
    let fresh = at < 0;
    if get_iterator_frame(job, at, phase, v, it) < 0 { return; }
    if node(heap, f.id()).kind != node_kind::Closure {
        call_accessor(job, f, receiver, []);
        if heap.fault != Fault.None && fresh { job.frames.pop(); }
        return;
    }
    if !push_frame(job, f, receiver, [], false, job.operands.len) && fresh { job.frames.pop(); }
}
// The GetIterator frame, at row `at` or pushed when the operation has none,
// awaiting `phase` with iterable v and iterator `it` (Undefined until
// called); its row, or -1 at the frame limit.
fn get_iterator_frame(job: Job&, at: i64, phase: i64, v: Value, it: Value) -> i64 {
    if at >= 0 { job.frames[at].pc = phase; job.frames[at].input = it; return at; }
    if job.frames.len >= job.heap.limits.frames { job.heap.fault = Fault.StackLimit; return -1; }
    job.frames.push(Frame { function: -1, pc: phase, local_base: job.locals.len, operand_base: job.operands.len, callee: Value.Undefined, receiver: v, input: it, native: Native.GetIterator });
    return job.frames.len - 1;
}
// One step of a GetIterator frame (see Native): the awaited value arrived.
fn get_iterator_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let r = job.operands.pop();
    match frame.pc {
        1 => { get_iterator_method(job, at, frame.receiver, r); },
        2 => { get_iterator_next(job, at, r); },
        3 => { get_iterator_done(job, at, frame.input, r); },
        _ => { heap.fault = Fault.InvalidState; },
    }
}

// ---- Key enumeration (light-cljs-804; the Pants engine's vm/keys.rs and
// vm/native/copy_properties.rs, QuickJS's build_for_in_iterator and
// js_for_in_next).
//
// A key list is node_kind::KeyList a job allocates and only its own frames
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
    if !realm_intrinsic(object.id()) && object.id() < job.protected_nodes || job.root_receiver.is_object() && object == job.root_receiver { job.heap.fault = Fault.NativeBoundary; return false; }
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
    var count: i64 = 0; for e in keys { if symbols || e.key.is_string() { count += 1; } }
    let used: i64 = node(heap, o).key_count;
    if used + count > node(heap, o).key_capacity {
        let start = node(heap, o).keys; let capacity = max(4, 2 * (used + count));
        let at = add_properties(heap, capacity);
        if at < 0 { return false; }
        for i in used { set_property_row(heap, at + i, property_row(heap, start + i)); set_property_row(heap, start + i, Property {}); }
        node_mut(heap, o).keys = at as u32; node_mut(heap, o).key_capacity = capacity as u32;
    }
    var i = used;
    for e in keys { if symbols || e.key.is_string() { set_property_row(heap, node(heap, o).keys + i, Property { key: e.key, attrs: e.attrs }); i += 1; } }
    node_mut(heap, o).key_count = i as u32;
    return true;
}
fn key_list(heap: Heap&, object: Value, keys: const OwnKey[:], symbols: bool) -> Value {
    let list = allocate(heap, Node { kind: node_kind::KeyList, value: object });
    if !list.is_object() || !list_append(heap, list.id(), keys, symbols) { return Value.Undefined; }
    return list;
}
// ForInStart: v → it, a key list of v's own string keys, enumerable or
// not, in own_keys order (a non-enumerable key is never visited but hides the
// name further up the chain); a string's indices; none for any other
// primitive. A proxy's keys are listed by the first ForInNext
// (key_list::Pending), which may run its traps.
fn for_in_start(job: Job&) {
    let heap .= job.heap;
    let v = job.operands.pop();
    if !valid(heap, v) { return; }
    var keys: OwnKey[>..] = []; var walked = Value.Undefined;
    let proxy = v.is_object() && node(heap, v.id()).kind == node_kind::Proxy;
    if v.is_object() { if !listable(job, v) || !proxy && !own_property_keys(heap, v, keys) { return; } walked = v; }
    else if v.is_string() && !string_keys(heap, v, false, keys) { return; }
    let list = key_list(heap, walked, keys, false);
    if heap.fault != Fault.None { return; }
    if proxy { node_mut(heap, list.id()).function = key_list::Pending; }
    job.operands.push(list);
}
// Whether key k is among the first `start` records of key list o: the names
// of the objects a for-in walk has left, except a proxy's keys its
// descriptor reported missing (value Null). One work unit a record.
fn hidden(heap: Heap&, o: i64, k: Value) -> bool {
    let start: i64 = node(heap, o).start;
    if !charge(heap, start) { return false; }
    let w = key_word(k); let keys: i64 = node(heap, o).keys;
    for i in start { let r .= property_row(heap, keys + i); if key_word(r.key) == w && !r.value.is_null() { return true; } }
    return false;
}
// ForInNext: it → key more. The next record that was enumerable when
// listed, is not hidden by an object the walk has left, and is still an own
// enumerable property of the object it was listed from (a key deleted or
// made non-enumerable before it is reached is skipped; keys added after the
// listing are not visited), and true; or undefined and false when the walk
// ends. When an object's keys are used up the walk lists its prototype's,
// read then, so a prototype changed before the walk reaches it counts. The
// walk ends at Null and at the realm's pending %Object.prototype% (Empty),
// which has no enumerable property (light-cljs-788 builds it). One work unit
// a record visited.
//
// A proxy on the walk (light-cljs-845; the Pants engine's for_in_request,
// ECMAScript 14.7.5.9): its keys come from [[OwnPropertyKeys]] once, each
// key's [[GetOwnProperty]] is asked just before the key would be visited
// (after the names the walk has left hide it), and the walk's next object
// is its [[GetPrototypeOf]]. Each runs in a Proxy frame above a ForIn frame
// (proxy.goose, for_in_step), pushed only then: an ordinary walk keeps this
// plain path.
fn for_in_next(job: Job&) {
    let heap .= job.heap;
    let it = job.operands.pop();
    if !valid(heap, it) { return; }
    if !it.is_object() || node(heap, it.id()).kind != node_kind::KeyList { heap.fault = Fault.InvalidState; return; }
    if !writable(job, it.id()) { heap.fault = Fault.NativeBoundary; return; }
    for_in_walk(job, -1, it.id());
}
// The walk of key list o from its position, for the ForIn frame at `at`
// (-1: ForInNext itself, with no frame yet).
fn for_in_walk(job: Job&, at: i64, o: i64) {
    let heap .= job.heap;
    loop {
        if !charge(heap) { return; }
        let walked = node(heap, o).value;
        if node(heap, o).function == key_list::Pending { for_in_proxy(job, at, o, 1, proxy_method::OwnPropertyKeys, walked, Value.Undefined); return; }
        let next: i64 = node(heap, o).length;
        if next < node(heap, o).key_count {
            let r = property_row(heap, node(heap, o).keys + next);
            node_mut(heap, o).length = (next + 1) as u32;
            if r.attrs & attr_enumerable() == 0 || hidden(heap, o, r.key) { if heap.fault != Fault.None { return; } continue; }
            if walked.is_object() {
                if node(heap, walked.id()).kind == node_kind::Proxy { for_in_proxy(job, at, o, 2, proxy_method::GetOwnProperty, walked, r.key); return; }
                let e = enumerable_entry(heap, walked, r.key);
                if heap.fault != Fault.None { return; }
                if !e.found || e.attrs & attr_enumerable() == 0 { continue; }
            }
            for_in_yield(job, at, r.key, true); return;
        }
        if walked.is_object() && node(heap, walked.id()).kind == node_kind::Proxy { for_in_proxy(job, at, o, 3, proxy_method::GetPrototypeOf, walked, Value.Undefined); return; }
        let proto = if walked.is_object() { prototype(heap, walked.id()) } else { Value.Undefined };
        if !for_in_enter(job, at, o, proto) { return; }
    }
}
// The walk moves to `proto`: its keys are listed (a proxy's by the next
// step), or, at Null, the walk ends and its result is delivered. False
// when the walk ended or failed.
fn for_in_enter(job: Job&, at: i64, o: i64, proto: Value) -> bool {
    let heap .= job.heap;
    if !proto.is_object() {
        node_mut(heap, o).value = Value.Undefined;
        for_in_yield(job, at, Value.Undefined, false); return false;
    }
    if !listable(job, proto) { return false; }
    let used: i64 = node(heap, o).key_count;
    if node(heap, proto.id()).kind == node_kind::Proxy { node_mut(heap, o).function = key_list::Pending; }
    else {
        var keys: OwnKey[>..] = [];
        if !own_property_keys(heap, proto, keys) || !list_append(heap, o, keys, false) { return false; }
    }
    node_mut(heap, o).value = proto; node_mut(heap, o).start = used as u32; node_mut(heap, o).length = used as u32;
    return true;
}
// ForInNext's results: pushed, or delivered by the ForIn frame at `at`.
fn for_in_yield(job: Job&, at: i64, k: Value, more: bool) {
    if at >= 0 { let base = job.frames[at].operand_base; job.frames.pop(); job.operands.resize(base); }
    job.operands.push(k); job.operands.push(boolean(more));
}
// OwnKeys: source → keys, a new array of the source's own keys,
// symbols included, before object rest copies them: an index as a number
// (as ToPropertyKey gives it), a name as a string. A string's are its indices
// and `length`; undefined and null are a TypeError; other primitives have
// none. A symbol key is listed as itself (light-cljs-885).
fn own_keys_op(job: Job&) {
    let heap .= job.heap;
    let source = job.operands.pop();
    if !valid(heap, source) { return; }
    if source.tag() <= value_tag::Null { heap.fault = Fault.TypeError; return; }
    var keys: OwnKey[>..] = [];
    if source.is_object() { if !listable(job, source) || !own_property_keys(heap, source, keys) { return; } }
    else if source.is_string() && !string_keys(heap, source, true, keys) { return; }
    let array = allocate(heap, Node { kind: node_kind::Array });
    if !array.is_object() { return; }
    for e, i in keys {
        if !e.key.is_string() && !e.key.is_symbol() { heap.fault = Fault.UnsupportedOperand; return; }
        if !set_element(heap, array, i, if e.index >= 0 { number(e.index) } else { e.key }) { return; }
    }
    job.operands.push(array);
}
// IsEnumerable: source key → whether the source has an own enumerable
// property `key` (already converted by ToPropertyKey): its current
// attributes, not a snapshot's. A string's indices are enumerable; other
// primitives have no own properties.
fn is_enumerable(job: Job&) {
    let heap .= job.heap;
    let kv = job.operands.pop(); let source = job.operands.pop();
    if !valid(heap, source) || !key_valid(heap, kv) { return; }
    var result = false;
    if source.is_object() || source.is_string() {
        if source.is_object() && !listable(job, source) { return; }
        let k = key(heap, kv);
        if heap.fault != Fault.None { return; }
        if source.is_string() { let i = index(heap, k); result = i >= 0 && i < str_len(heap, source); }
        else {
            let ik = intern(heap, k);
            if heap.fault != Fault.None { return; }
            let e = enumerable_entry(heap, source, ik);
            result = e.found && e.attrs & attr_enumerable() != 0;
        }
    }
    if heap.fault == Fault.None { job.operands.push(boolean(result)); }
}
// CopyDataProperties: target source → target, an object spread. An
// undefined or null source, or one with no own keys, copies nothing; a
// CopyData native frame copies the rest, one key a step: each key still an
// own enumerable property of the source when reached is read ([[Get]], a
// getter running as a frame above) and defined on the target
// (CreateDataProperty).
fn copy_data(job: Job&) {
    let heap .= job.heap;
    let source = job.operands.pop(); let target = job.operands[job.operands.len - 1];
    if !valid(heap, source) || !valid(heap, target) || source.tag() <= value_tag::Null { return; }
    if !target.is_object() || node(heap, target.id()).kind != node_kind::Object { heap.fault = Fault.UnsupportedOperand; return; }
    if !writable(job, target.id()) { heap.fault = Fault.NativeBoundary; return; }
    var keys: OwnKey[>..] = []; var walked = Value.Undefined;
    if source.is_object() { if !listable(job, source) || !own_property_keys(heap, source, keys) { return; } walked = source; }
    else if source.is_string() && !string_keys(heap, source, false, keys) { return; }
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
    if frame.input.is_string() { let i = index(heap, k); if heap.fault == Fault.None { copy_define(job, frame.output, k, unit_string(heap, frame.input, i)); } return; }
    let e = enumerable_entry(heap, frame.input, k);
    if heap.fault != Fault.None || !e.found || e.attrs & attr_enumerable() == 0 { return; }
    if !is_accessor(e.value) { copy_define(job, frame.output, k, e.value); return; }
    let getter = pair(heap, e.value).get;
    if !getter.is_object() { copy_define(job, frame.output, k, Value.Undefined); return; }
    job.frames[at].pc = 1;
    // A getter that is no closure runs on the general call path (light-cljs-881).
    if node(heap, getter.id()).kind == node_kind::Closure { enter(job, getter, frame.input, []); } else { call_accessor(job, getter, frame.input, []); }
}
// ToPropertyKey: o k → o k', the key converted once for a read and a
// later write of o[k] or an object pattern's key: an array index as a
// number, any other name as a string. Undefined or null o is a TypeError; an
// object key converts before this (the Convert frame, light-cljs-828), and a
// light-cljs runtime value fails UnsupportedOperand, as key() does.
fn to_property_key(job: Job&) {
    let heap .= job.heap;
    let kv = job.operands.pop(); let base = job.operands[job.operands.len - 1];
    if !valid(heap, base) || !key_valid(heap, kv) { return; }
    if base.tag() <= value_tag::Null { base_error(job, base, kv, false, false); return; }
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
        // The safe point of reclamation inside a job (reclaim.goose): every
        // value the job holds is in its state or its rows here. Bytecode runs
        // in slices of at most reclaim_interval() instructions to reach it.
        if job.heap.reclaim { reclaim(job); }
        // No frame left once the entry finished: the next promise job runs
        // (continuations.goose).
        if job.frames.len == 0 && job.draining { drain_step(job); used += 1; }
        else if job.frames.len == 0 || job.frames[job.frames.len - 1].function < 0 { native_step(job); used += 1; }
        else { used += execute(job, if budget < 0 { reclaim_interval() } else { min(budget - used, reclaim_interval()) }); }
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
    match v {
        Undefined => { out.append("undefined"); }, Null => { out.append("null"); }, Bool => { out.append(if v.flag() { "boolean:true" } else { "boolean:false" }); },
        Number => { if v.num() != v.num() { out.append("number:NaN"); } else if v.num() == 0 && 1.0 / v.num() < 0 { out.append("number:-0"); } else { format(out, "number:", v.num()); } },
        String => { out.append("utf16:["); for i in str_len(heap, v) { if i > 0 { out.push(','); } format(out, unit(heap, v, i)); } out.push(']'); },
        Object => { format(out, "identity:", heap.identity, ":", v.id(), ":", node(heap, v.id()).kind); },
        Symbol => { out.append("symbol"); },
        _ => { out.append("invalid:tag"); },
    }
}
