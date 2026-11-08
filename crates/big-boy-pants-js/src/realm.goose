import std;
import .source_vm;
import .array_callbacks;
import .array_methods;

namespace source;

// The realm (light-cljs-788; decisions 5 and 6 of docs/javascript-coverage.md).
//
// initialize() builds it once, as the first rows of a heap's tables, in the
// program that owns the session image; job workers inherit it in their
// spawn copy of that image (snapshot::restore) and never build it again.
// Its layout comes from the static tables below, so its node rows are fixed:
// rows 0 to fixed_rows() - 1 are the intrinsics every kind's default
// prototype names (realm_row), then one built-in function object per entry
// of REALM_FUNCTIONS, then ExceptionInfo.prototype
// (exception_info_prototype_row), realm_nodes() rows in all. Every intrinsic is an
// ordinary row of the VM tables (plain u32 IDs, decision 2): its state is
// its rows, which the worker reset restores like any image row, and
// JavaScript in a compiler job cannot write it (Job.owned_nodes,
// NativeBoundary). The static tables themselves are `let` data, never rows.
//
// A built-in function is node_kind::Builtin whose `function` field is its
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
    // Atoms, volatiles and cells (light-cljs-799; cells.goose).
    Atom, Deref, Reset, Swap, CompareAndSet, SwapVals, ResetVals, Volatile, Vreset, AddWatch, RemoveWatch, SetValidator,
    // Delays (light-cljs-808; cells.goose).
    Delay, Force, Realized, IsDelay, NativeConstruct,
    // Generators (light-cljs-790; continuations.goose).
    GeneratorPrototypeNext, GeneratorPrototypeReturn, GeneratorPrototypeThrow, IteratorPrototypeIterator,
    // %eval% (light-cljs-823): a direct-eval call of it is a direct eval
    // (light-cljs-858; source_vm.goose, Ins.DirectEval); any other call
    // is an indirect eval (light-cljs-856; runtime_compile.goose).
    Eval,
    // The primitive wrapper constructors (light-cljs-832; WRAPPER_NAMES
    // order) and their prototypes' methods.
    Number, String, Boolean,
    NumberPrototypeValueOf, NumberPrototypeToString, StringPrototypeValueOf, StringPrototypeToString, BooleanPrototypeValueOf, BooleanPrototypeToString,
    // Function.prototype[@@hasInstance] (light-cljs-834), a native frame (source_vm.goose, instanceof).
    FunctionPrototypeHasInstance,
    // Array.prototype.push, join and toString (light-cljs-842), native
    // frames (array_methods.goose).
    ArrayPrototypePush, ArrayPrototypeJoin, ArrayPrototypeToString,
    // The built-in array and string iterators (light-cljs-844;
    // iterators.goose): Array.prototype.values (also its @@iterator), keys
    // and entries, %ArrayIteratorPrototype%.next, String.prototype[@@iterator]
    // and %StringIteratorPrototype%.next.
    ArrayPrototypeValues, ArrayPrototypeKeys, ArrayPrototypeEntries, ArrayIteratorPrototypeNext, StringPrototypeIterator, StringIteratorPrototypeNext,
    // The Proxy constructor (light-cljs-845; proxy.goose).
    Proxy,
    // Function.prototype.apply, Reflect.apply and Reflect.construct
    // (light-cljs-851; function_calls.goose), which with
    // Function.prototype.call forward a call as a native frame.
    FunctionPrototypeApply, ReflectApply, ReflectConstruct,
    // Function.prototype.bind (light-cljs-852; function_calls.goose), a
    // native frame that makes a bound function object.
    FunctionPrototypeBind,
    // Function.prototype.toString and %ThrowTypeError%, the getter and
    // setter of Function.prototype's `caller` and `arguments`
    // (light-cljs-853; function_calls.goose).
    FunctionPrototypeToString, ThrowTypeError,
    // The runtime leaves a native macro unit reads for interop and errors
    // (light-cljs-872): rt.nativeInterop and rt.declarationReceiver, and
    // rt.core's exInfo, exData, exMessage, exCause and getIn.
    NativeInterop, DeclarationReceiver, ExInfo, ExData, ExMessage, ExCause, GetIn,
}
// A built-in's code (its position here, a node's `function`), its `name`
// and its `length`.
struct BuiltinInfo { which: Builtin, name: u8[..24], length: u8 = 0 }
let BUILTINS: BuiltinInfo[101] = [
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
    BuiltinInfo { which: Builtin.Atom, name: "atom", length: 1 }, BuiltinInfo { which: Builtin.Deref, name: "deref", length: 1 },
    BuiltinInfo { which: Builtin.Reset, name: "reset", length: 2 }, BuiltinInfo { which: Builtin.Swap, name: "swap", length: 2 },
    BuiltinInfo { which: Builtin.CompareAndSet, name: "compareAndSet", length: 3 }, BuiltinInfo { which: Builtin.SwapVals, name: "swapVals", length: 2 },
    BuiltinInfo { which: Builtin.ResetVals, name: "resetVals", length: 2 }, BuiltinInfo { which: Builtin.Volatile, name: "volatile", length: 1 },
    BuiltinInfo { which: Builtin.Vreset, name: "vreset", length: 2 }, BuiltinInfo { which: Builtin.AddWatch, name: "addWatch", length: 3 },
    BuiltinInfo { which: Builtin.RemoveWatch, name: "removeWatch", length: 2 }, BuiltinInfo { which: Builtin.SetValidator, name: "setValidator", length: 2 },
    BuiltinInfo { which: Builtin.Delay, name: "Delay", length: 2 }, BuiltinInfo { which: Builtin.Force, name: "force", length: 1 },
    BuiltinInfo { which: Builtin.Realized, name: "realized", length: 1 }, BuiltinInfo { which: Builtin.IsDelay, name: "isDelay", length: 1 },
    BuiltinInfo { which: Builtin.NativeConstruct, name: "nativeConstruct", length: 1 },
    BuiltinInfo { which: Builtin.GeneratorPrototypeNext, name: "next", length: 1 }, BuiltinInfo { which: Builtin.GeneratorPrototypeReturn, name: "return", length: 1 },
    BuiltinInfo { which: Builtin.GeneratorPrototypeThrow, name: "throw", length: 1 }, BuiltinInfo { which: Builtin.IteratorPrototypeIterator, name: "[Symbol.iterator]" },
    BuiltinInfo { which: Builtin.Eval, name: "eval", length: 1 },
    BuiltinInfo { which: Builtin.Number, name: "Number", length: 1 }, BuiltinInfo { which: Builtin.String, name: "String", length: 1 }, BuiltinInfo { which: Builtin.Boolean, name: "Boolean", length: 1 },
    BuiltinInfo { which: Builtin.NumberPrototypeValueOf, name: "valueOf" }, BuiltinInfo { which: Builtin.NumberPrototypeToString, name: "toString", length: 1 },
    BuiltinInfo { which: Builtin.StringPrototypeValueOf, name: "valueOf" }, BuiltinInfo { which: Builtin.StringPrototypeToString, name: "toString" },
    BuiltinInfo { which: Builtin.BooleanPrototypeValueOf, name: "valueOf" }, BuiltinInfo { which: Builtin.BooleanPrototypeToString, name: "toString" },
    BuiltinInfo { which: Builtin.FunctionPrototypeHasInstance, name: "[Symbol.hasInstance]", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypePush, name: "push", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeJoin, name: "join", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeToString, name: "toString" },
    BuiltinInfo { which: Builtin.ArrayPrototypeValues, name: "values" }, BuiltinInfo { which: Builtin.ArrayPrototypeKeys, name: "keys" },
    BuiltinInfo { which: Builtin.ArrayPrototypeEntries, name: "entries" }, BuiltinInfo { which: Builtin.ArrayIteratorPrototypeNext, name: "next" },
    BuiltinInfo { which: Builtin.StringPrototypeIterator, name: "[Symbol.iterator]" }, BuiltinInfo { which: Builtin.StringIteratorPrototypeNext, name: "next" },
    BuiltinInfo { which: Builtin.Proxy, name: "Proxy", length: 2 },
    BuiltinInfo { which: Builtin.FunctionPrototypeApply, name: "apply", length: 2 }, BuiltinInfo { which: Builtin.ReflectApply, name: "apply", length: 3 },
    BuiltinInfo { which: Builtin.ReflectConstruct, name: "construct", length: 2 },
    BuiltinInfo { which: Builtin.FunctionPrototypeBind, name: "bind", length: 1 },
    BuiltinInfo { which: Builtin.FunctionPrototypeToString, name: "toString" }, BuiltinInfo { which: Builtin.ThrowTypeError, name: "" },
    BuiltinInfo { which: Builtin.NativeInterop, name: "nativeInterop", length: 2 }, BuiltinInfo { which: Builtin.DeclarationReceiver, name: "declarationReceiver", length: 1 },
    BuiltinInfo { which: Builtin.ExInfo, name: "exInfo" }, BuiltinInfo { which: Builtin.ExData, name: "exData", length: 1 },
    BuiltinInfo { which: Builtin.ExMessage, name: "exMessage", length: 1 }, BuiltinInfo { which: Builtin.ExCause, name: "exCause", length: 1 },
    BuiltinInfo { which: Builtin.GetIn, name: "getIn" },
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
// %IteratorPrototype%, %GeneratorPrototype% (%GeneratorFunction.prototype.prototype%)
// and %Promise.prototype% (light-cljs-790), ordinary objects.
inline fn iterator_prototype_row() -> i64 { return 23; }
inline fn generator_prototype_row() -> i64 { return 24; }
inline fn promise_prototype_row() -> i64 { return 25; }
// The primitive wrappers (light-cljs-832), indexed as WRAPPER_NAMES:
// %Number.prototype%, %String.prototype% and %Boolean.prototype%, which are
// themselves wrapper objects (node_kind::Wrapper) of +0, "" and false, then the three
// constructors.
inline fn wrapper_prototype_row(t: i64) -> i64 { return 26 + t; }
inline fn wrapper_constructor_row(t: i64) -> i64 { return 29 + t; }
// %ArrayIteratorPrototype% and %StringIteratorPrototype% (light-cljs-844),
// ordinary objects inheriting %IteratorPrototype%.
inline fn array_iterator_prototype_row() -> i64 { return 32; }
inline fn string_iterator_prototype_row() -> i64 { return 33; }
// The Reflect namespace object (light-cljs-851), an ordinary object.
inline fn reflect_row() -> i64 { return 34; }
// The arguments object templates (light-cljs-854; source_vm.goose
// new_arguments): two node_kind::Arguments rows no JavaScript reaches,
// whose shapes hold `length`, @@iterator and `callee`, and whose slots hold
// @@iterator's value and, for the unmapped one, `callee`'s %ThrowTypeError%
// pair: the unmapped object's (strict code, or a non-simple parameter list)
// and the mapped object's, whose `callee` each object fills in.
inline fn arguments_template_row(mapped: bool) -> i64 { return if mapped { 36 } else { 35 }; }
// Array.prototype[@@unscopables] (light-cljs-855; ECMAScript 23.1.3.41): an
// ordinary object with a null [[Prototype]] whose UNSCOPABLE_NAMES are own
// data properties, all true.
inline fn array_unscopables_row() -> i64 { return 37; }
// %GeneratorFunction.prototype%, %AsyncFunction.prototype% and
// %AsyncGeneratorFunction.prototype% (light-cljs-857), ordinary objects
// inheriting %Function.prototype%: the [[Prototype]] of generator, async and
// async generator closures (prototype). Their `constructor`s, the run-time
// constructors of those kinds, are pending for the generators area
// (light-cljs-768), so `(function* () {}).constructor` never reaches the
// Function constructor. Indexed by function_kind:: (FUNCTION_KIND_TAGS
// order).
let function_kind::Generator: i64 = 0;
let function_kind::Async: i64 = 1;
let function_kind::AsyncGenerator: i64 = 2;
inline fn function_kind_prototype_row(k: i64) -> i64 { return 38 + k; }
inline fn fixed_rows() -> i64 { return 41; }
struct Name { text: u8[..24] }
let FUNCTION_KIND_TAGS: Name[3] = [Name { text: "GeneratorFunction" }, Name { text: "AsyncFunction" }, Name { text: "AsyncGeneratorFunction" }];
// The function_kind:: of closure o's code, -1 for an ordinary function.
fn function_kind_of(heap: const Heap&, o: i64) -> i64 {
    let c: i64 = node(heap, o).function;
    if c < 0 || c >= heap.code.functions.len { return -1; }
    let f .= heap.code.functions[c];
    return if f.generator { if f.asynchronous { function_kind::AsyncGenerator } else { function_kind::Generator } } else if f.asynchronous { function_kind::Async } else { -1 };
}
let UNSCOPABLE_NAMES: Name[16] = [
    Name { text: "at" }, Name { text: "copyWithin" }, Name { text: "entries" }, Name { text: "fill" }, Name { text: "find" }, Name { text: "findIndex" }, Name { text: "findLast" }, Name { text: "findLastIndex" },
    Name { text: "flat" }, Name { text: "flatMap" }, Name { text: "includes" }, Name { text: "keys" }, Name { text: "toReversed" }, Name { text: "toSorted" }, Name { text: "toSpliced" }, Name { text: "values" },
];
let WRAPPER_NAMES:    Name[3]    = [Name { text: "Number" }, Name { text: "String" }, Name { text: "Boolean" }];
let WRAPPER_BUILTINS: Builtin[3] = [Builtin.Number, Builtin.String, Builtin.Boolean];
let ERROR_NAMES:      Name[8]    = [Name { text: "Error" }, Name { text: "EvalError" }, Name { text: "RangeError" }, Name { text: "ReferenceError" }, Name { text: "SyntaxError" }, Name { text: "TypeError" }, Name { text: "URIError" }, Name { text: "InternalError" }];
let ERROR_BUILTINS:   Builtin[8] = [Builtin.Error, Builtin.EvalError, Builtin.RangeError, Builtin.ReferenceError, Builtin.SyntaxError, Builtin.TypeError, Builtin.URIError, Builtin.InternalError];
// A built-in method: a property `name` (its BuiltinInfo name) of the fixed row `holder`.
struct Method { which: Builtin, holder: i64 }
let REALM_FUNCTIONS: Method[41] = [
    Method { which: Builtin.FunctionPrototypeCall, holder: 2 },
    Method { which: Builtin.ObjectGetPrototypeOf, holder: 12 }, Method { which: Builtin.ObjectIs, holder: 12 }, Method { which: Builtin.ObjectHasOwn, holder: 12 },
    Method { which: Builtin.ObjectPrototypeHasOwnProperty, holder: 1 }, Method { which: Builtin.ObjectPrototypeValueOf, holder: 1 }, Method { which: Builtin.ObjectPrototypeToString, holder: 1 },
    Method { which: Builtin.ArrayIsArray, holder: 14 }, Method { which: Builtin.ErrorPrototypeToString, holder: 4 },
    Method { which: Builtin.ObjectGetOwnPropertyDescriptor, holder: 12 },
    // The Symbol function, row symbol_row(), holds the well-known symbols.
    Method { which: Builtin.SymbolFunction, holder: 0 },
    Method { which: Builtin.ArrayPrototypeMap, holder: 3 }, Method { which: Builtin.ArrayPrototypeForEach, holder: 3 },
    // %GeneratorPrototype%'s methods and %IteratorPrototype%[@@iterator]
    // (light-cljs-790), whose key is the well-known symbol, not its name.
    Method { which: Builtin.GeneratorPrototypeNext, holder: 24 }, Method { which: Builtin.GeneratorPrototypeReturn, holder: 24 },
    Method { which: Builtin.GeneratorPrototypeThrow, holder: 24 }, Method { which: Builtin.IteratorPrototypeIterator, holder: 23 },
    // The global `eval` (light-cljs-823).
    Method { which: Builtin.Eval, holder: 0 },
    // %Number.prototype%, %String.prototype% and %Boolean.prototype%'s
    // valueOf and toString (light-cljs-832).
    Method { which: Builtin.NumberPrototypeToString, holder: 26 }, Method { which: Builtin.NumberPrototypeValueOf, holder: 26 },
    Method { which: Builtin.StringPrototypeToString, holder: 27 }, Method { which: Builtin.StringPrototypeValueOf, holder: 27 },
    Method { which: Builtin.BooleanPrototypeToString, holder: 28 }, Method { which: Builtin.BooleanPrototypeValueOf, holder: 28 },
    // Function.prototype[@@hasInstance] (light-cljs-834), keyed by the well-known symbol.
    Method { which: Builtin.FunctionPrototypeHasInstance, holder: 2 },
    // Array.prototype.push, join and toString (light-cljs-842).
    Method { which: Builtin.ArrayPrototypePush, holder: 3 }, Method { which: Builtin.ArrayPrototypeJoin, holder: 3 }, Method { which: Builtin.ArrayPrototypeToString, holder: 3 },
    // The built-in iterators (light-cljs-844): `values` is also
    // Array.prototype[@@iterator], String.prototype's method is keyed by the
    // well-known symbol, and each iterator prototype holds its `next`.
    Method { which: Builtin.ArrayPrototypeValues, holder: 3 }, Method { which: Builtin.ArrayPrototypeKeys, holder: 3 }, Method { which: Builtin.ArrayPrototypeEntries, holder: 3 },
    Method { which: Builtin.ArrayIteratorPrototypeNext, holder: 32 }, Method { which: Builtin.StringPrototypeIterator, holder: 27 }, Method { which: Builtin.StringIteratorPrototypeNext, holder: 33 },
    // The global Proxy (light-cljs-845), which has no `prototype`.
    Method { which: Builtin.Proxy, holder: 0 },
    // Function.prototype.apply and the Reflect object's apply and construct
    // (light-cljs-851); its other eleven methods are pending names.
    Method { which: Builtin.FunctionPrototypeApply, holder: 2 }, Method { which: Builtin.ReflectApply, holder: 34 }, Method { which: Builtin.ReflectConstruct, holder: 34 },
    // Function.prototype.bind (light-cljs-852).
    Method { which: Builtin.FunctionPrototypeBind, holder: 2 },
    // Function.prototype.toString, and %ThrowTypeError%, which is no
    // property: Function.prototype's `caller` and `arguments` accessors hold
    // it as getter and setter (light-cljs-853).
    Method { which: Builtin.FunctionPrototypeToString, holder: 2 }, Method { which: Builtin.ThrowTypeError, holder: 2 },
];
// cljs.core's ExceptionInfo.prototype (light-cljs-872), the realm's last
// row, after the REALM_FUNCTIONS: an ordinary object inheriting
// %Error.prototype%, the [[Prototype]] of what rt.core.exInfo makes. No
// property reaches it: JavaScript gets it only from such an object. Its
// `constructor` and `toString` (in the V8 build, the ExceptionInfo function
// and its printer) are pending.
inline fn exception_info_prototype_row() -> i64 { return fixed_rows() + REALM_FUNCTIONS.len; }
inline fn realm_nodes() -> i64 { return exception_info_prototype_row() + 1; }
// The node row of built-in b's REALM_FUNCTIONS entry.
fn realm_function_row(b: Builtin) -> i64 {
    for m, j in REALM_FUNCTIONS { if m.which == b { return fixed_rows() + j; } }
    abort("builtin is not in REALM_FUNCTIONS"); return 0;
}
// The Symbol function's row: its REALM_FUNCTIONS entry, the eleventh.
inline fn symbol_row() -> i64 { return fixed_rows() + 10; }
// A pending property of the fixed row `holder`: a name, or with `symbol`
// >= 0 a well-known symbol (export_well_known_symbol).
struct PendingName { holder: i64, key: u8[..32], symbol: i64 = -1 }
let PENDING: PendingName[204] = [
    // Object.prototype
    PendingName { holder: 1, key: "__proto__" }, PendingName { holder: 1, key: "isPrototypeOf" }, PendingName { holder: 1, key: "propertyIsEnumerable" }, PendingName { holder: 1, key: "toLocaleString" },
    PendingName { holder: 1, key: "__defineGetter__" }, PendingName { holder: 1, key: "__defineSetter__" }, PendingName { holder: 1, key: "__lookupGetter__" }, PendingName { holder: 1, key: "__lookupSetter__" },
    // Array.prototype
    PendingName { holder: 3, key: "at" }, PendingName { holder: 3, key: "concat" }, PendingName { holder: 3, key: "copyWithin" }, PendingName { holder: 3, key: "every" },
    PendingName { holder: 3, key: "fill" }, PendingName { holder: 3, key: "filter" }, PendingName { holder: 3, key: "find" }, PendingName { holder: 3, key: "findIndex" }, PendingName { holder: 3, key: "findLast" },
    PendingName { holder: 3, key: "findLastIndex" }, PendingName { holder: 3, key: "flat" }, PendingName { holder: 3, key: "flatMap" }, PendingName { holder: 3, key: "includes" },
    PendingName { holder: 3, key: "indexOf" }, PendingName { holder: 3, key: "lastIndexOf" },
    PendingName { holder: 3, key: "pop" }, PendingName { holder: 3, key: "reduce" }, PendingName { holder: 3, key: "reduceRight" }, PendingName { holder: 3, key: "reverse" },
    PendingName { holder: 3, key: "shift" }, PendingName { holder: 3, key: "slice" }, PendingName { holder: 3, key: "some" }, PendingName { holder: 3, key: "sort" }, PendingName { holder: 3, key: "splice" },
    PendingName { holder: 3, key: "toLocaleString" }, PendingName { holder: 3, key: "toReversed" }, PendingName { holder: 3, key: "toSorted" }, PendingName { holder: 3, key: "toSpliced" },
    PendingName { holder: 3, key: "unshift" }, PendingName { holder: 3, key: "with" },
    // Object
    PendingName { holder: 12, key: "assign" }, PendingName { holder: 12, key: "create" }, PendingName { holder: 12, key: "defineProperties" }, PendingName { holder: 12, key: "defineProperty" },
    PendingName { holder: 12, key: "entries" }, PendingName { holder: 12, key: "freeze" }, PendingName { holder: 12, key: "fromEntries" },
    PendingName { holder: 12, key: "getOwnPropertyDescriptors" }, PendingName { holder: 12, key: "getOwnPropertyNames" }, PendingName { holder: 12, key: "getOwnPropertySymbols" }, PendingName { holder: 12, key: "groupBy" },
    PendingName { holder: 12, key: "isExtensible" }, PendingName { holder: 12, key: "isFrozen" }, PendingName { holder: 12, key: "isSealed" }, PendingName { holder: 12, key: "keys" },
    PendingName { holder: 12, key: "preventExtensions" }, PendingName { holder: 12, key: "seal" }, PendingName { holder: 12, key: "setPrototypeOf" }, PendingName { holder: 12, key: "values" },
    // Symbol (symbol_row(); its well-known symbols are real)
    PendingName { holder: 51, key: "prototype" }, PendingName { holder: 51, key: "for" }, PendingName { holder: 51, key: "keyFor" },
    // %GeneratorFunction%, %AsyncFunction% and %AsyncGeneratorFunction%
    // (function_kind_prototype_row; the generators area, light-cljs-768).
    PendingName { holder: 38, key: "constructor" }, PendingName { holder: 39, key: "constructor" }, PendingName { holder: 40, key: "constructor" },
    // Reflect (reflect_row()): apply and construct are light-cljs-851's;
    // the other eleven belong to the objects and classes area (light-cljs-766).
    PendingName { holder: 34, key: "defineProperty" }, PendingName { holder: 34, key: "deleteProperty" }, PendingName { holder: 34, key: "get" }, PendingName { holder: 34, key: "getOwnPropertyDescriptor" },
    PendingName { holder: 34, key: "getPrototypeOf" }, PendingName { holder: 34, key: "has" }, PendingName { holder: 34, key: "isExtensible" }, PendingName { holder: 34, key: "ownKeys" },
    PendingName { holder: 34, key: "preventExtensions" }, PendingName { holder: 34, key: "set" }, PendingName { holder: 34, key: "setPrototypeOf" },
    // %IteratorPrototype% (its helpers), %GeneratorPrototype% and
    // %Promise.prototype% (light-cljs-790; the Promise area makes the last's).
    PendingName { holder: 23, key: "constructor" }, PendingName { holder: 23, key: "drop" }, PendingName { holder: 23, key: "every" }, PendingName { holder: 23, key: "filter" },
    PendingName { holder: 23, key: "find" }, PendingName { holder: 23, key: "flatMap" }, PendingName { holder: 23, key: "forEach" }, PendingName { holder: 23, key: "map" },
    PendingName { holder: 23, key: "reduce" }, PendingName { holder: 23, key: "some" }, PendingName { holder: 23, key: "take" }, PendingName { holder: 23, key: "toArray" },
    PendingName { holder: 23, key: "", symbol: 4 }, PendingName { holder: 24, key: "constructor" },
    PendingName { holder: 25, key: "constructor" }, PendingName { holder: 25, key: "then" }, PendingName { holder: 25, key: "catch" }, PendingName { holder: 25, key: "finally" },
    // Array
    PendingName { holder: 14, key: "from" }, PendingName { holder: 14, key: "of" }, PendingName { holder: 14, key: "fromAsync" }, PendingName { holder: 14, key: "", symbol: 8 },
    // The global object's other standard names.
    PendingName { holder: 0, key: "AggregateError" }, PendingName { holder: 0, key: "ArrayBuffer" }, PendingName { holder: 0, key: "Atomics" }, PendingName { holder: 0, key: "BigInt" },
    PendingName { holder: 0, key: "BigInt64Array" }, PendingName { holder: 0, key: "BigUint64Array" }, PendingName { holder: 0, key: "DataView" },
    PendingName { holder: 0, key: "Date" }, PendingName { holder: 0, key: "FinalizationRegistry" }, PendingName { holder: 0, key: "Float32Array" }, PendingName { holder: 0, key: "Float64Array" },
    PendingName { holder: 0, key: "Int8Array" }, PendingName { holder: 0, key: "Int16Array" }, PendingName { holder: 0, key: "Int32Array" }, PendingName { holder: 0, key: "Iterator" },
    PendingName { holder: 0, key: "JSON" }, PendingName { holder: 0, key: "Map" }, PendingName { holder: 0, key: "Math" },
    PendingName { holder: 0, key: "Promise" }, PendingName { holder: 0, key: "RegExp" },
    PendingName { holder: 0, key: "Set" }, PendingName { holder: 0, key: "SharedArrayBuffer" },
    PendingName { holder: 0, key: "Uint8Array" }, PendingName { holder: 0, key: "Uint8ClampedArray" }, PendingName { holder: 0, key: "Uint16Array" }, PendingName { holder: 0, key: "Uint32Array" },
    PendingName { holder: 0, key: "WeakMap" }, PendingName { holder: 0, key: "WeakRef" }, PendingName { holder: 0, key: "WeakSet" }, PendingName { holder: 0, key: "decodeURI" },
    PendingName { holder: 0, key: "decodeURIComponent" }, PendingName { holder: 0, key: "encodeURI" }, PendingName { holder: 0, key: "encodeURIComponent" }, PendingName { holder: 0, key: "escape" },
    PendingName { holder: 0, key: "isFinite" }, PendingName { holder: 0, key: "isNaN" }, PendingName { holder: 0, key: "parseFloat" },
    PendingName { holder: 0, key: "parseInt" }, PendingName { holder: 0, key: "unescape" },
    // Number and %Number.prototype% (light-cljs-832; the core-builtins
    // area makes them, light-cljs-767).
    PendingName { holder: 29, key: "parseInt" }, PendingName { holder: 29, key: "parseFloat" }, PendingName { holder: 29, key: "isNaN" }, PendingName { holder: 29, key: "isFinite" },
    PendingName { holder: 29, key: "isInteger" }, PendingName { holder: 29, key: "isSafeInteger" }, PendingName { holder: 29, key: "MAX_VALUE" }, PendingName { holder: 29, key: "MIN_VALUE" },
    PendingName { holder: 29, key: "NaN" }, PendingName { holder: 29, key: "NEGATIVE_INFINITY" }, PendingName { holder: 29, key: "POSITIVE_INFINITY" }, PendingName { holder: 29, key: "EPSILON" },
    PendingName { holder: 29, key: "MAX_SAFE_INTEGER" }, PendingName { holder: 29, key: "MIN_SAFE_INTEGER" },
    PendingName { holder: 26, key: "toExponential" }, PendingName { holder: 26, key: "toFixed" }, PendingName { holder: 26, key: "toPrecision" }, PendingName { holder: 26, key: "toLocaleString" },
    // String and %String.prototype%.
    PendingName { holder: 30, key: "fromCharCode" }, PendingName { holder: 30, key: "fromCodePoint" }, PendingName { holder: 30, key: "raw" },
    PendingName { holder: 27, key: "at" }, PendingName { holder: 27, key: "charCodeAt" }, PendingName { holder: 27, key: "charAt" }, PendingName { holder: 27, key: "concat" },
    PendingName { holder: 27, key: "codePointAt" }, PendingName { holder: 27, key: "isWellFormed" }, PendingName { holder: 27, key: "toWellFormed" }, PendingName { holder: 27, key: "indexOf" },
    PendingName { holder: 27, key: "lastIndexOf" }, PendingName { holder: 27, key: "includes" }, PendingName { holder: 27, key: "endsWith" }, PendingName { holder: 27, key: "startsWith" },
    PendingName { holder: 27, key: "match" }, PendingName { holder: 27, key: "matchAll" }, PendingName { holder: 27, key: "search" }, PendingName { holder: 27, key: "split" },
    PendingName { holder: 27, key: "substring" }, PendingName { holder: 27, key: "substr" }, PendingName { holder: 27, key: "slice" }, PendingName { holder: 27, key: "repeat" },
    PendingName { holder: 27, key: "replace" }, PendingName { holder: 27, key: "replaceAll" }, PendingName { holder: 27, key: "padEnd" }, PendingName { holder: 27, key: "padStart" },
    PendingName { holder: 27, key: "trim" }, PendingName { holder: 27, key: "trimEnd" }, PendingName { holder: 27, key: "trimRight" }, PendingName { holder: 27, key: "trimStart" },
    PendingName { holder: 27, key: "trimLeft" }, PendingName { holder: 27, key: "toLowerCase" }, PendingName { holder: 27, key: "toUpperCase" }, PendingName { holder: 27, key: "toLocaleLowerCase" },
    PendingName { holder: 27, key: "toLocaleUpperCase" }, PendingName { holder: 27, key: "normalize" }, PendingName { holder: 27, key: "localeCompare" }, PendingName { holder: 27, key: "anchor" },
    PendingName { holder: 27, key: "big" }, PendingName { holder: 27, key: "blink" }, PendingName { holder: 27, key: "bold" }, PendingName { holder: 27, key: "fixed" },
    PendingName { holder: 27, key: "fontcolor" }, PendingName { holder: 27, key: "fontsize" }, PendingName { holder: 27, key: "italics" }, PendingName { holder: 27, key: "link" },
    PendingName { holder: 27, key: "small" }, PendingName { holder: 27, key: "strike" }, PendingName { holder: 27, key: "sub" }, PendingName { holder: 27, key: "sup" },
];
// The type names `typeof` gives, interned with the realm.
let TYPE_NAMES: Name[8] = [Name { text: "undefined" }, Name { text: "object" }, Name { text: "boolean" }, Name { text: "number" }, Name { text: "string" }, Name { text: "symbol" }, Name { text: "bigint" }, Name { text: "function" }];
// The light-cljs runtime leaves boot_runtime installs on `rt` (core false)
// or `rt.core`, in this order.
struct Leaf { core: bool, key: u8[..24], which: Builtin }
let RUNTIME_LEAVES: Leaf[43] = [
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
    // Atoms, volatiles and cells (cells.goose), by their cljs.core runtime names.
    Leaf { core: true, key: "atom", which: Builtin.Atom }, Leaf { core: true, key: "deref", which: Builtin.Deref },
    Leaf { core: true, key: "reset", which: Builtin.Reset }, Leaf { core: true, key: "swap", which: Builtin.Swap },
    Leaf { core: true, key: "compareAndSet", which: Builtin.CompareAndSet }, Leaf { core: true, key: "swapVals", which: Builtin.SwapVals },
    Leaf { core: true, key: "resetVals", which: Builtin.ResetVals }, Leaf { core: true, key: "volatile", which: Builtin.Volatile },
    Leaf { core: true, key: "vreset", which: Builtin.Vreset }, Leaf { core: true, key: "addWatch", which: Builtin.AddWatch },
    Leaf { core: true, key: "removeWatch", which: Builtin.RemoveWatch }, Leaf { core: true, key: "setValidator", which: Builtin.SetValidator },
    // -deref and -vreset!, which vswap! expands to.
    Leaf { core: true, key: "derefProtocol", which: Builtin.Deref }, Leaf { core: true, key: "vresetProtocol", which: Builtin.Vreset },
    // Delays (cells.goose): what the compiler emits for (delay ...), new
    // rt.core.Delay(thunk, null), force, realized? and delay?.
    Leaf { core: true, key: "Delay", which: Builtin.Delay }, Leaf { core: true, key: "force", which: Builtin.Force },
    Leaf { core: true, key: "realized", which: Builtin.Realized }, Leaf { core: true, key: "isDelay", which: Builtin.IsDelay },
    // What a native compile-time unit emits for (C. args) when C is no
    // struct type: only a constructor whose construction is its call.
    Leaf { core: false, key: "nativeConstruct", which: Builtin.NativeConstruct },
    // Compiled interop's receiver and operand checks and the Object
    // declaration demand of a member read (light-cljs-872).
    Leaf { core: false, key: "nativeInterop", which: Builtin.NativeInterop }, Leaf { core: false, key: "declarationReceiver", which: Builtin.DeclarationReceiver },
    // ex-info and its readers, and get-in (light-cljs-872). The
    // ExceptionInfo function is no leaf: its prototype is a realm row
    // (exception_info_prototype_row), which no leaf may reach, as a helper
    // publication's graph walk from `rt` must meet no cycle of objects
    // (callable.goose graph).
    Leaf { core: true, key: "exInfo", which: Builtin.ExInfo }, Leaf { core: true, key: "exData", which: Builtin.ExData },
    Leaf { core: true, key: "exMessage", which: Builtin.ExMessage }, Leaf { core: true, key: "exCause", which: Builtin.ExCause },
    Leaf { core: true, key: "getIn", which: Builtin.GetIn },
];

// A built-in's code: its position in BUILTINS.
fn code(b: Builtin) -> u32 {
    for e, i in BUILTINS { if e.which == b { return i as u32; } }
    abort("builtin is not in BUILTINS"); return 0;
}
inline fn builtin_count() -> i64 { return BUILTINS.len; }
// The Builtin of built-in function object o (node_kind::Builtin); its code was checked
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
    if k.is_string() { append_utf16(job.heap, k, job.detail); } else { for c in "@@" { job.detail.push(c); } for c in export_well_known_symbol(symbol_index(k)) { job.detail.push(c); } }
}
// The [[Prototype]] of node o: its own link, or for Empty, the realm's
// default prototype for its kind (Object.prototype for ordinary objects,
// errors and arguments objects, Array.prototype for arrays,
// Function.prototype for ordinary closures and built-in functions, and
// %GeneratorFunction.prototype% or a sibling for generator and async
// closures, light-cljs-857). Forms and the
// other runtime kinds have none, and so does every node of a heap built
// without initialize (hand-made test heaps, whose global is not set).
fn prototype(heap: const Heap&, o: i64) -> Value {
    let p = node(heap, o).proto;
    if !p.is_hole() { return p; }
    if !heap.global.is_object() { return Value.Null; }
    return match node(heap, o).kind {
        node_kind::Object, node_kind::Arguments => object(object_prototype_row()), node_kind::Array => object(array_prototype_row()), node_kind::Builtin, node_kind::BoundFunction => object(function_prototype_row()), node_kind::Error => object(error_prototype_row(0)),
        node_kind::Closure => { let k = function_kind_of(heap, o); object(if k < 0 { function_prototype_row() } else { function_kind_prototype_row(k) }) },
        node_kind::Wrapper => primitive_prototype(node(heap, o).value),
        _ => Value.Null,
    };
}
// The wrapper type (WRAPPER_NAMES) of primitive v, -1 for one without a
// wrapper here: undefined and null have none, and Symbol and BigInt
// wrappers belong to their areas (light-cljs-766, light-cljs-771).
fn wrapper_type(v: Value) -> i64 { return match v { Number => 0, String => 1, Bool => 2, _ => -1 }; }
// The realm's prototype for primitive v's wrapper (ToObject's), Null for a
// primitive without one.
fn primitive_prototype(v: Value) -> Value { let t = wrapper_type(v); return if t < 0 { Value.Null } else { object(wrapper_prototype_row(t)) }; }
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
    if nodes.len < realm_nodes() || accessors.len < 1 || !accessors[0].get.is_undefined() || !accessors[0].set.is_undefined() || !global.is_object() || global.id() != global_row() { return false; }
    let kinds: i64[] = [node_kind::Object, node_kind::Object, node_kind::Builtin, node_kind::Array];
    for k, o in kinds { if nodes[o].kind != k { return false; } }
    if !nodes[object_prototype_row()].proto.is_null() || nodes[function_prototype_row()].function != code(Builtin.FunctionPrototype) { return false; }
    for t in ERROR_NAMES.len {
        if nodes[error_prototype_row(t)].kind != node_kind::Object || nodes[error_constructor_row(t)].kind != node_kind::Builtin || nodes[error_constructor_row(t)].function != code(ERROR_BUILTINS[t]) { return false; }
    }
    let constructors: Builtin[] = [Builtin.Object, Builtin.Function, Builtin.Array];
    for b, i in constructors { let o = object_constructor_row() + i; if nodes[o].kind != node_kind::Builtin || nodes[o].function != code(b) { return false; } }
    // The wrapper prototypes hold a primitive of their type.
    for b, t in WRAPPER_BUILTINS {
        let p .= nodes[wrapper_prototype_row(t)]; let c .= nodes[wrapper_constructor_row(t)];
        if p.kind != node_kind::Wrapper || wrapper_type(p.value) != t || !p.proto.is_object() || p.proto.id() != object_prototype_row() || c.kind != node_kind::Builtin || c.function != code(b) { return false; }
    }
    // The iterator prototypes inherit %IteratorPrototype%.
    let iterator_prototypes: i64[] = [array_iterator_prototype_row(), string_iterator_prototype_row()];
    for o in iterator_prototypes { if nodes[o].kind != node_kind::Object || !nodes[o].proto.is_object() || nodes[o].proto.id() != iterator_prototype_row() { return false; } }
    // Reflect is an ordinary object (light-cljs-851).
    if nodes[reflect_row()].kind != node_kind::Object || !nodes[reflect_row()].proto.is_object() || nodes[reflect_row()].proto.id() != object_prototype_row() { return false; }
    // The arguments templates (light-cljs-854): shape-mode, three properties,
    // no elements, `length` 0 and @@iterator %Array.prototype.values%.
    for mapped in [false, true] {
        let t .= nodes[arguments_template_row(mapped)];
        if t.kind != node_kind::Arguments || t.initialized || t.shape == 0 || t.key_count != 3 || t.count != 0 || !t.proto.is_hole() || !same_bits(t.slots[0], number(0)) || !t.slots[1].is_object() || t.slots[1].id() != realm_function_row(Builtin.ArrayPrototypeValues) { return false; }
    }
    // Array.prototype[@@unscopables] (light-cljs-855): an ordinary object.
    // Its prototype and properties are JavaScript's to change (an unprotected
    // job may delete or add names), so only its kind is layout.
    if nodes[array_unscopables_row()].kind != node_kind::Object { return false; }
    // %GeneratorFunction.prototype% and its siblings (light-cljs-857).
    for k in FUNCTION_KIND_TAGS.len { let n .= nodes[function_kind_prototype_row(k)]; if n.kind != node_kind::Object || !n.proto.is_object() || n.proto.id() != function_prototype_row() { return false; } }
    for m, j in REALM_FUNCTIONS { let o = fixed_rows() + j; if nodes[o].kind != node_kind::Builtin || nodes[o].function != code(m.which) || m.which == Builtin.FunctionPrototypeCall && (!call.is_object() || call.id() != o) { return false; } }
    // ExceptionInfo.prototype (light-cljs-872).
    let e .= nodes[exception_info_prototype_row()];
    if e.kind != node_kind::Object || !e.proto.is_object() || e.proto.id() != error_prototype_row(0) { return false; }
    return true;
}

// Build the realm into an empty heap (initialize). A rejection
// (MemoryLimit, WorkLimit) leaves the heap unusable, as any failed
// initialize does.
fn build_realm(heap: Heap&) -> bool {
    if heap.nodes.len != 0 || heap.accessors.len != 0 { heap.fault = Fault.InvalidState; return false; }
    if add_accessor(heap, Pair {}) != 0 { return false; }
    // %String.prototype%'s [[StringData]], the empty string.
    let empty = key_named(heap, "");
    if heap.fault != Fault.None { return false; }
    var rows: Node[>..] = [];
    rows.push(Node { kind: node_kind::Object });
    rows.push(Node { kind: node_kind::Object, proto: Value.Null });
    rows.push(Node { kind: node_kind::Builtin, function: code(Builtin.FunctionPrototype), proto: object(object_prototype_row()) });
    rows.push(Node { kind: node_kind::Array, proto: object(object_prototype_row()) });
    for t in ERROR_NAMES.len { rows.push(Node { kind: node_kind::Object, proto: object(if t == 0 { object_prototype_row() } else { error_prototype_row(0) }) }); }
    rows.push(Node { kind: node_kind::Builtin, function: code(Builtin.Object) }); rows.push(Node { kind: node_kind::Builtin, function: code(Builtin.Function) }); rows.push(Node { kind: node_kind::Builtin, function: code(Builtin.Array) });
    for b, t in ERROR_BUILTINS { rows.push(if t == 0 { Node { kind: node_kind::Builtin, function: code(b) } } else { Node { kind: node_kind::Builtin, function: code(b), proto: object(error_constructor_row(0)) } }); }
    rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) }); rows.push(Node { kind: node_kind::Object, proto: object(iterator_prototype_row()) }); rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) });
    let wrapped: Value[3] = [number(0), empty, boolean(false)];
    for v in wrapped { rows.push(Node { kind: node_kind::Wrapper, value: v, proto: object(object_prototype_row()) }); }
    for b in WRAPPER_BUILTINS { rows.push(Node { kind: node_kind::Builtin, function: code(b) }); }
    rows.push(Node { kind: node_kind::Object, proto: object(iterator_prototype_row()) }); rows.push(Node { kind: node_kind::Object, proto: object(iterator_prototype_row()) });
    rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) });
    rows.push(Node { kind: node_kind::Arguments }); rows.push(Node { kind: node_kind::Arguments });
    rows.push(Node { kind: node_kind::Object, proto: Value.Null });
    for k in FUNCTION_KIND_TAGS.len { rows.push(Node { kind: node_kind::Object, proto: object(function_prototype_row()) }); }
    assert(rows.len == fixed_rows());
    for m in REALM_FUNCTIONS { rows.push(Node { kind: node_kind::Builtin, function: code(m.which) }); }
    rows.push(Node { kind: node_kind::Object, proto: object(error_prototype_row(0)) });
    if !charge(heap, rows.len) || add_nodes(heap, rows) != 0 { if heap.fault == Fault.None { memory(heap); } return false; }
    heap.global = object(global_row());
    // Every built-in function's `length` and `name`.
    for o in rows.len { if rows[o].kind == node_kind::Builtin && !builtin_metadata(heap, o) { return false; } }
    // Constructors and their prototypes.
    let pairs: i64[] = [object_constructor_row(), object_prototype_row(), function_constructor_row(), function_prototype_row(), array_constructor_row(), array_prototype_row()];
    var i: i64 = 0;
    while i < pairs.len { if !link_constructor(heap, pairs[i], pairs[i + 1]) { return false; } i += 2; }
    for t in WRAPPER_NAMES.len { if !link_constructor(heap, wrapper_constructor_row(t), wrapper_prototype_row(t)) { return false; } }
    for t in ERROR_NAMES.len {
        let p = error_prototype_row(t);
        if !link_constructor(heap, error_constructor_row(t), p) || !realm_value(heap, p, "name", key_named(heap, ERROR_NAMES[t].text), attr_writable() | attr_configurable()) || !realm_value(heap, p, "message", key_named(heap, ""), attr_writable() | attr_configurable()) { return false; }
    }
    var thrower_pair = Value.Undefined;
    for m, j in REALM_FUNCTIONS {
        if m.which == Builtin.IteratorPrototypeIterator || m.which == Builtin.StringPrototypeIterator { if !add_own(heap, m.holder, Value.Symbol { s: well_known::Iterator }, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; } continue; }
        // Non-writable, non-enumerable, non-configurable (ECMAScript 20.2.3.6).
        if m.which == Builtin.FunctionPrototypeHasInstance { if !add_own(heap, m.holder, Value.Symbol { s: well_known::HasInstance }, object(fixed_rows() + j), 0) { return false; } continue; }
        // Function.prototype.caller and .arguments: one accessor pair with
        // %ThrowTypeError% as getter and setter, configurable, not
        // enumerable (QuickJS's JS_AddIntrinsicBaseObjects).
        if m.which == Builtin.ThrowTypeError {
            let thrower = object(fixed_rows() + j);
            let a = add_accessor(heap, Pair { get: thrower, set: thrower });
            if a < 0 { if heap.fault == Fault.None { memory(heap); } return false; }
            thrower_pair = Value.Accessor { a: a as u32 };
            let names: Name[2] = [Name { text: "caller" }, Name { text: "arguments" }];
            for n in names { let k = key_named(heap, n.text); if heap.fault != Fault.None || !add_own(heap, m.holder, k, Value.Accessor { a: a as u32 }, attr_configurable()) { return false; } }
            continue;
        }
        if !realm_value(heap, m.holder, BUILTINS[code(m.which)].name, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; }
        if m.which == Builtin.FunctionPrototypeCall { heap.call_method = object(fixed_rows() + j); }
        // Array.prototype[@@iterator] is the `values` function object (ECMAScript 23.1.3.40).
        if m.which == Builtin.ArrayPrototypeValues && !add_own(heap, m.holder, Value.Symbol { s: well_known::Iterator }, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; }
    }
    // The arguments object templates: `length` 0 and @@iterator (writable,
    // configurable), then `callee`: the %ThrowTypeError% pair (neither
    // enumerable nor configurable) or a writable, configurable data property
    // (QuickJS's js_build_arguments and js_build_mapped_arguments).
    let callee = key_named(heap, "callee"); let length = key_named(heap, "length");
    if heap.fault != Fault.None || !is_accessor(thrower_pair) { if heap.fault == Fault.None { heap.fault = Fault.InvalidState; } return false; }
    for mapped in [false, true] {
        let t = arguments_template_row(mapped);
        if !add_own(heap, t, length, number(0), attr_writable() | attr_configurable()) || !add_own(heap, t, Value.Symbol { s: well_known::Iterator }, object(realm_function_row(Builtin.ArrayPrototypeValues)), attr_writable() | attr_configurable()) { return false; }
        if !add_own(heap, t, callee, if mapped { Value.Undefined } else { thrower_pair }, if mapped { attr_writable() | attr_configurable() } else { 0 }) { return false; }
    }
    // Array.prototype[@@unscopables]: each name a writable, enumerable,
    // configurable `true` (CreateDataPropertyOrThrow); the property itself
    // configurable only (ECMAScript 23.1.3.41).
    for n in UNSCOPABLE_NAMES { if !realm_value(heap, array_unscopables_row(), n.text, boolean(true), attr_writable() | attr_enumerable() | attr_configurable()) { return false; } }
    if !add_own(heap, array_prototype_row(), Value.Symbol { s: well_known::Unscopables }, object(array_unscopables_row()), attr_configurable()) { return false; }
    // %ArrayIteratorPrototype%[@@toStringTag] and %StringIteratorPrototype%[@@toStringTag].
    if !add_own(heap, array_iterator_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Array Iterator"), attr_configurable()) || !add_own(heap, string_iterator_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "String Iterator"), attr_configurable()) { return false; }
    // %GeneratorPrototype%[@@toStringTag] and %Promise.prototype%[@@toStringTag].
    if !add_own(heap, generator_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Generator"), attr_configurable()) || !add_own(heap, promise_prototype_row(), Value.Symbol { s: 4 }, key_named(heap, "Promise"), attr_configurable()) { return false; }
    // %GeneratorFunction.prototype% and its siblings: @@toStringTag
    // (ECMAScript 27.3.3.3 and its siblings), and for generators `prototype`
    // = %GeneratorPrototype% (27.3.3.2), configurable only.
    for t, k in FUNCTION_KIND_TAGS { if !add_own(heap, function_kind_prototype_row(k), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, t.text), attr_configurable()) { return false; } }
    if !realm_value(heap, function_kind_prototype_row(function_kind::Generator), "prototype", object(generator_prototype_row()), attr_configurable()) { return false; }
    // Reflect[@@toStringTag] (ECMAScript 28.1.14).
    if !add_own(heap, reflect_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Reflect"), attr_configurable()) { return false; }
    // The well-known symbols, Symbol's non-writable, non-enumerable,
    // non-configurable data properties; their values are the exporter's
    // fixed indices (decision 2), so they take no rows.
    assert(REALM_FUNCTIONS[symbol_row() - fixed_rows()].which == Builtin.SymbolFunction);
    var s: i64 = 0;
    while export_well_known_symbol(s).len > 0 { if !realm_value(heap, symbol_row(), export_well_known_symbol(s), Value.Symbol { s: s as u32 }, 0) { return false; } s += 1; }
    // The global object's own properties.
    if !realm_value(heap, global_row(), "globalThis", heap.global, attr_writable() | attr_configurable()) { return false; }
    let globals: i64[] = [object_constructor_row(), function_constructor_row(), array_constructor_row(), wrapper_constructor_row(0), wrapper_constructor_row(1), wrapper_constructor_row(2)];
    for o in globals { if !realm_value(heap, global_row(), BUILTINS[node(heap, o).function].name, object(o), attr_writable() | attr_configurable()) { return false; } }
    if !realm_value(heap, global_row(), "Reflect", object(reflect_row()), attr_writable() | attr_configurable()) { return false; }
    for t in ERROR_NAMES.len { if !realm_value(heap, global_row(), ERROR_NAMES[t].text, object(error_constructor_row(t)), attr_writable() | attr_configurable()) { return false; } }
    if !realm_value(heap, global_row(), "NaN", number(0.0 / 0.0), 0) || !realm_value(heap, global_row(), "Infinity", number(1.0 / 0.0), 0) || !realm_value(heap, global_row(), "undefined", Value.Undefined, 0) { return false; }
    for p in PENDING {
        // Symbol's pending names sit on symbol_row(), which moves when a
        // REALM_FUNCTIONS entry is added before it.
        assert(p.key != "keyFor" || p.holder == symbol_row());
        let k: Value = if p.symbol >= 0 { Value.Symbol { s: p.symbol as u32 } } else { key_named(heap, p.key) };
        if heap.fault != Fault.None || !add_own(heap, p.holder, k, pending(), attr_configurable()) { return false; }
    }
    let exception_info_pending: Name[2] = [Name { text: "constructor" }, Name { text: "toString" }];
    for n in exception_info_pending { let k = key_named(heap, n.text); if heap.fault != Fault.None || !add_own(heap, exception_info_prototype_row(), k, pending(), attr_configurable()) { return false; } }
    for n in TYPE_NAMES { key_named(heap, n.text); }
    if heap.fault != Fault.None { return false; }
    assert(heap.nodes.len == realm_nodes());
    return true;
}
// Built-in function o's `length` and `name` from BUILTINS (configurable
// only; %ThrowTypeError%'s are not configurable either, QuickJS sealing it).
fn builtin_metadata(heap: Heap&, o: i64) -> bool {
    let info = BUILTINS[node(heap, o).function];
    let attrs = if info.which == Builtin.ThrowTypeError { 0 } else { attr_configurable() };
    return realm_value(heap, o, "length", number(info.length), attrs) && realm_value(heap, o, "name", key_named(heap, info.name), attrs);
}
fn realm_value(heap: Heap&, o: i64, name: const u8[:], v: Value, attrs: u8) -> bool {
    let k = key_named(heap, name);
    return heap.fault == Fault.None && v.tag() >= value_tag::Undefined && add_own(heap, o, k, v, attrs);
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
    let rt = allocate(heap, Node { kind: node_kind::Object }); let core = allocate(heap, Node { kind: node_kind::Object });
    install_field(heap, rt, "core", core);
    install_field(heap, heap.global, "__lightNativeRuntime", rt);
    for leaf in RUNTIME_LEAVES {
        let f = allocate(heap, Node { kind: node_kind::Builtin, function: code(leaf.which) });
        if !f.is_object() || !builtin_metadata(heap, f.id()) { return Value.Undefined; }
        install_field(heap, if leaf.core { core } else { rt }, leaf.key, f);
    }
    if heap.fault == Fault.None { heap.boot_count += 1; }
    return rt;
}

// The error type (ERROR_NAMES) a catchable fault raises.
fn fault_type(fault: Fault) -> i64 { return match fault { TypeError => 5, ReferenceError => 3, RangeError => 2, SyntaxError => 4, StackLimit => 7, _ => 0 }; }
// An error object (node_kind::Error): `code` is error_code of the VM fault it was
// raised for, 0 for one JavaScript made; its prototype is its type's. A
// fault's error also keeps its message in the node's value, out of
// JavaScript's reach, for the report if it is rethrown uncaught
// (source_vm.goose uncaught).
fn error_object(heap: Heap&, t: i64, code: u32, message: Value) -> Value {
    let e = allocate(heap, Node { kind: node_kind::Error, function: code, value: if code != 0 && message.is_string() { message } else { Value.Undefined }, proto: object(error_prototype_row(t)) });
    if heap.fault != Fault.None { return Value.Undefined; }
    if message.is_string() && !add_own(heap, e.id(), key_named(heap, "message"), message, attr_writable() | attr_configurable()) { return Value.Undefined; }
    return e;
}
// The message of a VM fault's error: Job.detail when the operation named
// one, else the QuickJS wording for the instruction that faulted. A
// property read, write, delete, ToPropertyKey or RequireCoercible on an
// undefined or null base words its own message (source_vm.goose
// base_error, light-cljs-871), naming the key; a refused strict write keeps
// "cannot set property" (QuickJS's "'x' is read-only" and "no setter for
// property" and the Pants engine's wordings all differ). The cljs.core
// form operations raise cljs.core's Errors (source_vm.goose CoreError,
// light-cljs-873), not TypeErrors. "invalid operand" is left to the
// instructions no JavaScript source makes fault this way: the compiler's
// own form operations (formNth, formWithMeta and the like) on operands the
// emitted code never gives them, a literal's AppendHole, Append,
// CheckSpread, SetProto and DefineField on a value the bytecode compiler
// never gives them.
fn fault_message(job: Job&, fault: Fault, frame: Frame) -> Value {
    let heap .= job.heap;
    if job.detail.len > 0 { return string(heap, job.detail); }
    let code .= heap.code;
    let op = if frame.function >= 0 && frame.pc >= 0 && frame.pc < code.ops.len { code.ops[frame.pc].code } else { -1 };
    return text(heap, fault_words(fault, op));
}
// fault_message's wording for a fault without Job.detail at opcode `op`
// (-1 for none), shared with the uncaught report (describe_uncaught).
fn fault_words(fault: Fault, op: i64) -> const u8[:] {
    return match fault {
        StackLimit => "stack overflow",
        RangeError => "invalid array length",
        ReferenceError => if op == opcode::CellInit || op == opcode::CellGet || op == opcode::CellSet { "lexical variable is not initialized" } else { "variable is not defined" },
        TypeError => match op { opcode::Call, opcode::CallMethod => "not a function", opcode::Construct => "not a constructor", opcode::PutField, opcode::PutElem, opcode::PutFieldStrict, opcode::PutElemStrict => "cannot set property", opcode::In => "invalid 'in' operand", opcode::Plus, opcode::ToString => "Cannot convert a Symbol value to a string",
            // ToNumber of a symbol (light-cljs-827); `+` without a string
            // operand names its conversion in Job.detail (symbol_detail).
            opcode::Neg, opcode::ToNumber, opcode::ToNumeric, opcode::Inc, opcode::Dec, opcode::ToNumberPair, opcode::Sub, opcode::Mul, opcode::Div, opcode::Rem, opcode::Lt, opcode::Gt, opcode::Le, opcode::Ge, opcode::CompareLt, opcode::CompareGt, opcode::CompareLe, opcode::CompareGe => "Cannot convert a Symbol value to a number",
            _ => "invalid operand",
        },
        SyntaxError => "invalid global declaration",
        _ => "host effect failed",
    };
}
// Name an unsupported built-in step (Job.detail) and fail UnsupportedOperand.
// Whether `v` is the realm's %eval% (a direct-eval site's callee check).
fn realm_eval(heap: Heap&, v: Value) -> bool {
    return valid(heap, v) && v.is_object() && node(heap, v.id()).kind == node_kind::Builtin && node(heap, v.id()).function == code(Builtin.Eval);
}
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

// Primitive wrappers (light-cljs-832; the Pants engine's realm.rs and
// vm/builtin.rs, QuickJS's js_number_constructor, js_string_constructor,
// js_boolean_constructor and JS_ToObject). A wrapper is node_kind::Wrapper
// holding its primitive (source_vm.goose, string_key); a job's wrappers are
// job rows (decision 3), the realm's three prototypes image rows.
//
// ToObject(v): an object is itself; a number, string or boolean is a new
// wrapper inheriting its realm prototype; undefined and null are a
// TypeError. A Symbol or BigInt wrapper fails by name: they are the objects
// and BigInt areas' (light-cljs-766, light-cljs-771).
fn to_object(job: Job&, v: Value) -> Value {
    if v.is_object() { return v; }
    if v.tag() <= value_tag::Null { return type_error(job, "cannot convert to object"); }
    return wrap(job, v, primitive_prototype(v));
}
fn wrap(job: Job&, v: Value, proto: Value) -> Value {
    if wrapper_type(v) < 0 { unsupported_detail(job, if v.is_symbol() { "ToObject: Symbol wrapper objects" } else { "ToObject: BigInt wrapper objects" }); return Value.Undefined; }
    return allocate(job.heap, Node { kind: node_kind::Wrapper, value: v, proto: proto });
}
// thisNumberValue, thisStringValue and thisBooleanValue for built-in b: the
// primitive of wrapper type t that `receiver` is or wraps; anything else is
// a TypeError (QuickJS's wording).
fn this_primitive(job: Job&, b: Builtin, t: i64, receiver: Value) -> Value {
    let heap .= job.heap;
    let p = if receiver.is_object() && node(heap, receiver.id()).kind == node_kind::Wrapper { node(heap, receiver.id()).value } else { receiver };
    if wrapper_type(p) == t { return p; }
    return type_error(job, match t { 0 => "not a number", 1 => "not a string", _ => "not a boolean" });
}
// ToNumber of primitive v for built-in b, without user code: a symbol is a
// TypeError, a BigInt (the BigInt area, light-cljs-771) and an object
// (converted before this in a call instruction) fail by name.
fn primitive_number(job: Job&, b: Builtin, v: Value) -> f64 {
    let heap .= job.heap;
    let x = form_value(heap, v);
    if x.is_symbol() { type_error(job, "Cannot convert a Symbol value to a number"); return 0; }
    if x.is_bigint() { unsupported_step(job, b, "a BigInt argument"); return 0; }
    if x.is_object() { unsupported_step(job, b, if convertible(heap, x) { "ToNumber of an object outside a call instruction" } else { "ToNumber of a light-cljs runtime value" }); return 0; }
    return numeric(heap, x);
}
// The primitive Number(v), String(v) or Boolean(v) gives (WRAPPER_NAMES
// type t), and so the one new Number(v), new String(v) or new Boolean(v)
// wraps: Number is ToNumeric then Number (+0 without an argument); String
// is ToString ("" without one), but a call of it gives a symbol's
// SymbolDescriptiveString where a construction throws; Boolean is
// ToBoolean. An object argument of Number or String converted before this
// (argument_converts); one that reaches here fails by name.
fn wrapper_value(job: Job&, b: Builtin, t: i64, args: const Value[:], construct: bool) -> Value {
    let heap .= job.heap;
    let v = form_value(heap, arg(args, 0));
    if t == 2 { return boolean(truthy(heap, v)); }
    if t == 0 {
        if args.len == 0 { return number(0); }
        let n = primitive_number(job, b, v);
        return if heap.fault == Fault.None { number(n) } else { Value.Undefined };
    }
    if args.len == 0 { return key_named(heap, ""); }
    if v.is_symbol() && !construct {
        var units: u8[>..] = []; units.append("Symbol(Symbol."); units.append(export_well_known_symbol(symbol_index(v))); units.push(41);
        return text(heap, units);
    }
    if v.is_bigint() { return unsupported_step(job, b, "a BigInt argument"); }
    if v.is_object() { return unsupported_step(job, b, if convertible(heap, v) { "ToString of an object outside a call instruction" } else { "ToString of a light-cljs runtime value" }); }
    if v.is_symbol() { return type_error(job, "Cannot convert a Symbol value to a string"); }
    return to_string(heap, v);
}
// new Number(v), new String(v), new Boolean(v): a wrapper of their call's
// primitive. new.target is the constructor itself (no subclass or
// Reflect.construct reaches here yet), so the prototype is the realm's.
fn construct_wrapper(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    var t: i64 = 0; for w, i in WRAPPER_BUILTINS { if w == b { t = i; } }
    let v = wrapper_value(job, b, t, args, true);
    if job.heap.fault != Fault.None { return Value.Undefined; }
    return wrap(job, v, object(wrapper_prototype_row(t)));
}
// Number.prototype.toString(radix): radix 10, or undefined, is the number's
// ToString; a radix outside 2 to 36 is a RangeError and another one fails
// by name (bbp-149, the core-builtins area, light-cljs-767).
fn number_to_string(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let x = this_primitive(job, b, 0, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    let r = form_value(heap, arg(args, 0));
    if !r.is_undefined() {
        if r.is_object() { return unsupported_step(job, b, "an object radix"); }
        let n = primitive_number(job, b, r);
        if heap.fault != Fault.None { return Value.Undefined; }
        // ToIntegerOrInfinity: NaN is 0.
        let radix = if n != n { 0.0 } else { trunc(n) };
        if radix < 2 || radix > 36 { job.detail.clear(); for c in "radix must be between 2 and 36" { job.detail.push(c); } heap.fault = Fault.RangeError; return Value.Undefined; }
        if radix != 10 { return unsupported_step(job, b, "a radix other than 10"); }
    }
    return to_string(heap, x);
}
// ToString without user code: a primitive's (source_vm.goose to_string,
// light-cljs-826; a symbol is a TypeError). An object's ToPrimitive is not
// run here: it fails UnsupportedOperand, naming built-in b.
fn primitive_string(job: Job&, b: Builtin, v: Value) -> Value {
    if v.is_object() { return unsupported_step(job, b, "ToString of an object"); }
    if v.is_symbol() { return type_error(job, "Cannot convert a Symbol value to a string"); }
    return to_string(job.heap, v);
}
// ToPropertyKey without user code (an object's ToPrimitive is not run here).
fn property_key(job: Job&, b: Builtin, v: Value) -> Value {
    let k = if v.is_object() { primitive_string(job, b, v) } else { key(job.heap, v) };
    return if job.heap.fault == Fault.None { intern(job.heap, k) } else { Value.Undefined };
}
// Whether ToObject(o) has own property k (interned): strings their indices
// and length, numbers and booleans nothing, objects by kind.
fn has_own(job: Job&, b: Builtin, o: Value, k: Value) -> bool {
    let heap .= job.heap;
    if o.is_string() { if key_is(heap, k, "length") { return true; } let i = index(heap, k); return i >= 0 && i < str_len(heap, o); }
    if !o.is_object() { return false; }
    if lazy_prototype(heap, o.id()) && key_is(heap, k, "prototype") { return true; }
    return own_entry(heap, o, k).found;
}
// Object.getOwnPropertyDescriptor(o, key) (light-cljs-806): undefined for
// no own property `key`, otherwise a new ordinary object
// (FromPropertyDescriptor) with `value` and `writable`, or `get` and `set`,
// then `enumerable` and `configurable`. An ordinary function's own lazy
// `prototype` is made first, as a read makes it (prepare_function); an
// inherited one is not looked at. A primitive o is its ToObject wrapper (a
// string's `length` and indices; light-cljs-832); a Symbol's and a pending
// property fail by name.
fn own_descriptor(job: Job&, b: Builtin, value: Value, kv: Value) -> Value {
    let heap .= job.heap;
    if value.tag() <= value_tag::Null { return type_error(job, "cannot convert to object"); }
    let k = property_key(job, b, kv);
    if heap.fault != Fault.None { return Value.Undefined; }
    let o = to_object(job, value);
    if heap.fault != Fault.None { return Value.Undefined; }
    if lazy_prototype(heap, o.id()) && key_is(heap, k, "prototype") { prepare_function(job, o, k, false); }
    let p = own_entry(heap, o, k);
    if heap.fault != Fault.None || !p.found { return Value.Undefined; }
    if is_pending(p.value) { pending_fault(heap, k); return Value.Undefined; }
    let d = allocate(heap, Node { kind: node_kind::Object });
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
    let core = if rt.is_object() { data_get(job, b, rt, "core") } else { Value.Undefined };
    if heap.fault != Fault.None { return Value.Undefined; }
    if !core.is_object() { heap.fault = Fault.InvalidState; return Value.Undefined; }
    let d = if name.is_string() { own_descriptor(job, b, core, name) } else { Value.Undefined };
    if heap.fault != Fault.None || d.is_object() { return d; }
    job.detail.clear();
    for c in BUILTINS[code(b)].name { job.detail.push(c); }
    for c in ": no core leaf " { job.detail.push(c); }
    if name.is_string() { append_utf16(heap, name, job.detail); }
    heap.fault = Fault.NativeBoundary;
    return Value.Undefined;
}
// The runtime leaves for interop and errors (light-cljs-872), as the V8
// build's native macro runtime (src/bbp_runtime.js) defines them. An Error
// that runtime throws is a catchable HostEffect fault here: an error object
// of type Error whose message is Job.detail.
fn runtime_error(job: Job&, before: const u8[:], after: const u16[:]) -> Value {
    job.detail.clear(); for c in before { job.detail.push(c); } job.detail.append(after);
    if job.heap.fault == Fault.None { job.heap.fault = Fault.HostEffect; }
    return Value.Undefined;
}
// `Invalid arity: n`, the runtime's `arity(n)`.
fn arity_error(job: Job&, n: i64) -> Value {
    var digits: u16[>..] = []; var m = n;
    loop { digits.push((48 + m % 10) as u16); m = m / 10; if m == 0 { break; } }
    digits.reverse();
    return runtime_error(job, "Invalid arity: ", digits);
}
// Whether x has p on its prototype chain: `x instanceof C` for a C whose
// @@hasInstance is %Function.prototype%'s and whose prototype is p. A
// light-cljs runtime value (a form, atom or delay) is no instance; a
// proxy's [[GetPrototypeOf]] fails by name (proxy.goose).
fn inherits(job: Job&, x: Value, p: Value) -> bool {
    let heap .= job.heap;
    if !p.is_object() || !instance_kind(heap, x) { return false; }
    var at = x;
    loop {
        if node(heap, at.id()).kind == node_kind::Proxy { proxy_fault(heap, proxy_method::GetPrototypeOf); return false; }
        let q = prototype(heap, at.id());
        if !q.is_object() { return false; }
        if q.id() == p.id() { return true; }
        if !charge(heap) || !instance_kind(heap, q) { return false; }
        at = q;
    }
}
// exData, exMessage and exCause: `val(x) instanceof T ? val(x).field : null`,
// T being ExceptionInfo, or Error for exMessage. A getter on the way
// fails by name (data_get).
fn ex_field(job: Job&, b: Builtin, v: Value) -> Value {
    let heap .= job.heap;
    let x = form_value(heap, v);
    if heap.fault != Fault.None { return Value.Undefined; }
    let p = object(if b == Builtin.ExMessage { error_prototype_row(0) } else { exception_info_prototype_row() });
    if !inherits(job, x, p) { return if heap.fault == Fault.None { nil() } else { Value.Undefined }; }
    return data_get(job, b, x, if b == Builtin.ExData { "data" } else if b == Builtin.ExCause { "cause" } else { "message" });
}
// The fields the V8 build's ExceptionInfo copies from `new Error(message)`
// after message, data and cause. An Error of this realm has none of them
// own, so each is read from %Error.prototype% and its chain.
let COPIED_ERROR_FIELDS: Name[7] = [Name { text: "name" }, Name { text: "description" }, Name { text: "number" }, Name { text: "fileName" }, Name { text: "lineNumber" }, Name { text: "columnNumber" }, Name { text: "stack" }];
// rt.core.exInfo(message, data, cause?) (cljs.core/ex-info): `new
// ExceptionInfo(...)`, an ordinary object inheriting ExceptionInfo.prototype
// whose own data properties are message (as given), data, cause (null
// unless given) and the COPIED_ERROR_FIELDS. The message's ToString, which
// `new Error` runs, runs no JavaScript here (primitive_string).
fn ex_info(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if args.len != 2 && args.len != 3 { return arity_error(job, args.len); }
    let m = form_value(heap, args[0]);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !m.is_undefined() { primitive_string(job, b, m); if heap.fault != Fault.None { return Value.Undefined; } }
    let e = allocate(heap, Node { kind: node_kind::Object, proto: object(exception_info_prototype_row()) });
    if heap.fault != Fault.None { return Value.Undefined; }
    let fields: Value[3] = [args[0], args[1], if args.len == 3 { args[2] } else { nil() }];
    let names:  Name[3]  = [Name { text: "message" }, Name { text: "data" }, Name { text: "cause" }];
    for v, i in fields { if !realm_value(heap, e.id(), names[i].text, v, attrs_data()) { return Value.Undefined; } }
    for f in COPIED_ERROR_FIELDS {
        let v = data_get(job, b, object(error_prototype_row(0)), f.text);
        if heap.fault != Fault.None || !realm_value(heap, e.id(), f.text, v, attrs_data()) { return Value.Undefined; }
    }
    return e;
}
// rt.core.getIn(m, ks, not-found?) (cljs.core/get-in): `get` along the
// path's elements; with not-found, a step that misses returns it.
fn get_in(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if args.len != 2 && args.len != 3 { return arity_error(job, args.len); }
    let path = form_value(heap, args[1]);
    if heap.fault != Fault.None { return Value.Undefined; }
    if path.tag() <= value_tag::Null { return args[0]; }
    let kind = form_kind(heap, path);
    if kind != node_kind::List && kind != node_kind::Vector && kind != node_kind::Concat {
        // A path that is not seqable is cljs.core's error (light-cljs-873).
        if seq_error(heap, path) == CoreError.NotSeqable { not_seqable(job, path); return Value.Undefined; }
        return unsupported_step(job, b, "a path that is not a list or vector");
    }
    let start: i64 = node(heap, path.id()).start; let count: i64 = node(heap, path.id()).count;
    var value = args[0];
    for i in count {
        if !charge(heap) { return Value.Undefined; }
        // Value.Empty, which no form holds, marks a miss.
        value = form_get(heap, value, slot(heap, start + i), if args.len == 3 { Value.Empty } else { nil() });
        if heap.fault != Fault.None { return Value.Undefined; }
        if value.is_hole() { return args[2]; }
    }
    return value;
}
// into of two arguments, (reduce conj to from), as cljs.core's (and the V8
// build's runtime's) (light-cljs-873): `from` is seqed first, then each
// item conj'd as the conj form operation does it, a map target taking
// entries (map_conj). A target that cannot conj fails only when `from`
// has an item, with the protocol error. A map's or set's items as a source
// are in the runtime's iteration order, which this one does not model.
fn into(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if args.len != 2 { return native_value(heap, code(b), args); }
    let to = args[0]; let from = form_value(heap, args[1]);
    if !seq_source(job, from) { return Value.Undefined; }
    let count = form_count(heap, from); let source = form_kind(heap, from);
    if count == 0 { return to; }
    if source == node_kind::Map || source == node_kind::Set { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
    let target = form_kind(heap, to);
    if form_value(heap, to).tag() > value_tag::Null && !(target >= node_kind::List && target <= node_kind::Set || target == node_kind::Concat) {
        let none: u16[0] = []; return runtime_error(job, "No protocol method ICollection/-conj defined for this value", none);
    }
    var out = to;
    for i in count {
        if !charge(heap) { return Value.Undefined; }
        let item = form_nth(heap, from, i);
        if heap.fault != Fault.None { return Value.Undefined; }
        if form_kind(heap, out) == node_kind::Map {
            let e, culprit = conj_error(heap, out, item);
            if heap.fault != Fault.None { return Value.Undefined; }
            if e == CoreError.NotSeqable { not_seqable(job, culprit); return Value.Undefined; }
            if e == CoreError.Boundary { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
            if e != CoreError.None { return core_raise(job, e); }
            out = map_conj(heap, out, item);
        } else { out = form_conj(heap, out, item); }
        if heap.fault != Fault.None { return Value.Undefined; }
    }
    return out;
}
// Units of `[object Tag]`.
fn object_tag(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap;
    if receiver.is_undefined() { return text(heap, "[object Undefined]"); }
    if receiver.is_null() { return text(heap, "[object Null]"); }
    // A primitive's ToObject is its wrapper, whose @@toStringTag is its
    // prototype's: the lookup starts there and no wrapper is made.
    let holder = if receiver.is_object() { receiver } else { primitive_prototype(receiver) };
    if !holder.is_object() { return unsupported_step(job, Builtin.ObjectPrototypeToString, "a Symbol or BigInt receiver"); }
    let tag, getter = lookup(heap, holder, Value.Symbol { s: well_known::ToStringTag });
    if heap.fault != Fault.None { return Value.Undefined; }
    if getter.is_object() { return unsupported_step(job, Builtin.ObjectPrototypeToString, "a @@toStringTag getter"); }
    var units: u16[>..] = [];
    for c in "[object " { units.push(c); }
    if tag.is_string() { append_utf16(heap, tag, units); }
    else {
        let wrapped = if receiver.is_object() { if node(heap, receiver.id()).kind == node_kind::Wrapper { node(heap, receiver.id()).value } else { Value.Undefined } } else { receiver };
        let name: const u8[:] = if wrapper_type(wrapped) >= 0 { WRAPPER_NAMES[wrapper_type(wrapped)].text } else { match node(heap, receiver.id()).kind { node_kind::Array => "Array", node_kind::Closure, node_kind::Builtin, node_kind::BoundFunction => "Function", node_kind::Error => "Error", node_kind::Arguments => "Arguments", _ => "Object" } };
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
    if getter.is_object() && heap.fault == Fault.None { return unsupported_step(job, b, "a getter"); }
    return v;
}
// ---- Error constructors and Error.prototype.toString (light-cljs-870)
// ECMAScript 20.5.1.1 Error(message, options), 20.5.6.1.1
// NativeError(message, options), 20.5.8.1 InstallErrorCause and 20.5.3.4
// Error.prototype.toString; the Pants engine's rust/src/vm/native/error.rs
// (ErrorState, bbp-228 and bbp-229), QuickJS's js_error_constructor and
// js_error_toString. ToString(message) of an object, a getter for
// `options.cause`, and Error.prototype.toString's reads of `name` and
// `message` (a getter, own or inherited, or an object to convert) run
// JavaScript, so the dispatch loop, a Forward frame, a Convert frame, a
// proxy trap call and the native callbacks that start native frames run
// these built-ins through start_error and start_error_text (decision 4).
// Each takes its plain path, a Goose function with no frame (decision 5),
// when no step can run JavaScript: `new Error('literal')` and a toString of
// data-property strings push nothing. Otherwise:
//   ErrorConstruct  output the new error object (its prototype the
//              constructor's type's, or newTarget's `prototype`, set before
//              any step runs, as OrdinaryCreateFromConstructor does first);
//              receiver the options argument.
//     pc 0     the message on the operand top: undefined installs nothing,
//              a primitive installs its ToString, an object converts by
//              ToString in a Convert frame above (hint string), and this
//              step runs again on the string.
//              Then InstallErrorCause: when options is an object with a
//              `cause` (HasProperty), a data value installs at once and
//              the frame finishes; a getter runs above it (receiver
//              options) and
//     pc 2     awaits that getter's value, which it installs.
//   ErrorText  receiver the this object; input the name, once converted.
//     pc 0     awaits Get(O, "name") (a getter runs above the frame, with
//              O as receiver); undefined is "Error", an object converts in
//              a Convert frame above and this step runs again.
//     pc 1     awaits Get(O, "message") the same way; undefined is "".
// An abrupt completion of any step unwinds the frame like any other
// (unwind_natives: it holds nothing to release), so the order of throws is
// the steps' order. Everything the frames hold is in their value fields,
// which in-job reclamation marks (reclaim.goose mark_frame), saved
// continuations copy (continuations.goose SavedFrame) and snapshot
// validation checks (snapshot.goose native_valid). call_builtin, for the
// callers that run a built-in to completion, keeps the plain functions,
// which fail by name where a step would run JavaScript.

// The error type (ERROR_BUILTINS position) of constructor b, or -1.
fn error_type(b: Builtin) -> i64 { for e, i in ERROR_BUILTINS { if e == b { return i; } } return -1; }
// Whether built-in b runs through start_error or start_error_text.
fn error_native(b: Builtin) -> bool { return match b { Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError, ErrorPrototypeToString => true, _ => false }; }
// Start error built-in b, called with `receiver` and `args`, with operand
// base `base` (native_start's contract).
fn start_error_native(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    if b == Builtin.ErrorPrototypeToString { start_error_text(job, receiver, base); return; }
    start_error(job, b, args, base, Value.Empty);
}
// "name: message" from the converted strings (step 9 and on).
fn error_text(job: Job&, name: Value, message: Value) -> Value {
    let heap .= job.heap;
    if str_len(heap, message) == 0 { return name; }
    if str_len(heap, name) == 0 { return message; }
    var units: u16[>..] = []; append_utf16(heap, name, units); units.push(58); units.push(32); append_utf16(heap, message, units);
    if !charge(heap, units.len) { return Value.Undefined; }
    return string(heap, units);
}
// Error.prototype.toString run to completion (no getter, no object-valued
// name or message: those fail by name here).
fn error_to_string(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap; let b = Builtin.ErrorPrototypeToString;
    if !receiver.is_object() { return type_error(job, "not an object"); }
    let n = data_get(job, b, receiver, "name"); if heap.fault != Fault.None { return Value.Undefined; }
    let name = if n.is_undefined() { key_named(heap, "Error") } else { primitive_string(job, b, n) }; if heap.fault != Fault.None { return Value.Undefined; }
    let m = data_get(job, b, receiver, "message"); if heap.fault != Fault.None { return Value.Undefined; }
    let message = if m.is_undefined() { key_named(heap, "") } else { primitive_string(job, b, m) }; if heap.fault != Fault.None { return Value.Undefined; }
    return error_text(job, name, message);
}
// Whether Get(o, name) runs JavaScript or yields an object to convert (a
// scalar form too: the frame takes its primitive).
// The data value read, when there is no getter.
fn read_runs_code(heap: Heap&, o: Value, name: const u8[:]) -> bool, Value {
    let k = key_named(heap, name);
    if heap.fault != Fault.None { return false, Value.Undefined; }
    let v, getter = lookup(heap, o, k);
    return heap.fault == Fault.None && (getter.is_object() || v.is_object()), v;
}
// Error.prototype.toString with `receiver` as its this value: the plain
// path, or the ErrorText frame (see above). The probes keep the steps'
// order: ToString of a Symbol name throws before `message` is read.
fn start_error_text(job: Job&, receiver: Value, base: i64) {
    let heap .= job.heap; let b = Builtin.ErrorPrototypeToString;
    if !receiver.is_object() { type_error(job, "not an object"); return; }
    let name_runs, n = read_runs_code(heap, receiver, "name");
    if heap.fault != Fault.None { return; }
    var runs = name_runs;
    if !runs && !n.is_symbol() { let message_runs, _ = read_runs_code(heap, receiver, "message"); runs = message_runs; }
    if heap.fault != Fault.None { return; }
    if !runs {
        let v = error_to_string(job, receiver);
        if heap.fault == Fault.None { job.operands.resize(base); deliver(job, v); }
        return;
    }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: receiver, native: Native.ErrorText });
    get_named(job, b, receiver, "name");
}
// The operand top, a read's value for error built-in b's frame: an object
// starts its ToString in a Convert frame above and gives false (the step
// runs again on the string); otherwise true and the value.
fn error_operand(job: Job&, b: Builtin) -> bool, Value {
    let heap .= job.heap;
    // A scalar form (a primitive with its provenance, as a root read
    // returns one) is its primitive.
    let v = form_value(heap, job.operands.pop());
    if !v.is_object() { return true, v; }
    if !convertible(heap, v) { unsupported_step(job, b, "ToString of a light-cljs runtime value"); return false, Value.Undefined; }
    job.operands.push(v); start_convert(job, Hint.String, true, 1, 1);
    return false, Value.Undefined;
}
// One step of the ErrorText frame at the top (see above).
fn error_text_step(job: Job&, frame: Frame) {
    let heap .= job.heap; let b = Builtin.ErrorPrototypeToString; let at = job.frames.len - 1;
    if frame.pc < 0 || frame.pc > 1 { heap.fault = Fault.InvalidState; return; }
    let ready, v = error_operand(job, b);
    if !ready { return; }
    let s = if v.is_undefined() { key_named(heap, if frame.pc == 0 { "Error" } else { "" }) } else { primitive_string(job, b, v) };
    if heap.fault != Fault.None { return; }
    if frame.pc == 0 {
        job.frames[at].input = s; job.frames[at].pc = 1;
        get_named(job, b, frame.receiver, "message");
        return;
    }
    if !frame.input.is_string() { heap.fault = Fault.InvalidState; return; }
    let r = error_text(job, frame.input, s);
    if heap.fault == Fault.None { finish_native(job, frame, r); }
}
// Whether options has a `cause` behind a getter (InstallErrorCause's Get runs it).
fn cause_runs_code(heap: Heap&, options: Value) -> bool {
    if !options.is_object() { return false; }
    let k = key_named(heap, "cause");
    if heap.fault != Fault.None || !has_property(heap, options, k) { return false; }
    let _, getter = lookup(heap, options, k);
    return heap.fault == Fault.None && getter.is_object();
}
// NativeError b called (Value.Empty proto) or constructed with `args`,
// operand base `base`: the plain path, or the ErrorConstruct frame (see
// above). `proto`, when an object, is newTarget's `prototype`.
fn start_error(job: Job&, b: Builtin, args: const Value[:], base: i64, proto: Value) {
    let heap .= job.heap;
    let t = error_type(b);
    if t < 0 { heap.fault = Fault.InvalidState; return; }
    let m = arg(args, 0); let options = arg(args, 1);
    // ToString(message) precedes InstallErrorCause: a Symbol message throws
    // before options is probed.
    if m.is_symbol() { type_error(job, "Cannot convert a Symbol value to a string"); return; }
    let runs = m.is_object() || cause_runs_code(heap, options);
    if heap.fault != Fault.None { return; }
    if !runs {
        let e = construct_error(job, b, t, args);
        if heap.fault != Fault.None { return; }
        if proto.is_object() { node_mut(heap, e.id()).proto = proto; }
        job.operands.resize(base); deliver(job, e);
        return;
    }
    if job.frames.len >= heap.limits.frames { heap.fault = Fault.StackLimit; return; }
    let e = error_object(heap, t, 0, Value.Undefined);
    if heap.fault != Fault.None { return; }
    if proto.is_object() { node_mut(heap, e.id()).proto = proto; }
    job.operands.resize(base);
    job.frames.push(Frame { function: -1, pc: 0, local_base: job.locals.len, operand_base: base, callee: Value.Undefined, receiver: options, output: e, native: Native.ErrorConstruct { which: b } });
    if push_operand(job, m) { error_construct_step(job, job.frames[job.frames.len - 1], b); }
}
// Install own property `name` = v on error object e (writable,
// configurable, not enumerable).
fn install_error_field(job: Job&, e: Value, name: const u8[:], v: Value) {
    let heap .= job.heap;
    let k = key_named(heap, name);
    if heap.fault == Fault.None && !add_own(heap, e.id(), k, v, attr_writable() | attr_configurable()) && heap.fault == Fault.None { heap.fault = Fault.InvalidState; }
}
// One step of the ErrorConstruct frame at the top (see above).
fn error_construct_step(job: Job&, frame: Frame, b: Builtin) {
    let heap .= job.heap; let at = job.frames.len - 1;
    let e = frame.output;
    if !e.is_object() || node(heap, e.id()).kind != node_kind::Error { heap.fault = Fault.InvalidState; return; }
    if frame.pc == 2 {
        install_error_field(job, e, "cause", job.operands.pop());
        if heap.fault == Fault.None { finish_native(job, frame, e); }
        return;
    }
    if frame.pc != 0 { heap.fault = Fault.InvalidState; return; }
    let ready, m = error_operand(job, b);
    if !ready { return; }
    if !m.is_undefined() {
        let s = primitive_string(job, b, m);
        if heap.fault != Fault.None { return; }
        install_error_field(job, e, "message", s);
        if heap.fault != Fault.None { return; }
    }
    // InstallErrorCause: HasProperty(options, "cause"), then its Get.
    let options = frame.receiver;
    if options.is_object() {
        let k = key_named(heap, "cause");
        if heap.fault != Fault.None { return; }
        if has_property(heap, options, k) {
            let cause, getter = lookup(heap, options, k);
            if heap.fault != Fault.None { return; }
            if getter.is_object() { job.frames[at].pc = 2; run_getter(job, b, getter, options); return; }
            install_error_field(job, e, "cause", cause);
        }
        if heap.fault != Fault.None { return; }
    }
    finish_native(job, frame, e);
}
// NativeError(message, options) called as a function: a new error object of
// type t whose own `message` is ToString(message) and, when options has a
// `cause`, whose own `cause` is that value.
fn construct_error(job: Job&, b: Builtin, t: i64, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let m = arg(args, 0);
    let message = if m.is_undefined() { Value.Undefined } else { primitive_string(job, b, m) };
    if heap.fault != Fault.None { return Value.Undefined; }
    let e = error_object(heap, t, 0, message);
    let options = arg(args, 1);
    if heap.fault == Fault.None && options.is_object() {
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
    let a = allocate(heap, Node { kind: node_kind::Array });
    if heap.fault != Fault.None { return Value.Undefined; }
    if args.len == 1 && args[0].is_number() {
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
            if v.tag() <= value_tag::Null { return allocate(heap, Node { kind: node_kind::Object }); }
            return to_object(job, v);
        },
        // The Function constructor runs as a DynamicFunction native frame
        // (light-cljs-857; runtime_compile.goose). Generator, async and async
        // generator functions built at run time (%GeneratorFunction% and its
        // siblings) are the generators area's (light-cljs-768): their realm
        // prototypes' `constructor`s are pending (function_kind_prototype_row).
        Function => { return unsupported_step(job, b, "called outside a native frame"); },
        Array => { return construct_array(job, args); },
        Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError => {
            return construct_error(job, b, error_type(b), args);
        },
        ObjectGetPrototypeOf => {
            let v = arg(args, 0);
            // QuickJS's js_object_getPrototypeOf words this "not an object".
            if v.tag() <= value_tag::Null { return type_error(job, "not an object"); }
            // ToObject's wrapper inherits its realm prototype; none is made.
            if !v.is_object() { let p = primitive_prototype(v); return if p.is_object() { p } else { unsupported_step(job, b, "Symbol and BigInt prototypes") }; }
            let kind = node(heap, v.id()).kind;
            // A proxy's [[GetPrototypeOf]] outside for-in fails by name (proxy.goose).
            if kind == node_kind::Proxy { proxy_fault(heap, proxy_method::GetPrototypeOf); return Value.Undefined; }
            if kind != node_kind::Object && kind != node_kind::Array && kind != node_kind::Closure && kind != node_kind::Builtin && kind != node_kind::BoundFunction && kind != node_kind::Error && kind != node_kind::Arguments && kind != node_kind::Generator && kind != node_kind::Promise && kind != node_kind::Wrapper && kind != node_kind::Iterator { return unsupported_step(job, b, "a runtime form"); }
            return prototype(heap, v.id());
        },
        ObjectIs => { return boolean(same_value(heap, arg(args, 0), arg(args, 1))); },
        ObjectHasOwn, ObjectPrototypeHasOwnProperty => {
            let own = b == Builtin.ObjectHasOwn;
            let o = if own { arg(args, 0) } else { receiver };
            // Object.hasOwn converts its object first, hasOwnProperty its key.
            if own && o.tag() <= value_tag::Null { return type_error(job, "cannot convert to object"); }
            let k = property_key(job, b, arg(args, if own { 1 } else { 0 }));
            if heap.fault != Fault.None { return Value.Undefined; }
            if o.tag() <= value_tag::Null { return type_error(job, "cannot convert to object"); }
            let found = has_own(job, b, o, k);
            return if heap.fault == Fault.None { boolean(found) } else { Value.Undefined };
        },
        ObjectPrototypeValueOf => {
            return to_object(job, receiver);
        },
        ObjectPrototypeToString => { return object_tag(job, receiver); },
        ObjectGetOwnPropertyDescriptor => { return own_descriptor(job, b, arg(args, 0), arg(args, 1)); },
        // IsArray: a proxy is its final target's (light-cljs-845).
        ArrayIsArray => { var v = arg(args, 0); while is_proxy(heap, v) { v = node(heap, v.id()).value; } return boolean(v.is_object() && node(heap, v.id()).kind == node_kind::Array); },
        // Proxy called without `new` (light-cljs-845; construct makes one).
        Proxy => { return type_error(job, "Proxy: constructor requires 'new'"); },
        ErrorPrototypeToString => { return error_to_string(job, receiver); },
        // Function.prototype.toString and %ThrowTypeError% (light-cljs-853; function_calls.goose).
        FunctionPrototypeToString => { return function_text(job, receiver); },
        ThrowTypeError => { return throw_type_error(job, receiver, args.len); },
        // The forwarding built-ins run as Forward native frames
        // (function_calls.goose, native_start); a caller that runs a
        // built-in to completion here cannot.
        FunctionPrototypeCall, FunctionPrototypeApply, ReflectApply, ReflectConstruct, FunctionPrototypeBind => { return unsupported_step(job, b, "called outside a native frame"); },
        // The dispatch loop runs these as native frames (native_start); a
        // caller that runs a built-in to completion here cannot.
        ArrayPrototypeMap, ArrayPrototypeForEach, ArrayPrototypePush, ArrayPrototypeJoin, ArrayPrototypeToString, ArrayIteratorPrototypeNext, StringPrototypeIterator => { return unsupported_step(job, b, "called from a native frame"); },
        // The built-in iterators that run no JavaScript (light-cljs-844; iterators.goose).
        ArrayPrototypeValues, ArrayPrototypeKeys, ArrayPrototypeEntries => { return array_iterator(job, b, receiver); },
        StringIteratorPrototypeNext => { return string_iterator_next(job, receiver); },
        SymbolFunction => { return unsupported_step(job, b, "creating symbols"); },
        NativeCoreBinding => { return core_binding(job, b, arg(args, 0)); },
        Atom, Deref, Reset, CompareAndSet, ResetVals, Volatile, Vreset, AddWatch, RemoveWatch, SetValidator => { return cell_builtin(job, b, args); },
        Swap, SwapVals, GeneratorPrototypeNext, GeneratorPrototypeReturn, GeneratorPrototypeThrow, FunctionPrototypeHasInstance => { return unsupported_step(job, b, "called from a native frame"); },
        // %IteratorPrototype%[@@iterator]: returns its receiver.
        IteratorPrototypeIterator => { return receiver; },
        // A direct-eval site's call of %eval% never reaches here (Ins.DirectEval),
        // and every other call runs it as a native frame (start_indirect_eval).
        Eval => { return unsupported_step(job, b, "called outside a native frame"); },
        Delay, Force, Realized, IsDelay => { return delay_builtin(job, b, args); },
        NativeConstruct => {
            if !constructible(heap, arg(args, 0)) { return unsupported_step(job, b, "constructing a value other than a Delay"); }
            var rest: Value[>..] = []; if args.len > 1 { rest.append(args[1..]); }
            return delay_builtin(job, Builtin.Delay, rest);
        },
        Number => { return wrapper_value(job, b, 0, args, false); },
        String => { return wrapper_value(job, b, 1, args, false); },
        Boolean => { return wrapper_value(job, b, 2, args, false); },
        NumberPrototypeValueOf => { return this_primitive(job, b, 0, receiver); },
        NumberPrototypeToString => { return number_to_string(job, b, receiver, args); },
        StringPrototypeValueOf, StringPrototypeToString => { return this_primitive(job, b, 1, receiver); },
        BooleanPrototypeValueOf => { return this_primitive(job, b, 2, receiver); },
        BooleanPrototypeToString => {
            let x = this_primitive(job, b, 2, receiver);
            return if heap.fault == Fault.None { key_named(heap, if x.flag() { "true" } else { "false" }) } else { Value.Undefined };
        },
        // The interop and error leaves (light-cljs-872). rt.nativeInterop
        // unwraps compiled interop's receiver or operand; in the V8 build no
        // value is a native form there, so it rejects none (a member read of
        // a form then fails by name in the property model). The Object
        // declaration demand a member read makes in the V8 build installs
        // deferred protocol extensions; this runtime has none, so the
        // receiver passes unchanged.
        NativeInterop => { return form_value(heap, arg(args, 0)); },
        DeclarationReceiver => { return arg(args, 0); },
        ExInfo => { return ex_info(job, b, args); },
        ExData, ExMessage, ExCause => { return ex_field(job, b, arg(args, 0)); },
        GetIn => { return get_in(job, b, args); },
        // A non-seqable source is cljs.core's error (light-cljs-873).
        Vec, Sequence => { if args.len == 1 && !seq_source(job, args[0]) { return Value.Undefined; } return native_value(heap, id, args); },
        Into => { return into(job, b, args); },
        _ => { return native_value(heap, id, args); },
    }
}
// `typeof v` (Typeof, ECMAScript TypeofOperator): closures, built-in
// functions, bound functions and proxies of callable targets are "function", every other
// object "object".
fn type_name(heap: Heap&, v: Value) -> Value {
    let t: i64 = match v { Undefined => 0, Null => 1, Bool => 2, Number => 3, String => 4, Symbol => 5, BigInt => 6, Object x => if node(heap, x.o).kind == node_kind::Closure || node(heap, x.o).kind == node_kind::Builtin || node(heap, x.o).kind == node_kind::BoundFunction || node(heap, x.o).kind == node_kind::Proxy && proxy_callable(heap, v) { 7 } else { 1 }, _ => 1 };
    return key_named(heap, TYPE_NAMES[t].text);
}
