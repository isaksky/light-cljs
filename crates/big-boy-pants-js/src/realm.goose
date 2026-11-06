import std;
import .source_vm;

namespace source;

// The realm (light-cljs-788; decisions 5 and 6 of docs/javascript-coverage.md).
//
// initialize() builds it once, as the first rows of a heap's tables, in the
// program that owns the session image; job workers inherit it in their
// spawn copy of that image (snapshot::restore) and never build it again.
// Its layout comes from the static tables below, so its node rows are fixed:
// rows 0 to fixed_rows() - 1 are the intrinsics every kind's default
// prototype names (realm_row), then one built-in function object per entry
// of REALM_FUNCTIONS, realm_nodes() rows in all. Every intrinsic is an
// ordinary row of the VM tables (plain u32 IDs, decision 2): its state is
// its rows, which the worker reset restores like any image row, and
// JavaScript in a compiler job cannot write it (Job.owned_nodes,
// NativeBoundary). The static tables themselves are `let` data, never rows.
//
// A built-in function is a node of kind 4 whose `function` field is its
// code, the position of its Builtin in BUILTINS; calls dispatch on the
// Builtin by `match` (call_builtin). The light-cljs runtime leaves
// (rt.core.*, boot_runtime) are the same kind of object, installed from
// RUNTIME_LEAVES.
//
// A property the specification defines and this VM does not implement yet
// is a *pending* property: an accessor whose slot names accessor row 0, the
// realm's pending pair (pending()). `in` and hasOwnProperty see it; reading,
// writing or calling through it fails UnsupportedOperand, never a value
// JavaScript could take for the real one (decision 1).

enum Builtin {
    // The runtime leaves, at their historical codes 0 to 20 (9 is retired).
    MacroReaderFunction, Inc, Mapv, Vec, Into, Gensym, SharedIncrement, SharedRead, External, Retired,
    VectorFromArray, ListFromArray, MapFromArray, SetFromArray, FunctionPrototypeCall, FnRest, Sequence, MapSeq, List, Symbol, IntrinsicRegExp,
    // The realm's built-in functions.
    FunctionPrototype, Object, Function, Array,
    Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError,
    ObjectGetPrototypeOf, ObjectIs, ObjectHasOwn, ObjectPrototypeHasOwnProperty, ObjectPrototypeValueOf, ObjectPrototypeToString,
    ArrayIsArray, ErrorPrototypeToString, SymbolFunction, ObjectGetOwnPropertyDescriptor,
    // A runtime leaf added after the realm's functions (light-cljs-807).
    NativeCoreBinding,
    // Array.prototype built-ins that run user code (light-cljs-789; array_callbacks.goose).
    ArrayPrototypeMap, ArrayPrototypeForEach,
}
// A built-in's code (its position here, a node's `function`), its `name`
// and its `length`.
struct BuiltinInfo { which: Builtin, name: u8[..24], length: u8 = 0 }
let BUILTINS: BuiltinInfo[46] = [
    BuiltinInfo { which: Builtin.MacroReaderFunction, name: "macroReaderFunction", length: 1 }, BuiltinInfo { which: Builtin.Inc, name: "inc", length: 1 },
    BuiltinInfo { which: Builtin.Mapv, name: "mapv", length: 2 }, BuiltinInfo { which: Builtin.Vec, name: "vec", length: 1 }, BuiltinInfo { which: Builtin.Into, name: "into", length: 2 },
    BuiltinInfo { which: Builtin.Gensym, name: "gensym" }, BuiltinInfo { which: Builtin.SharedIncrement, name: "sharedIncrement" }, BuiltinInfo { which: Builtin.SharedRead, name: "sharedRead" },
    BuiltinInfo { which: Builtin.External, name: "external" }, BuiltinInfo { which: Builtin.Retired, name: "" },
    BuiltinInfo { which: Builtin.VectorFromArray, name: "vectorFromArray", length: 1 }, BuiltinInfo { which: Builtin.ListFromArray, name: "listFromArray", length: 1 },
    BuiltinInfo { which: Builtin.MapFromArray, name: "mapFromArray", length: 1 }, BuiltinInfo { which: Builtin.SetFromArray, name: "setFromArray", length: 1 },
    BuiltinInfo { which: Builtin.FunctionPrototypeCall, name: "call", length: 1 }, BuiltinInfo { which: Builtin.FnRest, name: "fnRest", length: 2 },
    BuiltinInfo { which: Builtin.Sequence, name: "sequence", length: 1 }, BuiltinInfo { which: Builtin.MapSeq, name: "mapSeq", length: 2 },
    BuiltinInfo { which: Builtin.List, name: "list" }, BuiltinInfo { which: Builtin.Symbol, name: "symbol", length: 2 },
    BuiltinInfo { which: Builtin.IntrinsicRegExp, name: "intrinsicRegExp" },
    BuiltinInfo { which: Builtin.FunctionPrototype, name: "" }, BuiltinInfo { which: Builtin.Object, name: "Object", length: 1 },
    BuiltinInfo { which: Builtin.Function, name: "Function", length: 1 }, BuiltinInfo { which: Builtin.Array, name: "Array", length: 1 },
    BuiltinInfo { which: Builtin.Error, name: "Error", length: 1 }, BuiltinInfo { which: Builtin.EvalError, name: "EvalError", length: 1 },
    BuiltinInfo { which: Builtin.RangeError, name: "RangeError", length: 1 }, BuiltinInfo { which: Builtin.ReferenceError, name: "ReferenceError", length: 1 },
    BuiltinInfo { which: Builtin.SyntaxError, name: "SyntaxError", length: 1 }, BuiltinInfo { which: Builtin.TypeError, name: "TypeError", length: 1 },
    BuiltinInfo { which: Builtin.URIError, name: "URIError", length: 1 }, BuiltinInfo { which: Builtin.InternalError, name: "InternalError", length: 1 },
    BuiltinInfo { which: Builtin.ObjectGetPrototypeOf, name: "getPrototypeOf", length: 1 }, BuiltinInfo { which: Builtin.ObjectIs, name: "is", length: 2 },
    BuiltinInfo { which: Builtin.ObjectHasOwn, name: "hasOwn", length: 2 }, BuiltinInfo { which: Builtin.ObjectPrototypeHasOwnProperty, name: "hasOwnProperty", length: 1 },
    BuiltinInfo { which: Builtin.ObjectPrototypeValueOf, name: "valueOf" }, BuiltinInfo { which: Builtin.ObjectPrototypeToString, name: "toString" },
    BuiltinInfo { which: Builtin.ArrayIsArray, name: "isArray", length: 1 }, BuiltinInfo { which: Builtin.ErrorPrototypeToString, name: "toString" },
    BuiltinInfo { which: Builtin.SymbolFunction, name: "Symbol" }, BuiltinInfo { which: Builtin.ObjectGetOwnPropertyDescriptor, name: "getOwnPropertyDescriptor", length: 2 },
    BuiltinInfo { which: Builtin.NativeCoreBinding, name: "nativeCoreBinding", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeMap, name: "map", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeForEach, name: "forEach", length: 1 },
];
// The fixed rows (realm_row). The error types are indexed as ERROR_NAMES:
// Error, its six native subclasses, and QuickJS's InternalError (the stack
// overflow error).
inline fn global_row() -> i64 { return 0; }
inline fn object_prototype_row() -> i64 { return 1; }
inline fn function_prototype_row() -> i64 { return 2; }
inline fn array_prototype_row() -> i64 { return 3; }
inline fn error_prototype_row(t: i64) -> i64 { return 4 + t; }
inline fn object_constructor_row() -> i64 { return 12; }
inline fn function_constructor_row() -> i64 { return 13; }
inline fn array_constructor_row() -> i64 { return 14; }
inline fn error_constructor_row(t: i64) -> i64 { return 15 + t; }
inline fn fixed_rows() -> i64 { return 23; }
struct Name { text: u8[..24] }
let ERROR_NAMES:    Name[8]    = [Name { text: "Error" }, Name { text: "EvalError" }, Name { text: "RangeError" }, Name { text: "ReferenceError" }, Name { text: "SyntaxError" }, Name { text: "TypeError" }, Name { text: "URIError" }, Name { text: "InternalError" }];
let ERROR_BUILTINS: Builtin[8] = [Builtin.Error, Builtin.EvalError, Builtin.RangeError, Builtin.ReferenceError, Builtin.SyntaxError, Builtin.TypeError, Builtin.URIError, Builtin.InternalError];
// A built-in method: a property `name` (its BuiltinInfo name) of the fixed row `holder`.
struct Method { which: Builtin, holder: i64 }
let REALM_FUNCTIONS: Method[13] = [
    Method { which: Builtin.FunctionPrototypeCall, holder: 2 },
    Method { which: Builtin.ObjectGetPrototypeOf, holder: 12 }, Method { which: Builtin.ObjectIs, holder: 12 }, Method { which: Builtin.ObjectHasOwn, holder: 12 },
    Method { which: Builtin.ObjectPrototypeHasOwnProperty, holder: 1 }, Method { which: Builtin.ObjectPrototypeValueOf, holder: 1 }, Method { which: Builtin.ObjectPrototypeToString, holder: 1 },
    Method { which: Builtin.ArrayIsArray, holder: 14 }, Method { which: Builtin.ErrorPrototypeToString, holder: 4 },
    Method { which: Builtin.ObjectGetOwnPropertyDescriptor, holder: 12 },
    // The Symbol function, row symbol_row(), holds the well-known symbols.
    Method { which: Builtin.SymbolFunction, holder: 0 },
    Method { which: Builtin.ArrayPrototypeMap, holder: 3 }, Method { which: Builtin.ArrayPrototypeForEach, holder: 3 },
];
inline fn realm_nodes() -> i64 { return fixed_rows() + REALM_FUNCTIONS.len; }
// The Symbol function's row: its REALM_FUNCTIONS entry, the eleventh.
inline fn symbol_row() -> i64 { return fixed_rows() + 10; }
// A pending property of the fixed row `holder`: a name, or with `symbol`
// >= 0 a well-known symbol (export_well_known_symbol).
struct PendingName { holder: i64, key: u8[..32], symbol: i64 = -1 }
let PENDING: PendingName[123] = [
    // Object.prototype
    PendingName { holder: 1, key: "__proto__" }, PendingName { holder: 1, key: "isPrototypeOf" }, PendingName { holder: 1, key: "propertyIsEnumerable" }, PendingName { holder: 1, key: "toLocaleString" },
    PendingName { holder: 1, key: "__defineGetter__" }, PendingName { holder: 1, key: "__defineSetter__" }, PendingName { holder: 1, key: "__lookupGetter__" }, PendingName { holder: 1, key: "__lookupSetter__" },
    // Function.prototype
    PendingName { holder: 2, key: "apply" }, PendingName { holder: 2, key: "bind" }, PendingName { holder: 2, key: "toString" }, PendingName { holder: 2, key: "arguments" }, PendingName { holder: 2, key: "caller" },
    PendingName { holder: 2, key: "", symbol: 3 },
    // Array.prototype
    PendingName { holder: 3, key: "at" }, PendingName { holder: 3, key: "concat" }, PendingName { holder: 3, key: "copyWithin" }, PendingName { holder: 3, key: "entries" }, PendingName { holder: 3, key: "every" },
    PendingName { holder: 3, key: "fill" }, PendingName { holder: 3, key: "filter" }, PendingName { holder: 3, key: "find" }, PendingName { holder: 3, key: "findIndex" }, PendingName { holder: 3, key: "findLast" },
    PendingName { holder: 3, key: "findLastIndex" }, PendingName { holder: 3, key: "flat" }, PendingName { holder: 3, key: "flatMap" }, PendingName { holder: 3, key: "includes" },
    PendingName { holder: 3, key: "indexOf" }, PendingName { holder: 3, key: "join" }, PendingName { holder: 3, key: "keys" }, PendingName { holder: 3, key: "lastIndexOf" },
    PendingName { holder: 3, key: "pop" }, PendingName { holder: 3, key: "push" }, PendingName { holder: 3, key: "reduce" }, PendingName { holder: 3, key: "reduceRight" }, PendingName { holder: 3, key: "reverse" },
    PendingName { holder: 3, key: "shift" }, PendingName { holder: 3, key: "slice" }, PendingName { holder: 3, key: "some" }, PendingName { holder: 3, key: "sort" }, PendingName { holder: 3, key: "splice" },
    PendingName { holder: 3, key: "toLocaleString" }, PendingName { holder: 3, key: "toReversed" }, PendingName { holder: 3, key: "toSorted" }, PendingName { holder: 3, key: "toSpliced" }, PendingName { holder: 3, key: "toString" },
    PendingName { holder: 3, key: "unshift" }, PendingName { holder: 3, key: "values" }, PendingName { holder: 3, key: "with" }, PendingName { holder: 3, key: "", symbol: 0 }, PendingName { holder: 3, key: "", symbol: 7 },
    // Object
    PendingName { holder: 12, key: "assign" }, PendingName { holder: 12, key: "create" }, PendingName { holder: 12, key: "defineProperties" }, PendingName { holder: 12, key: "defineProperty" },
    PendingName { holder: 12, key: "entries" }, PendingName { holder: 12, key: "freeze" }, PendingName { holder: 12, key: "fromEntries" },
    PendingName { holder: 12, key: "getOwnPropertyDescriptors" }, PendingName { holder: 12, key: "getOwnPropertyNames" }, PendingName { holder: 12, key: "getOwnPropertySymbols" }, PendingName { holder: 12, key: "groupBy" },
    PendingName { holder: 12, key: "isExtensible" }, PendingName { holder: 12, key: "isFrozen" }, PendingName { holder: 12, key: "isSealed" }, PendingName { holder: 12, key: "keys" },
    PendingName { holder: 12, key: "preventExtensions" }, PendingName { holder: 12, key: "seal" }, PendingName { holder: 12, key: "setPrototypeOf" }, PendingName { holder: 12, key: "values" },
    // Symbol (symbol_row(); its well-known symbols are real)
    PendingName { holder: 33, key: "prototype" }, PendingName { holder: 33, key: "for" }, PendingName { holder: 33, key: "keyFor" },
    // Array
    PendingName { holder: 14, key: "from" }, PendingName { holder: 14, key: "of" }, PendingName { holder: 14, key: "fromAsync" }, PendingName { holder: 14, key: "", symbol: 8 },
    // The global object's other standard names.
    PendingName { holder: 0, key: "AggregateError" }, PendingName { holder: 0, key: "ArrayBuffer" }, PendingName { holder: 0, key: "Atomics" }, PendingName { holder: 0, key: "BigInt" },
    PendingName { holder: 0, key: "BigInt64Array" }, PendingName { holder: 0, key: "BigUint64Array" }, PendingName { holder: 0, key: "Boolean" }, PendingName { holder: 0, key: "DataView" },
    PendingName { holder: 0, key: "Date" }, PendingName { holder: 0, key: "FinalizationRegistry" }, PendingName { holder: 0, key: "Float32Array" }, PendingName { holder: 0, key: "Float64Array" },
    PendingName { holder: 0, key: "Int8Array" }, PendingName { holder: 0, key: "Int16Array" }, PendingName { holder: 0, key: "Int32Array" }, PendingName { holder: 0, key: "Iterator" },
    PendingName { holder: 0, key: "JSON" }, PendingName { holder: 0, key: "Map" }, PendingName { holder: 0, key: "Math" }, PendingName { holder: 0, key: "Number" },
    PendingName { holder: 0, key: "Promise" }, PendingName { holder: 0, key: "Proxy" }, PendingName { holder: 0, key: "Reflect" }, PendingName { holder: 0, key: "RegExp" },
    PendingName { holder: 0, key: "Set" }, PendingName { holder: 0, key: "SharedArrayBuffer" }, PendingName { holder: 0, key: "String" },
    PendingName { holder: 0, key: "Uint8Array" }, PendingName { holder: 0, key: "Uint8ClampedArray" }, PendingName { holder: 0, key: "Uint16Array" }, PendingName { holder: 0, key: "Uint32Array" },
    PendingName { holder: 0, key: "WeakMap" }, PendingName { holder: 0, key: "WeakRef" }, PendingName { holder: 0, key: "WeakSet" }, PendingName { holder: 0, key: "decodeURI" },
    PendingName { holder: 0, key: "decodeURIComponent" }, PendingName { holder: 0, key: "encodeURI" }, PendingName { holder: 0, key: "encodeURIComponent" }, PendingName { holder: 0, key: "escape" },
    PendingName { holder: 0, key: "eval" }, PendingName { holder: 0, key: "isFinite" }, PendingName { holder: 0, key: "isNaN" }, PendingName { holder: 0, key: "parseFloat" },
    PendingName { holder: 0, key: "parseInt" }, PendingName { holder: 0, key: "unescape" },
];
// The type names `typeof` gives, interned with the realm.
let TYPE_NAMES: Name[8] = [Name { text: "undefined" }, Name { text: "object" }, Name { text: "boolean" }, Name { text: "number" }, Name { text: "string" }, Name { text: "symbol" }, Name { text: "bigint" }, Name { text: "function" }];
// The light-cljs runtime leaves boot_runtime installs on `rt` (core false)
// or `rt.core`, in this order.
struct Leaf { core: bool, key: u8[..24], which: Builtin }
let RUNTIME_LEAVES: Leaf[17] = [
    Leaf { core: true, key: "macroReaderFunction", which: Builtin.MacroReaderFunction }, Leaf { core: true, key: "inc", which: Builtin.Inc },
    Leaf { core: true, key: "mapv", which: Builtin.Mapv }, Leaf { core: true, key: "vec", which: Builtin.Vec }, Leaf { core: true, key: "into", which: Builtin.Into },
    Leaf { core: true, key: "sequence", which: Builtin.Sequence }, Leaf { core: true, key: "mapSeq", which: Builtin.MapSeq }, Leaf { core: true, key: "list", which: Builtin.List },
    // The emitted call classifier compares against intrinsicRegExp's
    // identity; executing regular expressions remains an explicit boundary.
    Leaf { core: false, key: "symbol", which: Builtin.Symbol }, Leaf { core: false, key: "intrinsicRegExp", which: Builtin.IntrinsicRegExp },
    // gensym numbers in the job's compiler generated-name scope (gensym).
    // Other shared-state leaves are installed explicitly by the compiler.
    Leaf { core: true, key: "gensym", which: Builtin.Gensym },
    Leaf { core: false, key: "vectorFromArray", which: Builtin.VectorFromArray }, Leaf { core: false, key: "listFromArray", which: Builtin.ListFromArray },
    Leaf { core: false, key: "mapFromArray", which: Builtin.MapFromArray }, Leaf { core: false, key: "setFromArray", which: Builtin.SetFromArray },
    Leaf { core: false, key: "fnRest", which: Builtin.FnRest },
    // The emitted prologue reads the core operations it calls through it.
    Leaf { core: false, key: "nativeCoreBinding", which: Builtin.NativeCoreBinding },
];

// A built-in's code: its position in BUILTINS.
fn code(b: Builtin) -> u32 {
    for e, i in BUILTINS { if e.which == b { return i as u32; } }
    abort("builtin is not in BUILTINS"); return 0;
}
inline fn builtin_count() -> i64 { return BUILTINS.len; }
// The Builtin of built-in function object o (kind 4); its code was checked
// when the row was made or restored (snapshot::validate).
fn builtin_of(heap: const Heap&, o: i64) -> Builtin {
    let c: i64 = node(heap, o).function;
    if c < 0 || c >= BUILTINS.len { abort("row is not a built-in function"); }
    return BUILTINS[c].which;
}
// The leaves the compiler answers as host effects (request_effect), by code.
fn effect_builtin(b: Builtin) -> bool { return b == Builtin.Gensym || b == Builtin.SharedIncrement || b == Builtin.SharedRead || b == Builtin.External; }
// The pending property's slot: an accessor naming the realm's pending pair.
inline fn pending() -> Value { return Value.Accessor { a: 0 }; }
inline fn is_pending(v: Value) -> bool { return match v { Accessor x => x.a == 0, _ => false }; }
// A read, write or definition reached pending property k: UnsupportedOperand,
// naming k for the report (Heap.pending, fail).
fn pending_fault(heap: Heap&, k: Value) { if heap.fault == Fault.None { heap.fault = Fault.UnsupportedOperand; heap.pending = k; } }
fn pending_detail(job: Job&) {
    let k = job.heap.pending; job.detail.clear();
    for c in "pending " { job.detail.push(c); }
    if k.tag() == 4 { append_utf16(job.heap, k, job.detail); } else { for c in "@@" { job.detail.push(c); } for c in export_well_known_symbol(symbol_index(k)) { job.detail.push(c); } }
}
// The [[Prototype]] of node o: its own link, or for Empty, the realm's
// default prototype for its kind (Object.prototype for ordinary objects,
// errors and arguments objects, Array.prototype for arrays,
// Function.prototype for closures and built-in functions). Forms and the
// other runtime kinds have none, and so does every node of a heap built
// without initialize (hand-made test heaps, whose global is not set).
fn prototype(heap: const Heap&, o: i64) -> Value {
    let p = node(heap, o).proto;
    if p.tag() != -1 { return p; }
    if heap.global.tag() != 5 { return Value.Null; }
    return match node(heap, o).kind {
        0, 14 => object(object_prototype_row()), 1 => object(array_prototype_row()), 2, 4 => object(function_prototype_row()), 13 => object(error_prototype_row(0)),
        _ => Value.Null,
    };
}
// Whether node o is a realm row: the image owner made it in initialize.
inline fn realm_row(o: i64) -> bool { return o >= 0 && o < realm_nodes(); }
// Whether a compiler job reads node o through its prototype chain although
// the image holds it (Job.protected_nodes): every realm row but the global
// object, whose names the compiler installs and which lacks many of the V8
// build's, so a name it does not have stays a NativeBoundary there.
inline fn realm_intrinsic(o: i64) -> bool { return o > global_row() && o < realm_nodes(); }
// Whether an image's first rows are this realm's layout (snapshot::validate):
// the fixed rows' kinds, prototypes and built-in codes, one built-in per
// REALM_FUNCTIONS entry, the pending pair at accessor row 0, the global at
// row 0 and Function.prototype.call as the call method. Their properties are
// validated with every other row's.
fn realm_valid(nodes: const Node[:], accessors: const Pair[:], global: Value, call: Value) -> bool {
    if nodes.len < realm_nodes() || accessors.len < 1 || accessors[0].get.tag() != 0 || accessors[0].set.tag() != 0 || global.tag() != 5 || global.id() != global_row() { return false; }
    let kinds: i64[] = [0, 0, 4, 1];
    for k, o in kinds { if nodes[o].kind != k { return false; } }
    if nodes[object_prototype_row()].proto.tag() != 1 || nodes[function_prototype_row()].function != code(Builtin.FunctionPrototype) { return false; }
    for t in ERROR_NAMES.len {
        if nodes[error_prototype_row(t)].kind != 0 || nodes[error_constructor_row(t)].kind != 4 || nodes[error_constructor_row(t)].function != code(ERROR_BUILTINS[t]) { return false; }
    }
    let constructors: Builtin[] = [Builtin.Object, Builtin.Function, Builtin.Array];
    for b, i in constructors { let o = object_constructor_row() + i; if nodes[o].kind != 4 || nodes[o].function != code(b) { return false; } }
    for m, j in REALM_FUNCTIONS { let o = fixed_rows() + j; if nodes[o].kind != 4 || nodes[o].function != code(m.which) || m.which == Builtin.FunctionPrototypeCall && (call.tag() != 5 || call.id() != o) { return false; } }
    return true;
}

// Build the realm into an empty heap (initialize). A rejection
// (MemoryLimit, WorkLimit) leaves the heap unusable, as any failed
// initialize does.
fn build_realm(heap: Heap&) -> bool {
    if heap.nodes.len != 0 || heap.accessors.len != 0 { heap.fault = Fault.InvalidState; return false; }
    if add_accessor(heap, Pair {}) != 0 { return false; }
    var rows: Node[>..] = [];
    rows.push(Node { kind: 0 });
    rows.push(Node { kind: 0, proto: Value.Null });
    rows.push(Node { kind: 4, function: code(Builtin.FunctionPrototype), proto: object(object_prototype_row()) });
    rows.push(Node { kind: 1, proto: object(object_prototype_row()) });
    for t in ERROR_NAMES.len { rows.push(Node { kind: 0, proto: object(if t == 0 { object_prototype_row() } else { error_prototype_row(0) }) }); }
    rows.push(Node { kind: 4, function: code(Builtin.Object) }); rows.push(Node { kind: 4, function: code(Builtin.Function) }); rows.push(Node { kind: 4, function: code(Builtin.Array) });
    for b, t in ERROR_BUILTINS { rows.push(if t == 0 { Node { kind: 4, function: code(b) } } else { Node { kind: 4, function: code(b), proto: object(error_constructor_row(0)) } }); }
    assert(rows.len == fixed_rows());
    for m in REALM_FUNCTIONS { rows.push(Node { kind: 4, function: code(m.which) }); }
    if !charge(heap, rows.len) || add_nodes(heap, rows) != 0 { if heap.fault == Fault.None { memory(heap); } return false; }
    heap.global = object(global_row());
    // Every built-in function's `length` and `name`.
    for o in rows.len { if rows[o].kind == 4 && !builtin_metadata(heap, o) { return false; } }
    // Constructors and their prototypes.
    let pairs: i64[] = [object_constructor_row(), object_prototype_row(), function_constructor_row(), function_prototype_row(), array_constructor_row(), array_prototype_row()];
    var i: i64 = 0;
    while i < pairs.len { if !link_constructor(heap, pairs[i], pairs[i + 1]) { return false; } i += 2; }
    for t in ERROR_NAMES.len {
        let p = error_prototype_row(t);
        if !link_constructor(heap, error_constructor_row(t), p) || !realm_value(heap, p, "name", key_named(heap, ERROR_NAMES[t].text), attr_writable() | attr_configurable()) || !realm_value(heap, p, "message", key_named(heap, ""), attr_writable() | attr_configurable()) { return false; }
    }
    for m, j in REALM_FUNCTIONS {
        if !realm_value(heap, m.holder, BUILTINS[code(m.which)].name, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; }
        if m.which == Builtin.FunctionPrototypeCall { heap.call_method = object(fixed_rows() + j); }
    }
    // The well-known symbols, Symbol's non-writable, non-enumerable,
    // non-configurable data properties; their values are the exporter's
    // fixed indices (decision 2), so they take no rows.
    assert(REALM_FUNCTIONS[symbol_row() - fixed_rows()].which == Builtin.SymbolFunction);
    var s: i64 = 0;
    while export_well_known_symbol(s).len > 0 { if !realm_value(heap, symbol_row(), export_well_known_symbol(s), Value.Symbol { s: s as u32 }, 0) { return false; } s += 1; }
    // The global object's own properties.
    if !realm_value(heap, global_row(), "globalThis", heap.global, attr_writable() | attr_configurable()) { return false; }
    let globals: i64[] = [object_constructor_row(), function_constructor_row(), array_constructor_row()];
    for o in globals { if !realm_value(heap, global_row(), BUILTINS[node(heap, o).function].name, object(o), attr_writable() | attr_configurable()) { return false; } }
    for t in ERROR_NAMES.len { if !realm_value(heap, global_row(), ERROR_NAMES[t].text, object(error_constructor_row(t)), attr_writable() | attr_configurable()) { return false; } }
    if !realm_value(heap, global_row(), "NaN", number(0.0 / 0.0), 0) || !realm_value(heap, global_row(), "Infinity", number(1.0 / 0.0), 0) || !realm_value(heap, global_row(), "undefined", Value.Undefined, 0) { return false; }
    for p in PENDING {
        // Symbol's pending names sit on symbol_row(), which moves when a
        // REALM_FUNCTIONS entry is added before it.
        assert(p.key != "keyFor" || p.holder == symbol_row());
        let k: Value = if p.symbol >= 0 { Value.Symbol { s: p.symbol as u32 } } else { key_named(heap, p.key) };
        if heap.fault != Fault.None || !add_own(heap, p.holder, k, pending(), attr_configurable()) { return false; }
    }
    for n in TYPE_NAMES { key_named(heap, n.text); }
    if heap.fault != Fault.None { return false; }
    assert(heap.nodes.len == realm_nodes());
    return true;
}
// Built-in function o's `length` and `name` from BUILTINS (configurable only).
fn builtin_metadata(heap: Heap&, o: i64) -> bool {
    let info = BUILTINS[node(heap, o).function];
    return realm_value(heap, o, "length", number(info.length), attr_configurable()) && realm_value(heap, o, "name", key_named(heap, info.name), attr_configurable());
}
fn realm_value(heap: Heap&, o: i64, name: const u8[:], v: Value, attrs: u8) -> bool {
    let k = key_named(heap, name);
    return heap.fault == Fault.None && v.tag() >= 0 && add_own(heap, o, k, v, attrs);
}
// C.prototype = P (no attributes) and P.constructor = C (writable,
// configurable).
fn link_constructor(heap: Heap&, c: i64, p: i64) -> bool {
    return realm_value(heap, c, "prototype", object(p), 0) && realm_value(heap, p, "constructor", object(c), attr_writable() | attr_configurable());
}
// The light-cljs runtime (`rt`, global __lightNativeRuntime) and its
// leaves (RUNTIME_LEAVES): built once, in the image owner, after the realm.
fn boot_runtime(heap: Heap&) -> Value {
    if heap.boot_count != 0 { heap.fault = Fault.InvalidState; return Value.Undefined; }
    let rt = allocate(heap, Node { kind: 0 }); let core = allocate(heap, Node { kind: 0 });
    install_field(heap, rt, "core", core);
    install_field(heap, heap.global, "__lightNativeRuntime", rt);
    for leaf in RUNTIME_LEAVES {
        let f = allocate(heap, Node { kind: 4, function: code(leaf.which) });
        if f.tag() != 5 || !builtin_metadata(heap, f.id()) { return Value.Undefined; }
        install_field(heap, if leaf.core { core } else { rt }, leaf.key, f);
    }
    if heap.fault == Fault.None { heap.boot_count += 1; }
    return rt;
}

// The error type (ERROR_NAMES) a catchable fault raises.
fn fault_type(fault: Fault) -> i64 { return match fault { TypeError => 5, ReferenceError => 3, RangeError => 2, StackLimit => 7, _ => 0 }; }
// An error object (kind 13): `code` is error_code of the VM fault it was
// raised for, 0 for one JavaScript made; its prototype is its type's.
fn error_object(heap: Heap&, t: i64, code: u32, message: Value) -> Value {
    let e = allocate(heap, Node { kind: 13, function: code, proto: object(error_prototype_row(t)) });
    if heap.fault != Fault.None { return Value.Undefined; }
    if message.tag() == 4 && !add_own(heap, e.id(), key_named(heap, "message"), message, attr_writable() | attr_configurable()) { return Value.Undefined; }
    return e;
}
// The message of a VM fault's error: Job.detail when the operation named
// one, else the QuickJS wording for the instruction that faulted.
fn fault_message(job: Job&, fault: Fault, frame: Frame) -> Value {
    let heap .= job.heap;
    if job.detail.len > 0 { return string(heap, job.detail); }
    let code .= heap.code;
    let op = if frame.function >= 0 && frame.pc >= 0 && frame.pc < code.ops.len { code.ops[frame.pc].code } else { -1 };
    let words: const u8[:] = match fault {
        StackLimit => "stack overflow",
        RangeError => "invalid array length",
        ReferenceError => if op >= 48 && op <= 50 { "lexical variable is not initialized" } else { "variable is not defined" },
        TypeError => match op { 44, 83 => "not a function", 79, 90 => "cannot read property of undefined or null", 80, 91, 151, 152 => "cannot set property", 98 => "invalid 'in' operand", _ => "invalid operand" },
        _ => "host effect failed",
    };
    return text(heap, words);
}
// Name an unsupported built-in step (Job.detail) and fail UnsupportedOperand.
fn unsupported_step(job: Job&, b: Builtin, what: const u8[:]) -> Value {
    job.detail.clear();
    for c in BUILTINS[code(b)].name { job.detail.push(c); }
    if what.len > 0 { job.detail.push(58); job.detail.push(32); for c in what { job.detail.push(c); } }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.UnsupportedOperand; }
    return Value.Undefined;
}
// A TypeError a built-in throws, with its message.
fn type_error(job: Job&, message: const u8[:]) -> Value {
    job.detail.clear(); for c in message { job.detail.push(c); }
    if job.heap.fault == Fault.None { job.heap.fault = Fault.TypeError; }
    return Value.Undefined;
}
inline fn arg(args: const Value[:], i: i64) -> Value { return if i < args.len { args[i] } else { Value.Undefined }; }
// ToString of a primitive without user code: strings, undefined, null,
// booleans and integral numbers (the Number area owns the rest); -2 tag
// (Undefined result with UnsupportedOperand) otherwise.
fn primitive_string(job: Job&, b: Builtin, v: Value) -> Value {
    let heap .= job.heap;
    return match v {
        String => v, Undefined => key_named(heap, "undefined"), Null => key_named(heap, "null"),
        Bool x => key_named(heap, if x.v { "true" } else { "false" }),
        Number x => if x.v != x.v { key_named(heap, "NaN") } else if x.v == trunc(x.v) && x.v > -9007199254740992.0 && x.v < 9007199254740992.0 && !(x.v == 0 && 1.0 / x.v < 0) { text(heap, str(x.v as i64)) } else { unsupported_step(job, b, "ToString of a non-integral number") },
        _ => unsupported_step(job, b, "ToString of an object"),
    };
}
// ToPropertyKey without user code (an object's ToPrimitive is not run here).
fn property_key(job: Job&, b: Builtin, v: Value) -> Value {
    let k = if v.tag() == 3 && v.num() >= 0 && v.num() < 4294967295.0 && v.num() == trunc(v.num()) { key(job.heap, v) } else { primitive_string(job, b, v) };
    return if job.heap.fault == Fault.None { intern(job.heap, k) } else { Value.Undefined };
}
// Whether ToObject(o) has own property k (interned): strings their indices
// and length, numbers and booleans nothing, objects by kind.
fn has_own(job: Job&, b: Builtin, o: Value, k: Value) -> bool {
    let heap .= job.heap;
    if o.tag() == 4 { if key_is(heap, k, "length") { return true; } let i = index(heap, k); return i >= 0 && i < str_len(heap, o); }
    if o.tag() != 5 { return false; }
    if lazy_prototype(heap, o.id()) && key_is(heap, k, "prototype") { return true; }
    return own_entry(heap, o, k).found;
}
// Object.getOwnPropertyDescriptor(o, key) (light-cljs-806): undefined for
// no own property `key`, otherwise a new ordinary object
// (FromPropertyDescriptor) with `value` and `writable`, or `get` and `set`,
// then `enumerable` and `configurable`. An ordinary function's own lazy
// `prototype` is made first, as a read makes it (prepare_function); an
// inherited one is not looked at. A
// primitive o other than undefined and null (ToObject) and a pending
// property fail by name.
fn own_descriptor(job: Job&, b: Builtin, o: Value, kv: Value) -> Value {
    let heap .= job.heap;
    if o.tag() <= 1 { return type_error(job, "cannot convert undefined or null to object"); }
    let k = property_key(job, b, kv);
    if heap.fault != Fault.None { return Value.Undefined; }
    if o.tag() != 5 { return unsupported_step(job, b, "primitive wrapper objects"); }
    if lazy_prototype(heap, o.id()) && key_is(heap, k, "prototype") { prepare_function(job, o, k, false); }
    let p = own_entry(heap, o, k);
    if heap.fault != Fault.None || !p.found { return Value.Undefined; }
    if is_pending(p.value) { pending_fault(heap, k); return Value.Undefined; }
    let d = allocate(heap, Node { kind: 0 });
    if heap.fault != Fault.None { return Value.Undefined; }
    if is_accessor(p.value) {
        let fns = pair(heap, p.value);
        if !realm_value(heap, d.id(), "get", fns.get, attrs_data()) || !realm_value(heap, d.id(), "set", fns.set, attrs_data()) { return Value.Undefined; }
    } else if !realm_value(heap, d.id(), "value", p.value, attrs_data()) || !realm_value(heap, d.id(), "writable", boolean(p.attrs & attr_writable() != 0), attrs_data()) { return Value.Undefined; }
    if !realm_value(heap, d.id(), "enumerable", boolean(p.attrs & attr_enumerable() != 0), attrs_data()) || !realm_value(heap, d.id(), "configurable", boolean(p.attrs & attr_configurable() != 0), attrs_data()) { return Value.Undefined; }
    return d;
}
// rt.nativeCoreBinding(name) (light-cljs-807): the V8 build's
// Object.getOwnPropertyDescriptor(core, name), core being the image's
// `rt.core` (reached through the global, which no job may write): a new
// ordinary object whose `value` is the leaf itself, `get` absent (no Goose
// core leaf is an accessor). A name the Goose runtime has no core leaf for
// fails NativeBoundary naming it (Job.detail), never undefined.
fn core_binding(job: Job&, b: Builtin, name: Value) -> Value {
    let heap .= job.heap;
    let rt = data_get(job, b, heap.global, "__lightNativeRuntime");
    if heap.fault != Fault.None { return Value.Undefined; }
    let core = if rt.tag() == 5 { data_get(job, b, rt, "core") } else { Value.Undefined };
    if heap.fault != Fault.None { return Value.Undefined; }
    if core.tag() != 5 { heap.fault = Fault.InvalidState; return Value.Undefined; }
    let d = if name.tag() == 4 { own_descriptor(job, b, core, name) } else { Value.Undefined };
    if heap.fault != Fault.None || d.tag() == 5 { return d; }
    job.detail.clear();
    for c in BUILTINS[code(b)].name { job.detail.push(c); }
    for c in ": no core leaf " { job.detail.push(c); }
    if name.tag() == 4 { append_utf16(heap, name, job.detail); }
    heap.fault = Fault.NativeBoundary;
    return Value.Undefined;
}
// Units of `[object Tag]`.
fn object_tag(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap;
    if receiver.tag() == 0 { return text(heap, "[object Undefined]"); }
    if receiver.tag() == 1 { return text(heap, "[object Null]"); }
    if receiver.tag() != 5 { return unsupported_step(job, Builtin.ObjectPrototypeToString, "a primitive receiver"); }
    let tag, getter = lookup(heap, receiver, Value.Symbol { s: 4 });
    if heap.fault != Fault.None { return Value.Undefined; }
    if getter.tag() == 5 { return unsupported_step(job, Builtin.ObjectPrototypeToString, "a @@toStringTag getter"); }
    var units: u16[>..] = [];
    for c in "[object " { units.push(c); }
    if tag.tag() == 4 { append_utf16(heap, tag, units); }
    else {
        let name: const u8[:] = match node(heap, receiver.id()).kind { 1 => "Array", 2, 4 => "Function", 13 => "Error", 14 => "Arguments", _ => "Object" };
        for c in name { units.push(c); }
    }
    units.push(93);
    if !charge(heap, units.len) { return Value.Undefined; }
    return string(heap, units);
}
// A data property's value for a built-in that reads one (Get without
// running a getter): its value, or Undefined with UnsupportedOperand for a getter.
fn data_get(job: Job&, b: Builtin, o: Value, name: const u8[:]) -> Value {
    let heap .= job.heap;
    let k = key_named(heap, name);
    if heap.fault != Fault.None { return Value.Undefined; }
    let v, getter = lookup(heap, o, k);
    if getter.tag() == 5 && heap.fault == Fault.None { return unsupported_step(job, b, "a getter"); }
    return v;
}
// Error.prototype.toString (no getter, no object-valued name or message).
fn error_to_string(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap; let b = Builtin.ErrorPrototypeToString;
    if receiver.tag() != 5 { return type_error(job, "not an object"); }
    let n = data_get(job, b, receiver, "name"); if heap.fault != Fault.None { return Value.Undefined; }
    let name = if n.tag() == 0 { key_named(heap, "Error") } else { primitive_string(job, b, n) }; if heap.fault != Fault.None { return Value.Undefined; }
    let m = data_get(job, b, receiver, "message"); if heap.fault != Fault.None { return Value.Undefined; }
    let message = if m.tag() == 0 { key_named(heap, "") } else { primitive_string(job, b, m) }; if heap.fault != Fault.None { return Value.Undefined; }
    if str_len(heap, message) == 0 { return name; }
    if str_len(heap, name) == 0 { return message; }
    var units: u16[>..] = []; append_utf16(heap, name, units); units.push(58); units.push(32); append_utf16(heap, message, units);
    if !charge(heap, units.len) { return Value.Undefined; }
    return string(heap, units);
}
// NativeError(message, options) called as a function: a new error object of
// type t whose own `message` is ToString(message) and, when options has a
// `cause`, whose own `cause` is that value.
fn construct_error(job: Job&, b: Builtin, t: i64, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let m = arg(args, 0);
    let message = if m.tag() == 0 { Value.Undefined } else { primitive_string(job, b, m) };
    if heap.fault != Fault.None { return Value.Undefined; }
    let e = error_object(heap, t, 0, message);
    let options = arg(args, 1);
    if heap.fault == Fault.None && options.tag() == 5 {
        let k = key_named(heap, "cause");
        if heap.fault == Fault.None && has_property(heap, options, k) {
            let cause = data_get(job, b, options, "cause");
            if heap.fault == Fault.None { add_own(heap, e.id(), k, cause, attr_writable() | attr_configurable()); }
        }
    }
    return if heap.fault == Fault.None { e } else { Value.Undefined };
}
// Array(...args) called as a function: one number argument is a length
// (a RangeError unless it is a uint32), otherwise the arguments are the
// elements.
fn construct_array(job: Job&, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let a = allocate(heap, Node { kind: 1 });
    if heap.fault != Fault.None { return Value.Undefined; }
    if args.len == 1 && args[0].tag() == 3 {
        let n = args[0].num();
        if !(n >= 0 && n <= 4294967295.0) || n != trunc(n) { job.detail.clear(); for c in "invalid array length" { job.detail.push(c); } heap.fault = Fault.RangeError; return Value.Undefined; }
        array_length(heap, a, args[0]);
    } else { for v, i in args { if !set_element(heap, a, i, v) { break; } } }
    return if heap.fault == Fault.None { a } else { Value.Undefined };
}
// Call built-in b with `receiver` as this (decision 5): the plain built-ins,
// which run no JavaScript, run to completion here. The runtime leaves
// delegate to native_value. A step not implemented yet fails
// UnsupportedOperand, naming the built-in in Job.detail. `id` is the
// callee's code (its node's `function`).
fn call_builtin(job: Job&, id: i64, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if id < 0 || id >= BUILTINS.len { heap.fault = Fault.InvalidState; return Value.Undefined; }
    let b = BUILTINS[id].which;
    match b {
        FunctionPrototype => { return Value.Undefined; },
        Object => {
            let v = arg(args, 0);
            if v.tag() <= 1 { return allocate(heap, Node { kind: 0 }); }
            if v.tag() == 5 { return v; }
            return unsupported_step(job, b, "primitive wrapper objects");
        },
        Function => { return unsupported_step(job, b, "dynamic functions"); },
        Array => { return construct_array(job, args); },
        Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError => {
            var t: i64 = 0; for e, i in ERROR_BUILTINS { if e == b { t = i; } }
            return construct_error(job, b, t, args);
        },
        ObjectGetPrototypeOf => {
            let v = arg(args, 0);
            if v.tag() <= 1 { return type_error(job, "cannot convert undefined or null to object"); }
            if v.tag() != 5 { return unsupported_step(job, b, "primitive wrapper prototypes"); }
            let kind = node(heap, v.id()).kind;
            if kind != 0 && kind != 1 && kind != 2 && kind != 4 && kind != 13 && kind != 14 { return unsupported_step(job, b, "a runtime form"); }
            return prototype(heap, v.id());
        },
        ObjectIs => { return boolean(same_value(heap, arg(args, 0), arg(args, 1))); },
        ObjectHasOwn, ObjectPrototypeHasOwnProperty => {
            let own = b == Builtin.ObjectHasOwn;
            let o = if own { arg(args, 0) } else { receiver };
            // Object.hasOwn converts its object first, hasOwnProperty its key.
            if own && o.tag() <= 1 { return type_error(job, "cannot convert undefined or null to object"); }
            let k = property_key(job, b, arg(args, if own { 1 } else { 0 }));
            if heap.fault != Fault.None { return Value.Undefined; }
            if o.tag() <= 1 { return type_error(job, "cannot convert undefined or null to object"); }
            let found = has_own(job, b, o, k);
            return if heap.fault == Fault.None { boolean(found) } else { Value.Undefined };
        },
        ObjectPrototypeValueOf => {
            if receiver.tag() <= 1 { return type_error(job, "cannot convert undefined or null to object"); }
            if receiver.tag() != 5 { return unsupported_step(job, b, "primitive wrapper objects"); }
            return receiver;
        },
        ObjectPrototypeToString => { return object_tag(job, receiver); },
        ObjectGetOwnPropertyDescriptor => { return own_descriptor(job, b, arg(args, 0), arg(args, 1)); },
        ArrayIsArray => { let v = arg(args, 0); return boolean(v.tag() == 5 && node(heap, v.id()).kind == 1); },
        ErrorPrototypeToString => { return error_to_string(job, receiver); },
        FunctionPrototypeCall => { return unsupported_step(job, b, "called from a native frame"); },
        // The dispatch loop runs these as native frames (native_start); a
        // caller that runs a built-in to completion here cannot.
        ArrayPrototypeMap, ArrayPrototypeForEach => { return unsupported_step(job, b, "called from a native frame"); },
        SymbolFunction => { return unsupported_step(job, b, "creating symbols"); },
        NativeCoreBinding => { return core_binding(job, b, arg(args, 0)); },
        _ => { return native_value(heap, id, args); },
    }
}
// `typeof v` (opcode 78, ECMAScript TypeofOperator): closures and built-in
// functions are "function", every other object "object".
fn type_name(heap: Heap&, v: Value) -> Value {
    let t: i64 = match v { Undefined => 0, Null => 1, Bool => 2, Number => 3, String => 4, Symbol => 5, BigInt => 6, Object x => if node(heap, x.o).kind == 2 || node(heap, x.o).kind == 4 { 7 } else { 1 }, _ => 1 };
    return key_named(heap, TYPE_NAMES[t].text);
}
