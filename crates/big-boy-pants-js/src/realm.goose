import std;
import .source_vm;
import .array_callbacks;
import .array_methods;
import .printer;
import .coerce;
import .number_text;
import .math_object;
import .regexp_object;
import .collections;
import .array_search;
import .array_mutators;
import .array_sort;
import .array_copy;
import .string_methods;
import .string_case;

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
// (exception_info_prototype_row), the ExceptionInfo function
// (exception_info_constructor_row), its prototype's `toString`
// (exception_info_to_string_row) and the four empty collection literals
// (empty_form_row), realm_nodes() rows in all. Every intrinsic is an
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
    // cljs.core.ExceptionInfo (light-cljs-875), a realm row
    // (exception_info_constructor_row) that rt.core.ExceptionInfo names.
    ExceptionInfo,
    // rt.core.isInstance (light-cljs-875): what a native macro unit emits
    // for `instance?` of a class that is no js/ global, a native frame
    // (source_vm.goose start_is_instance).
    IsInstance,
    // cljs.core's str, pr-str and identical? (rt.core.str, prStr and
    // identical) and ExceptionInfo.prototype.toString, a realm row
    // (exception_info_to_string_row) (light-cljs-876; printer.goose).
    Str, PrStr, Identical, ExceptionInfoPrototypeToString,
    // cljs.core's range of numbers, realized (light-cljs-877; core_range).
    Range,
    // Property descriptors (light-cljs-881; descriptors.goose):
    // Object.defineProperty, Object.defineProperties and
    // Reflect.defineProperty (Descriptor native frames),
    // Reflect.getOwnPropertyDescriptor, Reflect.deleteProperty and Annex B's
    // Object.prototype accessor methods.
    ObjectDefineProperty, ObjectDefineProperties, ReflectDefineProperty, ReflectGetOwnPropertyDescriptor, ReflectDeleteProperty,
    ObjectPrototypeDefineGetter, ObjectPrototypeDefineSetter, ObjectPrototypeLookupGetter, ObjectPrototypeLookupSetter,
    // [[Extensible]] and integrity levels (light-cljs-882; integrity.goose).
    ObjectPreventExtensions, ObjectIsExtensible, ObjectSeal, ObjectIsSealed, ObjectFreeze, ObjectIsFrozen, ReflectIsExtensible, ReflectPreventExtensions,
    // Prototypes (light-cljs-883; prototypes.goose): Object.create (a
    // native frame with a properties argument), Object.setPrototypeOf,
    // Object.prototype.isPrototypeOf, the getter and setter of
    // Object.prototype's `__proto__`, and Reflect.getPrototypeOf and
    // setPrototypeOf.
    ObjectCreate, ObjectSetPrototypeOf, ObjectPrototypeIsPrototypeOf, ObjectPrototypeProtoGetter, ObjectPrototypeProtoSetter, ReflectGetPrototypeOf, ReflectSetPrototypeOf,
    // The Object statics that expose own keys, with Reflect.ownKeys and
    // Reflect.has (light-cljs-884; object_statics.goose), Statics native frames.
    ObjectKeys, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectGetOwnPropertySymbols, ObjectGetOwnPropertyDescriptors, ObjectAssign, ObjectFromEntries, ObjectGroupBy,
    ObjectPrototypePropertyIsEnumerable, ObjectPrototypeToLocaleString, ReflectOwnKeys, ReflectHas,
    // Symbol.for and keyFor, and Symbol.prototype's methods and
    // `description` getter (light-cljs-885; symbols.goose).
    SymbolFor, SymbolKeyFor, SymbolPrototypeToString, SymbolPrototypeValueOf, SymbolPrototypeDescription, SymbolPrototypeToPrimitive,
    // Reflect.get and Reflect.set with their receiver (light-cljs-888;
    // object_statics.goose), Statics native frames.
    ReflectGet, ReflectSet,
    // Proxy.revocable and the revoke function it makes, a job row naming
    // its proxy (light-cljs-891; proxy.goose).
    ProxyRevocable, ProxyRevoke,
    // The global number functions and Number's predicates (light-cljs-900;
    // number_builtin): isNaN, isFinite, parseInt and parseFloat, which Number
    // also holds as parseInt and parseFloat, and Number.isNaN, isFinite,
    // isInteger and isSafeInteger.
    IsNaN, IsFinite, ParseInt, ParseFloat, NumberIsNaN, NumberIsFinite, NumberIsInteger, NumberIsSafeInteger,
    // The Math object's functions (light-cljs-901; math_object.goose), in
    // QuickJS's js_math_funcs order; sumPrecise is a native frame.
    MathMin, MathMax, MathAbs, MathFloor, MathCeil, MathRound, MathSqrt, MathAcos, MathAsin, MathAtan, MathAtan2, MathCos, MathExp, MathLog, MathPow, MathSin, MathTan,
    MathTrunc, MathSign, MathCosh, MathSinh, MathTanh, MathAcosh, MathAsinh, MathAtanh, MathExpm1, MathLog1p, MathLog2, MathLog10, MathCbrt, MathHypot, MathRandom,
    MathF16round, MathFround, MathImul, MathClz32, MathSumPrecise,
    // rt.nativeType, what a native compile-time unit runs for a deftype or
    // defrecord, and globalThis.__lightCompilerProductTarget, where a
    // declaration a macro runs directly writes its products
    // (light-cljs-920; native_type, product_target).
    NativeType, ProductTarget,
    // RegExp (light-cljs-932; regexp_object.goose): the constructor and
    // RegExp.prototype's flags getter and toString, RegExp native frames;
    // its source and flag getters and RegExp[@@species], plain.
    RegExp, RegExpPrototypeFlags, RegExpPrototypeSource, RegExpPrototypeGlobal, RegExpPrototypeIgnoreCase, RegExpPrototypeMultiline, RegExpPrototypeDotAll,
    RegExpPrototypeUnicode, RegExpPrototypeUnicodeSets, RegExpPrototypeSticky, RegExpPrototypeHasIndices, RegExpPrototypeToString, RegExpSpecies,
    // RegExp.prototype[@@match] (light-cljs-932): a function object, since
    // IsRegExp reads it; calling it is light-cljs-937's, a named step until then.
    RegExpPrototypeMatch,
    // RegExp.prototype.exec and test (light-cljs-933; regexp_object.goose),
    // RegExp native frames.
    RegExpPrototypeExec, RegExpPrototypeTest,
    // String.prototype.replace and replaceAll and RegExp.prototype[@@replace]
    // (light-cljs-934; regexp_replace.goose), RegExp native frames.
    StringPrototypeReplace, StringPrototypeReplaceAll, RegExpPrototypeReplace,
    // Map and Set (light-cljs-943; collections.goose): the constructors,
    // which construct only and run their iterable in a Collection native
    // frame; their prototypes' keyed methods and size getters and their
    // @@species, plain.
    Map, MapPrototypeSet, MapPrototypeGet, MapPrototypeHas, MapPrototypeDelete, MapPrototypeClear, MapPrototypeSize, MapSpecies,
    Set, SetPrototypeAdd, SetPrototypeHas, SetPrototypeDelete, SetPrototypeClear, SetPrototypeSize, SetSpecies,
    // Array[@@species], plain; the remaining Array.prototype callback
    // methods, ArrayCallbacks native frames (array_callbacks.goose); and
    // the search methods, ArrayMethod native frames (array_search.goose)
    // (light-cljs-908).
    ArraySpecies, ArrayPrototypeFilter, ArrayPrototypeReduce, ArrayPrototypeReduceRight, ArrayPrototypeSome, ArrayPrototypeEvery, ArrayPrototypeFlatMap,
    ArrayPrototypeFind, ArrayPrototypeFindIndex, ArrayPrototypeFindLast, ArrayPrototypeFindLastIndex,
    ArrayPrototypeIndexOf, ArrayPrototypeLastIndexOf, ArrayPrototypeIncludes, ArrayPrototypeAt,
    // The Array.prototype methods that mutate their receiver, and toSpliced
    // (light-cljs-909; array_mutators.goose), ArrayMethod native frames.
    ArrayPrototypePop, ArrayPrototypeShift, ArrayPrototypeUnshift, ArrayPrototypeSplice, ArrayPrototypeReverse, ArrayPrototypeFill, ArrayPrototypeCopyWithin, ArrayPrototypeToSpliced,
    // Array.prototype.sort and toSorted (light-cljs-910; array_sort.goose),
    // in the mutators' frame.
    ArrayPrototypeSort, ArrayPrototypeToSorted,
    // The Array methods that copy and the statics Array.of and Array.from
    // (light-cljs-911; array_copy.goose), in the mutators' frame.
    ArrayPrototypeSlice, ArrayPrototypeConcat, ArrayPrototypeFlat, ArrayPrototypeToReversed, ArrayPrototypeWith, ArrayOf, ArrayFrom,
    // String code units, statics and search (light-cljs-903).
    StringFromCharCode, StringFromCodePoint, StringRaw, StringPrototypeCharAt, StringPrototypeCharCodeAt, StringPrototypeCodePointAt, StringPrototypeAt, StringPrototypeIndexOf, StringPrototypeLastIndexOf, StringPrototypeIncludes, StringPrototypeStartsWith, StringPrototypeEndsWith, StringPrototypeIsWellFormed, StringPrototypeToWellFormed,
    // Live Map and Set iteration (light-cljs-944; collections.goose).
    MapPrototypeKeys, MapPrototypeValues, MapPrototypeEntries, MapPrototypeForEach, MapIteratorPrototypeNext,
    SetPrototypeValues, SetPrototypeEntries, SetPrototypeForEach, SetIteratorPrototypeNext,
    // Exact number text after the shared thisNumberValue/Integer plan
    // (light-cljs-902; number_text.goose).
    NumberPrototypeToFixed, NumberPrototypeToExponential, NumberPrototypeToPrecision,
    // Extraction, padding, trimming, repetition, split and Annex B (light-cljs-904).
    StringPrototypeSubstring, StringPrototypeSubstr, StringPrototypeSlice, StringPrototypeConcat, StringPrototypePadStart, StringPrototypePadEnd, StringPrototypeRepeat, StringPrototypeTrim, StringPrototypeTrimStart, StringPrototypeTrimEnd, StringPrototypeSplit, StringPrototypeAnchor, StringPrototypeBig, StringPrototypeBlink, StringPrototypeBold, StringPrototypeFixed, StringPrototypeFontcolor, StringPrototypeFontsize, StringPrototypeItalics, StringPrototypeLink, StringPrototypeSmall, StringPrototypeStrike, StringPrototypeSub, StringPrototypeSup,
    // Case mapping and normalization with Unicode 17 data (light-cljs-905;
    // string_case.goose); the locale-sensitive two run as StringMethod frames.
    StringPrototypeToUpperCase, StringPrototypeToLowerCase, StringPrototypeToLocaleUpperCase, StringPrototypeToLocaleLowerCase, StringPrototypeNormalize,
}
// A built-in's code (its position here, a node's `function`), its `name`,
// its `length` and its conversion plan (coerce.goose; plan::None for none).
struct BuiltinInfo { which: Builtin, name: u8[..32], length: u8 = 0, plan: u8 = 0 }
let BUILTINS: BuiltinInfo[323] = [
    BuiltinInfo { which: Builtin.MacroReaderFunction, name: "macroReaderFunction", length: 1 }, BuiltinInfo { which: Builtin.Inc, name: "inc", length: 1 },
    BuiltinInfo { which: Builtin.Mapv, name: "mapv", length: 2 }, BuiltinInfo { which: Builtin.Vec, name: "vec", length: 1 }, BuiltinInfo { which: Builtin.Into, name: "into", length: 2 },
    BuiltinInfo { which: Builtin.Gensym, name: "gensym" }, BuiltinInfo { which: Builtin.SharedIncrement, name: "sharedIncrement" }, BuiltinInfo { which: Builtin.SharedRead, name: "sharedRead" },
    BuiltinInfo { which: Builtin.External, name: "external" }, BuiltinInfo { which: Builtin.Retired, name: "" },
    BuiltinInfo { which: Builtin.VectorFromArray, name: "vectorFromArray", length: 1 }, BuiltinInfo { which: Builtin.ListFromArray, name: "listFromArray", length: 1 },
    BuiltinInfo { which: Builtin.MapFromArray, name: "mapFromArray", length: 1 }, BuiltinInfo { which: Builtin.SetFromArray, name: "setFromArray", length: 1 },
    BuiltinInfo { which: Builtin.FunctionPrototypeCall, name: "call", length: 1 }, BuiltinInfo { which: Builtin.FnRest, name: "fnRest", length: 2 },
    BuiltinInfo { which: Builtin.Sequence, name: "sequence", length: 1 }, BuiltinInfo { which: Builtin.MapSeq, name: "mapSeq", length: 2 },
    BuiltinInfo { which: Builtin.List, name: "list" }, BuiltinInfo { which: Builtin.Symbol, name: "symbol", length: 2 },
    // Its code stays; boot_runtime installs the realm's RegExp in its place
    // (light-cljs-935), so no row is this built-in.
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
    BuiltinInfo { which: Builtin.SymbolFunction, name: "Symbol", plan: plan::Symbol }, BuiltinInfo { which: Builtin.ObjectGetOwnPropertyDescriptor, name: "getOwnPropertyDescriptor", length: 2 },
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
    BuiltinInfo { which: Builtin.Number, name: "Number", length: 1, plan: plan::Number }, BuiltinInfo { which: Builtin.String, name: "String", length: 1, plan: plan::String }, BuiltinInfo { which: Builtin.Boolean, name: "Boolean", length: 1 },
    BuiltinInfo { which: Builtin.NumberPrototypeValueOf, name: "valueOf" }, BuiltinInfo { which: Builtin.NumberPrototypeToString, name: "toString", length: 1, plan: plan::NumberRadix },
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
    BuiltinInfo { which: Builtin.GetIn, name: "getIn" }, BuiltinInfo { which: Builtin.ExceptionInfo, name: "ExceptionInfo", length: 3 },
    BuiltinInfo { which: Builtin.IsInstance, name: "isInstance", length: 2 },
    // The names and lengths of the V8 build's str, prStr and identical
    // (`identical: (a, b) =>`, an arrow, so no constructor:
    // function_calls.goose constructor_builtin), and of the anonymous
    // function assigned to ExceptionInfo.prototype.toString.
    BuiltinInfo { which: Builtin.Str, name: "str" }, BuiltinInfo { which: Builtin.PrStr, name: "prStr" },
    BuiltinInfo { which: Builtin.Identical, name: "identical", length: 2 }, BuiltinInfo { which: Builtin.ExceptionInfoPrototypeToString, name: "" },
    // `range: (...a) =>`, an arrow (constructor_builtin).
    BuiltinInfo { which: Builtin.Range, name: "range" },
    BuiltinInfo { which: Builtin.ObjectDefineProperty, name: "defineProperty", length: 3 }, BuiltinInfo { which: Builtin.ObjectDefineProperties, name: "defineProperties", length: 2 },
    BuiltinInfo { which: Builtin.ReflectDefineProperty, name: "defineProperty", length: 3 }, BuiltinInfo { which: Builtin.ReflectGetOwnPropertyDescriptor, name: "getOwnPropertyDescriptor", length: 2 },
    BuiltinInfo { which: Builtin.ReflectDeleteProperty, name: "deleteProperty", length: 2 },
    BuiltinInfo { which: Builtin.ObjectPrototypeDefineGetter, name: "__defineGetter__", length: 2 }, BuiltinInfo { which: Builtin.ObjectPrototypeDefineSetter, name: "__defineSetter__", length: 2 },
    BuiltinInfo { which: Builtin.ObjectPrototypeLookupGetter, name: "__lookupGetter__", length: 1 }, BuiltinInfo { which: Builtin.ObjectPrototypeLookupSetter, name: "__lookupSetter__", length: 1 },
    BuiltinInfo { which: Builtin.ObjectPreventExtensions, name: "preventExtensions", length: 1 }, BuiltinInfo { which: Builtin.ObjectIsExtensible, name: "isExtensible", length: 1 },
    BuiltinInfo { which: Builtin.ObjectSeal, name: "seal", length: 1 }, BuiltinInfo { which: Builtin.ObjectIsSealed, name: "isSealed", length: 1 },
    BuiltinInfo { which: Builtin.ObjectFreeze, name: "freeze", length: 1 }, BuiltinInfo { which: Builtin.ObjectIsFrozen, name: "isFrozen", length: 1 },
    BuiltinInfo { which: Builtin.ReflectIsExtensible, name: "isExtensible", length: 1 }, BuiltinInfo { which: Builtin.ReflectPreventExtensions, name: "preventExtensions", length: 1 },
    BuiltinInfo { which: Builtin.ObjectCreate, name: "create", length: 2 }, BuiltinInfo { which: Builtin.ObjectSetPrototypeOf, name: "setPrototypeOf", length: 2 },
    BuiltinInfo { which: Builtin.ObjectPrototypeIsPrototypeOf, name: "isPrototypeOf", length: 1 },
    BuiltinInfo { which: Builtin.ObjectPrototypeProtoGetter, name: "get __proto__" }, BuiltinInfo { which: Builtin.ObjectPrototypeProtoSetter, name: "set __proto__", length: 1 },
    BuiltinInfo { which: Builtin.ReflectGetPrototypeOf, name: "getPrototypeOf", length: 1 }, BuiltinInfo { which: Builtin.ReflectSetPrototypeOf, name: "setPrototypeOf", length: 2 },
    BuiltinInfo { which: Builtin.ObjectKeys, name: "keys", length: 1 }, BuiltinInfo { which: Builtin.ObjectValues, name: "values", length: 1 }, BuiltinInfo { which: Builtin.ObjectEntries, name: "entries", length: 1 },
    BuiltinInfo { which: Builtin.ObjectGetOwnPropertyNames, name: "getOwnPropertyNames", length: 1 }, BuiltinInfo { which: Builtin.ObjectGetOwnPropertySymbols, name: "getOwnPropertySymbols", length: 1 },
    BuiltinInfo { which: Builtin.ObjectGetOwnPropertyDescriptors, name: "getOwnPropertyDescriptors", length: 1 }, BuiltinInfo { which: Builtin.ObjectAssign, name: "assign", length: 2 },
    BuiltinInfo { which: Builtin.ObjectFromEntries, name: "fromEntries", length: 1 }, BuiltinInfo { which: Builtin.ObjectGroupBy, name: "groupBy", length: 2 },
    BuiltinInfo { which: Builtin.ObjectPrototypePropertyIsEnumerable, name: "propertyIsEnumerable", length: 1 }, BuiltinInfo { which: Builtin.ObjectPrototypeToLocaleString, name: "toLocaleString" },
    BuiltinInfo { which: Builtin.ReflectOwnKeys, name: "ownKeys", length: 1 }, BuiltinInfo { which: Builtin.ReflectHas, name: "has", length: 2 },
    BuiltinInfo { which: Builtin.SymbolFor, name: "for", length: 1, plan: plan::Symbol }, BuiltinInfo { which: Builtin.SymbolKeyFor, name: "keyFor", length: 1 },
    BuiltinInfo { which: Builtin.SymbolPrototypeToString, name: "toString" }, BuiltinInfo { which: Builtin.SymbolPrototypeValueOf, name: "valueOf" },
    BuiltinInfo { which: Builtin.SymbolPrototypeDescription, name: "get description" }, BuiltinInfo { which: Builtin.SymbolPrototypeToPrimitive, name: "[Symbol.toPrimitive]", length: 1 },
    BuiltinInfo { which: Builtin.ReflectGet, name: "get", length: 2 }, BuiltinInfo { which: Builtin.ReflectSet, name: "set", length: 3 },
    BuiltinInfo { which: Builtin.ProxyRevocable, name: "revocable", length: 2 }, BuiltinInfo { which: Builtin.ProxyRevoke, name: "" },
    BuiltinInfo { which: Builtin.IsNaN, name: "isNaN", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.IsFinite, name: "isFinite", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.ParseInt, name: "parseInt", length: 2, plan: plan::ParseInt }, BuiltinInfo { which: Builtin.ParseFloat, name: "parseFloat", length: 1, plan: plan::OneString },
    BuiltinInfo { which: Builtin.NumberIsNaN, name: "isNaN", length: 1 }, BuiltinInfo { which: Builtin.NumberIsFinite, name: "isFinite", length: 1 },
    BuiltinInfo { which: Builtin.NumberIsInteger, name: "isInteger", length: 1 }, BuiltinInfo { which: Builtin.NumberIsSafeInteger, name: "isSafeInteger", length: 1 },
    // Math (light-cljs-901): ToNumber of every argument for min, max and
    // hypot, of two for atan2 and pow, ToUint32 of two for imul and of one
    // for clz32, ToNumber of one for the rest; random and sumPrecise
    // convert nothing.
    BuiltinInfo { which: Builtin.MathMin, name: "min", length: 2, plan: plan::Numbers }, BuiltinInfo { which: Builtin.MathMax, name: "max", length: 2, plan: plan::Numbers },
    BuiltinInfo { which: Builtin.MathAbs, name: "abs", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathFloor, name: "floor", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathCeil, name: "ceil", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathRound, name: "round", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathSqrt, name: "sqrt", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathAcos, name: "acos", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathAsin, name: "asin", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathAtan, name: "atan", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathAtan2, name: "atan2", length: 2, plan: plan::TwoNumbers }, BuiltinInfo { which: Builtin.MathCos, name: "cos", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathExp, name: "exp", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathLog, name: "log", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathPow, name: "pow", length: 2, plan: plan::TwoNumbers }, BuiltinInfo { which: Builtin.MathSin, name: "sin", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathTan, name: "tan", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathTrunc, name: "trunc", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathSign, name: "sign", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathCosh, name: "cosh", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathSinh, name: "sinh", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathTanh, name: "tanh", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathAcosh, name: "acosh", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathAsinh, name: "asinh", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathAtanh, name: "atanh", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathExpm1, name: "expm1", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathLog1p, name: "log1p", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathLog2, name: "log2", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathLog10, name: "log10", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathCbrt, name: "cbrt", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathHypot, name: "hypot", length: 2, plan: plan::Numbers }, BuiltinInfo { which: Builtin.MathRandom, name: "random" },
    BuiltinInfo { which: Builtin.MathF16round, name: "f16round", length: 1, plan: plan::OneNumber }, BuiltinInfo { which: Builtin.MathFround, name: "fround", length: 1, plan: plan::OneNumber },
    BuiltinInfo { which: Builtin.MathImul, name: "imul", length: 2, plan: plan::TwoUint32 }, BuiltinInfo { which: Builtin.MathClz32, name: "clz32", length: 1, plan: plan::OneUint32 },
    BuiltinInfo { which: Builtin.MathSumPrecise, name: "sumPrecise", length: 1 },
    // `nativeType:(fields,specs,record,name,view,mutable,ty,fieldMembers)=>` and
    // `__lightCompilerProductTarget=name=>`, arrows of src/bbp_runtime.js.
    BuiltinInfo { which: Builtin.NativeType, name: "nativeType", length: 8 }, BuiltinInfo { which: Builtin.ProductTarget, name: "__lightCompilerProductTarget", length: 1 },
    // RegExp (light-cljs-932): its getters are named "get <key>" (each
    // property's key, regexp_accessor).
    BuiltinInfo { which: Builtin.RegExp, name: "RegExp", length: 2 }, BuiltinInfo { which: Builtin.RegExpPrototypeFlags, name: "get flags" },
    BuiltinInfo { which: Builtin.RegExpPrototypeSource, name: "get source" }, BuiltinInfo { which: Builtin.RegExpPrototypeGlobal, name: "get global" },
    BuiltinInfo { which: Builtin.RegExpPrototypeIgnoreCase, name: "get ignoreCase" }, BuiltinInfo { which: Builtin.RegExpPrototypeMultiline, name: "get multiline" },
    BuiltinInfo { which: Builtin.RegExpPrototypeDotAll, name: "get dotAll" }, BuiltinInfo { which: Builtin.RegExpPrototypeUnicode, name: "get unicode" },
    BuiltinInfo { which: Builtin.RegExpPrototypeUnicodeSets, name: "get unicodeSets" }, BuiltinInfo { which: Builtin.RegExpPrototypeSticky, name: "get sticky" },
    BuiltinInfo { which: Builtin.RegExpPrototypeHasIndices, name: "get hasIndices" }, BuiltinInfo { which: Builtin.RegExpPrototypeToString, name: "toString" },
    BuiltinInfo { which: Builtin.RegExpSpecies, name: "get [Symbol.species]" }, BuiltinInfo { which: Builtin.RegExpPrototypeMatch, name: "[Symbol.match]", length: 1 },
    BuiltinInfo { which: Builtin.RegExpPrototypeExec, name: "exec", length: 1 }, BuiltinInfo { which: Builtin.RegExpPrototypeTest, name: "test", length: 1 },
    // Their conversions follow the @@replace lookup and reads (coerce.goose):
    // no plan.
    BuiltinInfo { which: Builtin.StringPrototypeReplace, name: "replace", length: 2 }, BuiltinInfo { which: Builtin.StringPrototypeReplaceAll, name: "replaceAll", length: 2 },
    BuiltinInfo { which: Builtin.RegExpPrototypeReplace, name: "[Symbol.replace]", length: 2 },
    // Map and Set (light-cljs-943): no conversions (coerce.goose); the
    // getters are named "get <key>".
    BuiltinInfo { which: Builtin.Map, name: "Map" }, BuiltinInfo { which: Builtin.MapPrototypeSet, name: "set", length: 2 }, BuiltinInfo { which: Builtin.MapPrototypeGet, name: "get", length: 1 },
    BuiltinInfo { which: Builtin.MapPrototypeHas, name: "has", length: 1 }, BuiltinInfo { which: Builtin.MapPrototypeDelete, name: "delete", length: 1 }, BuiltinInfo { which: Builtin.MapPrototypeClear, name: "clear" },
    BuiltinInfo { which: Builtin.MapPrototypeSize, name: "get size" }, BuiltinInfo { which: Builtin.MapSpecies, name: "get [Symbol.species]" },
    BuiltinInfo { which: Builtin.Set, name: "Set" }, BuiltinInfo { which: Builtin.SetPrototypeAdd, name: "add", length: 1 }, BuiltinInfo { which: Builtin.SetPrototypeHas, name: "has", length: 1 },
    BuiltinInfo { which: Builtin.SetPrototypeDelete, name: "delete", length: 1 }, BuiltinInfo { which: Builtin.SetPrototypeClear, name: "clear" }, BuiltinInfo { which: Builtin.SetPrototypeSize, name: "get size" },
    BuiltinInfo { which: Builtin.SetSpecies, name: "get [Symbol.species]" },
    // Array[@@species] and Array.prototype's remaining callback methods
    // and search methods (light-cljs-908): no plan (coerce.goose); the
    // search methods convert fromIndex after reading the length, in their
    // frame.
    BuiltinInfo { which: Builtin.ArraySpecies, name: "get [Symbol.species]" }, BuiltinInfo { which: Builtin.ArrayPrototypeFilter, name: "filter", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeReduce, name: "reduce", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeReduceRight, name: "reduceRight", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeSome, name: "some", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeEvery, name: "every", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeFlatMap, name: "flatMap", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeFind, name: "find", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeFindIndex, name: "findIndex", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeFindLast, name: "findLast", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeFindLastIndex, name: "findLastIndex", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeIndexOf, name: "indexOf", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeLastIndexOf, name: "lastIndexOf", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeIncludes, name: "includes", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeAt, name: "at", length: 1 },
    // The mutators and toSpliced (light-cljs-909): no plan; their relative
    // indices convert after the length, in their frame.
    BuiltinInfo { which: Builtin.ArrayPrototypePop, name: "pop" }, BuiltinInfo { which: Builtin.ArrayPrototypeShift, name: "shift" },
    BuiltinInfo { which: Builtin.ArrayPrototypeUnshift, name: "unshift", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeSplice, name: "splice", length: 2 },
    BuiltinInfo { which: Builtin.ArrayPrototypeReverse, name: "reverse" }, BuiltinInfo { which: Builtin.ArrayPrototypeFill, name: "fill", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeCopyWithin, name: "copyWithin", length: 2 }, BuiltinInfo { which: Builtin.ArrayPrototypeToSpliced, name: "toSpliced", length: 2 },
    // sort and toSorted (light-cljs-910): no plan.
    BuiltinInfo { which: Builtin.ArrayPrototypeSort, name: "sort", length: 1 }, BuiltinInfo { which: Builtin.ArrayPrototypeToSorted, name: "toSorted", length: 1 },
    // The copiers (light-cljs-911): no plan.
    BuiltinInfo { which: Builtin.ArrayPrototypeSlice, name: "slice", length: 2 }, BuiltinInfo { which: Builtin.ArrayPrototypeConcat, name: "concat", length: 1 },
    BuiltinInfo { which: Builtin.ArrayPrototypeFlat, name: "flat" }, BuiltinInfo { which: Builtin.ArrayPrototypeToReversed, name: "toReversed" },
    BuiltinInfo { which: Builtin.ArrayPrototypeWith, name: "with", length: 2 }, BuiltinInfo { which: Builtin.ArrayOf, name: "of" },
    BuiltinInfo { which: Builtin.ArrayFrom, name: "from", length: 1 },
    BuiltinInfo { which: Builtin.StringFromCharCode, name: "fromCharCode", length: 1 },
    BuiltinInfo { which: Builtin.StringFromCodePoint, name: "fromCodePoint", length: 1 },
    BuiltinInfo { which: Builtin.StringRaw, name: "raw", length: 1 },
    BuiltinInfo { which: Builtin.StringPrototypeCharAt, name: "charAt", length: 1, plan: plan::StringPosition },
    BuiltinInfo { which: Builtin.StringPrototypeCharCodeAt, name: "charCodeAt", length: 1, plan: plan::StringPosition },
    BuiltinInfo { which: Builtin.StringPrototypeCodePointAt, name: "codePointAt", length: 1, plan: plan::StringPosition },
    BuiltinInfo { which: Builtin.StringPrototypeAt, name: "at", length: 1, plan: plan::StringPosition },
    BuiltinInfo { which: Builtin.StringPrototypeIndexOf, name: "indexOf", length: 1, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeLastIndexOf, name: "lastIndexOf", length: 1, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeIncludes, name: "includes", length: 1, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeStartsWith, name: "startsWith", length: 1, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeEndsWith, name: "endsWith", length: 1, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeIsWellFormed, name: "isWellFormed", plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeToWellFormed, name: "toWellFormed", plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.MapPrototypeKeys, name: "keys" }, BuiltinInfo { which: Builtin.MapPrototypeValues, name: "values" },
    BuiltinInfo { which: Builtin.MapPrototypeEntries, name: "entries" }, BuiltinInfo { which: Builtin.MapPrototypeForEach, name: "forEach", length: 1 },
    BuiltinInfo { which: Builtin.MapIteratorPrototypeNext, name: "next" },
    BuiltinInfo { which: Builtin.SetPrototypeValues, name: "values" }, BuiltinInfo { which: Builtin.SetPrototypeEntries, name: "entries" },
    BuiltinInfo { which: Builtin.SetPrototypeForEach, name: "forEach", length: 1 }, BuiltinInfo { which: Builtin.SetIteratorPrototypeNext, name: "next" },
    BuiltinInfo { which: Builtin.NumberPrototypeToFixed, name: "toFixed", length: 1, plan: plan::NumberRadix },
    BuiltinInfo { which: Builtin.NumberPrototypeToExponential, name: "toExponential", length: 1, plan: plan::NumberRadix },
    BuiltinInfo { which: Builtin.NumberPrototypeToPrecision, name: "toPrecision", length: 1, plan: plan::NumberRadix },
    BuiltinInfo { which: Builtin.StringPrototypeSubstring, name: "substring", length: 2, plan: plan::StringRange },
    BuiltinInfo { which: Builtin.StringPrototypeSubstr, name: "substr", length: 2, plan: plan::StringRange },
    BuiltinInfo { which: Builtin.StringPrototypeSlice, name: "slice", length: 2, plan: plan::StringRange },
    BuiltinInfo { which: Builtin.StringPrototypeConcat, name: "concat", length: 1, plan: plan::StringConcat },
    BuiltinInfo { which: Builtin.StringPrototypePadStart, name: "padStart", length: 1, plan: plan::StringPosition },
    BuiltinInfo { which: Builtin.StringPrototypePadEnd, name: "padEnd", length: 1, plan: plan::StringPosition },
    BuiltinInfo { which: Builtin.StringPrototypeRepeat, name: "repeat", length: 1, plan: plan::StringPosition },
    BuiltinInfo { which: Builtin.StringPrototypeTrim, name: "trim", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeTrimStart, name: "trimStart", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeTrimEnd, name: "trimEnd", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeSplit, name: "split", length: 2, plan: plan::None },
    BuiltinInfo { which: Builtin.StringPrototypeAnchor, name: "anchor", length: 1, plan: plan::StringAttribute },
    BuiltinInfo { which: Builtin.StringPrototypeBig, name: "big", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeBlink, name: "blink", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeBold, name: "bold", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeFixed, name: "fixed", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeFontcolor, name: "fontcolor", length: 1, plan: plan::StringAttribute },
    BuiltinInfo { which: Builtin.StringPrototypeFontsize, name: "fontsize", length: 1, plan: plan::StringAttribute },
    BuiltinInfo { which: Builtin.StringPrototypeItalics, name: "italics", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeLink, name: "link", length: 1, plan: plan::StringAttribute },
    BuiltinInfo { which: Builtin.StringPrototypeSmall, name: "small", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeStrike, name: "strike", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeSub, name: "sub", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeSup, name: "sup", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeToUpperCase, name: "toUpperCase", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeToLowerCase, name: "toLowerCase", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeToLocaleUpperCase, name: "toLocaleUpperCase", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeToLocaleLowerCase, name: "toLocaleLowerCase", length: 0, plan: plan::StringReceiver },
    BuiltinInfo { which: Builtin.StringPrototypeNormalize, name: "normalize", length: 0, plan: plan::StringAttribute },
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
// %Symbol.prototype% (light-cljs-885; symbols.goose), an ordinary object
// inheriting %Object.prototype%, not a wrapper: the [[Prototype]] of symbol
// wrappers and the prototype a symbol primitive's property reads start at.
inline fn symbol_prototype_row() -> i64 { return 41; }
// The Math namespace object (light-cljs-901; math_object.goose), an
// ordinary object inheriting %Object.prototype%, like Reflect.
inline fn math_row() -> i64 { return 42; }
// %RegExp.prototype% (light-cljs-932; regexp_object.goose), an ordinary
// object inheriting %Object.prototype%, not a RegExp; then the RegExp
// template, an ordinary object no JavaScript reaches whose one property,
// `lastIndex` (writable only), gives every RegExp its shape (new_regexp).
inline fn regexp_prototype_row() -> i64 { return 43; }
inline fn regexp_template_row() -> i64 { return 44; }
// %Map.prototype% and %Set.prototype% (light-cljs-943; collections.goose),
// ordinary objects inheriting %Object.prototype%, neither a collection.
inline fn map_prototype_row() -> i64 { return 45; }
inline fn set_prototype_row() -> i64 { return 46; }
inline fn map_iterator_prototype_row() -> i64 { return 47; }
inline fn set_iterator_prototype_row() -> i64 { return 48; }
inline fn fixed_rows() -> i64 { return 49; }
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
// Number's constants (light-cljs-900), in QuickJS's order.
struct NumberConstant { name: Name, value: f64 }
let NUMBER_CONSTANTS: NumberConstant[8] = [
    NumberConstant { name: Name { text: "MAX_VALUE" }, value: 1.7976931348623157e308 }, NumberConstant { name: Name { text: "MIN_VALUE" }, value: 4.9406564584124654e-324 },
    NumberConstant { name: Name { text: "NaN" }, value: 0.0 / 0.0 }, NumberConstant { name: Name { text: "NEGATIVE_INFINITY" }, value: -1.0 / 0.0 },
    NumberConstant { name: Name { text: "POSITIVE_INFINITY" }, value: 1.0 / 0.0 }, NumberConstant { name: Name { text: "EPSILON" }, value: 2.220446049250313e-16 },
    NumberConstant { name: Name { text: "MAX_SAFE_INTEGER" }, value: 9007199254740991.0 }, NumberConstant { name: Name { text: "MIN_SAFE_INTEGER" }, value: -9007199254740991.0 },
];
// Math's value properties (light-cljs-901; ECMAScript 21.3.1), in QuickJS's
// order: static data, as Number's are.
let MATH_CONSTANTS: NumberConstant[8] = [
    NumberConstant { name: Name { text: "E" }, value: 2.718281828459045 }, NumberConstant { name: Name { text: "LN10" }, value: 2.302585092994046 },
    NumberConstant { name: Name { text: "LN2" }, value: 0.6931471805599453 }, NumberConstant { name: Name { text: "LOG2E" }, value: 1.4426950408889634 },
    NumberConstant { name: Name { text: "LOG10E" }, value: 0.4342944819032518 }, NumberConstant { name: Name { text: "PI" }, value: 3.141592653589793 },
    NumberConstant { name: Name { text: "SQRT1_2" }, value: 0.7071067811865476 }, NumberConstant { name: Name { text: "SQRT2" }, value: 1.4142135623730951 },
];
let ERROR_BUILTINS: Builtin[8] = [Builtin.Error, Builtin.EvalError, Builtin.RangeError, Builtin.ReferenceError, Builtin.SyntaxError, Builtin.TypeError, Builtin.URIError, Builtin.InternalError];
// A built-in method: a property `name` (its BuiltinInfo name) of the fixed
// row `holder`. A holder that is itself a REALM_FUNCTIONS row is named by its
// row function (symbol_row, proxy_row), so a fixed row added before them
// moves it with them.
struct Method { which: Builtin, holder: i64 }
let REALM_FUNCTIONS: Method[253] = [
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
    // (light-cljs-851); its other methods follow below.
    Method { which: Builtin.FunctionPrototypeApply, holder: 2 }, Method { which: Builtin.ReflectApply, holder: 34 }, Method { which: Builtin.ReflectConstruct, holder: 34 },
    // Function.prototype.bind (light-cljs-852).
    Method { which: Builtin.FunctionPrototypeBind, holder: 2 },
    // Function.prototype.toString, and %ThrowTypeError%, which is no
    // property: Function.prototype's `caller` and `arguments` accessors hold
    // it as getter and setter (light-cljs-853).
    Method { which: Builtin.FunctionPrototypeToString, holder: 2 }, Method { which: Builtin.ThrowTypeError, holder: 2 },
    // Property descriptors (light-cljs-881; descriptors.goose): Object's
    // and Reflect's descriptor methods and Annex B's accessor methods of
    // Object.prototype.
    Method { which: Builtin.ObjectDefineProperty, holder: 12 }, Method { which: Builtin.ObjectDefineProperties, holder: 12 },
    Method { which: Builtin.ReflectDefineProperty, holder: 34 }, Method { which: Builtin.ReflectGetOwnPropertyDescriptor, holder: 34 }, Method { which: Builtin.ReflectDeleteProperty, holder: 34 },
    Method { which: Builtin.ObjectPrototypeDefineGetter, holder: 1 }, Method { which: Builtin.ObjectPrototypeDefineSetter, holder: 1 },
    Method { which: Builtin.ObjectPrototypeLookupGetter, holder: 1 }, Method { which: Builtin.ObjectPrototypeLookupSetter, holder: 1 },
    // [[Extensible]] and integrity levels (light-cljs-882; integrity.goose).
    Method { which: Builtin.ObjectPreventExtensions, holder: 12 }, Method { which: Builtin.ObjectIsExtensible, holder: 12 }, Method { which: Builtin.ObjectSeal, holder: 12 }, Method { which: Builtin.ObjectIsSealed, holder: 12 },
    Method { which: Builtin.ObjectFreeze, holder: 12 }, Method { which: Builtin.ObjectIsFrozen, holder: 12 }, Method { which: Builtin.ReflectIsExtensible, holder: 34 }, Method { which: Builtin.ReflectPreventExtensions, holder: 34 },
    // Prototypes (light-cljs-883; prototypes.goose). The `__proto__` getter
    // and setter are no properties: Object.prototype's `__proto__` accessor
    // (proto_pair) holds them.
    Method { which: Builtin.ObjectCreate, holder: 12 }, Method { which: Builtin.ObjectSetPrototypeOf, holder: 12 }, Method { which: Builtin.ObjectPrototypeIsPrototypeOf, holder: 1 },
    Method { which: Builtin.ObjectPrototypeProtoGetter, holder: 1 }, Method { which: Builtin.ObjectPrototypeProtoSetter, holder: 1 },
    Method { which: Builtin.ReflectGetPrototypeOf, holder: 34 }, Method { which: Builtin.ReflectSetPrototypeOf, holder: 34 },
    // The Object statics that expose own keys, Reflect.ownKeys and has
    // (light-cljs-884; object_statics.goose).
    Method { which: Builtin.ObjectAssign, holder: 12 }, Method { which: Builtin.ObjectEntries, holder: 12 }, Method { which: Builtin.ObjectFromEntries, holder: 12 },
    Method { which: Builtin.ObjectGetOwnPropertyDescriptors, holder: 12 }, Method { which: Builtin.ObjectGetOwnPropertyNames, holder: 12 }, Method { which: Builtin.ObjectGetOwnPropertySymbols, holder: 12 },
    Method { which: Builtin.ObjectGroupBy, holder: 12 }, Method { which: Builtin.ObjectKeys, holder: 12 }, Method { which: Builtin.ObjectValues, holder: 12 },
    Method { which: Builtin.ObjectPrototypePropertyIsEnumerable, holder: 1 }, Method { which: Builtin.ObjectPrototypeToLocaleString, holder: 1 },
    Method { which: Builtin.ReflectHas, holder: 34 }, Method { which: Builtin.ReflectOwnKeys, holder: 34 },
    // Symbol.for and keyFor on the Symbol function (symbol_row()), and
    // Symbol.prototype's methods (light-cljs-885; symbols.goose): its
    // @@toPrimitive is keyed by the well-known symbol, and its `description`
    // getter is no property but the getter of its accessor pair.
    Method { which: Builtin.SymbolFor, holder: symbol_row() }, Method { which: Builtin.SymbolKeyFor, holder: symbol_row() },
    Method { which: Builtin.SymbolPrototypeToString, holder: 41 }, Method { which: Builtin.SymbolPrototypeValueOf, holder: 41 },
    Method { which: Builtin.SymbolPrototypeDescription, holder: 41 }, Method { which: Builtin.SymbolPrototypeToPrimitive, holder: 41 },
    // Reflect.get and Reflect.set (light-cljs-888; object_statics.goose).
    Method { which: Builtin.ReflectGet, holder: 34 }, Method { which: Builtin.ReflectSet, holder: 34 },
    // Proxy.revocable (light-cljs-891; proxy.goose): a property of the
    // Proxy function (proxy_row()).
    Method { which: Builtin.ProxyRevocable, holder: proxy_row() },
    // The global number functions and Number's predicates (light-cljs-900):
    // parseInt and parseFloat are also Number's own, one function object
    // under two names (build_realm), as Number.parseInt === parseInt.
    Method { which: Builtin.ParseInt, holder: 0 }, Method { which: Builtin.ParseFloat, holder: 0 }, Method { which: Builtin.IsNaN, holder: 0 }, Method { which: Builtin.IsFinite, holder: 0 },
    Method { which: Builtin.NumberIsNaN, holder: 29 }, Method { which: Builtin.NumberIsFinite, holder: 29 }, Method { which: Builtin.NumberIsInteger, holder: 29 }, Method { which: Builtin.NumberIsSafeInteger, holder: 29 },
    // The Math object's functions (light-cljs-901; math_object.goose), in
    // QuickJS's order; its @@toStringTag and constants follow them
    // (build_realm).
    Method { which: Builtin.MathMin, holder: math_row() }, Method { which: Builtin.MathMax, holder: math_row() }, Method { which: Builtin.MathAbs, holder: math_row() },
    Method { which: Builtin.MathFloor, holder: math_row() }, Method { which: Builtin.MathCeil, holder: math_row() }, Method { which: Builtin.MathRound, holder: math_row() },
    Method { which: Builtin.MathSqrt, holder: math_row() }, Method { which: Builtin.MathAcos, holder: math_row() }, Method { which: Builtin.MathAsin, holder: math_row() },
    Method { which: Builtin.MathAtan, holder: math_row() }, Method { which: Builtin.MathAtan2, holder: math_row() }, Method { which: Builtin.MathCos, holder: math_row() },
    Method { which: Builtin.MathExp, holder: math_row() }, Method { which: Builtin.MathLog, holder: math_row() }, Method { which: Builtin.MathPow, holder: math_row() },
    Method { which: Builtin.MathSin, holder: math_row() }, Method { which: Builtin.MathTan, holder: math_row() }, Method { which: Builtin.MathTrunc, holder: math_row() },
    Method { which: Builtin.MathSign, holder: math_row() }, Method { which: Builtin.MathCosh, holder: math_row() }, Method { which: Builtin.MathSinh, holder: math_row() },
    Method { which: Builtin.MathTanh, holder: math_row() }, Method { which: Builtin.MathAcosh, holder: math_row() }, Method { which: Builtin.MathAsinh, holder: math_row() },
    Method { which: Builtin.MathAtanh, holder: math_row() }, Method { which: Builtin.MathExpm1, holder: math_row() }, Method { which: Builtin.MathLog1p, holder: math_row() },
    Method { which: Builtin.MathLog2, holder: math_row() }, Method { which: Builtin.MathLog10, holder: math_row() }, Method { which: Builtin.MathCbrt, holder: math_row() },
    Method { which: Builtin.MathHypot, holder: math_row() }, Method { which: Builtin.MathRandom, holder: math_row() }, Method { which: Builtin.MathF16round, holder: math_row() },
    Method { which: Builtin.MathFround, holder: math_row() }, Method { which: Builtin.MathImul, holder: math_row() }, Method { which: Builtin.MathClz32, holder: math_row() },
    Method { which: Builtin.MathSumPrecise, holder: math_row() },
    // The global RegExp (light-cljs-932; regexp_object.goose), row
    // regexp_row(), then %RegExp.prototype%'s accessors in QuickJS's order
    // (regexp_accessor) and toString, and RegExp[@@species].
    Method { which: Builtin.RegExp, holder: 0 },
    Method { which: Builtin.RegExpPrototypeFlags, holder: regexp_prototype_row() }, Method { which: Builtin.RegExpPrototypeSource, holder: regexp_prototype_row() },
    Method { which: Builtin.RegExpPrototypeGlobal, holder: regexp_prototype_row() }, Method { which: Builtin.RegExpPrototypeIgnoreCase, holder: regexp_prototype_row() },
    Method { which: Builtin.RegExpPrototypeMultiline, holder: regexp_prototype_row() }, Method { which: Builtin.RegExpPrototypeDotAll, holder: regexp_prototype_row() },
    Method { which: Builtin.RegExpPrototypeUnicode, holder: regexp_prototype_row() }, Method { which: Builtin.RegExpPrototypeUnicodeSets, holder: regexp_prototype_row() },
    Method { which: Builtin.RegExpPrototypeSticky, holder: regexp_prototype_row() }, Method { which: Builtin.RegExpPrototypeHasIndices, holder: regexp_prototype_row() },
    Method { which: Builtin.RegExpPrototypeToString, holder: regexp_prototype_row() }, Method { which: Builtin.RegExpSpecies, holder: regexp_row() },
    Method { which: Builtin.RegExpPrototypeMatch, holder: regexp_prototype_row() },
    // RegExp.prototype.exec and test (light-cljs-933).
    Method { which: Builtin.RegExpPrototypeExec, holder: regexp_prototype_row() }, Method { which: Builtin.RegExpPrototypeTest, holder: regexp_prototype_row() },
    // String.prototype.replace and replaceAll, and RegExp.prototype[@@replace],
    // keyed by the well-known symbol (light-cljs-934).
    Method { which: Builtin.StringPrototypeReplace, holder: 27 }, Method { which: Builtin.StringPrototypeReplaceAll, holder: 27 },
    Method { which: Builtin.RegExpPrototypeReplace, holder: regexp_prototype_row() },
    // The global Map and Set (light-cljs-943; collections.goose), rows
    // map_row() and set_row(), each followed by its prototype's methods
    // and `size` getter, in QuickJS's order, and its @@species.
    Method { which: Builtin.Map, holder: 0 },
    Method { which: Builtin.MapPrototypeSet, holder: map_prototype_row() }, Method { which: Builtin.MapPrototypeGet, holder: map_prototype_row() }, Method { which: Builtin.MapPrototypeHas, holder: map_prototype_row() },
    Method { which: Builtin.MapPrototypeDelete, holder: map_prototype_row() }, Method { which: Builtin.MapPrototypeClear, holder: map_prototype_row() }, Method { which: Builtin.MapPrototypeSize, holder: map_prototype_row() },
    Method { which: Builtin.MapSpecies, holder: map_row() },
    Method { which: Builtin.Set, holder: 0 },
    Method { which: Builtin.SetPrototypeAdd, holder: set_prototype_row() }, Method { which: Builtin.SetPrototypeHas, holder: set_prototype_row() }, Method { which: Builtin.SetPrototypeDelete, holder: set_prototype_row() },
    Method { which: Builtin.SetPrototypeClear, holder: set_prototype_row() }, Method { which: Builtin.SetPrototypeSize, holder: set_prototype_row() },
    Method { which: Builtin.SetSpecies, holder: set_row() },
    // Array[@@species] and Array.prototype's remaining callback and search
    // methods (light-cljs-908; array_callbacks.goose, array_search.goose).
    Method { which: Builtin.ArraySpecies, holder: 14 }, Method { which: Builtin.ArrayPrototypeFilter, holder: 3 }, Method { which: Builtin.ArrayPrototypeReduce, holder: 3 },
    Method { which: Builtin.ArrayPrototypeReduceRight, holder: 3 }, Method { which: Builtin.ArrayPrototypeSome, holder: 3 }, Method { which: Builtin.ArrayPrototypeEvery, holder: 3 },
    Method { which: Builtin.ArrayPrototypeFlatMap, holder: 3 }, Method { which: Builtin.ArrayPrototypeFind, holder: 3 }, Method { which: Builtin.ArrayPrototypeFindIndex, holder: 3 },
    Method { which: Builtin.ArrayPrototypeFindLast, holder: 3 }, Method { which: Builtin.ArrayPrototypeFindLastIndex, holder: 3 }, Method { which: Builtin.ArrayPrototypeIndexOf, holder: 3 },
    Method { which: Builtin.ArrayPrototypeLastIndexOf, holder: 3 }, Method { which: Builtin.ArrayPrototypeIncludes, holder: 3 }, Method { which: Builtin.ArrayPrototypeAt, holder: 3 },
    // Array.prototype's mutators and toSpliced (light-cljs-909; array_mutators.goose).
    Method { which: Builtin.ArrayPrototypePop, holder: 3 }, Method { which: Builtin.ArrayPrototypeShift, holder: 3 }, Method { which: Builtin.ArrayPrototypeUnshift, holder: 3 },
    Method { which: Builtin.ArrayPrototypeSplice, holder: 3 }, Method { which: Builtin.ArrayPrototypeReverse, holder: 3 }, Method { which: Builtin.ArrayPrototypeFill, holder: 3 },
    Method { which: Builtin.ArrayPrototypeCopyWithin, holder: 3 }, Method { which: Builtin.ArrayPrototypeToSpliced, holder: 3 },
    // Array.prototype.sort and toSorted (light-cljs-910; array_sort.goose).
    Method { which: Builtin.ArrayPrototypeSort, holder: 3 }, Method { which: Builtin.ArrayPrototypeToSorted, holder: 3 },
    // The copiers, and Array.of and Array.from on the Array constructor
    // (light-cljs-911; array_copy.goose).
    Method { which: Builtin.ArrayPrototypeSlice, holder: 3 }, Method { which: Builtin.ArrayPrototypeConcat, holder: 3 }, Method { which: Builtin.ArrayPrototypeFlat, holder: 3 },
    Method { which: Builtin.ArrayPrototypeToReversed, holder: 3 }, Method { which: Builtin.ArrayPrototypeWith, holder: 3 },
    Method { which: Builtin.ArrayOf, holder: 14 }, Method { which: Builtin.ArrayFrom, holder: 14 },
    Method { which: Builtin.StringFromCharCode, holder: 30 },
    Method { which: Builtin.StringFromCodePoint, holder: 30 },
    Method { which: Builtin.StringRaw, holder: 30 },
    Method { which: Builtin.StringPrototypeCharAt, holder: 27 },
    Method { which: Builtin.StringPrototypeCharCodeAt, holder: 27 },
    Method { which: Builtin.StringPrototypeCodePointAt, holder: 27 },
    Method { which: Builtin.StringPrototypeAt, holder: 27 },
    Method { which: Builtin.StringPrototypeIndexOf, holder: 27 },
    Method { which: Builtin.StringPrototypeLastIndexOf, holder: 27 },
    Method { which: Builtin.StringPrototypeIncludes, holder: 27 },
    Method { which: Builtin.StringPrototypeStartsWith, holder: 27 },
    Method { which: Builtin.StringPrototypeEndsWith, holder: 27 },
    Method { which: Builtin.StringPrototypeIsWellFormed, holder: 27 },
    Method { which: Builtin.StringPrototypeToWellFormed, holder: 27 },
    Method { which: Builtin.MapPrototypeKeys, holder: map_prototype_row() }, Method { which: Builtin.MapPrototypeValues, holder: map_prototype_row() },
    Method { which: Builtin.MapPrototypeEntries, holder: map_prototype_row() }, Method { which: Builtin.MapPrototypeForEach, holder: map_prototype_row() },
    Method { which: Builtin.MapIteratorPrototypeNext, holder: map_iterator_prototype_row() },
    Method { which: Builtin.SetPrototypeValues, holder: set_prototype_row() }, Method { which: Builtin.SetPrototypeEntries, holder: set_prototype_row() },
    Method { which: Builtin.SetPrototypeForEach, holder: set_prototype_row() }, Method { which: Builtin.SetIteratorPrototypeNext, holder: set_iterator_prototype_row() },
    Method { which: Builtin.NumberPrototypeToFixed, holder: wrapper_prototype_row(0) },
    Method { which: Builtin.NumberPrototypeToExponential, holder: wrapper_prototype_row(0) },
    Method { which: Builtin.NumberPrototypeToPrecision, holder: wrapper_prototype_row(0) },
    Method { which: Builtin.StringPrototypeSubstring, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeSubstr, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeSlice, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeConcat, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypePadStart, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypePadEnd, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeRepeat, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeTrim, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeTrimStart, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeTrimEnd, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeSplit, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeAnchor, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeBig, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeBlink, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeBold, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeFixed, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeFontcolor, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeFontsize, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeItalics, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeLink, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeSmall, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeStrike, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeSub, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeSup, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeToUpperCase, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeToLowerCase, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeToLocaleUpperCase, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeToLocaleLowerCase, holder: wrapper_prototype_row(1) },
    Method { which: Builtin.StringPrototypeNormalize, holder: wrapper_prototype_row(1) },
];
// cljs.core's ExceptionInfo.prototype (light-cljs-872), after the
// REALM_FUNCTIONS: an ordinary object inheriting %Error.prototype%, the
// [[Prototype]] of what rt.core.exInfo and the ExceptionInfo function make.
// Then the ExceptionInfo function itself (light-cljs-875), the realm's last
// row: a built-in with [[Construct]] that rt.core.ExceptionInfo names. As
// the V8 build's runtime (src/bbp_runtime.js) defines an ordinary function,
// its `prototype` is writable, and the prototype's `constructor` is it.
// Then the prototype's `toString` (light-cljs-876), the realm's last row:
// the V8 build's cljs printer (`#error {:message ...}`, printer.goose), a
// writable, enumerable, configurable data property, as the runtime's
// assignment makes it.
inline fn exception_info_prototype_row() -> i64 { return fixed_rows() + REALM_FUNCTIONS.len; }
inline fn exception_info_constructor_row() -> i64 { return exception_info_prototype_row() + 1; }
inline fn exception_info_to_string_row() -> i64 { return exception_info_constructor_row() + 1; }
// Then the empty list, vector, map and set (light-cljs-878), in
// node_kind::List..Set order: the V8 build's runtime reads its `EMPTY`
// singletons for `'()`, `[]`, `{}` and `#{}` (List.EMPTY,
// PersistentVector.EMPTY, PersistentArrayMap.EMPTY, PersistentHashSet.EMPTY),
// so every empty literal of a kind, in every unit, job and worker, is one
// object. A literal that evaluates to one (Ins.Literal of no operands but a
// map's, form_op::LiteralMap of no entries) answers this row instead of a
// new node. Forms are immutable and have no own properties, so no job
// writes it (a new collection, with-meta's included, is a new row); the map
// and set are insertion-ordered (map_order::Insertion), as the array map and
// set the runtime's are. A form the compiler hands a job (the form bridge)
// is a new row, so an imported `[]` is not identical to a literal one, as in
// the V8 build. In a publication or cell value the row keeps its identity
// (form_bridge.goose realm_empty, cells.goose realm_identity).
inline fn empty_form_row(kind: i64) -> i64 { return exception_info_to_string_row() + 1 + kind - node_kind::List; }
inline fn empty_form(o: i64) -> bool { return o >= empty_form_row(node_kind::List) && o <= empty_form_row(node_kind::Set); }
inline fn realm_nodes() -> i64 { return empty_form_row(node_kind::Set) + 1; }
// The node row of built-in b's REALM_FUNCTIONS entry.
fn realm_function_row(b: Builtin) -> i64 {
    for m, j in REALM_FUNCTIONS { if m.which == b { return fixed_rows() + j; } }
    abort("builtin is not in REALM_FUNCTIONS"); return 0;
}
// The Symbol function's row: its REALM_FUNCTIONS entry, the eleventh; and
// the Proxy function's, the thirty-fifth (build_realm checks both).
inline fn symbol_row() -> i64 { return fixed_rows() + 10; }
inline fn proxy_row() -> i64 { return fixed_rows() + 34; }
// The RegExp function's row, its REALM_FUNCTIONS entry, the 133rd (light-cljs-932).
inline fn regexp_row() -> i64 { return fixed_rows() + 132; }
// The Map and Set functions' rows, their REALM_FUNCTIONS entries, the
// 152nd and 160th (light-cljs-943).
inline fn map_row() -> i64 { return fixed_rows() + 151; }
inline fn set_row() -> i64 { return fixed_rows() + 159; }
// A pending property of the fixed row `holder`: a name, or with `symbol`
// >= 0 a well-known symbol (export_well_known_symbol).
struct PendingName { holder: i64, key: u8[..32], symbol: i64 = -1 }
let PENDING: PendingName[74] = [
    // Object.prototype
    // Array.prototype

    PendingName { holder: 3, key: "toLocaleString" },

    // Object
    // %GeneratorFunction%, %AsyncFunction% and %AsyncGeneratorFunction%
    // (function_kind_prototype_row; the generators area, light-cljs-768).
    PendingName { holder: 38, key: "constructor" }, PendingName { holder: 39, key: "constructor" }, PendingName { holder: 40, key: "constructor" },
    // Reflect (reflect_row()): apply and construct are light-cljs-851's,
    // defineProperty, deleteProperty and getOwnPropertyDescriptor light-cljs-881's,
    // isExtensible and preventExtensions light-cljs-882's, getPrototypeOf
    // and setPrototypeOf light-cljs-883's, has and ownKeys light-cljs-884's,
    // get and set light-cljs-888's: none is pending.
    // %IteratorPrototype% (its helpers), %GeneratorPrototype% and
    // %Promise.prototype% (light-cljs-790; the Promise area makes the last's).
    PendingName { holder: 23, key: "constructor" }, PendingName { holder: 23, key: "drop" }, PendingName { holder: 23, key: "every" }, PendingName { holder: 23, key: "filter" },
    PendingName { holder: 23, key: "find" }, PendingName { holder: 23, key: "flatMap" }, PendingName { holder: 23, key: "forEach" }, PendingName { holder: 23, key: "map" },
    PendingName { holder: 23, key: "reduce" }, PendingName { holder: 23, key: "some" }, PendingName { holder: 23, key: "take" }, PendingName { holder: 23, key: "toArray" },
    PendingName { holder: 23, key: "", symbol: 4 }, PendingName { holder: 24, key: "constructor" },
    PendingName { holder: 25, key: "constructor" }, PendingName { holder: 25, key: "then" }, PendingName { holder: 25, key: "catch" }, PendingName { holder: 25, key: "finally" },
    // Array
    PendingName { holder: 14, key: "fromAsync" },
    // The global object's other standard names.
    PendingName { holder: 0, key: "AggregateError" }, PendingName { holder: 0, key: "ArrayBuffer" }, PendingName { holder: 0, key: "Atomics" }, PendingName { holder: 0, key: "BigInt" },
    PendingName { holder: 0, key: "BigInt64Array" }, PendingName { holder: 0, key: "BigUint64Array" }, PendingName { holder: 0, key: "DataView" },
    PendingName { holder: 0, key: "Date" }, PendingName { holder: 0, key: "FinalizationRegistry" }, PendingName { holder: 0, key: "Float32Array" }, PendingName { holder: 0, key: "Float64Array" },
    PendingName { holder: 0, key: "Int8Array" }, PendingName { holder: 0, key: "Int16Array" }, PendingName { holder: 0, key: "Int32Array" }, PendingName { holder: 0, key: "Iterator" },
    PendingName { holder: 0, key: "JSON" },
    PendingName { holder: 0, key: "Promise" },
    PendingName { holder: 0, key: "SharedArrayBuffer" },
    PendingName { holder: 0, key: "Uint8Array" }, PendingName { holder: 0, key: "Uint8ClampedArray" }, PendingName { holder: 0, key: "Uint16Array" }, PendingName { holder: 0, key: "Uint32Array" },
    PendingName { holder: 0, key: "WeakMap" }, PendingName { holder: 0, key: "WeakRef" }, PendingName { holder: 0, key: "WeakSet" }, PendingName { holder: 0, key: "decodeURI" },
    PendingName { holder: 0, key: "decodeURIComponent" }, PendingName { holder: 0, key: "encodeURI" }, PendingName { holder: 0, key: "encodeURIComponent" }, PendingName { holder: 0, key: "escape" },
    PendingName { holder: 0, key: "unescape" },
    // %Number.prototype% (light-cljs-832; the core-builtins area makes
    // them, light-cljs-767; Number's statics are light-cljs-900's).
    PendingName { holder: 26, key: "toLocaleString" },
    // String and %String.prototype%.

    PendingName { holder: 27, key: "match" }, PendingName { holder: 27, key: "matchAll" }, PendingName { holder: 27, key: "search" },

    // localeCompare is Intl.Collator's (light-cljs-774).
    PendingName { holder: 27, key: "localeCompare" },

    // RegExp and %RegExp.prototype% (light-cljs-932): the symbol methods
    // are light-cljs-770's fourth to sixth children's, escape and compile
    // its eighth's (exec and test are light-cljs-933's, @@replace
    // light-cljs-934's).
    PendingName { holder: regexp_row(), key: "escape" },
    PendingName { holder: regexp_prototype_row(), key: "compile" },
    PendingName { holder: regexp_prototype_row(), key: "", symbol: well_known::MatchAll }, PendingName { holder: regexp_prototype_row(), key: "", symbol: well_known::Search },
    PendingName { holder: regexp_prototype_row(), key: "", symbol: well_known::Split },
    // Map, Set and their prototypes: Map.groupBy, getOrInsert,
    // getOrInsertComputed and the Set methods that take another set are
    // light-cljs-953's.
    PendingName { holder: map_row(), key: "groupBy" },
    PendingName { holder: map_prototype_row(), key: "getOrInsert" }, PendingName { holder: map_prototype_row(), key: "getOrInsertComputed" },
    PendingName { holder: set_prototype_row(), key: "isDisjointFrom" }, PendingName { holder: set_prototype_row(), key: "isSubsetOf" },
    PendingName { holder: set_prototype_row(), key: "isSupersetOf" }, PendingName { holder: set_prototype_row(), key: "intersection" }, PendingName { holder: set_prototype_row(), key: "difference" },
    PendingName { holder: set_prototype_row(), key: "symmetricDifference" }, PendingName { holder: set_prototype_row(), key: "union" },
];
// The type names `typeof` gives, interned with the realm.
let TYPE_NAMES: Name[8] = [Name { text: "undefined" }, Name { text: "object" }, Name { text: "boolean" }, Name { text: "number" }, Name { text: "string" }, Name { text: "symbol" }, Name { text: "bigint" }, Name { text: "function" }];
// The light-cljs runtime leaves boot_runtime installs on `rt` (core false)
// or `rt.core`, in this order.
struct Leaf { core: bool, key: u8[..24], which: Builtin }
let RUNTIME_LEAVES: Leaf[50] = [
    Leaf { core: true, key: "macroReaderFunction", which: Builtin.MacroReaderFunction }, Leaf { core: true, key: "inc", which: Builtin.Inc },
    Leaf { core: true, key: "mapv", which: Builtin.Mapv }, Leaf { core: true, key: "vec", which: Builtin.Vec }, Leaf { core: true, key: "into", which: Builtin.Into },
    Leaf { core: true, key: "sequence", which: Builtin.Sequence }, Leaf { core: true, key: "mapSeq", which: Builtin.MapSeq }, Leaf { core: true, key: "list", which: Builtin.List },
    // The emitted call classifier compares against intrinsicRegExp's
    // identity: the realm's RegExp constructor itself (light-cljs-935;
    // boot_runtime), no built-in row of its own.
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
    // struct type: a constructor whose construction is its call (Delay),
    // or the ExceptionInfo function (light-cljs-875).
    Leaf { core: false, key: "nativeConstruct", which: Builtin.NativeConstruct },
    // Compiled interop's receiver and operand checks and the Object
    // declaration demand of a member read (light-cljs-872).
    Leaf { core: false, key: "nativeInterop", which: Builtin.NativeInterop }, Leaf { core: false, key: "declarationReceiver", which: Builtin.DeclarationReceiver },
    // ex-info and its readers, and get-in (light-cljs-872).
    Leaf { core: true, key: "exInfo", which: Builtin.ExInfo }, Leaf { core: true, key: "exData", which: Builtin.ExData },
    Leaf { core: true, key: "exMessage", which: Builtin.ExMessage }, Leaf { core: true, key: "exCause", which: Builtin.ExCause },
    Leaf { core: true, key: "getIn", which: Builtin.GetIn },
    // The ExceptionInfo function (light-cljs-875): no new object but the
    // realm row exception_info_constructor_row, so it is the `constructor`
    // of what exInfo makes. A helper publication's graph walk from `rt`
    // stops at realm rows (callable.goose graph), so the constructor and
    // prototype cycles it reaches are no rejected cycle.
    Leaf { core: true, key: "ExceptionInfo", which: Builtin.ExceptionInfo },
    // `instance?` of a class that is no js/ global (light-cljs-875).
    Leaf { core: true, key: "isInstance", which: Builtin.IsInstance },
    // str, pr-str and identical? (light-cljs-876; printer.goose).
    Leaf { core: true, key: "str", which: Builtin.Str }, Leaf { core: true, key: "prStr", which: Builtin.PrStr },
    Leaf { core: true, key: "identical", which: Builtin.Identical },
    // range (light-cljs-877; core_range).
    Leaf { core: true, key: "range", which: Builtin.Range },
    // A deftype's or defrecord's type (light-cljs-920; native_type).
    Leaf { core: false, key: "nativeType", which: Builtin.NativeType },
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
// Object.prototype's `__proto__` accessor (light-cljs-883): accessor row 2,
// after the pending pair and %ThrowTypeError%'s.
inline fn proto_pair() -> i64 { return 2; }
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
    return kind_prototype(heap, o);
}
// What Empty means as node o's [[Prototype]] (prototype).
fn kind_prototype(heap: const Heap&, o: i64) -> Value {
    if !heap.global.is_object() { return Value.Null; }
    return match node(heap, o).kind {
        node_kind::Object, node_kind::Arguments, node_kind::Struct => object(object_prototype_row()), node_kind::Array => object(array_prototype_row()), node_kind::Builtin, node_kind::BoundFunction => object(function_prototype_row()), node_kind::Error => object(error_prototype_row(0)),
        node_kind::Closure => { let k = function_kind_of(heap, o); object(if k < 0 { function_prototype_row() } else { function_kind_prototype_row(k) }) },
        node_kind::Wrapper => primitive_prototype(node(heap, o).value),
        _ => Value.Null,
    };
}
// The wrapper type (WRAPPER_NAMES) of primitive v, -1 for one without a
// wrapper type here: undefined and null have none, a Symbol wrapper
// (light-cljs-885) has its own checks (symbols.goose this_symbol), and
// BigInt wrappers belong to their area (light-cljs-771).
fn wrapper_type(v: Value) -> i64 { return match v { Number => 0, String => 1, Bool => 2, _ => -1 }; }
// The realm's prototype for primitive v's wrapper (ToObject's), Null for a
// primitive without one: a symbol's is %Symbol.prototype% (light-cljs-885).
fn primitive_prototype(v: Value) -> Value { if v.is_symbol() { return object(symbol_prototype_row()); } let t = wrapper_type(v); return if t < 0 { Value.Null } else { object(wrapper_prototype_row(t)) }; }
// Whether node o is a realm row: the image owner made it in initialize.
inline fn realm_row(o: i64) -> bool { return o >= 0 && o < realm_nodes(); }
// Whether a compiler job reads node o through its prototype chain although
// the image holds it (Job.protected_nodes): every realm row but the global
// object, whose names the compiler installs and which lacks many of the V8
// build's, so a name it does not have stays a NativeBoundary there.
inline fn realm_intrinsic(o: i64) -> bool { return o > global_row() && o < realm_nodes(); }
// Whether node o is the realm's %Error.prototype%, a native error type's
// prototype or ExceptionInfo.prototype (light-cljs-875); a heap built
// without initialize has none.
fn error_prototype(heap: const Heap&, o: i64) -> bool {
    return heap.global.is_object() && (o >= error_prototype_row(0) && o < error_prototype_row(ERROR_NAMES.len) || o == exception_info_prototype_row());
}
// Whether an image's first rows are this realm's layout (snapshot::validate):
// the fixed rows' kinds, prototypes and built-in codes, one built-in per
// REALM_FUNCTIONS entry, the pending pair at accessor row 0, the global at
// row 0 and Function.prototype.call as the call method. Their properties are
// validated with every other row's.
fn realm_valid(nodes: const Node[:], accessors: const Pair[:], global: Value, call: Value) -> bool {
    if nodes.len < realm_nodes() || accessors.len <= proto_pair() || !accessors[0].get.is_undefined() || !accessors[0].set.is_undefined() || !global.is_object() || global.id() != global_row() { return false; }
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
    let iterator_prototypes: i64[] = [array_iterator_prototype_row(), string_iterator_prototype_row(), map_iterator_prototype_row(), set_iterator_prototype_row()];
    for o in iterator_prototypes { if nodes[o].kind != node_kind::Object || !nodes[o].proto.is_object() || nodes[o].proto.id() != iterator_prototype_row() { return false; } }
    // Symbol.prototype is an ordinary object (light-cljs-885).
    if nodes[symbol_prototype_row()].kind != node_kind::Object || !nodes[symbol_prototype_row()].proto.is_object() || nodes[symbol_prototype_row()].proto.id() != object_prototype_row() { return false; }
    // Reflect (light-cljs-851) and Math (light-cljs-901) are ordinary objects.
    for o in [reflect_row(), math_row()] { if nodes[o].kind != node_kind::Object || !nodes[o].proto.is_object() || nodes[o].proto.id() != object_prototype_row() { return false; } }
    // RegExp.prototype (light-cljs-932) is an ordinary object; the RegExp
    // template is shape-mode with `lastIndex` 0 alone, no elements and no
    // prototype, since every RegExp takes its shape (new_regexp).
    let rp .= nodes[regexp_prototype_row()]; let rt .= nodes[regexp_template_row()];
    if rp.kind != node_kind::Object || !rp.proto.is_object() || rp.proto.id() != object_prototype_row() || rt.kind != node_kind::Object || rt.initialized || rt.shape == 0 || rt.key_count != 1 || rt.count != 0 || !rt.proto.is_null() || !same_bits(rt.slots[0], number(0)) { return false; }
    // %Map.prototype% and %Set.prototype% (light-cljs-943) are ordinary objects.
    for o in [map_prototype_row(), set_prototype_row()] { if nodes[o].kind != node_kind::Object || !nodes[o].proto.is_object() || nodes[o].proto.id() != object_prototype_row() { return false; } }
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
    // Object.prototype's `__proto__` pair (light-cljs-883) holds its getter and setter.
    let pp .= accessors[proto_pair()]; let getter = realm_function_row(Builtin.ObjectPrototypeProtoGetter); let setter = realm_function_row(Builtin.ObjectPrototypeProtoSetter);
    if !pp.get.is_object() || pp.get.id() != getter || !pp.set.is_object() || pp.set.id() != setter { return false; }
    // ExceptionInfo.prototype (light-cljs-872) and the ExceptionInfo function (light-cljs-875).
    let e .= nodes[exception_info_prototype_row()];
    if e.kind != node_kind::Object || !e.proto.is_object() || e.proto.id() != error_prototype_row(0) { return false; }
    let f .= nodes[exception_info_constructor_row()];
    if f.kind != node_kind::Builtin || f.function != code(Builtin.ExceptionInfo) || !f.proto.is_hole() { return false; }
    let t .= nodes[exception_info_to_string_row()];
    if t.kind != node_kind::Builtin || t.function != code(Builtin.ExceptionInfoPrototypeToString) || !t.proto.is_hole() { return false; }
    // The empty collection literals (light-cljs-878): no values, metadata,
    // properties or provenance, insertion-ordered.
    for k in node_kind::List..(node_kind::Set + 1) {
        let n .= nodes[empty_form_row(k)];
        if n.kind != k || n.count != 0 || n.length != map_order::Insertion || !n.metadata.is_undefined() || n.key_count != 0 || n.key_capacity != 0 || n.shape != 0 || n.initialized || n.origin != -1 || n.source != no_row() { return false; }
    }
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
    rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) });
    rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) });
    rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) }); rows.push(Node { kind: node_kind::Object, proto: Value.Null });
    rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) }); rows.push(Node { kind: node_kind::Object, proto: object(object_prototype_row()) });
    rows.push(Node { kind: node_kind::Object, proto: object(iterator_prototype_row()) }); rows.push(Node { kind: node_kind::Object, proto: object(iterator_prototype_row()) });
    assert(rows.len == fixed_rows());
    for m in REALM_FUNCTIONS { rows.push(Node { kind: node_kind::Builtin, function: code(m.which) }); }
    rows.push(Node { kind: node_kind::Object, proto: object(error_prototype_row(0)) });
    rows.push(Node { kind: node_kind::Builtin, function: code(Builtin.ExceptionInfo) });
    rows.push(Node { kind: node_kind::Builtin, function: code(Builtin.ExceptionInfoPrototypeToString) });
    for k in node_kind::List..(node_kind::Set + 1) { rows.push(Node { kind: k }); }
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
        // Object.prototype.__proto__ (light-cljs-883; ECMAScript 20.1.3.10):
        // one accessor pair, the getter's row and the setter's next to it,
        // configurable, not enumerable.
        if m.which == Builtin.ObjectPrototypeProtoSetter { continue; }
        // Symbol.prototype[@@toPrimitive] (ECMAScript 20.4.3.5),
        // configurable only, and its `description` (20.4.3.2), an accessor
        // with no setter, configurable (light-cljs-885).
        if m.which == Builtin.SymbolPrototypeToPrimitive { if !add_own(heap, m.holder, Value.Symbol { s: well_known::ToPrimitive }, object(fixed_rows() + j), attr_configurable()) { return false; } continue; }
        // RegExp.prototype[@@match] (light-cljs-932), writable and configurable.
        if m.which == Builtin.RegExpPrototypeMatch { if !add_own(heap, m.holder, Value.Symbol { s: well_known::Match }, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; } continue; }
        // RegExp.prototype[@@replace] (light-cljs-934), writable and configurable.
        if m.which == Builtin.RegExpPrototypeReplace { if !add_own(heap, m.holder, Value.Symbol { s: well_known::Replace }, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; } continue; }
        if m.which == Builtin.SymbolPrototypeDescription {
            let a = add_accessor(heap, Pair { get: object(fixed_rows() + j) });
            if a < 0 { if heap.fault == Fault.None { memory(heap); } return false; }
            let k = key_named(heap, "description");
            if heap.fault != Fault.None || !add_own(heap, m.holder, k, Value.Accessor { a: a as u32 }, attr_configurable()) { return false; }
            continue;
        }
        // %RegExp.prototype%'s getters and RegExp[@@species] (light-cljs-932;
        // ECMAScript 22.2.6 and 22.2.5.2): accessors with no setter,
        // configurable, each keyed by its function's name after "get ".
        if regexp_accessor(m.which) {
            let a = add_accessor(heap, Pair { get: object(fixed_rows() + j) });
            if a < 0 { if heap.fault == Fault.None { memory(heap); } return false; }
            let k: Value = if m.which == Builtin.RegExpSpecies { Value.Symbol { s: well_known::Species } } else { key_named(heap, BUILTINS[code(m.which)].name[4..]) };
            if heap.fault != Fault.None || !add_own(heap, m.holder, k, Value.Accessor { a: a as u32 }, attr_configurable()) { return false; }
            continue;
        }
        // The size getters and Map[@@species] and Set[@@species]
        // (light-cljs-943; ECMAScript 24.1.3.10, 24.2.3.9, 24.1.2.2 and
        // 24.2.2.2): accessors with no setter, configurable.
        // Array[@@species] (light-cljs-908; ECMAScript 23.1.2.5): an accessor
        // with no setter, configurable.
        if m.which == Builtin.ArraySpecies {
            let a = add_accessor(heap, Pair { get: object(fixed_rows() + j) });
            if a < 0 { if heap.fault == Fault.None { memory(heap); } return false; }
            if !add_own(heap, m.holder, Value.Symbol { s: well_known::Species }, Value.Accessor { a: a as u32 }, attr_configurable()) { return false; }
            continue;
        }
        if collection_accessor(m.which) {
            let a = add_accessor(heap, Pair { get: object(fixed_rows() + j) });
            if a < 0 { if heap.fault == Fault.None { memory(heap); } return false; }
            let k: Value = if m.which == Builtin.MapSpecies || m.which == Builtin.SetSpecies { Value.Symbol { s: well_known::Species } } else { key_named(heap, "size") };
            if heap.fault != Fault.None || !add_own(heap, m.holder, k, Value.Accessor { a: a as u32 }, attr_configurable()) { return false; }
            continue;
        }
        if m.which == Builtin.ObjectPrototypeProtoGetter {
            let a = add_accessor(heap, Pair { get: object(fixed_rows() + j), set: object(fixed_rows() + j + 1) });
            if a < 0 { if heap.fault == Fault.None { memory(heap); } return false; }
            if a != proto_pair() { heap.fault = Fault.InvalidState; return false; }
            let k = key_named(heap, "__proto__");
            if heap.fault != Fault.None || !add_own(heap, m.holder, k, Value.Accessor { a: a as u32 }, attr_configurable()) { return false; }
            continue;
        }
        if !realm_value(heap, m.holder, BUILTINS[code(m.which)].name, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; }
        if m.which == Builtin.FunctionPrototypeCall { heap.call_method = object(fixed_rows() + j); }
        // Array.prototype[@@iterator] is the `values` function object (ECMAScript 23.1.3.40).
        if m.which == Builtin.ArrayPrototypeValues && !add_own(heap, m.holder, Value.Symbol { s: well_known::Iterator }, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; }
        // Number.parseInt and Number.parseFloat are the global functions
        // (ECMAScript 21.1.2.12 and 21.1.2.13), writable and configurable.
        if (m.which == Builtin.ParseInt || m.which == Builtin.ParseFloat) && !realm_value(heap, wrapper_constructor_row(0), BUILTINS[code(m.which)].name, object(fixed_rows() + j), attr_writable() | attr_configurable()) { return false; }
    }
    // Number's value properties (ECMAScript 21.1.2), neither writable,
    // enumerable nor configurable.
    for c in NUMBER_CONSTANTS { if !realm_value(heap, wrapper_constructor_row(0), c.name.text, number(c.value), 0) { return false; } }
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
    // Annex B trim aliases share their function objects and canonical names.
    if !realm_value(heap, wrapper_prototype_row(1), "trimLeft", object(realm_function_row(Builtin.StringPrototypeTrimStart)), attr_writable() | attr_configurable())
        || !realm_value(heap, wrapper_prototype_row(1), "trimRight", object(realm_function_row(Builtin.StringPrototypeTrimEnd)), attr_writable() | attr_configurable()) { return false; }
    // Collection aliases are the same function objects, including their names.
    if !add_own(heap, map_prototype_row(), Value.Symbol { s: well_known::Iterator }, object(realm_function_row(Builtin.MapPrototypeEntries)), attr_writable() | attr_configurable())
        || !add_own(heap, set_prototype_row(), Value.Symbol { s: well_known::Iterator }, object(realm_function_row(Builtin.SetPrototypeValues)), attr_writable() | attr_configurable())
        || !add_own(heap, set_prototype_row(), key_named(heap, "keys"), object(realm_function_row(Builtin.SetPrototypeValues)), attr_writable() | attr_configurable())
        || !add_own(heap, map_iterator_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Map Iterator"), attr_configurable())
        || !add_own(heap, set_iterator_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Set Iterator"), attr_configurable()) { return false; }
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
    // Math[@@toStringTag] (ECMAScript 21.3.1.9), configurable only, after
    // its functions, then its value properties (21.3.1.1 to 21.3.1.8),
    // neither writable, enumerable nor configurable, in QuickJS's order
    // (light-cljs-901).
    if !add_own(heap, math_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Math"), attr_configurable()) { return false; }
    for c in MATH_CONSTANTS { if !realm_value(heap, math_row(), c.name.text, number(c.value), 0) { return false; } }
    // Symbol.prototype and its `constructor`, and its @@toStringTag (ECMAScript
    // 20.4.3.6), configurable only (light-cljs-885).
    if !link_constructor(heap, symbol_row(), symbol_prototype_row()) || !add_own(heap, symbol_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Symbol"), attr_configurable()) { return false; }
    // The well-known symbols, Symbol's non-writable, non-enumerable,
    // non-configurable data properties; their values are the exporter's
    // fixed indices (decision 2), so they take no rows.
    assert(REALM_FUNCTIONS[symbol_row() - fixed_rows()].which == Builtin.SymbolFunction && REALM_FUNCTIONS[realm_function_row(Builtin.SymbolFor) - fixed_rows()].holder == symbol_row());
    assert(REALM_FUNCTIONS[proxy_row() - fixed_rows()].which == Builtin.Proxy && REALM_FUNCTIONS[realm_function_row(Builtin.ProxyRevocable) - fixed_rows()].holder == proxy_row());
    // RegExp and RegExp.prototype (light-cljs-932): `prototype` and
    // `constructor`, and the template's `lastIndex`, writable only.
    assert(REALM_FUNCTIONS[regexp_row() - fixed_rows()].which == Builtin.RegExp);
    if !link_constructor(heap, regexp_row(), regexp_prototype_row()) || !realm_value(heap, regexp_template_row(), "lastIndex", number(0), attr_writable()) { return false; }
    // Map and Set (light-cljs-943): `prototype` and `constructor`, and the
    // prototypes' @@toStringTag (ECMAScript 24.1.3.13, 24.2.3.12),
    // configurable only.
    assert(REALM_FUNCTIONS[map_row() - fixed_rows()].which == Builtin.Map && REALM_FUNCTIONS[set_row() - fixed_rows()].which == Builtin.Set);
    if !link_constructor(heap, map_row(), map_prototype_row()) || !link_constructor(heap, set_row(), set_prototype_row()) { return false; }
    if !add_own(heap, map_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Map"), attr_configurable()) || !add_own(heap, set_prototype_row(), Value.Symbol { s: well_known::ToStringTag }, key_named(heap, "Set"), attr_configurable()) { return false; }
    var s: i64 = 0;
    while export_well_known_symbol(s).len > 0 { if !realm_value(heap, symbol_row(), export_well_known_symbol(s), Value.Symbol { s: s as u32 }, 0) { return false; } s += 1; }
    // The global object's own properties.
    if !realm_value(heap, global_row(), "globalThis", heap.global, attr_writable() | attr_configurable()) { return false; }
    let globals: i64[] = [object_constructor_row(), function_constructor_row(), array_constructor_row(), wrapper_constructor_row(0), wrapper_constructor_row(1), wrapper_constructor_row(2)];
    for o in globals { if !realm_value(heap, global_row(), BUILTINS[node(heap, o).function].name, object(o), attr_writable() | attr_configurable()) { return false; } }
    if !realm_value(heap, global_row(), "Reflect", object(reflect_row()), attr_writable() | attr_configurable()) || !realm_value(heap, global_row(), "Math", object(math_row()), attr_writable() | attr_configurable()) { return false; }
    for t in ERROR_NAMES.len { if !realm_value(heap, global_row(), ERROR_NAMES[t].text, object(error_constructor_row(t)), attr_writable() | attr_configurable()) { return false; } }
    if !realm_value(heap, global_row(), "NaN", number(0.0 / 0.0), 0) || !realm_value(heap, global_row(), "Infinity", number(1.0 / 0.0), 0) || !realm_value(heap, global_row(), "undefined", Value.Undefined, 0) { return false; }
    for p in PENDING {
        let k: Value = if p.symbol >= 0 { Value.Symbol { s: p.symbol as u32 } } else { key_named(heap, p.key) };
        if heap.fault != Fault.None || !add_own(heap, p.holder, k, pending(), attr_configurable()) { return false; }
    }
    // The ExceptionInfo function's `prototype` (an ordinary function's:
    // writable, neither enumerable nor configurable), its prototype's
    // `constructor`, and its `toString` (light-cljs-876).
    let ei = exception_info_constructor_row(); let eip = exception_info_prototype_row();
    if !realm_value(heap, ei, "prototype", object(eip), attr_writable()) || !realm_value(heap, eip, "constructor", object(ei), attr_writable() | attr_configurable()) { return false; }
    if !realm_value(heap, eip, "toString", object(exception_info_to_string_row()), attr_writable() | attr_enumerable() | attr_configurable()) { return false; }
    for n in TYPE_NAMES { key_named(heap, n.text); }
    if heap.fault != Fault.None { return false; }
    assert(heap.nodes.len == realm_nodes());
    return compact_realm_slots(heap);
}
// Shape overflow growth leaves old slot runs behind while the realm is
// built. Keep only the current runs before this immutable image is copied
// into every prepared definition (light-cljs-903's 200k gate), each cut to
// the slots its properties use (light-cljs-904): the doubling growth's spare
// slots are a quarter of the realm's values, and a job cannot add to a realm
// object it does not own (NativeBoundary); a job that owns the realm grows
// a full run as usual. The global object keeps its spare slots, which the
// runtime's boot and the compiler session fill. Object rows, shapes,
// properties and observable values keep their identity; only these private
// slot offsets and capacities change. Realm nodes have no element runs.
fn compact_realm_slots(heap: Heap&) -> bool {
    if !charge(heap, heap.nodes.len + heap.values.len) { return false; }
    var live: Value[>..] = [];
    for n, row in heap.nodes {
        assert(n.count == 0);
        if n.initialized || n.key_capacity == 0 { continue; }
        let start = live.len;
        let kept: i64 = if row == global_row() { n.key_capacity } else { max(n.key_count - inline_slots(), 0) };
        live.append(heap.values[n.keys..(n.keys + kept)]);
        node_mut(heap, row).keys = start as u32; node_mut(heap, row).key_capacity = kept as u32;
    }
    heap.values.clear(); heap.values.append(live);
    return true;
}
// Built-in function o's `length` and `name` from BUILTINS (configurable
// only; %ThrowTypeError%'s are not configurable either, QuickJS sealing it).
fn builtin_metadata(heap: Heap&, o: i64) -> bool {
    let info = BUILTINS[node(heap, o).function];
    let attrs = if info.which == Builtin.ThrowTypeError { 0 } else { attr_configurable() };
    if !realm_value(heap, o, "length", number(info.length), attrs) || !realm_value(heap, o, "name", key_named(heap, info.name), attrs) { return false; }
    // QuickJS seals %ThrowTypeError%: it is not extensible either (light-cljs-882).
    if info.which == Builtin.ThrowTypeError { node_mut(heap, o).non_extensible = true; }
    return true;
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
    // rt ("core" and its leaves) and core get overflow runs sized for the
    // fields installed below, so the image keeps no runs that doubling growth
    // abandoned (light-cljs-904, as compact_realm_slots for the realm).
    var fields: i64[2] = [1, 0]; for leaf in RUNTIME_LEAVES { fields[if leaf.core { 1 } else { 0 }] += 1; }
    let rt = sized_object(heap, fields[0]); let core = sized_object(heap, fields[1]);
    install_field(heap, rt, "core", core);
    install_field(heap, heap.global, "__lightNativeRuntime", rt);
    for leaf in RUNTIME_LEAVES {
        if leaf.which == Builtin.ExceptionInfo { install_field(heap, core, leaf.key, object(exception_info_constructor_row())); continue; }
        // rt.intrinsicRegExp is the realm's RegExp constructor itself, as the
        // V8 build's runtime has `intrinsicRegExp = RegExp` (light-cljs-935).
        if leaf.which == Builtin.IntrinsicRegExp { install_field(heap, rt, leaf.key, object(regexp_row())); continue; }
        let f = allocate(heap, Node { kind: node_kind::Builtin, function: code(leaf.which) });
        if !f.is_object() || !builtin_metadata(heap, f.id()) { return Value.Undefined; }
        install_field(heap, if leaf.core { core } else { rt }, leaf.key, f);
    }
    // Where a declaration a macro runs directly writes its products
    // (light-cljs-920; product_target).
    let target = allocate(heap, Node { kind: node_kind::Builtin, function: code(Builtin.ProductTarget) });
    if !target.is_object() || !builtin_metadata(heap, target.id()) { return Value.Undefined; }
    install_field(heap, heap.global, "__lightCompilerProductTarget", target);
    if heap.fault == Fault.None { heap.boot_count += 1; }
    return rt;
}
// An ordinary object whose overflow run already holds `fields` properties.
fn sized_object(heap: Heap&, fields: i64) -> Value {
    let spilled = max(fields - inline_slots(), 0);
    let at = add_values(heap, [], spilled);
    if at < 0 { return Value.Undefined; }
    return allocate(heap, Node { kind: node_kind::Object, keys: at as u32, key_capacity: spilled as u32 });
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
        TypeError => match op { opcode::Call, opcode::CallMethod, opcode::CallSpread, opcode::CallMethodSpread => "not a function", opcode::Construct, opcode::ConstructSpread => "not a constructor", opcode::PutField, opcode::PutElem, opcode::PutFieldStrict, opcode::PutElemStrict => "cannot set property", opcode::In => "invalid 'in' operand", opcode::Plus, opcode::ToString => "Cannot convert a Symbol value to a string",
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
// QuickJS's JS_ThrowTypeErrorNotAConstructor and get_prop_string: quote a
// callable's string data `name`, own or on its immediate prototype. This
// is a diagnostic read, never [[Get]]: no getter, Proxy trap or conversion
// runs. A lazy closure's name already comes from the exported function row
// (function_name), so callee-expression spans need not change the image.
// QuickJS's JS_CallConstructorInternal calls primitives "not a function";
// an object without a usable name is "not a constructor". Ordinary invalid
// calls also say "not a function", without a source name.
fn not_constructor(job: Job&, callee: Value) {
    let heap .= job.heap;
    if !callee.is_object() { type_error(job, "not a function"); return; }
    var name = Value.Undefined;
    if callable(heap, callee) {
        var at = callee;
        for level in 2 {
            if lazy_function(heap, at.id()) { name = function_name(heap, at.id()); break; }
            let key = find_key(heap, "name");
            if key.is_undefined() { break; }
            let p = find_own(heap, at.id(), key);
            if heap.fault != Fault.None { return; }
            if p.found { name = p.value; break; }
            at = prototype(heap, at.id());
            if !at.is_object() { break; }
        }
    }
    if heap.fault != Fault.None { return; }
    if !name.is_string() { type_error(job, "not a constructor"); return; }
    // JS_ThrowError2's char buf[256] holds at most 255 UTF-8 bytes of
    // "%s is not a constructor". Count encoded bytes without allocating
    // the entire name; %s ends at NUL. JS_NewString replaces a sequence
    // split by that byte limit with one U+FFFD, retaining lone surrogates.
    let message_bytes: i64 = 255;
    job.detail.clear();
    var bytes: i64 = 0; var i: i64 = 0;
    while i < str_len(heap, name) && bytes < message_bytes {
        let u = unit(heap, name, i);
        if u == 0 { break; }
        let pair = u >= 0xd800 && u <= 0xdbff && i + 1 < str_len(heap, name) && unit(heap, name, i + 1) >= 0xdc00 && unit(heap, name, i + 1) <= 0xdfff;
        let width = if pair { 4 } else if u < 0x80 { 1 } else if u < 0x800 { 2 } else { 3 };
        if bytes + width > message_bytes { job.detail.push(0xfffd); bytes = message_bytes; break; }
        job.detail.push(u);
        if pair { i += 1; job.detail.push(unit(heap, name, i)); }
        bytes += width; i += 1;
    }
    for c in " is not a constructor" { if bytes == message_bytes { break; } job.detail.push(c); bytes += 1; }
    heap.fault = Fault.TypeError;
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
// TypeError; a symbol is a new wrapper inheriting %Symbol.prototype%
// (light-cljs-885). A BigInt wrapper fails by name: it is the BigInt
// area's (light-cljs-771).
fn to_object(job: Job&, v: Value) -> Value {
    if v.is_object() { return v; }
    if v.tag() <= value_tag::Null { return type_error(job, "cannot convert to object"); }
    return wrap(job, v, primitive_prototype(v));
}
fn wrap(job: Job&, v: Value, proto: Value) -> Value {
    if wrapper_type(v) < 0 && !v.is_symbol() { unsupported_detail(job, "ToObject: BigInt wrapper objects"); return Value.Undefined; }
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
// (its plan, coerce.goose); one that reaches here fails by name.
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
        var units: u16[>..] = []; append_descriptive(heap, v, units);
        if !units_fit(heap, units.len) { memory(heap); return Value.Undefined; }
        return string(heap, units);
    }
    if v.is_bigint() { return unsupported_step(job, b, "a BigInt argument"); }
    if v.is_object() { return unsupported_step(job, b, if convertible(heap, v) { "ToString of an object outside a call instruction" } else { "ToString of a light-cljs runtime value" }); }
    if v.is_symbol() { return type_error(job, "Cannot convert a Symbol value to a string"); }
    return to_string(heap, v);
}
// new Number(v), new String(v), new Boolean(v): a wrapper of their call's
// primitive, inheriting the realm's prototype; construct_builtin gives it
// another newTarget's `prototype` (light-cljs-887).
fn construct_wrapper(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let t = wrapper_index(b);
    let v = wrapper_value(job, b, t, args, true);
    if job.heap.fault != Fault.None { return Value.Undefined; }
    return wrap(job, v, object(wrapper_prototype_row(t)));
}
fn wrapper_index(b: Builtin) -> i64 { var t: i64 = 0; for w, i in WRAPPER_BUILTINS { if w == b { t = i; } } return t; }
// The primitive new Number(v), new String(v) or new Boolean(v) wraps: its
// conversion, which construct runs before newTarget's `prototype` is read
// (light-cljs-887); converting it again gives itself.
fn wrapper_primitive(job: Job&, b: Builtin, args: const Value[:]) -> Value { return wrapper_value(job, b, wrapper_index(b), args, true); }
// Bound digit-count parsing before StringToNumber scans a string, including
// one returned by ToPrimitive (converted calls this same check). The Pants
// number_digit_count limit is method-specific; radix conversion has none.
fn number_digit_conversion_allowed(job: Job&, b: Builtin, v: Value) -> bool {
    let method: const u8[:] = match b {
        NumberPrototypeToFixed => "toFixed",
        NumberPrototypeToExponential => "toExponential",
        NumberPrototypeToPrecision => "toPrecision",
        _ => { return true; },
    };
    if !v.is_string() || str_len(job.heap, v) <= 4096 { return true; }
    job.detail.clear();
    for c in method { job.detail.push(c); }
    for c in " digit conversion limit of 4096 UTF-16 units exceeded" { job.detail.push(c); }
    job.heap.fault = Fault.RangeError;
    return false;
}
// Number.prototype.toString(radix), toFixed, toExponential and toPrecision
// (light-cljs-902; bbp-149..151). The shared plan (coerce.goose) validates
// thisNumberValue before an object's argument converts. Only an originally
// undefined argument selects a default: Convert finishes an object's
// ToNumber, so a returned undefined arrives here as NaN.
fn number_to_string(job: Job&, b: Builtin, receiver: Value, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let x = this_primitive(job, b, 0, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    let r = form_value(heap, arg(args, 0));
    if r.is_undefined() && (b == Builtin.NumberPrototypeToString || b == Builtin.NumberPrototypeToPrecision) { return to_string(heap, x); }
    if !number_digit_conversion_allowed(job, b, r) { return Value.Undefined; }
    let n = primitive_number(job, b, r);
    if heap.fault != Fault.None { return Value.Undefined; }
    let count = to_integer_or_infinity(n); let value = x.num();
    let finite = value == value && value <= 1.7976931348623157e308 && value >= -1.7976931348623157e308;
    if !finite && (b == Builtin.NumberPrototypeToExponential || b == Builtin.NumberPrototypeToPrecision) { return to_string(heap, x); }
    let minimum: i64 = if b == Builtin.NumberPrototypeToString { 2 } else if b == Builtin.NumberPrototypeToPrecision { 1 } else { 0 };
    let maximum: i64 = if b == Builtin.NumberPrototypeToString { 36 } else { 100 };
    if count < minimum || count > maximum {
        job.detail.clear();
        let message = if b == Builtin.NumberPrototypeToString { "radix must be between 2 and 36" } else { "number of digits out of range" };
        for c in message { job.detail.push(c); } heap.fault = Fault.RangeError; return Value.Undefined;
    }
    var text: u8[>..] = []; var work: i64 = 0;
    match b {
        NumberPrototypeToString => { work = number::append_radix(text, value, count as i64); },
        NumberPrototypeToFixed => { work = number::append_fixed(text, value, count as i64); },
        NumberPrototypeToExponential => { work = number::append_significant(text, value, if r.is_undefined() { 0 } else { (count as i64) + 1 }, true); },
        NumberPrototypeToPrecision => { work = number::append_significant(text, value, count as i64, false); },
        _ => { heap.fault = Fault.InvalidState; return Value.Undefined; },
    }
    if !charge(heap, work) { return Value.Undefined; }
    return source::text(heap, text);
}
// The global number functions and Number's predicates (light-cljs-900;
// ECMAScript 19.2.2 to 19.2.5 and 21.1.2.2 to 21.1.2.5; the Pants engine's
// vm/native.rs NumberPredicate and NumberParse, QuickJS's js_global_isNaN,
// js_global_isFinite, js_parseInt, js_parseFloat, js_number_isInteger and
// js_number_isSafeInteger). isNaN, isFinite, parseInt and parseFloat have
// conversion plans (coerce.goose), so an object argument's ToPrimitive ran
// before this and their conversions here are a primitive's, in plan order;
// Number's predicates convert nothing.
fn number_builtin(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    let v = form_value(heap, arg(args, 0));
    if heap.fault != Fault.None { return Value.Undefined; }
    match b {
        IsNaN, IsFinite => {
            let n = primitive_number(job, b, v);
            if heap.fault != Fault.None { return Value.Undefined; }
            return boolean(if b == Builtin.IsNaN { n != n } else { n - n == 0 });
        },
        NumberIsNaN => { return boolean(v.is_number() && v.num() != v.num()); },
        NumberIsFinite => { return boolean(v.is_number() && v.num() - v.num() == 0); },
        NumberIsInteger, NumberIsSafeInteger => {
            if !v.is_number() { return boolean(false); }
            let n = v.num();
            if !(n - n == 0) || trunc(n) != n { return boolean(false); }
            return boolean(b == Builtin.NumberIsInteger || n <= 9007199254740991.0 && n >= -9007199254740991.0);
        },
        ParseInt, ParseFloat => {
            let s = primitive_string(job, b, v);
            if heap.fault != Fault.None { return Value.Undefined; }
            var radix: i64 = 0;
            if b == Builtin.ParseInt {
                let r = primitive_number(job, b, form_value(heap, arg(args, 1)));
                if heap.fault != Fault.None { return Value.Undefined; }
                radix = to_int32(r);
            }
            // Leading StrWhiteSpace, then the numeric prefix, read in place
            // in the string's run; every unit read is charged to the work
            // limit, the unit that ends the prefix included, and none past it.
            let n = str_len(heap, s);
            var start: i64 = 0; while start < n && str_white_space(unit(heap, s, start)) { start += 1; }
            let h = header(heap, s.sid());
            var value: f64 = 0; var read: i64 = 0;
            if h.wide { let t = wide_run(heap, h)[start..]; if b == Builtin.ParseInt { value, read = number::parse_int(t, radix); } else { value, read = number::parse_float(t); } }
            else { let t = narrow_run(heap, h)[start..]; if b == Builtin.ParseInt { value, read = number::parse_int(t, radix); } else { value, read = number::parse_float(t); } }
            if !charge(heap, start + read) { return Value.Undefined; }
            return number(value);
        },
        _ => { heap.fault = Fault.InvalidState; return Value.Undefined; },
    }
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
    return descriptor_object(heap, p);
}
// FromPropertyDescriptor of own property p (found, not pending): a new
// ordinary object, a job row.
fn descriptor_object(heap: Heap&, p: Own) -> Value {
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
// proxy's [[GetPrototypeOf]] fails by name (a runtime leaf runs to
// completion and starts no Proxy frame).
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
// ExceptionInfo(message, data, cause)` (construct_exception_info), cause
// null unless given.
fn ex_info(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    if args.len != 2 && args.len != 3 { return arity_error(job, args.len); }
    let fields: Value[3] = [args[0], args[1], if args.len == 3 { args[2] } else { nil() }];
    return construct_exception_info(job, object(exception_info_constructor_row()), fields[..]);
}
// The ExceptionInfo function's steps (src/bbp_runtime.js, light-cljs-875)
// with `this` (receiver) and message, data and cause `fields`: `new
// Error(val(message))`, whose ToString of the message runs no JavaScript
// here (primitive_string); the target `this == null ? globalThis :
// Object(this)`; then the fields and the COPIED_ERROR_FIELDS assigned to it
// as strict code (the V8 build's runtime is): a row the job did not
// allocate faults NativeBoundary (decision 3) before anything changes, a
// setter fails by name, and a refused assignment is a TypeError. Returns
// the target.
fn exception_info(job: Job&, b: Builtin, fields: const Value[:], receiver: Value) -> Value {
    let heap .= job.heap;
    let m = form_value(heap, fields[0]);
    if heap.fault != Fault.None { return Value.Undefined; }
    if !m.is_undefined() { primitive_string(job, b, m); if heap.fault != Fault.None { return Value.Undefined; } }
    let e = if receiver.tag() <= value_tag::Null { heap.global } else { to_object(job, receiver) };
    if heap.fault != Fault.None { return Value.Undefined; }
    if !writable(job, e.id()) { image_write(job, Value.Undefined, "an ExceptionInfo's fields"); return Value.Undefined; }
    let names: Name[3] = [Name { text: "message" }, Name { text: "data" }, Name { text: "cause" }];
    for v, i in fields { if !exception_info_field(job, b, e, names[i].text, v) { return Value.Undefined; } }
    for f in COPIED_ERROR_FIELDS {
        let v = data_get(job, b, object(error_prototype_row(0)), f.text);
        if heap.fault != Fault.None || !exception_info_field(job, b, e, f.text, v) { return Value.Undefined; }
    }
    return e;
}
// `new ExceptionInfo(...args)` with newTarget nt (construct,
// nativeConstruct and exInfo), as for an ordinary function: an ordinary
// object whose [[Prototype]] is nt's `prototype`, or %Object.prototype%
// when that is not an object (GetPrototypeFromConstructor), is `this` of
// the steps. With `resolved` the caller read it: `prototype` (Empty when
// it is not an object; construct, light-cljs-887).
fn construct_exception_info(job: Job&, nt: Value, args: const Value[:], resolved: bool = false, prototype: Value = Value.Empty) -> Value {
    let proto = if resolved { prototype } else { construct_prototype(job, nt) };
    if job.heap.fault != Fault.None { return Value.Undefined; }
    let o = allocate(job.heap, Node { kind: node_kind::Object, proto: if proto.is_object() { proto } else { object(object_prototype_row()) } });
    if job.heap.fault != Fault.None { return Value.Undefined; }
    let fields: Value[3] = [arg(args, 0), arg(args, 1), arg(args, 2)];
    return exception_info(job, Builtin.ExceptionInfo, fields, o);
}
// Whether v is a built-in function object of b.
fn is_builtin(heap: Heap&, v: Value, b: Builtin) -> bool { return valid(heap, v) && v.is_object() && node(heap, v.id()).kind == node_kind::Builtin && builtin_of(heap, v.id()) == b; }
// One field of exception_info: `e[name] = v` ([[Set]]).
fn exception_info_field(job: Job&, b: Builtin, e: Value, name: const u8[:], v: Value) -> bool {
    let heap .= job.heap;
    let k = key_named(heap, name);
    if heap.fault != Fault.None { return false; }
    prepare_function(job, e, k, true);
    if heap.fault != Fault.None { return false; }
    let setter, ok = write(heap, e, k, v);
    if heap.fault != Fault.None { return false; }
    if setter.is_object() { unsupported_step(job, b, "a setter on its receiver"); return false; }
    if !ok { refused_write(job, e, k); return false; }
    record(job, e, k, v);
    return heap.fault == Fault.None;
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
// has an item, with the protocol error. A map's or set's items are its
// seq's, entries or members in traversal order, the order the V8 build's
// into takes them in (light-cljs-918).
fn into(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if args.len != 2 { return native_value(heap, code(b), args); }
    let to = args[0]; let given = form_value(heap, args[1]);
    if !seq_source(job, given) { return Value.Undefined; }
    let count = form_count(heap, given); let source = form_kind(heap, given);
    if count == 0 { return to; }
    // A map's, set's or record's (light-cljs-920) seq.
    let from = if source == node_kind::Map || source == node_kind::Set || record_value(heap, given) { form_sequence(heap, given, 0, true) } else { given };
    if heap.fault != Fault.None { return Value.Undefined; }
    let target = form_kind(heap, to);
    if form_value(heap, to).tag() > value_tag::Null && !(target >= node_kind::List && target <= node_kind::Set || target == node_kind::Concat || record_value(heap, to)) {
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
        } else if record_value(heap, out) {
            // A record takes entries as its conj does (light-cljs-920).
            let e = record_conj_error(heap, item);
            if e == CoreError.Boundary { heap.fault = Fault.NativeBoundary; return Value.Undefined; }
            if e != CoreError.None { return core_raise(job, e); }
            out = record_conj(heap, form_value(heap, out), item);
        } else { out = form_conj(heap, out, item); }
        if heap.fault != Fault.None { return Value.Undefined; }
    }
    return out;
}
// rt.core.range(...a) (light-cljs-877): the V8 build's runtime's `range`
// (src/js_runtime.js range and rangeNode) of up to three numbers, `(range
// end)`, `(range start end)` and `(range start end step)`: `()` when step
// > 0 and end <= start, step < 0 and end >= start, or otherwise end ==
// start; else start, then each next x + step while it is < end (step > 0)
// or > end (step < 0). That runtime makes a lazy Range, an IntegerRange
// when start, end and step are integers (its isInteger: finite, integral,
// under 1e21 in magnitude); this one realizes it as a sequence
// (node_kind::Concat, as seq and rest make), seq_flavor::IntegerRange for
// an integer one, which reads, counts, indexes, prints and compares as the
// lazy one does. What it cannot realize fails by name: no argument (0 to
// Number.MAX_VALUE) or a nonempty range of step 0 or NaN (an endless
// repeat), and a non-number argument; one longer than the job's value
// limit (one toward an infinite end, too) is its MemoryLimit, each number
// one unit of work. More than three arguments is the runtime's arity
// error.
fn range_integer(x: f64) -> bool { return x == trunc(x) && x < 1e21 && x > -1e21; }
fn core_range(job: Job&, b: Builtin, args: const Value[:]) -> Value {
    let heap .= job.heap;
    if args.len > 3 { return arity_error(job, args.len); }
    if args.len == 0 { return unsupported_step(job, b, "an endless range"); }
    var bounds: f64[3] = [0, 0, 1];
    for v, i in args {
        let x = form_value(heap, v);
        if heap.fault != Fault.None { return Value.Undefined; }
        if !x.is_number() { return unsupported_step(job, b, "a range of a value that is no number"); }
        bounds[if args.len == 1 { 1 } else { i }] = x.num();
    }
    let first = bounds[0]; let end = bounds[1]; let step = bounds[2];
    if (if step > 0 { end <= first } else if step < 0 { end >= first } else { end == first }) { return form_collection(heap, node_kind::List, []); }
    if !(step > 0) && !(step < 0) { return unsupported_step(job, b, "an endless range"); }
    var values: Value[>..] = [];
    var x = first;
    loop {
        if !values_fit(heap, values.len + 1) { memory(heap); return Value.Undefined; }
        if !charge(heap) { return Value.Undefined; }
        values.push(number(x));
        let next = x + step;
        if !(if step > 0 { next < end } else { next > end }) { break; }
        x = next;
    }
    if !nodes_fit(heap, 1) { memory(heap); return Value.Undefined; }
    let start = add_values(heap, values);
    let flavor = if range_integer(first) && range_integer(end) && range_integer(step) { seq_flavor::IntegerRange } else { seq_flavor::Sequence };
    return allocate(heap, Node { kind: node_kind::Concat, start: start as u32, count: values.len as u32, length: flavor as u32 });
}
// Units of `[object Tag]`.
fn object_tag(job: Job&, receiver: Value) -> Value {
    let heap .= job.heap;
    if receiver.is_undefined() { return text(heap, "[object Undefined]"); }
    if receiver.is_null() { return text(heap, "[object Null]"); }
    // A primitive's ToObject is its wrapper, whose @@toStringTag is its
    // prototype's: the lookup starts there and no wrapper is made.
    let tag, getter = tag_lookup(job, receiver);
    if heap.fault != Fault.None { return Value.Undefined; }
    // A caller that can start a native frame runs the getter there
    // (conditional_native, object_statics.goose start_tag).
    if getter.is_object() { return unsupported_step(job, Builtin.ObjectPrototypeToString, "a @@toStringTag getter"); }
    return object_tag_text(job, receiver, tag);
}
// Get(ToObject(receiver), @@toStringTag) without running JavaScript, for a
// receiver other than undefined and null: the value, or the getter to run.
fn tag_lookup(job: Job&, receiver: Value) -> Value, Value {
    let heap .= job.heap;
    if receiver.tag() <= value_tag::Null { return Value.Undefined, Value.Undefined; }
    // A proxy's builtinTag comes first (light-cljs-891): IsArray of a
    // revoked one is a TypeError before @@toStringTag is read.
    if is_proxy(heap, receiver) { is_array_value(job, receiver); if heap.fault != Fault.None { return Value.Undefined, Value.Undefined; } }
    let holder = if receiver.is_object() { receiver } else { primitive_prototype(receiver) };
    if !holder.is_object() { unsupported_step(job, Builtin.ObjectPrototypeToString, "a Symbol or BigInt receiver"); return Value.Undefined, Value.Undefined; }
    return lookup(heap, holder, Value.Symbol { s: well_known::ToStringTag });
}
// "[object tag]" for `receiver` (not undefined or null) and its
// @@toStringTag value: a string tag, or the builtinTag of its kind.
fn object_tag_text(job: Job&, receiver: Value, tag: Value) -> Value {
    let heap .= job.heap;
    var units: u16[>..] = [];
    for c in "[object " { units.push(c); }
    if tag.is_string() { append_utf16(heap, tag, units); }
    else {
        let wrapped = if receiver.is_object() { if node(heap, receiver.id()).kind == node_kind::Wrapper { node(heap, receiver.id()).value } else { Value.Undefined } } else { receiver };
        // A symbol or its wrapper without a string @@toStringTag is "Object"
        // (light-cljs-885), as any kind without a builtinTag.
        // A proxy's builtinTag (light-cljs-891): "Array" when IsArray,
        // "Function" when callable. Its revocation was checked before the
        // tag's read (tag_lookup); one since does not change the answer.
        let proxy = is_proxy(heap, receiver);
        let array = proxy && is_array(heap, receiver);
        let name: const u8[:] = if wrapper_type(wrapped) >= 0 { WRAPPER_NAMES[wrapper_type(wrapped)].text } else if !receiver.is_object() { "Object" } else if proxy { if array { "Array" } else if proxy_callable(heap, receiver) { "Function" } else { "Object" } } else { match node(heap, receiver.id()).kind { node_kind::Array => "Array", node_kind::Closure, node_kind::Builtin, node_kind::BoundFunction => "Function", node_kind::Error => "Error", node_kind::Arguments => "Arguments", node_kind::RegExp => "RegExp", _ => "Object" } };
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
//              options), as does a proxy's [[Get]] (light-cljs-890), and
//     pc 2     awaits that getter's value, which it installs.
//     pc 3     awaits a proxy options object's [[HasProperty]] answer
//              (light-cljs-890), then reads `cause` as above.
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
// Whether built-in b is plain unless a step runs JavaScript, so a caller
// that can start a native frame starts it through start_conditional_native:
// the error built-ins (start_error, start_error_text), and Object.prototype.toString's
// @@toStringTag getter and Object.hasOwn's and hasOwnProperty's key
// conversion (light-cljs-884; object_statics.goose), and
// Reflect.deleteProperty's proxy trap (light-cljs-890; descriptors.goose),
// Object.getOwnPropertyDescriptor's and Reflect.getOwnPropertyDescriptor's
// (light-cljs-895; descriptors.goose start_own_descriptor), and the built-ins that reach a proxy's [[IsExtensible]],
// [[PreventExtensions]], [[SetPrototypeOf]] or, for isPrototypeOf,
// [[GetPrototypeOf]] (light-cljs-891; proxy.goose), and Object.seal,
// freeze, isSealed and isFrozen of a proxy (light-cljs-897; integrity.goose).
fn conditional_native(b: Builtin) -> bool { return match b { Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, InternalError, ErrorPrototypeToString, ReflectDeleteProperty, ObjectGetOwnPropertyDescriptor, ReflectGetOwnPropertyDescriptor => true, _ => statics_conditional(b) || proxy_conditional(b) || integrity_walks(b) }; }
// Start built-in b (conditional_native), called with `receiver` and
// `args`, with operand base `base` (native_start's contract).
fn start_conditional_native(job: Job&, b: Builtin, receiver: Value, args: const Value[:], base: i64) {
    if proxy_conditional(b) { start_proxy_conditional(job, b, receiver, args, base); return; }
    if integrity_walks(b) { start_integrity(job, b, args, base); return; }
    if statics_conditional(b) { start_statics(job, b, receiver, args, base); return; }
    if b == Builtin.ErrorPrototypeToString { start_error_text(job, receiver, base); return; }
    if b == Builtin.ReflectDeleteProperty { start_reflect_delete(job, b, args, base); return; }
    if own_descriptor_reads(b) { start_own_descriptor(job, b, args, base); return; }
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
// Whether options has a `cause` behind a getter (InstallErrorCause's Get
// runs it), or reaches a proxy, whose [[HasProperty]] or [[Get]] runs its
// trap (light-cljs-890): the frame path asks again.
fn cause_runs_code(heap: Heap&, options: Value) -> bool {
    if !options.is_object() { return false; }
    let k = key_named(heap, "cause");
    if heap.fault != Fault.None { return false; }
    let has = has_property(heap, options, k);
    if proxy_pending(heap, proxy_method::HasProperty) { take_proxy(heap); return true; }
    if heap.fault != Fault.None || !has { return false; }
    let _, getter = lookup(heap, options, k);
    if is_hit(heap, getter) { take_proxy(heap); }
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
    // A proxy's answer to HasProperty(options, "cause") (light-cljs-890).
    if frame.pc == 3 {
        if truthy(heap, job.operands.pop()) { error_cause(job, at, b, e, frame.receiver); } else { finish_native(job, frame, e); }
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
        let has = has_property(heap, options, k);
        if proxy_pending(heap, proxy_method::HasProperty) { job.frames[at].pc = 3; proxy_take(job, proxy_method::HasProperty); return; }
        if heap.fault != Fault.None { return; }
        if has { error_cause(job, at, b, e, options); return; }
    }
    finish_native(job, frame, e);
}
// InstallErrorCause's Get(options, "cause") for the ErrorConstruct frame at
// `at`: a getter (or a proxy's [[Get]]) above it, its value arriving at pc 2.
fn error_cause(job: Job&, at: i64, b: Builtin, e: Value, options: Value) {
    let heap .= job.heap;
    let k = key_named(heap, "cause");
    if heap.fault != Fault.None { return; }
    let cause, getter = lookup(heap, options, k);
    if heap.fault != Fault.None { return; }
    if getter.is_object() { job.frames[at].pc = 2; run_getter(job, b, getter, options); return; }
    install_error_field(job, e, "cause", cause);
    if heap.fault == Fault.None { finish_native(job, job.frames[at], e); }
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
// callee's code (its node's `function`). A NativeBoundary its step did not
// name names the built-in (light-cljs-915), whichever path called it.
fn call_builtin(job: Job&, id: i64, receiver: Value, args: const Value[:]) -> Value {
    let value = builtin_value(job, id, receiver, args);
    if job.heap.fault == Fault.NativeBoundary && job.detail.len == 0 { unnamed_boundary(job, id, args); }
    return value;
}
// What built-in `id` lacks for `args`: mapSeq, the runtime's `map`, maps
// only a finite list (native_value), so its other sources lack the core
// leaf for cljs.core/map (`mapSeq: no core leaf map over a vector`).
fn unnamed_boundary(job: Job&, id: i64, args: const Value[:]) {
    if id < 0 || id >= BUILTINS.len || BUILTINS[id].which != Builtin.MapSeq { builtin_boundary(job, id, "unsupported arguments"); return; }
    // The source is read with the fault set aside (valid refuses a faulted
    // heap); builtin_boundary sets it again.
    job.heap.fault = Fault.None;
    let kind = if args.len == 2 { form_kind(job.heap, args[1]) } else { node_kind::None };
    builtin_boundary(job, id, if args.len != 2 { "no core leaf map over other than one source" } else { match kind { node_kind::Vector => "no core leaf map over a vector", node_kind::Map => "no core leaf map over a map", node_kind::Set => "no core leaf map over a set", node_kind::LazySeq => "no core leaf map over a lazy sequence", _ => "no core leaf map over this source" } });
}
fn builtin_value(job: Job&, id: i64, receiver: Value, args: const Value[:]) -> Value {
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
        // Prototypes (light-cljs-883; prototypes.goose). Object.create runs
        // as a native frame (native_start).
        ObjectGetPrototypeOf, ObjectSetPrototypeOf, ObjectPrototypeIsPrototypeOf, ObjectPrototypeProtoGetter, ObjectPrototypeProtoSetter, ReflectGetPrototypeOf, ReflectSetPrototypeOf => { return prototype_builtin(job, b, receiver, args); },
        ObjectCreate => { return unsupported_step(job, b, "called outside a native frame"); },
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
        // IsArray: a proxy is its final target's (light-cljs-845), a
        // revoked one a TypeError (light-cljs-891).
        ArrayIsArray => { let r = is_array_value(job, arg(args, 0)); return if heap.fault == Fault.None { boolean(r) } else { Value.Undefined }; },
        // Proxy called without `new` (light-cljs-845; construct makes one).
        Proxy => { return type_error(job, "Proxy: constructor requires 'new'"); },
        // Proxy.revocable (light-cljs-891; proxy.goose). A revoke function
        // needs its own row, which the call paths that know the callee
        // give it (revoke_call); any other reach fails by name.
        ProxyRevocable => { return proxy_revocable(job, args); },
        ProxyRevoke => { return unsupported_step(job, b, "a revoke function called where its row is not known"); },
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
        ArrayPrototypeMap, ArrayPrototypeForEach, ArrayPrototypePush, ArrayPrototypeJoin, ArrayPrototypeToString, ArrayIteratorPrototypeNext, StringPrototypeIterator,
        ArrayPrototypeFilter, ArrayPrototypeReduce, ArrayPrototypeReduceRight, ArrayPrototypeSome, ArrayPrototypeEvery, ArrayPrototypeFlatMap, ArrayPrototypeFind, ArrayPrototypeFindIndex, ArrayPrototypeFindLast, ArrayPrototypeFindLastIndex,
        ArrayPrototypeIndexOf, ArrayPrototypeLastIndexOf, ArrayPrototypeIncludes, ArrayPrototypeAt,
        ArrayPrototypePop, ArrayPrototypeShift, ArrayPrototypeUnshift, ArrayPrototypeSplice, ArrayPrototypeReverse, ArrayPrototypeFill, ArrayPrototypeCopyWithin, ArrayPrototypeToSpliced, ArrayPrototypeSort, ArrayPrototypeToSorted,
        ArrayPrototypeSlice, ArrayPrototypeConcat, ArrayPrototypeFlat, ArrayPrototypeToReversed, ArrayPrototypeWith, ArrayOf, ArrayFrom => { return unsupported_step(job, b, "called from a native frame"); },
        // Array[@@species] (light-cljs-908) returns its receiver.
        ArraySpecies => { return receiver; },
        // The built-in iterators that run no JavaScript (light-cljs-844; iterators.goose).
        ArrayPrototypeValues, ArrayPrototypeKeys, ArrayPrototypeEntries => { return array_iterator(job, b, receiver); },
        StringIteratorPrototypeNext => { return string_iterator_next(job, receiver); },
        MapPrototypeKeys, MapPrototypeValues, MapPrototypeEntries, SetPrototypeValues, SetPrototypeEntries => { return collection_iterator(job, b, receiver); },
        MapIteratorPrototypeNext, SetIteratorPrototypeNext => { return collection_next(job, b, receiver); },
        MapPrototypeForEach, SetPrototypeForEach => { return unsupported_step(job, b, "a callback needs a native frame"); },
        // Symbol values (light-cljs-885; symbols.goose).
        SymbolFunction, SymbolFor, SymbolKeyFor, SymbolPrototypeToString, SymbolPrototypeValueOf, SymbolPrototypeDescription, SymbolPrototypeToPrimitive => { return symbol_builtin(job, b, receiver, args); },
        NativeCoreBinding => { return core_binding(job, b, arg(args, 0)); },
        Atom, Deref, Reset, CompareAndSet, ResetVals, Volatile, Vreset, AddWatch, RemoveWatch, SetValidator => { return cell_builtin(job, b, args); },
        Swap, SwapVals, GeneratorPrototypeNext, GeneratorPrototypeReturn, GeneratorPrototypeThrow, FunctionPrototypeHasInstance, IsInstance => { return unsupported_step(job, b, "called from a native frame"); },
        // %IteratorPrototype%[@@iterator]: returns its receiver.
        IteratorPrototypeIterator => { return receiver; },
        // A direct-eval site's call of %eval% never reaches here (Ins.DirectEval),
        // and every other call runs it as a native frame (start_indirect_eval).
        Eval => { return unsupported_step(job, b, "called outside a native frame"); },
        Delay, Force, Realized, IsDelay => { return delay_builtin(job, b, args); },
        NativeConstruct => {
            // `new C(...)` of the ExceptionInfo function (light-cljs-875).
            if is_builtin(heap, arg(args, 0), Builtin.ExceptionInfo) { return construct_exception_info(job, args[0], args[1..]); }
            if !constructible(heap, arg(args, 0)) { return unsupported_step(job, b, "constructing a value other than a Delay"); }
            var rest: Value[>..] = []; if args.len > 1 { rest.append(args[1..]); }
            return delay_builtin(job, Builtin.Delay, rest);
        },
        Number => { return wrapper_value(job, b, 0, args, false); },
        String => { return wrapper_value(job, b, 1, args, false); },
        Boolean => { return wrapper_value(job, b, 2, args, false); },
        NumberPrototypeValueOf => { return this_primitive(job, b, 0, receiver); },
        StringPrototypeSubstring, StringPrototypeSubstr, StringPrototypeSlice, StringPrototypeConcat, StringPrototypeRepeat, StringPrototypeTrim, StringPrototypeTrimStart, StringPrototypeTrimEnd, StringPrototypeAnchor, StringPrototypeBig, StringPrototypeBlink, StringPrototypeBold, StringPrototypeFixed, StringPrototypeFontcolor, StringPrototypeFontsize, StringPrototypeItalics, StringPrototypeLink, StringPrototypeSmall, StringPrototypeStrike, StringPrototypeSub, StringPrototypeSup => { return string_transform_builtin(job, b, receiver, args); },
        StringPrototypeCharAt, StringPrototypeCharCodeAt, StringPrototypeCodePointAt, StringPrototypeAt => { return string_access_builtin(job, b, receiver, args); },
        // Case mapping and normalization (light-cljs-905; string_case.goose).
        StringPrototypeToUpperCase, StringPrototypeToLowerCase, StringPrototypeNormalize => { return string_case_builtin(job, b, receiver, args); },
        NumberPrototypeToString, NumberPrototypeToFixed, NumberPrototypeToExponential, NumberPrototypeToPrecision => { return number_to_string(job, b, receiver, args); },
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
        NativeType => { return native_type(job, b, args); },
        ProductTarget => { return product_target(job, b); },
        ExInfo => { return ex_info(job, b, args); },
        // ExceptionInfo called, not constructed (light-cljs-875).
        ExceptionInfo => {
            let fields: Value[3] = [arg(args, 0), arg(args, 1), arg(args, 2)];
            return exception_info(job, b, fields, if receiver.is_hole() { Value.Undefined } else { receiver });
        },
        ExData, ExMessage, ExCause => { return ex_field(job, b, arg(args, 0)); },
        GetIn => { return get_in(job, b, args); },
        // str, pr-str, identical? and ExceptionInfo.prototype.toString
        // (light-cljs-876; printer.goose).
        Str => { return core_str(job, b, args); },
        PrStr => { return pr_str(job, b, args); },
        Identical => { return core_identical(job, args); },
        ExceptionInfoPrototypeToString => { return exception_info_to_string(job, b, receiver); },
        // A non-seqable source is cljs.core's error (light-cljs-873).
        Vec, Sequence => { if args.len == 1 && !seq_source(job, args[0]) { return Value.Undefined; } return native_value(heap, id, args); },
        Into => { return into(job, b, args); },
        Range => { return core_range(job, b, args); },
        // Property descriptors (light-cljs-881; descriptors.goose). The
        // three definers run as Descriptor native frames (native_start).
        ObjectDefineProperty, ObjectDefineProperties, ReflectDefineProperty => { return unsupported_step(job, b, "called outside a native frame"); },
        ReflectGetOwnPropertyDescriptor => { return reflect_own_descriptor(job, b, arg(args, 0), arg(args, 1)); },
        ReflectDeleteProperty => { return reflect_delete(job, b, arg(args, 0), arg(args, 1)); },
        ObjectPrototypeDefineGetter, ObjectPrototypeDefineSetter => { return define_accessor_half(job, b, receiver, arg(args, 0), arg(args, 1)); },
        ObjectPrototypeLookupGetter, ObjectPrototypeLookupSetter => { return lookup_accessor_half(job, b, receiver, arg(args, 0)); },
        // [[Extensible]] and integrity levels (light-cljs-882; integrity.goose).
        ObjectPreventExtensions, ObjectIsExtensible, ObjectSeal, ObjectIsSealed, ObjectFreeze, ObjectIsFrozen, ReflectIsExtensible, ReflectPreventExtensions => { return integrity_builtin(job, b, arg(args, 0)); },
        // The Object statics that expose own keys run as Statics native
        // frames (light-cljs-884; object_statics.goose, native_start).
        ObjectKeys, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectGetOwnPropertySymbols, ObjectGetOwnPropertyDescriptors, ObjectAssign, ObjectFromEntries, ObjectGroupBy,
        ObjectPrototypePropertyIsEnumerable, ObjectPrototypeToLocaleString, ReflectOwnKeys, ReflectHas, ReflectGet, ReflectSet => { return unsupported_step(job, b, "called outside a native frame"); },
        // The global number functions and Number's predicates (light-cljs-900).
        IsNaN, IsFinite, ParseInt, ParseFloat, NumberIsNaN, NumberIsFinite, NumberIsInteger, NumberIsSafeInteger => { return number_builtin(job, b, args); },
        // Math's functions (light-cljs-901; math_object.goose). sumPrecise
        // runs as a SumPrecise native frame (native_start).
        MathMin, MathMax, MathAbs, MathFloor, MathCeil, MathRound, MathSqrt, MathAcos, MathAsin, MathAtan, MathAtan2, MathCos, MathExp, MathLog, MathPow, MathSin, MathTan,
        MathTrunc, MathSign, MathCosh, MathSinh, MathTanh, MathAcosh, MathAsinh, MathAtanh, MathExpm1, MathLog1p, MathLog2, MathLog10, MathCbrt, MathHypot, MathRandom,
        MathF16round, MathFround, MathImul, MathClz32 => { return math_builtin(job, b, args); },
        MathSumPrecise => { return unsupported_step(job, b, "called outside a native frame"); },
        // RegExp (light-cljs-932; regexp_object.goose): the getters and
        // @@species are plain; the constructor, flags, toString, exec and
        // test (light-cljs-933) run as RegExp native frames (native_start).
        RegExpPrototypeSource, RegExpPrototypeGlobal, RegExpPrototypeIgnoreCase, RegExpPrototypeMultiline, RegExpPrototypeDotAll, RegExpPrototypeUnicode, RegExpPrototypeUnicodeSets, RegExpPrototypeSticky, RegExpPrototypeHasIndices, RegExpSpecies => { return regexp_builtin(job, b, receiver); },
        StringPrototypePadStart, StringPrototypePadEnd, StringPrototypeSplit,
        StringFromCharCode, StringFromCodePoint, StringRaw, StringPrototypeIndexOf, StringPrototypeLastIndexOf, StringPrototypeIncludes, StringPrototypeStartsWith, StringPrototypeEndsWith, StringPrototypeIsWellFormed, StringPrototypeToWellFormed,
        StringPrototypeToLocaleUpperCase, StringPrototypeToLocaleLowerCase => { return unsupported_step(job, b, "called outside a native frame"); },
        RegExp, RegExpPrototypeFlags, RegExpPrototypeToString, RegExpPrototypeExec, RegExpPrototypeTest => { return unsupported_step(job, b, "called outside a native frame"); },
        // replace, replaceAll and @@replace (light-cljs-934) run as RegExp native frames too.
        StringPrototypeReplace, StringPrototypeReplaceAll, RegExpPrototypeReplace => { return unsupported_step(job, b, "called outside a native frame"); },
        RegExpPrototypeMatch => { return unsupported_step(job, b, "matching (light-cljs-937)"); },
        // Map and Set (light-cljs-943; collections.goose): a constructor
        // called is a TypeError (construct makes the object); the keyed
        // methods, the size getters and @@species are plain.
        Map, Set => { return type_error(job, if b == Builtin.Map { "Constructor Map requires 'new'" } else { "Constructor Set requires 'new'" }); },
        MapPrototypeGet, MapPrototypeSet, MapPrototypeHas, MapPrototypeDelete, MapPrototypeClear, MapPrototypeSize, MapSpecies,
        SetPrototypeAdd, SetPrototypeHas, SetPrototypeDelete, SetPrototypeClear, SetPrototypeSize, SetSpecies => { return collection_builtin(job, b, receiver, args); },
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
