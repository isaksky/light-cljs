//! Built-in functions (clj-quickjs-117): the function objects of the realm
//! (`crate::realm`) whose behaviour is Rust code, after QuickJS's
//! `JS_CFUNC_DEF` functions (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8: `js_object_constructor`, `js_object_is`,
//! `js_object_toString`, `js_object_valueOf`; `js_function_proto`,
//! `js_function_call`, `js_function_apply`, `js_reflect_apply`,
//! `build_arg_list`); written fresh over this runtime's values.
//!
//! # The heap kind
//!
//! A built-in function is a heap object of kind `HeapObject::Builtin`
//! holding a [`BuiltinFunction`]: a [`Builtin`] naming its Rust code, and an
//! intrinsic [`OrdinaryObject`] for its own properties (`length`, `name`,
//! and for a constructor `prototype` and its static methods). It is a realm
//! intrinsic, so it is shared by every task, immutable and
//! [`Shareability::Persistent`](crate::value::share::Shareability). The
//! kind is general: every later built-in (`Function.prototype.call`, the
//! `Array`, `Symbol` and error constructors and their methods) is a new
//! [`Builtin`] variant, appended, with its arm in [`call`].
//!
//! `typeof` of a built-in function is `"function"`, and `Call` and
//! `CallMethod` accept it as a callee (as do the resumable natives that call
//! back into JavaScript, such as `__rt.reduce`). Its [`Trace`] reports its
//! property values; the root inventory lists it under the realm intrinsics.
//!
//! Its `[[Prototype]]` is `Function.prototype` (clj-quickjs-102), itself a
//! built-in function ([`Builtin::FunctionPrototype`]) whose prototype is
//! `Object.prototype`, so `Object.call` and `Object.is.apply` are
//! inherited. A lookup that reaches `Function.prototype` without finding a
//! standard name it lacks so far (`bind`, `toString`, ...;
//! `crate::vm::function::lacks_inherited`) fails with `UnsupportedOperand`
//! instead of answering from `Object.prototype` (`crate::vm::property`).
//!
//! # Calls
//!
//! A leaf built-in runs to completion inside its `Call`/`CallMethod`
//! instruction: it reads its receiver and arguments in place on the operand
//! stack (so they stay rooted), may allocate (the call is a poll point, and
//! the result replaces the operands in the same instruction, before the next
//! safepoint), calls no JavaScript and never suspends. A built-in that
//! must call JavaScript (a getter, a `toString` method, a callback) starts
//! a resumable native frame before reaching the leaf dispatcher
//! (`crate::vm::native`), so every JavaScript it runs is an ordinary VM
//! frame across which the task may yield, wait or migrate.
//!
//! The receiver is the base of a method call (`CallMethod`), `undefined`
//! for a plain call (strict mode).
//!
//! # Forwarding built-ins (clj-quickjs-102)
//!
//! `Function.prototype.call`, `Function.prototype.apply` and
//! `Reflect.apply` do nothing but call another function with a receiver
//! and arguments they compute, and return its result unchanged. They are
//! therefore not run by [`call`]. [`forward`] folds `call` synchronously
//! through `dispatch::resolve`, bounded by the task's forwarding depth.
//! `apply` and `Reflect.apply` stop that fold and start a native frame:
//! CreateListFromArrayLike may run getters for length and indices, or
//! ToPrimitive for an object-valued length. The frame retains the cursor
//! and expanded arguments, then requests the final call through the same
//! trampoline. No Rust frame survives a callback or wait, and nested
//! native forwarding obeys the task's native depth limit.
//!
//! | Built-in | Result |
//! |----------|--------|
//! | `Object(v)`, `new Object(v)` (clj-quickjs-280) | `undefined`/`null`/absent: a new ordinary object owned by the running task, with prototype `Object.prototype`; an object: `v` itself; any other primitive: its task-owned wrapper (clj-quickjs-302) |
//! | `Object.is(a, b)` | SameValue: `NaN` is `NaN`, `0` is not `-0`, strings by content, objects by identity |
//! | `Object.prototype.toString()` | `"[object Undefined]"` / `"[object Null]"` for those receivers; `"[object Function]"` for a function object or built-in function; `"[object Array]"` for an array (clj-quickjs-118); `"[object Object]"` for an ordinary object and for the runtime's other heap values (which behave as objects without properties); `"[object Error]"` for an Error instance; a primitive receiver: `UnsupportedOperand` (clj-quickjs-72/-73) |
//! | `Object.prototype.valueOf()` | ToObject(receiver): an object is itself; `undefined`/`null` throw a catchable `TypeError`; a primitive: `UnsupportedOperand` (wrappers, clj-quickjs-72) |
//! | `Array.prototype.join(separator)`, `.toString()` (clj-quickjs-156) | generic array-like join through resumable length/index getters and ToPrimitive; undefined separator defaults to comma; null/undefined elements give empty strings; bounded string building; toString dynamically calls join, or intrinsic Object.prototype.toString for a non-callable join; cycles reach catchable StackOverflow |
//! | `Array(...)`, `new Array(...)` (clj-quickjs-153) | a task-owned array: one Number argument sets a validated uint32 length with holes; otherwise the arguments become elements. Invalid lengths throw catchable RangeError; no argument coercion |
//! | `Array.isArray(v)` (clj-quickjs-153) | true for arrays and the immutable empty-array intrinsic `Array.prototype`, false for every other value (including persistent vectors) |
//! | `Array.prototype.push(...items)` (clj-quickjs-155) | ToObject(receiver), ToLength(`length`), Set each index and then `length` with Throw=true, return the new length; over `2^53 - 1`: catchable `TypeError`; null/undefined: catchable `TypeError`; primitive receiver wrappers: `UnsupportedOperand`; object lengths convert resumably |
//! | `Object.defineProperty(o, k, attributes)` | define or update the own data or accessor property `k` of `o` from the descriptor and return `o` (`crate::vm::descriptor`, clj-quickjs-119); a non-object `o` or descriptor, a descriptor with both `get`/`set` and `value`/`writable`, a define a non-configurable property forbids, a built-in object as `o`: catchable `TypeError` |
//! | `Object.getOwnPropertyDescriptor(o, k)` | `undefined` if `o` has no own property `k`, else a fresh object `{value, writable, enumerable, configurable}` (clj-quickjs-119); `undefined`/`null` `o`: catchable `TypeError`; a primitive `o`: its wrapper's own properties without allocating a wrapper (string indices and `length`, clj-quickjs-142; none for numbers or booleans) |
//! | `Object.getPrototypeOf(v)` (clj-quickjs-121) | the `[[Prototype]]` of `v` (`crate::vm::prototype`), `null` at the end of a chain; `Function.prototype` for a function or built-in function (clj-quickjs-102); `undefined`/`null` `v`: catchable `TypeError`; a primitive: `UnsupportedOperand` (wrappers); Error instances use their ordinary prototype |
//! | `Object.setPrototypeOf(o, proto)` (clj-quickjs-121) | set the `[[Prototype]]` of the object `o` and return `o`; a primitive `o` is returned unchanged; `undefined`/`null` `o`, a `proto` that is neither an object nor `null`, a cycle, a built-in or non-extensible `o`: catchable `TypeError` |
//! | `Object.create(proto)` (clj-quickjs-121) | a new ordinary object owned by the running task with prototype `proto` (an object or `null`, else a catchable `TypeError`); a second argument other than `undefined`: `UnsupportedOperand` (`Object.defineProperties`) |
//! | `Object.preventExtensions(v)`, `Object.freeze(v)` (clj-quickjs-122) | make `v` non-extensible / frozen and return it (`crate::vm::integrity` has the table per kind); a primitive is returned unchanged; a built-in object: catchable `TypeError` (immutable); another task's object: `ForeignObject` |
//! | `Object.isExtensible(v)`, `Object.isFrozen(v)` (clj-quickjs-122) | the `[[Extensible]]` flag / TestIntegrityLevel(frozen); a primitive is not extensible and is frozen; vectors, maps and published functions are frozen (`crate::vm::integrity`) |
//! | `Object.keys(o)`, `Object.getOwnPropertyNames(o)` (clj-quickjs-123) | a fresh array of the own enumerable (`keys`) or all (`getOwnPropertyNames`) string keys of `o`, array indices ascending first, then the other names in creation order (`crate::vm::keys`); `undefined`/`null` `o`: catchable `TypeError`; a primitive `o`: its wrapper's own properties without allocating a wrapper (string indices and `length`, clj-quickjs-142; none for numbers or booleans); `Object.prototype` lists its stored `__proto__` accessor (clj-quickjs-261) |
//! | `Number.prototype.toString(radix)`, `.valueOf()` (clj-quickjs-73) | thisNumberValue: a number receiver (a method call on a number primitive passes the number itself, no wrapper is allocated) or a Number wrapper (including `Number.prototype`, holding `+0`); `toString` gives its Number::toString (`crate::value::string::number_to_string`), with `radix` absent, `undefined` or 10; radixes 2..36 (clj-quickjs-149) produce exact integer and shortest round-trip fractional digits; object radixes use resumable ToNumber; an invalid radix throws catchable `RangeError`; any other receiver: catchable `TypeError` |
//! | `Number.prototype.toExponential(f)`, `.toPrecision(p)` (clj-quickjs-151) | thisNumberValue precedes resumable count coercion; exact binary64 significant rounding, ties away from zero; omitted exponential count uses shortest digits, omitted precision uses ToString; nonfinite values bypass range checks after conversion; counts f=0..100 / p=1..100, with a 4096-unit count-string resource limit |
//! | `String.prototype.toString()`, `.valueOf()` (clj-quickjs-73) | thisStringValue: a string receiver or a String wrapper (including `String.prototype`, holding `""`); any other receiver: catchable `TypeError` |
//! | `String.prototype.charAt`, `.charCodeAt`, `.codePointAt` (clj-quickjs-143) | RequireObjectCoercible, ToString, ToIntegerOrInfinity; UTF-16 unit access (a surrogate pair for `codePointAt`); out of bounds: empty string / NaN / undefined; object coercion uses a resumable native frame |
//! | `String.fromCharCode`, `.fromCodePoint` (clj-quickjs-143) | ToNumber then ToUint16 / integral code point validation; invalid point: catchable `RangeError`; object coercion uses a resumable native frame |
//! | `String.prototype.substring`, `.slice`, `.concat`, `.padStart`, `.padEnd` (clj-quickjs-145) | generic ToString receivers and resumable argument coercion; UTF-16 extraction/combination, with at most 256 output units copied per native step; length above `2^30 - 1`: catchable `InternalError` ("string too long") |
//! | `Number(...)`, `String(...)`, `new Number(...)`, `new String(...)` (clj-quickjs-72) | convert the argument (absent: 0 / ""); `new` allocates a task-owned ordinary object with [[NumberData]] / [[StringData]]; object arguments use resumable ToPrimitive with number / string hint |
//! | global `isNaN`, `isFinite`; `Number.isNaN`, `.isFinite`, `.isInteger`, `.isSafeInteger` (clj-quickjs-147) | globals apply ToNumber (objects through a resumable native conversion); Number methods return false for non-numbers without coercion; all ignore `this` and use only the first argument (absent: undefined) |

//! | `parseInt`, `parseFloat` (clj-quickjs-148) | resumable ToString and, for parseInt, ToInt32(radix); bounded prefix scanning in native::NumberParse; `Number` exposes the same functions |
//! | `Function.prototype(...)` (clj-quickjs-102) | `undefined`, whatever the receiver and arguments |
//! | `Function(...)`, `new Function(...)` (clj-quickjs-111) | resumable ToString of parameter/body arguments, then isolated global-scope compilation; returns an ordinary function named `anonymous` |
//! | `f.call(t, ...args)` (clj-quickjs-102) | forwards (above): calls the receiver `f` with `this` `t` and `args`; a receiver that is not callable: catchable `TypeError` |
//! | `f.apply(t, list)` (clj-quickjs-102) | forwards: calls `f` with `this` `t` and no arguments for an `undefined`/`null` `list`, else CreateListFromArrayLike(`list`): `length` through ToLength, then `list[0 .. length]`; a non-callable `f` or a primitive `list`: catchable `TypeError`; an object `length`: resumable ToPrimitive; a list longer than the value-stack room left: `StackOverflow` (QuickJS: a RangeError) |
//! | `Reflect.apply(f, t, list)` (clj-quickjs-102) | forwards: `f.apply(t, list)` without reading `f.apply`, except that `list` must be an object (`undefined` is a catchable `TypeError`) |

use crate::atom::same_value;
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::array::ArrayObject;
use crate::object::{OrdinaryObject, PrimitiveData};
use crate::realm;
use crate::value::string::number_to_string_radix;
use crate::value::{JsString, Value};
use crate::vm::descriptor;
use crate::vm::dispatch::{Fault, to_js_string, to_number};
use crate::vm::integrity;
use crate::vm::keys::{self, KeyFilter};
use crate::vm::prototype;

/// Which built-in function: the Rust code a [`BuiltinFunction`] runs.
/// Append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    /// `Object` (QuickJS `js_object_constructor`), called as a function.
    Object,
    /// `Object.is` (QuickJS `js_object_is`).
    ObjectIs,
    /// `Object.prototype.toString` (QuickJS `js_object_toString`).
    ObjectPrototypeToString,
    /// `Object.prototype.valueOf` (QuickJS `js_object_valueOf`).
    ObjectPrototypeValueOf,
    /// `Array.prototype.toString` (clj-quickjs-118/-156): dynamic join call.
    ArrayPrototypeToString,
    /// `Array` (clj-quickjs-153), callable and constructible.
    Array,
    /// `Object.defineProperty` (QuickJS `js_object_defineProperty`;
    /// clj-quickjs-119, `crate::vm::descriptor`).
    ObjectDefineProperty,
    /// `Object.getOwnPropertyDescriptor` (QuickJS
    /// `js_object_getOwnPropertyDescriptor`; clj-quickjs-119).
    ObjectGetOwnPropertyDescriptor,
    ObjectGetOwnPropertyDescriptors,
    /// `Object.getPrototypeOf` (QuickJS `js_object_getPrototypeOf`;
    /// clj-quickjs-121, `crate::vm::prototype`).
    ObjectGetPrototypeOf,
    /// `Object.setPrototypeOf` (QuickJS `js_object_setPrototypeOf`;
    /// clj-quickjs-121).
    ObjectSetPrototypeOf,
    /// `Object.create` (QuickJS `js_object_create`; clj-quickjs-121).
    ObjectCreate,
    /// `Object.preventExtensions` (QuickJS `js_object_preventExtensions`;
    /// clj-quickjs-122, `crate::vm::integrity`).
    ObjectPreventExtensions,
    /// `Object.isExtensible` (QuickJS `js_object_isExtensible`;
    /// clj-quickjs-122).
    ObjectIsExtensible,
    /// `Object.freeze` (QuickJS `js_object_seal`, freeze flag;
    /// clj-quickjs-122).
    ObjectFreeze,
    /// `Object.isFrozen` (QuickJS `js_object_isSealed`, frozen flag;
    /// clj-quickjs-122).
    ObjectIsFrozen,
    ObjectSeal,
    ObjectIsSealed,
    /// `Object.keys` (QuickJS `js_object_keys`; clj-quickjs-123,
    /// `crate::vm::keys`).
    ObjectKeys,
    /// `Object.getOwnPropertyNames` (QuickJS
    /// `js_object_getOwnPropertyNames`; clj-quickjs-123).
    ObjectGetOwnPropertyNames,
    /// `Number` (clj-quickjs-72/-203): ToNumeric then round BigInt to binary64,
    /// or a wrapper when constructed.
    Number,
    /// `Number.prototype.toString` (QuickJS `js_number_toString`;
    /// clj-quickjs-73/-149), radix 2..36.
    NumberPrototypeToString,
    /// `Number.prototype.valueOf` (QuickJS `js_number_valueOf`;
    /// clj-quickjs-73).
    NumberPrototypeValueOf,
    /// `String` (clj-quickjs-72): ToString, or a wrapper when constructed.
    String,
    /// `String.prototype.toString` (QuickJS `js_string_toString`;
    /// clj-quickjs-73).
    StringPrototypeToString,
    /// `String.prototype.valueOf` (clj-quickjs-73; the same function body
    /// as `toString`).
    StringPrototypeValueOf,
    /// `Function.prototype` itself (clj-quickjs-102): callable, accepts any
    /// arguments and returns `undefined` (ECMAScript §20.2.3).
    FunctionPrototype,
    /// `Function.prototype.call` (QuickJS `js_function_call`;
    /// clj-quickjs-102): a forwarding built-in ([`forward`]).
    FunctionPrototypeCall,
    /// `Function.prototype.apply` (QuickJS `js_function_apply`;
    /// clj-quickjs-102): a forwarding built-in.
    FunctionPrototypeApply,
    /// `Reflect.apply` (QuickJS `js_reflect_apply`; clj-quickjs-102): a
    /// forwarding built-in.
    ReflectApply,
    /// `Function` (clj-quickjs-111): callable and constructible dynamic
    /// compilation, also `Function.prototype.constructor`.
    Function,
    /// `Function.prototype.bind` (clj-quickjs-103).
    FunctionPrototypeBind,
    /// `Array.prototype.push` (QuickJS `js_array_push`, clj-quickjs-155).
    ArrayPrototypePush,
    /// UTF-16 access and construction (clj-quickjs-143).
    StringPrototypeCharAt,
    StringPrototypeCharCodeAt,
    StringPrototypeCodePointAt,
    StringFromCharCode,
    StringFromCodePoint,
    /// UTF-16 substring search (clj-quickjs-144).
    StringPrototypeIndexOf,
    StringPrototypeLastIndexOf,
    /// UTF-16 extraction and combination (clj-quickjs-145).
    StringPrototypeSubstring,
    StringPrototypeSlice,
    StringPrototypeConcat,
    StringPrototypePadStart,
    StringPrototypePadEnd,
    /// Coercing global number predicates (clj-quickjs-147).
    GlobalIsNaN,
    GlobalIsFinite,
    /// Non-coercing Number predicates (clj-quickjs-147).
    NumberIsNaN,
    NumberIsFinite,
    NumberIsInteger,
    NumberIsSafeInteger,

    /// Global and Number parsing functions (clj-quickjs-148).
    ParseInt,
    ParseFloat,
    /// Numeric Math methods (clj-quickjs-152).
    MathFloor,
    MathCeil,
    MathAbs,
    MathImul,
    MathFround,
    MathHypot,

    /// `Object.prototype.__proto__` accessor functions (clj-quickjs-261).
    ObjectPrototypeProtoGetter,
    ObjectPrototypeProtoSetter,

    /// Generic resumable array-like joining (clj-quickjs-156).
    ArrayPrototypeJoin,

    /// Error construction and generic stringification (clj-quickjs-228).
    Error,
    ErrorPrototypeToString,
    /// UTF-16 string splitting (clj-quickjs-157).
    StringPrototypeSplit,
    /// Array brand test (clj-quickjs-153).
    ArrayIsArray,

    /// NativeError constructors share Error's resumable implementation.
    NativeError(NativeErrorKind),
    /// Number exponentiation shared with `**` (clj-quickjs-164).
    MathPow,
    /// Unary IEEE Math methods (clj-quickjs-205).
    MathSqrt,
    MathLog2,
    JsonParse,
    StringRaw,

    /// Symbol primitives (clj-quickjs-127); callable, never constructible.
    Symbol,
    JsonStringify,

    /// Runtime-wide symbol registry (clj-quickjs-128).
    SymbolFor,
    SymbolKeyFor,

    /// Task-owned JavaScript Map constructor and leaf operations.
    Map,
    MapSet,
    MapGet,
    MapHas,
    MapDelete,
    MapClear,
    MapSize,

    ArrayBuffer,
    ArrayBufferByteLength,
    ArrayBufferDetached,
    ArrayBufferTransfer,
    ArrayBufferSlice,
    IntlLocale,
    IntlGetCanonicalLocales,
    IntlLocaleToString,
    IntlLocaleGet(crate::intl::locale::Field),

    TypedArray,
    /// Concrete Number typed arrays, including floating-point kinds. The
    /// original integer-only registry entry also supplies their continuation.
    IntegerTypedArray(crate::object::typed_array::ElementType),
    TypedArrayBuffer,
    TypedArrayLength,
    TypedArrayByteLength,
    TypedArrayByteOffset,

    IntlLocaleMaximize,
    IntlLocaleMinimize,

    /// UTC Date time-value core (clj-quickjs-179).
    Date,
    DateUtc,
    DateGetTime,
    DateValueOf,
    DateSetTime,
    DateToISOString,
    RegExp,
    RegExpExec,
    RegExpTest,
    RegExpToString,
    RegExpSource,
    RegExpFlags,
    RegExpFlag(char),

    /// BigInt conversion is callable, never constructible (clj-quickjs-201).
    BigInt,

    DateGetUtcField(u8),
    DateSetUtcField(u8),

    TypedArraySet,
    TypedArrayFill,
    TypedArrayJoin,

    StringPrototypeReplace,
    StringPrototypeReplaceAll,

    BigIntPrototypeToString,
    BigIntPrototypeValueOf,

    Promise,
    PromiseResolve,
    PromiseReject,
    PromiseThen,
    PromiseCatch,
    PromiseResolver(ObjectId, bool),
    PromiseReaction(ObjectId),

    DateNow,
    DateGetLocalField(u8),
    DateSetLocalField(u8),
    DateTimezoneOffset,

    IntlNumberFormat,
    IntlNumberFormatGetFormat,
    IntlNumberFormatBound,
    IntlNumberFormatResolvedOptions,
    IntlNumberFormatSupportedLocales,

    DateToLocalString(u8),
    DateToUtcString,

    PromiseResolveThenable(ObjectId),
    ArrayValues,
    TypedArrayValues,
    StringIterator,
    IteratorNext(super::native::iterator::Kind),
    IteratorIdentity,

    BigIntAsIntN,
    BigIntAsUintN,

    IntlPluralRules,
    IntlPluralRulesSelect,
    IntlPluralRulesResolvedOptions,
    IntlPluralRulesSupportedLocales,

    DateParse,
    MapForEach,
    MapKeys,
    MapValues,
    MapEntries,

    ArrayFrom,
    GeneratorNext,

    ArrayPrototypeSort,
    IntlCollator,
    IntlCollatorGetCompare,
    IntlCollatorBound,
    IntlCollatorResolvedOptions,
    IntlCollatorSupportedLocales,

    NumberPrototypeToFixed,

    WeakMap,
    WeakMapSet,
    WeakMapGet,
    WeakMapHas,
    WeakMapDelete,
    NumberPrototypeToExponential,
    NumberPrototypeToPrecision,

    IntlDateTimeFormat,
    IntlDateTimeFormatGetFormat,
    IntlDateTimeFormatBound,
    IntlDateTimeFormatResolvedOptions,
    IntlDateTimeFormatSupportedLocales,
    Eval,

    MathSumPrecise,

    SimGc,
    SimSetTimeout,
    SimClearTimeout,
    WeakRef,
    WeakRefDeref,

    SymbolPrototypeToString,
    SymbolPrototypeValueOf,
    SymbolPrototypeDescription,
    SymbolPrototypeToPrimitive,
    IntlListFormat,
    IntlListFormatFormat,
    IntlListFormatFormatToParts,
    IntlListFormatResolvedOptions,
    IntlListFormatSupportedLocales,

    FinalizationRegistry,
    FinalizationRegistryRegister,
    FinalizationRegistryUnregister,

    IntlRelativeTimeFormat,
    IntlRelativeTimeFormatFormat,
    IntlRelativeTimeFormatToParts,
    IntlRelativeTimeFormatResolvedOptions,
    IntlRelativeTimeFormatSupportedLocales,
    IntlSegmenter,
    IntlSegmenterSegment,
    IntlSegmenterResolvedOptions,
    IntlSegmenterSupportedLocales,
    IntlSegmentsContaining,
    IntlSegmentsIterator,
    IntlSegmentIteratorNext,

    IntlDisplayNames,
    IntlDisplayNamesOf,
    IntlDisplayNamesResolvedOptions,
    IntlDisplayNamesSupportedLocales,

    IntlDurationFormat,
    IntlDurationFormatFormat,
    IntlDurationFormatToParts,
    IntlDurationFormatResolvedOptions,
    IntlDurationFormatSupportedLocales,

    DisposableStack,
    DisposableStackUse,
    DisposableStackAdopt,
    DisposableStackDefer,
    DisposableStackDispose,
    DisposableStackMove,
    DisposableStackDisposed,

    AsyncDisposableStack,
    AsyncDisposableStackUse,
    AsyncDisposableStackAdopt,
    AsyncDisposableStackDefer,
    AsyncDisposableStackDispose,
    AsyncDisposableStackMove,
    AsyncDisposableStackDisposed,
    /// Private entry called only by the internal async dispose activation.
    AsyncDisposableStackDisposeBody,
    Proxy,
    ArrayPrototypePop,
    ArrayPrototypeShift,
    ArrayPrototypeUnshift,
    ArrayPrototypeSplice,
    ArrayPrototypeReverse,
    ArrayPrototypeFill,
    ArrayPrototypeCopyWithin,
    ArraySpecies,
    GeneratorReturn,
    GeneratorThrow,
    ArrayPrototypeIndexOf,
    ArrayPrototypeLastIndexOf,
    ArrayPrototypeIncludes,
    ArrayPrototypeFind,
    ArrayPrototypeFindIndex,
    ArrayPrototypeFindLast,
    ArrayPrototypeFindLastIndex,
    ArrayPrototypeAt,
    ArrayPrototypeForEach,
    ArrayPrototypeMap,
    ArrayPrototypeFilter,
    ArrayPrototypeReduce,
    ArrayPrototypeReduceRight,
    ArrayPrototypeSome,
    ArrayPrototypeEvery,
    ArrayPrototypeFlatMap,

    AsyncGeneratorNext,
    AsyncGeneratorReturn,
    AsyncGeneratorThrow,
    ArrayPrototypeSlice,
    ArrayPrototypeConcat,
    ArrayPrototypeFlat,
    ArrayPrototypeKeys,
    ArrayPrototypeEntries,
    ArrayPrototypeToReversed,
    ArrayPrototypeToSorted,
    ArrayPrototypeWith,
    ArrayOf,
    Set,
    SetAdd,
    SetHas,
    SetDelete,
    SetClear,
    SetSize,
    SetForEach,
    SetValues,
    SetEntries,
    WeakSet,
    WeakSetAdd,
    WeakSetHas,
    WeakSetDelete,

    Boolean,
    BooleanPrototypeToString,
    BooleanPrototypeValueOf,

    StringToUpperCase,
    StringToLowerCase,
    StringToLocaleUpperCase,
    StringToLocaleLowerCase,
    StringTrim,
    StringTrimStart,
    StringTrimEnd,
    StringRepeat,
    StringStartsWith,
    StringEndsWith,
    StringIncludes,
    StringAt,
    StringLocaleCompare,
    StringNormalize,

    PromiseAll,
    PromiseRace,
    PromiseAllSettled,
    PromiseAny,
    PromiseWithResolvers,
    PromiseFinally,
    PromiseCallback(ObjectId, super::promise::standard::Callback),
    ObjectAssign,
    ObjectEntries,
    ObjectValues,
    ObjectFromEntries,
    ObjectDefineProperties,

    StringMatch,
    StringMatchAll,
    StringPrototypeSearch,
    RegExpMatch,
    RegExpMatchAll,
    RegExpSearch,
    RegExpReplace,
    RegExpSplit,
    RegExpIteratorNext,

    MathMin,
    MathMax,
    MathRound,
    MathTrunc,
    MathSign,
    MathRandom,
    MathLog,
    MathLog10,
    MathLog1p,
    MathExp,
    MathExpm1,
    MathSin,
    MathCos,
    MathTan,
    MathAsin,
    MathAcos,
    MathAtan,
    MathAtan2,
    MathSinh,
    MathCosh,
    MathTanh,
    MathAsinh,
    MathAcosh,
    MathAtanh,
    MathCbrt,
    MathClz32,
    MathF16round,

    DateToPrimitive,
    IntlNumberFormatToParts,

    ReflectConstruct,
    ReflectDefineProperty,
    ReflectDeleteProperty,
    ReflectGet,
    ReflectGetOwnPropertyDescriptor,
    ReflectGetPrototypeOf,
    ReflectHas,
    ReflectIsExtensible,
    ReflectOwnKeys,
    ReflectPreventExtensions,
    ReflectSet,
    ReflectSetPrototypeOf,
    TypedArrayToStringTag,
}

/// Append-only kinds; discriminants index the realm's constructor/prototype pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeErrorKind {
    Type,
    Range,
    Reference,
    Syntax,
    Eval,
    Uri,
    Internal,
    Suppressed,
    Aggregate,
}

impl NativeErrorKind {
    pub(crate) const ALL: [Self; 9] = [
        Self::Type,
        Self::Range,
        Self::Reference,
        Self::Syntax,
        Self::Eval,
        Self::Uri,
        Self::Internal,
        Self::Suppressed,
        Self::Aggregate,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Type => "TypeError",
            Self::Range => "RangeError",
            Self::Reference => "ReferenceError",
            Self::Syntax => "SyntaxError",
            Self::Eval => "EvalError",
            Self::Uri => "URIError",
            Self::Internal => "InternalError",
            Self::Suppressed => "SuppressedError",
            Self::Aggregate => "AggregateError",
        }
    }
}

impl Builtin {
    /// The function's `name` property.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Builtin::ObjectAssign => "assign",
            Builtin::ObjectEntries => "entries",
            Builtin::ObjectValues => "values",
            Builtin::ObjectFromEntries => "fromEntries",
            Builtin::ObjectDefineProperties => "defineProperties",

            Self::StringMatch => "match",
            Self::StringMatchAll => "matchAll",
            Self::StringPrototypeSearch => "search",
            Self::RegExpMatch => "[Symbol.match]",
            Self::RegExpMatchAll => "[Symbol.matchAll]",
            Self::RegExpSearch => "[Symbol.search]",
            Self::RegExpReplace => "[Symbol.replace]",
            Self::RegExpSplit => "[Symbol.split]",
            Self::RegExpIteratorNext => "next",

            Builtin::ReflectConstruct => "construct",
            Builtin::ReflectDefineProperty => "defineProperty",
            Builtin::ReflectDeleteProperty => "deleteProperty",
            Builtin::ReflectGet => "get",
            Builtin::ReflectGetOwnPropertyDescriptor => "getOwnPropertyDescriptor",
            Builtin::ReflectGetPrototypeOf => "getPrototypeOf",
            Builtin::ReflectHas => "has",
            Builtin::ReflectIsExtensible => "isExtensible",
            Builtin::ReflectOwnKeys => "ownKeys",
            Builtin::ReflectPreventExtensions => "preventExtensions",
            Builtin::ReflectSet => "set",
            Builtin::ReflectSetPrototypeOf => "setPrototypeOf",
            Builtin::Set => "Set",
            Builtin::WeakSet => "WeakSet",
            Builtin::SetAdd | Builtin::WeakSetAdd => "add",
            Builtin::SetHas | Builtin::WeakSetHas => "has",
            Builtin::SetDelete | Builtin::WeakSetDelete => "delete",
            Builtin::SetClear => "clear",
            Builtin::SetSize => "get size",
            Builtin::SetForEach => "forEach",
            Builtin::SetValues => "values",
            Builtin::SetEntries => "entries",

            Builtin::Boolean => "Boolean",
            Builtin::BooleanPrototypeToString => "toString",
            Builtin::BooleanPrototypeValueOf => "valueOf",
            Builtin::ArrayPrototypeSlice => "slice",
            Builtin::ArrayPrototypeConcat => "concat",
            Builtin::ArrayPrototypeFlat => "flat",
            Builtin::ArrayPrototypeKeys => "keys",
            Builtin::ArrayPrototypeEntries => "entries",
            Builtin::ArrayPrototypeToReversed => "toReversed",
            Builtin::ArrayPrototypeToSorted => "toSorted",
            Builtin::ArrayPrototypeWith => "with",
            Builtin::ArrayOf => "of",
            Builtin::ArrayPrototypeForEach => "forEach",
            Builtin::ArrayPrototypeMap => "map",
            Builtin::ArrayPrototypeFilter => "filter",
            Builtin::ArrayPrototypeReduce => "reduce",
            Builtin::ArrayPrototypeReduceRight => "reduceRight",
            Builtin::ArrayPrototypeSome => "some",
            Builtin::ArrayPrototypeEvery => "every",
            Builtin::ArrayPrototypeFlatMap => "flatMap",
            Builtin::ArrayPrototypeIndexOf => "indexOf",
            Builtin::ArrayPrototypeLastIndexOf => "lastIndexOf",
            Builtin::ArrayPrototypeIncludes => "includes",
            Builtin::ArrayPrototypeFind => "find",
            Builtin::ArrayPrototypeFindIndex => "findIndex",
            Builtin::ArrayPrototypeFindLast => "findLast",
            Builtin::ArrayPrototypeFindLastIndex => "findLastIndex",
            Builtin::ArrayPrototypeAt => "at",
            Builtin::ArraySpecies => "get [Symbol.species]",
            Builtin::ArrayPrototypePop => "pop",
            Builtin::ArrayPrototypeShift => "shift",
            Builtin::ArrayPrototypeUnshift => "unshift",
            Builtin::ArrayPrototypeSplice => "splice",
            Builtin::ArrayPrototypeReverse => "reverse",
            Builtin::ArrayPrototypeFill => "fill",
            Builtin::ArrayPrototypeCopyWithin => "copyWithin",
            Builtin::Proxy => "Proxy",
            Builtin::MathSumPrecise => "sumPrecise",
            Builtin::NumberPrototypeToExponential => "toExponential",
            Builtin::NumberPrototypeToPrecision => "toPrecision",
            Builtin::NumberPrototypeToFixed => "toFixed",
            Builtin::ArrayFrom => "from",
            Builtin::ArrayPrototypeSort => "sort",
            Builtin::ArrayValues | Builtin::TypedArrayValues => "values",
            Builtin::StringIterator | Builtin::IteratorIdentity => "[Symbol.iterator]",
            Builtin::IteratorNext(_) => "next",
            Builtin::MapForEach => "forEach",
            Builtin::MapKeys => "keys",
            Builtin::MapValues => "values",
            Builtin::MapEntries => "entries",
            Builtin::BigInt => "BigInt",
            Builtin::BigIntAsIntN => "asIntN",
            Builtin::BigIntAsUintN => "asUintN",

            Builtin::TypedArraySet => "set",
            Builtin::TypedArrayFill => "fill",
            Builtin::TypedArrayJoin => "join",

            Builtin::BigIntPrototypeToString => "toString",
            Builtin::BigIntPrototypeValueOf => "valueOf",

            Builtin::IntlDurationFormat => "DurationFormat",
            Builtin::IntlDurationFormatFormat => "format",
            Builtin::IntlDurationFormatToParts => "formatToParts",
            Builtin::IntlDurationFormatResolvedOptions => "resolvedOptions",
            Builtin::IntlDurationFormatSupportedLocales => "supportedLocalesOf",
            Builtin::IntlRelativeTimeFormat => "RelativeTimeFormat",
            Builtin::IntlDisplayNames => "DisplayNames",
            Builtin::IntlDisplayNamesOf => "of",
            Builtin::IntlDisplayNamesResolvedOptions => "resolvedOptions",
            Builtin::IntlDisplayNamesSupportedLocales => "supportedLocalesOf",
            Builtin::IntlRelativeTimeFormatFormat => "format",
            Builtin::IntlRelativeTimeFormatToParts => "formatToParts",
            Builtin::IntlRelativeTimeFormatResolvedOptions => "resolvedOptions",
            Builtin::IntlRelativeTimeFormatSupportedLocales => "supportedLocalesOf",
            Builtin::IntlPluralRules => "PluralRules",
            Builtin::IntlSegmenter => "Segmenter",
            Builtin::IntlSegmenterSegment => "segment",
            Builtin::IntlSegmenterResolvedOptions => "resolvedOptions",
            Builtin::IntlSegmenterSupportedLocales => "supportedLocalesOf",
            Builtin::IntlSegmentsContaining => "containing",
            Builtin::IntlSegmentsIterator => "[Symbol.iterator]",
            Builtin::IntlSegmentIteratorNext => "next",
            Builtin::IntlListFormat => "ListFormat",
            Builtin::IntlListFormatFormat => "format",
            Builtin::IntlListFormatFormatToParts => "formatToParts",
            Builtin::IntlListFormatResolvedOptions => "resolvedOptions",
            Builtin::IntlListFormatSupportedLocales => "supportedLocalesOf",
            Builtin::IntlCollator => "Collator",
            Builtin::IntlCollatorGetCompare => "get compare",
            Builtin::IntlCollatorBound => "",
            Builtin::IntlCollatorResolvedOptions => "resolvedOptions",
            Builtin::IntlCollatorSupportedLocales => "supportedLocalesOf",
            Builtin::IntlPluralRulesSelect => "select",
            Builtin::IntlPluralRulesResolvedOptions => "resolvedOptions",
            Builtin::IntlPluralRulesSupportedLocales => "supportedLocalesOf",
            Builtin::IntlNumberFormat => "NumberFormat",
            Builtin::IntlDateTimeFormat => "DateTimeFormat",
            Builtin::IntlDateTimeFormatGetFormat => "get format",
            Builtin::IntlDateTimeFormatBound => "",
            Builtin::IntlDateTimeFormatResolvedOptions => "resolvedOptions",
            Builtin::IntlDateTimeFormatSupportedLocales => "supportedLocalesOf",
            Builtin::IntlNumberFormatGetFormat => "get format",
            Builtin::IntlNumberFormatBound => "",
            Builtin::IntlNumberFormatToParts => "formatToParts",
            Builtin::IntlNumberFormatResolvedOptions => "resolvedOptions",
            Builtin::IntlNumberFormatSupportedLocales => "supportedLocalesOf",
            Builtin::IntlLocale => "Locale",
            Builtin::IntlGetCanonicalLocales => "getCanonicalLocales",
            Builtin::IntlLocaleToString => "toString",
            Builtin::IntlLocaleGet(field) => field.getter_name(),

            Builtin::TypedArray => "TypedArray",
            Builtin::IntegerTypedArray(kind) => kind.name(),
            Builtin::TypedArrayBuffer => "get buffer",
            Builtin::TypedArrayLength => "get length",
            Builtin::TypedArrayByteLength => "get byteLength",
            Builtin::TypedArrayByteOffset => "get byteOffset",

            Builtin::IntlLocaleMaximize => "maximize",
            Builtin::IntlLocaleMinimize => "minimize",
            Builtin::RegExp => "RegExp",
            Builtin::StringPrototypeReplace => "replace",
            Builtin::StringPrototypeReplaceAll => "replaceAll",
            Builtin::RegExpExec => "exec",
            Builtin::RegExpTest => "test",
            Builtin::RegExpToString => "toString",
            Builtin::RegExpSource => "get source",
            Builtin::RegExpFlags => "get flags",
            Builtin::RegExpFlag(c) => match c {
                'd' => "get hasIndices",
                'g' => "get global",
                'i' => "get ignoreCase",
                'm' => "get multiline",
                's' => "get dotAll",
                'u' => "get unicode",
                'v' => "get unicodeSets",
                _ => "get sticky",
            },

            Builtin::Promise => "Promise",
            Builtin::PromiseResolve => "resolve",
            Builtin::PromiseReject => "reject",
            Builtin::PromiseThen => "then",
            Builtin::GeneratorNext | Builtin::AsyncGeneratorNext => "next",
            Builtin::GeneratorReturn | Builtin::AsyncGeneratorReturn => "return",
            Builtin::GeneratorThrow | Builtin::AsyncGeneratorThrow => "throw",
            Builtin::PromiseCatch => "catch",
            Builtin::PromiseAll => "all",
            Builtin::PromiseRace => "race",
            Builtin::PromiseAllSettled => "allSettled",
            Builtin::PromiseAny => "any",
            Builtin::PromiseWithResolvers => "withResolvers",
            Builtin::PromiseFinally => "finally",
            Builtin::PromiseCallback(_, _) => "",
            Builtin::PromiseResolver(_, _)
            | Builtin::PromiseReaction(_)
            | Builtin::PromiseResolveThenable(_) => "",
            Builtin::ArrayBuffer => "ArrayBuffer",
            Builtin::ArrayBufferByteLength => "get byteLength",
            Builtin::ArrayBufferDetached => "get detached",
            Builtin::ArrayBufferTransfer => "transfer",
            Builtin::ArrayBufferSlice => "slice",
            Builtin::ArrayIsArray => "isArray",

            Builtin::Symbol => "Symbol",
            Builtin::SymbolFor => "for",
            Builtin::SymbolKeyFor => "keyFor",
            Builtin::SymbolPrototypeToString => "toString",
            Builtin::SymbolPrototypeValueOf => "valueOf",
            Builtin::SymbolPrototypeDescription => "get description",
            Builtin::SymbolPrototypeToPrimitive | Builtin::DateToPrimitive => {
                "[Symbol.toPrimitive]"
            }
            Builtin::StringPrototypeSplit => "split",
            Builtin::Object => "Object",
            Builtin::ObjectIs => "is",
            Builtin::ObjectPrototypeToString => "toString",
            Builtin::TypedArrayToStringTag => "get [Symbol.toStringTag]",
            Builtin::ObjectPrototypeValueOf => "valueOf",
            Builtin::ArrayPrototypeToString => "toString",
            Builtin::Array => "Array",
            Builtin::ObjectDefineProperty => "defineProperty",
            Builtin::ObjectGetOwnPropertyDescriptor => "getOwnPropertyDescriptor",
            Builtin::ObjectGetOwnPropertyDescriptors => "getOwnPropertyDescriptors",
            Builtin::ObjectGetPrototypeOf => "getPrototypeOf",
            Builtin::ObjectSetPrototypeOf => "setPrototypeOf",
            Builtin::ObjectCreate => "create",
            Builtin::ObjectPreventExtensions => "preventExtensions",
            Builtin::ObjectIsExtensible => "isExtensible",
            Builtin::ObjectFreeze => "freeze",
            Builtin::ObjectIsFrozen => "isFrozen",
            Builtin::ObjectSeal => "seal",
            Builtin::ObjectIsSealed => "isSealed",
            Builtin::ObjectKeys => "keys",
            Builtin::ObjectGetOwnPropertyNames => "getOwnPropertyNames",
            Builtin::Number => "Number",
            Builtin::String => "String",
            Builtin::NumberPrototypeToString | Builtin::StringPrototypeToString => "toString",
            Builtin::NumberPrototypeValueOf | Builtin::StringPrototypeValueOf => "valueOf",
            Builtin::FunctionPrototype => "",
            Builtin::FunctionPrototypeCall => "call",
            Builtin::FunctionPrototypeApply | Builtin::ReflectApply => "apply",
            Builtin::Function => "Function",
            Builtin::FunctionPrototypeBind => "bind",
            Builtin::ArrayPrototypePush => "push",
            Builtin::ArrayPrototypeJoin => "join",
            Builtin::StringPrototypeCharAt => "charAt",
            Builtin::StringPrototypeCharCodeAt => "charCodeAt",
            Builtin::StringPrototypeCodePointAt => "codePointAt",
            Builtin::StringFromCharCode => "fromCharCode",
            Builtin::StringFromCodePoint => "fromCodePoint",
            Builtin::StringPrototypeIndexOf => "indexOf",
            Builtin::StringPrototypeLastIndexOf => "lastIndexOf",
            Builtin::StringPrototypeSubstring => "substring",
            Builtin::StringPrototypeSlice => "slice",
            Builtin::StringPrototypeConcat => "concat",
            Builtin::StringPrototypePadStart => "padStart",
            Builtin::StringPrototypePadEnd => "padEnd",
            Builtin::StringToUpperCase => "toUpperCase",
            Builtin::StringToLowerCase => "toLowerCase",
            Builtin::StringToLocaleUpperCase => "toLocaleUpperCase",
            Builtin::StringToLocaleLowerCase => "toLocaleLowerCase",
            Builtin::StringTrim => "trim",
            Builtin::StringTrimStart => "trimStart",
            Builtin::StringTrimEnd => "trimEnd",
            Builtin::StringRepeat => "repeat",
            Builtin::StringStartsWith => "startsWith",
            Builtin::StringEndsWith => "endsWith",
            Builtin::StringIncludes => "includes",
            Builtin::StringAt => "at",
            Builtin::StringLocaleCompare => "localeCompare",
            Builtin::StringNormalize => "normalize",
            Builtin::GlobalIsNaN | Builtin::NumberIsNaN => "isNaN",
            Builtin::GlobalIsFinite | Builtin::NumberIsFinite => "isFinite",
            Builtin::NumberIsInteger => "isInteger",
            Builtin::NumberIsSafeInteger => "isSafeInteger",

            Builtin::ParseInt => "parseInt",
            Builtin::ParseFloat => "parseFloat",
            Builtin::Eval => "eval",
            Builtin::MathFloor => "floor",
            Builtin::MathCeil => "ceil",
            Builtin::MathAbs => "abs",
            Builtin::MathImul => "imul",
            Builtin::MathFround => "fround",
            Builtin::MathHypot => "hypot",
            Builtin::MathPow => "pow",
            Builtin::MathSqrt => "sqrt",
            Builtin::MathLog2 => "log2",
            Builtin::MathMin => "min",
            Builtin::MathMax => "max",
            Builtin::MathRound => "round",
            Builtin::MathTrunc => "trunc",
            Builtin::MathSign => "sign",
            Builtin::MathRandom => "random",
            Builtin::MathLog => "log",
            Builtin::MathLog10 => "log10",
            Builtin::MathLog1p => "log1p",
            Builtin::MathExp => "exp",
            Builtin::MathExpm1 => "expm1",
            Builtin::MathSin => "sin",
            Builtin::MathCos => "cos",
            Builtin::MathTan => "tan",
            Builtin::MathAsin => "asin",
            Builtin::MathAcos => "acos",
            Builtin::MathAtan => "atan",
            Builtin::MathAtan2 => "atan2",
            Builtin::MathSinh => "sinh",
            Builtin::MathCosh => "cosh",
            Builtin::MathTanh => "tanh",
            Builtin::MathAsinh => "asinh",
            Builtin::MathAcosh => "acosh",
            Builtin::MathAtanh => "atanh",
            Builtin::MathCbrt => "cbrt",
            Builtin::MathClz32 => "clz32",
            Builtin::MathF16round => "f16round",
            Builtin::Map => "Map",
            Builtin::WeakMap => "WeakMap",
            Builtin::SimGc => "gc",
            Builtin::SimSetTimeout => "setTimeout",
            Builtin::SimClearTimeout => "clearTimeout",
            Builtin::WeakMapSet => "set",
            Builtin::WeakMapGet => "get",
            Builtin::WeakMapHas => "has",
            Builtin::WeakMapDelete => "delete",
            Builtin::WeakRef => "WeakRef",
            Builtin::DisposableStack => "DisposableStack",
            Builtin::AsyncDisposableStack => "AsyncDisposableStack",
            Builtin::DisposableStackUse | Builtin::AsyncDisposableStackUse => "use",
            Builtin::DisposableStackAdopt | Builtin::AsyncDisposableStackAdopt => "adopt",
            Builtin::DisposableStackDefer | Builtin::AsyncDisposableStackDefer => "defer",
            Builtin::DisposableStackDispose => "dispose",
            Builtin::AsyncDisposableStackDispose | Builtin::AsyncDisposableStackDisposeBody => {
                "disposeAsync"
            }
            Builtin::DisposableStackMove | Builtin::AsyncDisposableStackMove => "move",
            Builtin::DisposableStackDisposed | Builtin::AsyncDisposableStackDisposed => {
                "get disposed"
            }
            Builtin::WeakRefDeref => "deref",
            Builtin::FinalizationRegistry => "FinalizationRegistry",
            Builtin::FinalizationRegistryRegister => "register",
            Builtin::FinalizationRegistryUnregister => "unregister",
            Builtin::MapSet => "set",
            Builtin::MapGet => "get",
            Builtin::MapHas => "has",
            Builtin::MapDelete => "delete",
            Builtin::MapClear => "clear",
            Builtin::MapSize => "get size",
            Builtin::Date => "Date",
            Builtin::DateNow => "now",
            Builtin::DateParse => "parse",
            Builtin::DateGetLocalField(field) => super::date::LOCAL_GETTERS[field as usize],
            Builtin::DateSetLocalField(first) => super::date::LOCAL_SETTERS[first as usize].0,
            Builtin::DateTimezoneOffset => "getTimezoneOffset",
            Builtin::DateUtc => "UTC",
            Builtin::DateGetTime => "getTime",
            Builtin::DateValueOf => "valueOf",
            Builtin::DateSetTime => "setTime",
            Builtin::DateToISOString => "toISOString",
            Builtin::DateToLocalString(part) => match part {
                1 => "toDateString",
                2 => "toTimeString",
                3 => "toString",
                _ => unreachable!("invalid Date format part"),
            },
            Builtin::DateToUtcString => "toUTCString",
            Builtin::DateGetUtcField(field) => super::date::UTC_GETTERS[field as usize],
            Builtin::DateSetUtcField(first) => super::date::UTC_SETTERS[first as usize].0,

            Builtin::ObjectPrototypeProtoGetter => "get __proto__",
            Builtin::ObjectPrototypeProtoSetter => "set __proto__",
            Builtin::Error => "Error",
            Builtin::ErrorPrototypeToString => "toString",
            Builtin::NativeError(kind) => kind.name(),
            Builtin::JsonParse => "parse",
            Builtin::StringRaw => "raw",
            Builtin::JsonStringify => "stringify",
        }
    }

    /// The function's `length` property: its number of declared parameters.
    pub(crate) fn length(self) -> u32 {
        match self {
            Self::StringMatch
            | Self::StringMatchAll
            | Self::StringPrototypeSearch
            | Self::RegExpMatch
            | Self::RegExpMatchAll
            | Self::RegExpSearch => 1,
            Self::RegExpReplace | Self::RegExpSplit => 2,
            Self::RegExpIteratorNext => 0,

            Builtin::ReflectConstruct
            | Builtin::ReflectDeleteProperty
            | Builtin::ReflectGet
            | Builtin::ReflectGetOwnPropertyDescriptor
            | Builtin::ReflectHas
            | Builtin::ReflectSetPrototypeOf => 2,
            Builtin::ReflectDefineProperty | Builtin::ReflectSet => 3,
            Builtin::ReflectGetPrototypeOf
            | Builtin::ReflectIsExtensible
            | Builtin::ReflectOwnKeys
            | Builtin::ReflectPreventExtensions => 1,
            Builtin::Set
            | Builtin::WeakSet
            | Builtin::SetClear
            | Builtin::SetSize
            | Builtin::SetValues
            | Builtin::MapKeys
            | Builtin::MapValues
            | Builtin::MapEntries
            | Builtin::SetEntries => 0,
            Builtin::SetAdd
            | Builtin::SetHas
            | Builtin::SetDelete
            | Builtin::SetForEach
            | Builtin::WeakSetAdd
            | Builtin::WeakSetHas
            | Builtin::WeakSetDelete => 1,
            Builtin::ArrayPrototypeSlice | Builtin::ArrayPrototypeWith => 2,
            Builtin::ArrayPrototypeConcat | Builtin::ArrayPrototypeToSorted => 1,
            Builtin::ArrayPrototypeFlat
            | Builtin::ArrayPrototypeKeys
            | Builtin::ArrayPrototypeEntries
            | Builtin::ArrayPrototypeToReversed
            | Builtin::ArrayOf => 0,
            Builtin::ArrayPrototypeForEach
            | Builtin::ArrayPrototypeMap
            | Builtin::ArrayPrototypeFilter
            | Builtin::ArrayPrototypeReduce
            | Builtin::ArrayPrototypeReduceRight
            | Builtin::ArrayPrototypeSome
            | Builtin::ArrayPrototypeEvery
            | Builtin::ArrayPrototypeFlatMap => 1,
            Builtin::ArrayPrototypeIndexOf
            | Builtin::ArrayPrototypeLastIndexOf
            | Builtin::ArrayPrototypeIncludes
            | Builtin::ArrayPrototypeFind
            | Builtin::ArrayPrototypeFindIndex
            | Builtin::ArrayPrototypeFindLast
            | Builtin::ArrayPrototypeFindLastIndex
            | Builtin::ArrayPrototypeAt => 1,
            Builtin::ArraySpecies => 0,
            Builtin::ArrayPrototypePop
            | Builtin::ArrayPrototypeShift
            | Builtin::ArrayPrototypeReverse => 0,
            Builtin::ArrayPrototypeUnshift | Builtin::ArrayPrototypeFill => 1,
            Builtin::ArrayPrototypeSplice | Builtin::ArrayPrototypeCopyWithin => 2,
            Builtin::Proxy => 2,
            Builtin::ArrayFrom | Builtin::MathSumPrecise => 1,
            Builtin::ArrayPrototypeSort => 1,
            Builtin::ArrayValues
            | Builtin::TypedArrayValues
            | Builtin::StringIterator
            | Builtin::IteratorNext(_)
            | Builtin::IteratorIdentity => 0,
            Builtin::BigInt => 1,
            Builtin::BigIntAsIntN | Builtin::BigIntAsUintN => 2,

            Builtin::TypedArraySet => 1,
            Builtin::TypedArrayFill => 1,
            Builtin::TypedArrayJoin => 1,

            Builtin::BigIntPrototypeToString | Builtin::BigIntPrototypeValueOf => 0,

            Builtin::IntlPluralRules
            | Builtin::IntlPluralRulesSelect
            | Builtin::IntlPluralRulesSupportedLocales => 1,
            Builtin::IntlPluralRulesResolvedOptions => 0,
            Builtin::IntlSegmenter
            | Builtin::IntlSegmenterResolvedOptions
            | Builtin::IntlSegmentsIterator
            | Builtin::IntlSegmentIteratorNext => 0,
            Builtin::IntlSegmenterSegment
            | Builtin::IntlSegmenterSupportedLocales
            | Builtin::IntlSegmentsContaining => 1,
            Builtin::IntlListFormat | Builtin::IntlListFormatResolvedOptions => 0,
            Builtin::IntlListFormatFormat
            | Builtin::IntlListFormatFormatToParts
            | Builtin::IntlListFormatSupportedLocales => 1,

            Builtin::IntlDurationFormat | Builtin::IntlDurationFormatResolvedOptions => 0,
            Builtin::IntlDurationFormatFormat
            | Builtin::IntlDurationFormatToParts
            | Builtin::IntlDurationFormatSupportedLocales => 1,
            Builtin::IntlRelativeTimeFormat | Builtin::IntlRelativeTimeFormatResolvedOptions => 0,
            Builtin::IntlRelativeTimeFormatFormat | Builtin::IntlRelativeTimeFormatToParts => 2,
            Builtin::IntlRelativeTimeFormatSupportedLocales => 1,
            Builtin::IntlDisplayNames => 2,
            Builtin::IntlDisplayNamesOf | Builtin::IntlDisplayNamesSupportedLocales => 1,
            Builtin::IntlDisplayNamesResolvedOptions => 0,
            Builtin::IntlCollator
            | Builtin::IntlCollatorGetCompare
            | Builtin::IntlCollatorResolvedOptions => 0,
            Builtin::IntlCollatorBound => 2,
            Builtin::IntlCollatorSupportedLocales => 1,
            Builtin::IntlNumberFormat
            | Builtin::IntlNumberFormatBound
            | Builtin::IntlNumberFormatToParts
            | Builtin::IntlNumberFormatSupportedLocales => 1,
            Builtin::IntlNumberFormatGetFormat | Builtin::IntlNumberFormatResolvedOptions => 0,
            Builtin::IntlDateTimeFormat
            | Builtin::IntlDateTimeFormatBound
            | Builtin::IntlDateTimeFormatSupportedLocales => 1,
            Builtin::IntlDateTimeFormatGetFormat | Builtin::IntlDateTimeFormatResolvedOptions => 0,
            Builtin::IntlLocale | Builtin::IntlGetCanonicalLocales => 1,
            Builtin::IntegerTypedArray(_) => 3,
            Builtin::TypedArray => 0,
            Builtin::TypedArrayBuffer
            | Builtin::TypedArrayToStringTag
            | Builtin::TypedArrayLength
            | Builtin::TypedArrayByteLength
            | Builtin::TypedArrayByteOffset => 0,

            Builtin::IntlLocaleToString
            | Builtin::IntlLocaleGet(_)
            | Builtin::IntlLocaleMaximize
            | Builtin::IntlLocaleMinimize => 0,
            Builtin::RegExp => 2,
            Builtin::StringPrototypeReplace | Builtin::StringPrototypeReplaceAll => 2,
            Builtin::StringToUpperCase
            | Builtin::StringToLowerCase
            | Builtin::StringToLocaleUpperCase
            | Builtin::StringToLocaleLowerCase
            | Builtin::StringTrim
            | Builtin::StringTrimStart
            | Builtin::StringTrimEnd
            | Builtin::StringNormalize => 0,
            Builtin::StringRepeat
            | Builtin::StringStartsWith
            | Builtin::StringEndsWith
            | Builtin::StringIncludes
            | Builtin::StringAt
            | Builtin::StringLocaleCompare => 1,
            Builtin::RegExpExec | Builtin::RegExpTest => 1,
            Builtin::RegExpToString
            | Builtin::RegExpSource
            | Builtin::RegExpFlags
            | Builtin::RegExpFlag(_) => 0,

            Builtin::PromiseThen => 2,
            Builtin::PromiseAll
            | Builtin::PromiseRace
            | Builtin::PromiseAllSettled
            | Builtin::PromiseAny
            | Builtin::PromiseFinally => 1,
            Builtin::PromiseWithResolvers => 0,
            Builtin::PromiseCallback(_, callback) => callback.length(),
            Builtin::GeneratorNext
            | Builtin::GeneratorReturn
            | Builtin::GeneratorThrow
            | Builtin::AsyncGeneratorNext
            | Builtin::AsyncGeneratorReturn
            | Builtin::AsyncGeneratorThrow => 1,
            Builtin::Promise
            | Builtin::PromiseResolve
            | Builtin::PromiseReject
            | Builtin::PromiseCatch
            | Builtin::PromiseResolver(_, _) => 1,
            Builtin::PromiseReaction(_) | Builtin::PromiseResolveThenable(_) => 0,
            Builtin::ArrayBuffer => 1,
            Builtin::ArrayBufferByteLength
            | Builtin::ArrayBufferDetached
            | Builtin::ArrayBufferTransfer => 0,
            Builtin::ArrayBufferSlice => 2,
            Builtin::Object | Builtin::Array | Builtin::ArrayIsArray => 1,

            Builtin::Symbol => 0,
            Builtin::SymbolFor | Builtin::SymbolKeyFor => 1,
            Builtin::SymbolPrototypeToString
            | Builtin::SymbolPrototypeValueOf
            | Builtin::SymbolPrototypeDescription => 0,
            Builtin::SymbolPrototypeToPrimitive | Builtin::DateToPrimitive => 1,
            Builtin::ObjectIs => 2,
            Builtin::ObjectPrototypeToString
            | Builtin::ObjectPrototypeValueOf
            | Builtin::ArrayPrototypeToString => 0,
            Builtin::ObjectDefineProperty => 3,
            Builtin::ObjectGetOwnPropertyDescriptor => 2,
            Builtin::ObjectGetPrototypeOf => 1,
            Builtin::ObjectSetPrototypeOf | Builtin::ObjectCreate => 2,
            Builtin::ObjectPreventExtensions
            | Builtin::ObjectIsExtensible
            | Builtin::ObjectFreeze
            | Builtin::ObjectIsFrozen
            | Builtin::ObjectSeal
            | Builtin::ObjectIsSealed => 1,
            Builtin::ObjectKeys
            | Builtin::ObjectGetOwnPropertyNames
            | Builtin::ObjectGetOwnPropertyDescriptors
            | Builtin::ObjectEntries
            | Builtin::ObjectValues
            | Builtin::ObjectFromEntries => 1,
            Builtin::ObjectAssign | Builtin::ObjectDefineProperties => 2,
            Builtin::Boolean
            | Builtin::Number
            | Builtin::String
            | Builtin::NumberPrototypeToString
            | Builtin::NumberPrototypeToExponential
            | Builtin::NumberPrototypeToPrecision
            | Builtin::NumberPrototypeToFixed => 1,
            Builtin::BooleanPrototypeToString
            | Builtin::BooleanPrototypeValueOf
            | Builtin::NumberPrototypeValueOf
            | Builtin::StringPrototypeToString
            | Builtin::StringPrototypeValueOf => 0,
            Builtin::FunctionPrototype => 0,
            Builtin::FunctionPrototypeCall | Builtin::Function => 1,
            Builtin::FunctionPrototypeApply => 2,
            Builtin::ReflectApply => 3,
            Builtin::FunctionPrototypeBind => 1,
            Builtin::ArrayPrototypePush => 1,
            Builtin::ArrayPrototypeJoin => 1,
            Builtin::StringPrototypeCharAt
            | Builtin::StringPrototypeCharCodeAt
            | Builtin::StringPrototypeCodePointAt
            | Builtin::StringFromCharCode
            | Builtin::StringFromCodePoint
            | Builtin::StringPrototypeIndexOf
            | Builtin::StringPrototypeLastIndexOf
            | Builtin::StringPrototypeConcat
            | Builtin::StringPrototypePadStart
            | Builtin::StringPrototypePadEnd => 1,
            Builtin::StringPrototypeSubstring | Builtin::StringPrototypeSlice => 2,
            Builtin::StringPrototypeSplit => 2,
            Builtin::GlobalIsNaN
            | Builtin::GlobalIsFinite
            | Builtin::NumberIsNaN
            | Builtin::NumberIsFinite
            | Builtin::NumberIsInteger
            | Builtin::NumberIsSafeInteger => 1,

            Builtin::ParseInt => 2,
            Builtin::ParseFloat => 1,
            Builtin::Eval => 1,
            Builtin::MathFloor | Builtin::MathCeil | Builtin::MathAbs | Builtin::MathFround => 1,
            Builtin::MathImul | Builtin::MathHypot | Builtin::MathPow => 2,
            Builtin::MathSqrt | Builtin::MathLog2 => 1,
            Builtin::MathMin | Builtin::MathMax | Builtin::MathAtan2 => 2,
            Builtin::MathRandom => 0,
            Builtin::MathRound
            | Builtin::MathTrunc
            | Builtin::MathSign
            | Builtin::MathLog
            | Builtin::MathLog10
            | Builtin::MathLog1p
            | Builtin::MathExp
            | Builtin::MathExpm1
            | Builtin::MathSin
            | Builtin::MathCos
            | Builtin::MathTan
            | Builtin::MathAsin
            | Builtin::MathAcos
            | Builtin::MathAtan
            | Builtin::MathSinh
            | Builtin::MathCosh
            | Builtin::MathTanh
            | Builtin::MathAsinh
            | Builtin::MathAcosh
            | Builtin::MathAtanh
            | Builtin::MathCbrt
            | Builtin::MathClz32
            | Builtin::MathF16round => 1,
            Builtin::Map
            | Builtin::MapGet
            | Builtin::MapHas
            | Builtin::MapDelete
            | Builtin::MapForEach => 1,
            Builtin::MapSet | Builtin::WeakMapSet => 2,
            Builtin::WeakRef => 1,
            Builtin::DisposableStack
            | Builtin::AsyncDisposableStack
            | Builtin::AsyncDisposableStackDispose
            | Builtin::AsyncDisposableStackDisposeBody
            | Builtin::AsyncDisposableStackMove
            | Builtin::AsyncDisposableStackDisposed
            | Builtin::DisposableStackDispose
            | Builtin::DisposableStackMove
            | Builtin::DisposableStackDisposed => 0,
            Builtin::DisposableStackUse
            | Builtin::DisposableStackDefer
            | Builtin::AsyncDisposableStackUse
            | Builtin::AsyncDisposableStackDefer => 1,
            Builtin::DisposableStackAdopt | Builtin::AsyncDisposableStackAdopt => 2,
            Builtin::WeakRefDeref => 0,
            Builtin::FinalizationRegistry | Builtin::FinalizationRegistryUnregister => 1,
            Builtin::FinalizationRegistryRegister => 2,
            Builtin::WeakMap
            | Builtin::WeakMapGet
            | Builtin::WeakMapHas
            | Builtin::WeakMapDelete => 1,
            Builtin::MapClear | Builtin::MapSize => 0,
            Builtin::SimGc => 0,
            Builtin::SimSetTimeout => 2,
            Builtin::SimClearTimeout => 1,
            Builtin::Date => 7,
            Builtin::DateNow | Builtin::DateGetLocalField(_) | Builtin::DateTimezoneOffset => 0,
            Builtin::DateSetLocalField(first) => super::date::LOCAL_SETTERS[first as usize].1,
            Builtin::DateUtc => 2,
            Builtin::DateParse => 1,
            Builtin::DateGetTime | Builtin::DateValueOf => 0,
            Builtin::DateSetTime => 1,
            Builtin::DateToISOString => 0,
            Builtin::DateToLocalString(_) | Builtin::DateToUtcString => 0,
            Builtin::DateGetUtcField(_) => 0,
            Builtin::DateSetUtcField(first) => super::date::UTC_SETTERS[first as usize].1,

            Builtin::ObjectPrototypeProtoGetter => 0,
            Builtin::ObjectPrototypeProtoSetter => 1,
            Builtin::Error => 1,
            Builtin::ErrorPrototypeToString => 0,
            Builtin::NativeError(NativeErrorKind::Suppressed) => 3,
            Builtin::NativeError(NativeErrorKind::Aggregate) => 2,
            Builtin::NativeError(_) => 1,
            Builtin::JsonParse => 2,
            Builtin::StringRaw => 1,
            Builtin::JsonStringify => 3,
        }
    }
}

/// Storage of a built-in function. See the module docs.
#[derive(Debug)]
pub(crate) struct BuiltinFunction {
    pub(crate) which: Builtin,
    /// Own properties: an intrinsic, immutable ordinary-object store.
    pub(crate) properties: OrdinaryObject,
}

impl Trace for BuiltinFunction {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.properties.trace(visit);
        match self.which {
            Builtin::PromiseResolver(id, _)
            | Builtin::PromiseReaction(id)
            | Builtin::PromiseResolveThenable(id) => visit(id.into()),
            Builtin::PromiseCallback(id, _) => visit(id.into()),
            _ => {}
        }
    }
}

/// Call the built-in `which` for the running task `task` with receiver
/// `this` and arguments `args` (module docs, "Calls").
pub(crate) fn call(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    match which {
        Builtin::ArrayPrototypeForEach
        | Builtin::ArrayPrototypeMap
        | Builtin::ArrayPrototypeFilter
        | Builtin::ArrayPrototypeReduce
        | Builtin::ArrayPrototypeReduceRight
        | Builtin::ArrayPrototypeSome
        | Builtin::ArrayPrototypeEvery
        | Builtin::ArrayPrototypeFlatMap => Err(Fault::Internal(
            "array callback iteration requires a resumable frame",
        )),
        Builtin::ArrayPrototypeIndexOf
        | Builtin::ArrayPrototypeLastIndexOf
        | Builtin::ArrayPrototypeIncludes
        | Builtin::ArrayPrototypeFind
        | Builtin::ArrayPrototypeFindIndex
        | Builtin::ArrayPrototypeFindLast
        | Builtin::ArrayPrototypeFindLastIndex
        | Builtin::ArrayPrototypeAt => {
            Err(Fault::Internal("array search requires a resumable frame"))
        }
        Builtin::ArraySpecies => Ok(this.clone()),
        Builtin::SimGc | Builtin::SimSetTimeout | Builtin::SimClearTimeout => Err(Fault::Internal(
            "simulated host binding requires task state",
        )),
        Builtin::ArrayPrototypePop
        | Builtin::ArrayPrototypeShift
        | Builtin::ArrayPrototypeUnshift
        | Builtin::ArrayPrototypeSplice
        | Builtin::ArrayPrototypeReverse
        | Builtin::ArrayPrototypeFill
        | Builtin::ArrayPrototypeCopyWithin => {
            Err(Fault::Internal("array mutator requires a resumable frame"))
        }
        Builtin::ArrayPrototypeSlice
        | Builtin::ArrayPrototypeConcat
        | Builtin::ArrayPrototypeFlat
        | Builtin::ArrayPrototypeToReversed
        | Builtin::ArrayPrototypeWith
        | Builtin::ArrayOf => Err(Fault::Internal("array copy requires a resumable frame")),
        Builtin::ArrayPrototypeSort | Builtin::ArrayPrototypeToSorted => {
            Err(Fault::Internal("sort requires a resumable frame"))
        }
        Builtin::ArrayFrom => Err(Fault::Internal("Array.from requires a resumable frame")),
        Builtin::MathSumPrecise => Err(Fault::Internal("sumPrecise requires a resumable frame")),
        Builtin::ArrayValues => {
            super::native::iterator::create(heap, task, this, super::native::iterator::Kind::Array)
        }
        Builtin::ArrayPrototypeKeys | Builtin::ArrayPrototypeEntries => {
            super::native::iterator::create(
                heap,
                task,
                this,
                if which == Builtin::ArrayPrototypeKeys {
                    super::native::iterator::Kind::ArrayKeys
                } else {
                    super::native::iterator::Kind::ArrayEntries
                },
            )
        }
        Builtin::TypedArrayValues => super::native::iterator::create(
            heap,
            task,
            this,
            super::native::iterator::Kind::TypedArray,
        ),
        Builtin::IteratorIdentity => Ok(this.clone()),
        Builtin::StringIterator
        | Builtin::IteratorNext(_)
        | Builtin::MapForEach
        | Builtin::SetForEach => Err(Fault::Internal(
            "iterator callback requires a resumable frame",
        )),
        Builtin::TypedArraySet | Builtin::TypedArrayFill | Builtin::TypedArrayJoin => Err(
            Fault::Internal("typed array method requires a resumable frame"),
        ),
        Builtin::IntlDateTimeFormat
        | Builtin::IntlDateTimeFormatBound
        | Builtin::IntlDateTimeFormatSupportedLocales => {
            Err(Fault::Internal("DateTimeFormat requires native frame"))
        }
        Builtin::IntlDateTimeFormatGetFormat => {
            crate::intl::date_time_format::get_format(heap, task, this)
        }
        Builtin::IntlDateTimeFormatResolvedOptions => {
            crate::intl::date_time_format::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlDisplayNames => Err(Fault::TypeError("Intl.DisplayNames requires new".into())),
        Builtin::IntlDisplayNamesOf | Builtin::IntlDisplayNamesSupportedLocales => {
            Err(Fault::Internal("DisplayNames requires native frame"))
        }
        Builtin::IntlDisplayNamesResolvedOptions => {
            crate::intl::display_names::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlDurationFormat => {
            Err(Fault::TypeError("Intl.DurationFormat requires new".into()))
        }
        Builtin::IntlDurationFormatFormat
        | Builtin::IntlDurationFormatToParts
        | Builtin::IntlDurationFormatSupportedLocales => Err(Fault::Internal(
            "DurationFormat requires native continuation",
        )),
        Builtin::IntlDurationFormatResolvedOptions => {
            crate::intl::duration_format::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlRelativeTimeFormat => Err(Fault::TypeError(
            "Intl.RelativeTimeFormat requires new".into(),
        )),
        Builtin::IntlRelativeTimeFormatFormat
        | Builtin::IntlRelativeTimeFormatToParts
        | Builtin::IntlRelativeTimeFormatSupportedLocales => {
            Err(Fault::Internal("RelativeTimeFormat requires native frame"))
        }
        Builtin::IntlRelativeTimeFormatResolvedOptions => {
            crate::intl::relative_time_format::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlPluralRules => Err(Fault::TypeError("Intl.PluralRules requires new".into())),
        Builtin::IntlSegmenter => Err(Fault::TypeError("Intl.Segmenter requires new".into())),
        Builtin::IntlSegmenterSegment
        | Builtin::IntlSegmenterSupportedLocales
        | Builtin::IntlSegmentsContaining
        | Builtin::IntlSegmentIteratorNext => {
            Err(Fault::Internal("Segmenter requires native continuation"))
        }
        Builtin::IntlSegmenterResolvedOptions => crate::intl::segmenter::resolved(heap, task, this),
        Builtin::IntlSegmentsIterator => super::native::segmenter::iterator(heap, task, this),
        Builtin::IntlListFormat => Err(Fault::TypeError("Intl.ListFormat requires new".into())),
        Builtin::IntlListFormatFormat
        | Builtin::IntlListFormatFormatToParts
        | Builtin::IntlListFormatSupportedLocales => {
            Err(Fault::Internal("ListFormat requires native continuation"))
        }
        Builtin::IntlListFormatResolvedOptions => {
            crate::intl::list_format::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlCollator
        | Builtin::IntlCollatorBound
        | Builtin::IntlCollatorSupportedLocales => {
            Err(Fault::Internal("Collator requires native frame"))
        }
        Builtin::IntlCollatorGetCompare => crate::intl::collator::get_compare(heap, task, this),
        Builtin::IntlCollatorResolvedOptions => {
            crate::intl::collator::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlPluralRulesSelect | Builtin::IntlPluralRulesSupportedLocales => {
            Err(Fault::Internal("PluralRules requires native frame"))
        }
        Builtin::IntlPluralRulesResolvedOptions => {
            crate::intl::plural_rules::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlNumberFormat
        | Builtin::IntlNumberFormatBound
        | Builtin::IntlNumberFormatToParts
        | Builtin::IntlNumberFormatSupportedLocales => {
            Err(Fault::Internal("NumberFormat requires native frame"))
        }
        Builtin::IntlNumberFormatGetFormat => {
            crate::intl::number_format::get_format(heap, task, this)
        }
        Builtin::IntlNumberFormatResolvedOptions => {
            crate::intl::number_format::record(heap, task, this)?.resolved(heap, task)
        }
        Builtin::IntlLocale => Err(Fault::TypeError("Intl.Locale requires new".into())),
        Builtin::IntlGetCanonicalLocales => {
            Err(Fault::Internal("locale lists require a resumable frame"))
        }
        Builtin::IntlLocaleToString => crate::intl::locale::inspect(heap, task, this, None),
        Builtin::IntlLocaleMaximize | Builtin::IntlLocaleMinimize => {
            crate::intl::locale::likely_subtags(
                heap,
                task,
                this,
                which == Builtin::IntlLocaleMinimize,
            )
        }
        Builtin::IntlLocaleGet(field) => {
            crate::intl::locale::inspect(heap, task, this, Some(field))
        }

        Builtin::TypedArray | Builtin::IntegerTypedArray(_) => Err(Fault::TypeError(
            "typed array constructor requires new (TypedArray is abstract)".into(),
        )),
        Builtin::TypedArrayBuffer
        | Builtin::TypedArrayLength
        | Builtin::TypedArrayByteLength
        | Builtin::TypedArrayByteOffset => {
            crate::object::typed_array::inspect(heap, task, this, which)
        }
        Builtin::StringToUpperCase
        | Builtin::StringToLowerCase
        | Builtin::StringToLocaleUpperCase
        | Builtin::StringToLocaleLowerCase
        | Builtin::StringTrim
        | Builtin::StringTrimStart
        | Builtin::StringTrimEnd
        | Builtin::StringRepeat
        | Builtin::StringStartsWith
        | Builtin::StringEndsWith
        | Builtin::StringIncludes
        | Builtin::StringAt
        | Builtin::StringLocaleCompare
        | Builtin::StringNormalize => {
            Err(Fault::Internal("string method requires resumable frame"))
        }
        Builtin::RegExp
        | Builtin::StringMatch
        | Builtin::StringMatchAll
        | Builtin::StringPrototypeSearch
        | Builtin::RegExpMatch
        | Builtin::RegExpMatchAll
        | Builtin::RegExpSearch
        | Builtin::RegExpReplace
        | Builtin::RegExpSplit
        | Builtin::RegExpIteratorNext
        | Builtin::StringPrototypeReplace
        | Builtin::StringPrototypeReplaceAll
        | Builtin::RegExpExec
        | Builtin::RegExpTest
        | Builtin::RegExpToString
        | Builtin::RegExpFlags => Err(Fault::Internal("RegExp requires a resumable frame")),
        Builtin::RegExpSource | Builtin::RegExpFlag(_) => {
            crate::vm::native::regexp::getter(heap, task, which, this)
        }

        Builtin::GeneratorNext
        | Builtin::GeneratorReturn
        | Builtin::GeneratorThrow
        | Builtin::AsyncGeneratorNext
        | Builtin::AsyncGeneratorReturn
        | Builtin::AsyncGeneratorThrow => {
            Err(Fault::Internal("generator method requires VM state"))
        }
        Builtin::Promise
        | Builtin::PromiseResolve
        | Builtin::PromiseReject
        | Builtin::PromiseThen
        | Builtin::PromiseCatch
        | Builtin::PromiseResolver(_, _)
        | Builtin::PromiseReaction(_)
        | Builtin::PromiseAll
        | Builtin::PromiseRace
        | Builtin::PromiseAllSettled
        | Builtin::PromiseAny
        | Builtin::PromiseWithResolvers
        | Builtin::PromiseFinally
        | Builtin::PromiseCallback(_, _)
        | Builtin::PromiseResolveThenable(_) => {
            Err(Fault::Internal("promise builtin requires VM state"))
        }
        Builtin::ArrayBuffer => Err(Fault::TypeError("ArrayBuffer requires new".into())),
        Builtin::ArrayBufferByteLength | Builtin::ArrayBufferDetached => {
            crate::object::array_buffer::inspect(
                heap,
                task,
                this,
                which == Builtin::ArrayBufferDetached,
            )
        }
        Builtin::ArrayBufferTransfer | Builtin::ArrayBufferSlice => Err(Fault::Internal(
            "ArrayBuffer methods require a resumable frame",
        )),

        Builtin::Date
        | Builtin::DateParse
        | Builtin::DateUtc
        | Builtin::DateSetTime
        | Builtin::DateSetUtcField(_)
        | Builtin::DateSetLocalField(_)
        | Builtin::DateGetLocalField(_)
        | Builtin::DateNow
        | Builtin::DateTimezoneOffset
        | Builtin::DateToPrimitive
        | Builtin::DateToLocalString(_) => Err(Fault::Internal(
            "Date conversion requires a resumable frame",
        )),
        Builtin::DateGetTime | Builtin::DateValueOf => {
            super::date::this_time_value(heap, task, this).map(Value::Number)
        }
        Builtin::DateToISOString => super::date::to_iso_string(heap, task, this),
        Builtin::DateToUtcString => super::date::to_utc_string(heap, task, this),
        Builtin::DateGetUtcField(field) => super::date::get_utc_field(heap, task, this, field),
        Builtin::Error
        | Builtin::ErrorPrototypeToString
        | Builtin::NativeError(_)
        | Builtin::JsonParse
        | Builtin::JsonStringify => Err(Fault::Internal("built-in requires a resumable frame")),
        Builtin::StringRaw => Err(Fault::Internal("String.raw requires a resumable frame")),
        Builtin::Map => Err(Fault::TypeError("Map requires new".to_string())),
        Builtin::Set | Builtin::WeakSet => {
            Err(Fault::TypeError(format!("{} requires new", which.name())))
        }
        Builtin::SetAdd
        | Builtin::SetHas
        | Builtin::SetDelete
        | Builtin::SetClear
        | Builtin::SetSize => set_operation(heap, task, which, this, args),
        Builtin::WeakSetAdd | Builtin::WeakSetHas | Builtin::WeakSetDelete => {
            weak_set_operation(heap, task, which, this, args)
        }
        Builtin::MapKeys | Builtin::MapValues | Builtin::MapEntries => {
            super::native::iterator::create(
                heap,
                task,
                this,
                match which {
                    Builtin::MapKeys => super::native::iterator::Kind::MapKeys,
                    Builtin::MapValues => super::native::iterator::Kind::MapValues,
                    _ => super::native::iterator::Kind::MapEntries,
                },
            )
        }
        Builtin::SetValues | Builtin::SetEntries => super::native::iterator::create(
            heap,
            task,
            this,
            if which == Builtin::SetValues {
                super::native::iterator::Kind::SetValues
            } else {
                super::native::iterator::Kind::SetEntries
            },
        ),
        Builtin::WeakMap => Err(Fault::TypeError("WeakMap requires new".to_string())),
        Builtin::WeakRef => Err(Fault::TypeError("WeakRef requires new".into())),
        Builtin::DisposableStack => Err(Fault::TypeError("DisposableStack requires new".into())),
        Builtin::AsyncDisposableStack => {
            Err(Fault::TypeError("AsyncDisposableStack requires new".into()))
        }
        Builtin::DisposableStackUse
        | Builtin::DisposableStackDispose
        | Builtin::AsyncDisposableStackUse
        | Builtin::AsyncDisposableStackDispose
        | Builtin::AsyncDisposableStackDisposeBody => Err(Fault::Internal(
            "DisposableStack requires a native continuation",
        )),
        Builtin::DisposableStackAdopt
        | Builtin::DisposableStackDefer
        | Builtin::DisposableStackMove
        | Builtin::DisposableStackDisposed => {
            disposable_stack_method(heap, task, which, this, args)
        }
        Builtin::AsyncDisposableStackAdopt
        | Builtin::AsyncDisposableStackDefer
        | Builtin::AsyncDisposableStackMove
        | Builtin::AsyncDisposableStackDisposed => {
            disposable_stack_method(heap, task, which, this, args)
        }
        Builtin::WeakRefDeref => Err(Fault::Internal("WeakRef.deref requires task kept objects")),
        Builtin::FinalizationRegistry => {
            Err(Fault::TypeError("FinalizationRegistry requires new".into()))
        }
        Builtin::FinalizationRegistryRegister | Builtin::FinalizationRegistryUnregister => Err(
            Fault::Internal("FinalizationRegistry methods require task state"),
        ),
        Builtin::WeakMapSet
        | Builtin::WeakMapGet
        | Builtin::WeakMapHas
        | Builtin::WeakMapDelete => weak_map_operation(heap, task, which, this, args),
        Builtin::MapSet
        | Builtin::MapGet
        | Builtin::MapHas
        | Builtin::MapDelete
        | Builtin::MapClear
        | Builtin::MapSize => map_operation(heap, task, which, this, args),
        Builtin::Proxy => Err(Fault::TypeError("Proxy constructor requires new".into())),
        Builtin::Object => object_constructor(heap, task, args),
        Builtin::ObjectIs => Ok(Value::Bool(same_value(&arg(0), &arg(1)))),
        Builtin::MathFloor
        | Builtin::MathCeil
        | Builtin::MathAbs
        | Builtin::MathImul
        | Builtin::MathPow
        | Builtin::MathFround
        | Builtin::MathSqrt
        | Builtin::MathLog2
        | Builtin::MathRound
        | Builtin::MathTrunc
        | Builtin::MathSign
        | Builtin::MathLog
        | Builtin::MathLog10
        | Builtin::MathLog1p
        | Builtin::MathExp
        | Builtin::MathExpm1
        | Builtin::MathSin
        | Builtin::MathCos
        | Builtin::MathTan
        | Builtin::MathAsin
        | Builtin::MathAcos
        | Builtin::MathAtan
        | Builtin::MathAtan2
        | Builtin::MathSinh
        | Builtin::MathCosh
        | Builtin::MathTanh
        | Builtin::MathAsinh
        | Builtin::MathAcosh
        | Builtin::MathAtanh
        | Builtin::MathCbrt
        | Builtin::MathClz32
        | Builtin::MathF16round => math(which, args),
        Builtin::MathHypot | Builtin::MathMin | Builtin::MathMax => Err(Fault::Internal(
            "variadic Math requires a budgeted native frame",
        )),
        Builtin::MathRandom => Err(Fault::Internal("Math.random requires task state")),
        Builtin::ObjectPrototypeToString => Err(Fault::Internal(
            "Object.prototype.toString requires a resumable native frame",
        )),
        Builtin::TypedArrayToStringTag => {
            let Value::Object(id) = this else {
                return Ok(Value::Undefined);
            };
            heap.read(*id, |object| match object {
                HeapObject::TypedArray(array) if array.object.owner() != Some(task) => {
                    Err(Fault::ForeignObject {
                        owner: array.object.owner(),
                        caller: task,
                    })
                }
                HeapObject::TypedArray(array) => Ok(Value::String(array.view.kind.name().into())),
                _ => Ok(Value::Undefined),
            })
            .ok_or(Fault::Internal("typed array was reclaimed"))?
        }
        Builtin::ObjectPrototypeValueOf => match this {
            Value::Undefined | Value::Null => Err(Fault::TypeError(format!(
                "cannot convert {} to object",
                this.type_name()
            ))),
            Value::Object(_) => Ok(this.clone()),
            Value::Bool(_) | Value::BigInt(_) | Value::Symbol(_) => {
                array_string_receiver(heap, task, this)
            }
            v => Err(Fault::Unsupported(format!(
                "valueOf on a {} needs a wrapper object, which is not supported",
                v.type_name()
            ))),
        },
        Builtin::ArrayPrototypeToString | Builtin::ArrayPrototypeJoin => Err(Fault::Internal(
            "array string methods must run through a native frame",
        )),
        Builtin::ArrayPrototypePush => Err(Fault::Internal(
            "push must run through the native trampoline",
        )),
        Builtin::Array => array_constructor(heap, task, args),
        Builtin::ArrayIsArray => is_array(heap, &arg(0)).map(Value::Bool),
        Builtin::ObjectDefineProperty
        | Builtin::ObjectAssign
        | Builtin::ObjectEntries
        | Builtin::ObjectValues
        | Builtin::ObjectFromEntries
        | Builtin::ObjectDefineProperties
        | Builtin::ObjectGetOwnPropertyDescriptors => Err(Fault::Internal(
            "defineProperty must run through the native trampoline",
        )),
        Builtin::ReflectConstruct
        | Builtin::ReflectDefineProperty
        | Builtin::ReflectDeleteProperty
        | Builtin::ReflectGet
        | Builtin::ReflectGetOwnPropertyDescriptor
        | Builtin::ReflectGetPrototypeOf
        | Builtin::ReflectHas
        | Builtin::ReflectIsExtensible
        | Builtin::ReflectOwnKeys
        | Builtin::ReflectPreventExtensions
        | Builtin::ReflectSet
        | Builtin::ReflectSetPrototypeOf => Err(Fault::Internal(
            "Reflect method must run through the native trampoline",
        )),
        Builtin::ObjectGetOwnPropertyDescriptor => {
            descriptor::get_own_property_descriptor(heap, task, args)
        }
        Builtin::ObjectGetPrototypeOf => prototype::object_get_prototype_of(heap, task, args),
        Builtin::ObjectSetPrototypeOf => prototype::object_set_prototype_of(heap, task, args),
        Builtin::ObjectPrototypeProtoGetter => prototype::proto_getter(heap, task, this),
        Builtin::ObjectPrototypeProtoSetter => prototype::proto_setter(heap, task, this, &arg(0)),
        Builtin::ObjectCreate => prototype::object_create(heap, task, args),
        Builtin::ObjectPreventExtensions => integrity::prevent_extensions(heap, task, &arg(0)),
        Builtin::ObjectIsExtensible => integrity::is_extensible(heap, task, &arg(0)),
        Builtin::ObjectFreeze => integrity::freeze(heap, task, &arg(0)),
        Builtin::ObjectIsFrozen => integrity::is_frozen(heap, task, &arg(0)),
        Builtin::ObjectSeal => Ok(arg(0)), // Objects use the resumable integrity path.
        Builtin::ObjectIsSealed => Ok(Value::Bool(true)),
        Builtin::ObjectKeys => keys::object_keys(heap, task, args, KeyFilter::Enumerable),
        Builtin::ObjectGetOwnPropertyNames => keys::object_keys(heap, task, args, KeyFilter::All),
        Builtin::FunctionPrototype => Ok(Value::Undefined),
        Builtin::FunctionPrototypeBind => Err(Fault::Internal(
            "bind must run through the native trampoline",
        )),
        Builtin::Function => Err(Fault::Internal(
            "Function must run through the native trampoline",
        )),
        // Every call site resolves these with `forward` first.
        Builtin::FunctionPrototypeCall
        | Builtin::FunctionPrototypeApply
        | Builtin::ReflectApply => {
            Err(Fault::Internal("a forwarding built-in was called directly"))
        }
        Builtin::Boolean
        | Builtin::Number
        | Builtin::String
        | Builtin::Symbol
        | Builtin::BigInt => primitive_constructor(heap, task, which, args, false),
        Builtin::SymbolFor => {
            let key = to_js_string(&arg(0))?;
            Ok(Value::Symbol(heap.symbols.symbol_for(key)))
        }
        Builtin::SymbolKeyFor => match arg(0) {
            Value::Symbol(symbol) => Ok(heap
                .symbols
                .key_for(&symbol)
                .map(Value::String)
                .unwrap_or(Value::Undefined)),
            _ => Err(Fault::TypeError("Symbol.keyFor requires a symbol".into())),
        },
        Builtin::SymbolPrototypeToString => {
            symbol_descriptive_string(&this_symbol_value(heap, task, this)?).map(Value::String)
        }
        Builtin::SymbolPrototypeValueOf | Builtin::SymbolPrototypeToPrimitive => {
            this_symbol_value(heap, task, this).map(Value::Symbol)
        }
        Builtin::SymbolPrototypeDescription => Ok(this_symbol_value(heap, task, this)?
            .description()
            .cloned()
            .map(Value::String)
            .unwrap_or(Value::Undefined)),
        Builtin::BigIntPrototypeToString => Err(Fault::Internal(
            "BigInt formatting requires a resumable frame",
        )),
        Builtin::BigIntAsIntN | Builtin::BigIntAsUintN => Err(Fault::Internal(
            "BigInt wrapping requires a resumable frame",
        )),
        Builtin::BigIntPrototypeValueOf => this_bigint_value(heap, task, this).map(Value::BigInt),
        Builtin::BooleanPrototypeValueOf => this_boolean_value(heap, task, this).map(Value::Bool),
        Builtin::BooleanPrototypeToString => Ok(Value::String(JsString::from(
            if this_boolean_value(heap, task, this)? {
                "true"
            } else {
                "false"
            },
        ))),
        Builtin::NumberPrototypeToString => {
            let n = this_number_value(heap, task, this, "toString")?;
            number_to_string(n, &arg(0))
        }
        Builtin::NumberPrototypeToFixed => {
            let n = this_number_value(heap, task, this, "toFixed")?;
            number_to_fixed(n, &arg(0))
        }
        Builtin::NumberPrototypeToExponential | Builtin::NumberPrototypeToPrecision => {
            let n = this_number_value(heap, task, this, which.name())?;
            number_to_precision_format(which, n, &arg(0), matches!(arg(0), Value::Undefined))
        }
        Builtin::NumberPrototypeValueOf => {
            this_number_value(heap, task, this, "valueOf").map(Value::Number)
        }
        Builtin::StringPrototypeToString | Builtin::StringPrototypeValueOf => {
            this_string_value(heap, task, this, which.name()).map(Value::String)
        }
        Builtin::StringPrototypeCharAt
        | Builtin::StringPrototypeCharCodeAt
        | Builtin::StringPrototypeCodePointAt => string_access(which, this, &arg(0)),
        Builtin::StringFromCharCode | Builtin::StringFromCodePoint => Err(Fault::Internal(
            "string code construction requires a resumable native frame",
        )),
        Builtin::StringPrototypeIndexOf
        | Builtin::StringPrototypeLastIndexOf
        | Builtin::StringPrototypeSubstring
        | Builtin::StringPrototypeSlice
        | Builtin::StringPrototypeConcat
        | Builtin::StringPrototypePadStart
        | Builtin::StringPrototypePadEnd
        | Builtin::StringPrototypeSplit
        | Builtin::ParseInt
        | Builtin::ParseFloat
        | Builtin::Eval => Err(Fault::Internal(
            "operation requires a resumable native frame",
        )),
        Builtin::GlobalIsNaN
        | Builtin::GlobalIsFinite
        | Builtin::NumberIsNaN
        | Builtin::NumberIsFinite
        | Builtin::NumberIsInteger
        | Builtin::NumberIsSafeInteger => number_predicate(which, &arg(0)),
    }
}

/// Fixed-arity Math operations, after resumable ToPrimitive for objects.
/// ToInt32 and ToUint32 have identical bits, including after multiplication.
pub(super) fn math(which: Builtin, args: &[Value]) -> Result<Value, Fault> {
    let x = to_number(args.first().unwrap_or(&Value::Undefined))?;
    let result = match which {
        Builtin::MathFloor => x.floor(),
        Builtin::MathCeil => x.ceil(),
        Builtin::MathAbs => x.abs(),
        Builtin::MathFround => f64::from(x as f32),
        Builtin::MathPow => {
            let y = to_number(args.get(1).unwrap_or(&Value::Undefined))?;
            super::dispatch::exponentiate(x, y)
        }
        Builtin::MathSqrt => x.sqrt(),
        Builtin::MathLog2 => math_log2(x),
        Builtin::MathRound => {
            if !x.is_finite() || x == 0.0 {
                x
            } else {
                // Do not add 0.5 to x: that double-rounds the predecessor
                // of 0.5. Negative ties round toward positive infinity.
                let floor = x.floor();
                let rounded = if x - floor < 0.5 { floor } else { floor + 1.0 };
                if rounded == 0.0 {
                    0.0_f64.copysign(x)
                } else {
                    rounded
                }
            }
        }
        Builtin::MathTrunc => x.trunc(),
        Builtin::MathSign => {
            if x.is_nan() || x == 0.0 {
                x
            } else {
                x.signum()
            }
        }
        Builtin::MathLog => x.ln(),
        Builtin::MathLog10 => x.log10(),
        Builtin::MathLog1p => x.ln_1p(),
        Builtin::MathExp => x.exp(),
        Builtin::MathExpm1 => x.exp_m1(),
        Builtin::MathSin => x.sin(),
        Builtin::MathCos => x.cos(),
        Builtin::MathTan => x.tan(),
        Builtin::MathAsin => x.asin(),
        Builtin::MathAcos => x.acos(),
        Builtin::MathAtan => x.atan(),
        Builtin::MathAtan2 => x.atan2(to_number(args.get(1).unwrap_or(&Value::Undefined))?),
        Builtin::MathSinh => x.sinh(),
        Builtin::MathCosh => x.cosh(),
        Builtin::MathTanh => x.tanh(),
        Builtin::MathAsinh | Builtin::MathAcosh | Builtin::MathAtanh | Builtin::MathCbrt => {
            math_libm(which, x)
        }
        Builtin::MathClz32 => f64::from((super::dispatch::to_int32(x) as u32).leading_zeros()),
        Builtin::MathF16round => crate::object::typed_array::round_binary16(x),
        Builtin::MathImul => {
            let y = to_number(args.get(1).unwrap_or(&Value::Undefined))?;
            f64::from(super::dispatch::to_int32(x).wrapping_mul(super::dispatch::to_int32(y)))
        }
        _ => return Err(Fault::Internal("not a fixed-arity Math operation")),
    };
    Ok(Value::Number(result))
}

/// Powers of two are exact, including subnormals; other inputs use the
/// platform's log2, whose precision can vary across platforms/Rust versions.
fn math_log2(x: f64) -> f64 {
    if x > 0.0 && x.is_finite() {
        let bits = x.to_bits();
        let exponent = (bits >> 52) as i32;
        let fraction = bits & ((1_u64 << 52) - 1);
        if exponent != 0 && fraction == 0 {
            return f64::from(exponent - 1023);
        }
        if exponent == 0 && fraction.is_power_of_two() {
            return f64::from(fraction.trailing_zeros() as i32 - 1074);
        }
    }
    x.log2()
}

// Rust 1.95's cbrt and inverse hyperbolic formulas differ from QuickJS's
// system libm. In particular, asinh/acosh overflow for large finite inputs.
fn math_libm(which: Builtin, x: f64) -> f64 {
    #[cfg_attr(unix, link(name = "m"))]
    unsafe extern "C" {
        // On glibc use its binary64 aliases (glibc >= 2.27), so Rust's weak
        // compiler-builtins symbols cannot interpose the host libm calls.
        #[cfg_attr(all(target_os = "linux", target_env = "gnu"), link_name = "cbrtf64")]
        fn cbrt(x: f64) -> f64;
        #[cfg_attr(all(target_os = "linux", target_env = "gnu"), link_name = "asinhf64")]
        fn asinh(x: f64) -> f64;
        #[cfg_attr(all(target_os = "linux", target_env = "gnu"), link_name = "acoshf64")]
        fn acosh(x: f64) -> f64;
        #[cfg_attr(all(target_os = "linux", target_env = "gnu"), link_name = "atanhf64")]
        fn atanh(x: f64) -> f64;
    }
    // SAFETY: these libm functions accept every f64, touch no Rust storage,
    // and neither suspend nor reenter the VM. Binary64 aliases have the
    // same C ABI as their double counterparts on glibc.
    unsafe {
        match which {
            Builtin::MathCbrt => cbrt(x),
            Builtin::MathAsinh => asinh(x),
            Builtin::MathAcosh => acosh(x),
            Builtin::MathAtanh => atanh(x),
            _ => unreachable!("not a libm Math operation"),
        }
    }
}

/// Global predicates receive a primitive after resumable ToPrimitive;
/// Number predicates inspect only the Number type and never call JavaScript.
pub(super) fn number_predicate(which: Builtin, value: &Value) -> Result<Value, Fault> {
    let n = if matches!(which, Builtin::GlobalIsNaN | Builtin::GlobalIsFinite) {
        crate::vm::dispatch::to_number(value)?
    } else if let Value::Number(n) = value {
        *n
    } else {
        return Ok(Value::Bool(false));
    };
    let result = match which {
        Builtin::GlobalIsNaN | Builtin::NumberIsNaN => n.is_nan(),
        Builtin::GlobalIsFinite | Builtin::NumberIsFinite => n.is_finite(),
        Builtin::NumberIsInteger => n.is_finite() && n.trunc() == n,
        Builtin::NumberIsSafeInteger => {
            n.is_finite() && n.trunc() == n && n.abs() <= 9_007_199_254_740_991.0
        }
        _ => return Err(Fault::Internal("not a number predicate")),
    };
    Ok(Value::Bool(result))
}

/// RequireObjectCoercible, ToString, then ToIntegerOrInfinity. These methods
/// accept any primitive receiver; thisStringValue would be too restrictive.
/// Object operands have already passed through native::Convert.
fn string_access(which: Builtin, this: &Value, pos: &Value) -> Result<Value, Fault> {
    if matches!(this, Value::Undefined | Value::Null) {
        return Err(Fault::TypeError(format!(
            "String.prototype.{} called on {}",
            which.name(),
            this.type_name()
        )));
    }
    let string = crate::vm::dispatch::to_js_string(this)?;
    let number = crate::vm::dispatch::to_number(pos)?;
    let index = if number.is_nan() { 0.0 } else { number.trunc() };
    // Check before casting: infinity and huge indices must not saturate or wrap.
    let unit = if index >= 0.0 && index < string.len() as f64 {
        string.code_units().get(index as usize).copied()
    } else {
        None
    };
    Ok(match (which, unit) {
        (Builtin::StringPrototypeCharAt, Some(unit)) => {
            Value::String(JsString::from_code_units(vec![unit]).expect("one code unit fits"))
        }
        (Builtin::StringPrototypeCharAt, None) => Value::String(JsString::from("")),
        (Builtin::StringPrototypeCharCodeAt, None) => Value::Number(f64::NAN),
        (Builtin::StringPrototypeCodePointAt, Some(high @ 0xd800..=0xdbff)) => {
            let point = match string.code_units().get(index as usize + 1).copied() {
                Some(low @ 0xdc00..=0xdfff) => {
                    0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(low) - 0xdc00
                }
                _ => u32::from(high),
            };
            Value::Number(f64::from(point))
        }
        (_, Some(unit)) => Value::Number(f64::from(unit)),
        (_, None) => Value::Undefined,
    })
}

/// Leaf encoding after native::Convert has completed budgeted ToNumber and
/// validation in argument order. O(args), at most two output units per
/// argument; the configured stack limit bounds this final allocation/copy.
pub(super) fn string_from_codes(which: Builtin, args: &[Value]) -> Result<Value, Fault> {
    let mut units = Vec::new();
    for arg in args {
        let Value::Number(n) = *arg else {
            return Err(Fault::Internal("string code argument was not converted"));
        };
        if which != Builtin::StringFromCharCode {
            validate_code_point(n)?;
        }
        let width = usize::from(which != Builtin::StringFromCharCode && n > 0xffff as f64) + 1;
        if units.len() > crate::value::string::MAX_STRING_LEN - width {
            return Err(Fault::StringTooLong);
        }
        if which == Builtin::StringFromCharCode {
            // ToUint16: truncate, modulo 2^16, non-finite values become zero.
            let unit = if n.is_finite() {
                n.trunc() % 65536.0
            } else {
                0.0
            };
            let unit = if unit < 0.0 { unit + 65536.0 } else { unit };
            units.push(unit as u16);
        } else {
            let point = n as u32;
            if point <= 0xffff {
                units.push(point as u16);
            } else {
                let offset = point - 0x10000;
                units.push(0xd800 | (offset >> 10) as u16);
                units.push(0xdc00 | (offset & 0x3ff) as u16);
            }
        }
    }
    JsString::from_code_units(units)
        .map(Value::String)
        .ok_or(Fault::StringTooLong)
}

fn validate_code_point(n: f64) -> Result<(), Fault> {
    if !n.is_finite() || !(0.0..=0x10ffff as f64).contains(&n) || n.trunc() != n {
        return Err(Fault::RangeError("invalid code point".to_string()));
    }
    Ok(())
}

/// Finish each primitive conversion before native::Convert visits the next
/// operand. In particular, invalid code points must throw before later
/// arguments' valueOf/toString callbacks can run.
pub(crate) fn string_builtin_operand(
    which: Builtin,
    index: usize,
    value: &Value,
) -> Result<Value, Fault> {
    match which {
        Builtin::StringPrototypeCharAt
        | Builtin::StringPrototypeCharCodeAt
        | Builtin::StringPrototypeCodePointAt
            if index == 0 =>
        {
            crate::vm::dispatch::to_js_string(value).map(Value::String)
        }
        Builtin::StringPrototypeIndexOf | Builtin::StringPrototypeLastIndexOf if index < 2 => {
            crate::vm::dispatch::to_js_string(value).map(Value::String)
        }
        _ if string_operand_is_text(which, index) => {
            crate::vm::dispatch::to_js_string(value).map(Value::String)
        }
        _ => {
            let n = crate::vm::dispatch::to_number(value)?;
            if which == Builtin::StringFromCodePoint {
                validate_code_point(n)?;
            }
            Ok(Value::Number(n))
        }
    }
}

/// Conversion hints and primitive conversion must agree for every operand.
pub(crate) fn string_operand_is_text(which: Builtin, index: usize) -> bool {
    use Builtin::*;
    match which {
        StringPrototypeConcat => true,
        StringPrototypeIndexOf | StringPrototypeLastIndexOf => index < 2,
        StringPrototypePadStart | StringPrototypePadEnd => index != 1,
        _ => index == 0 && !matches!(which, StringFromCharCode | StringFromCodePoint),
    }
}

pub(crate) fn is_string_build(which: Builtin) -> bool {
    matches!(
        which,
        Builtin::StringPrototypeSubstring
            | Builtin::StringPrototypeSlice
            | Builtin::StringPrototypeConcat
            | Builtin::StringPrototypePadStart
            | Builtin::StringPrototypePadEnd
    )
}

/// After receiver/arguments have been converted in observable order, plan
/// bounded copying. Undefined ends default to length; padding checks its
/// empty filler before the engine length limit, including an infinite target.
pub(crate) fn string_build(
    heap: &Heap,
    which: Builtin,
    operands: &[Value],
) -> Result<crate::value::string::StringBuild, Fault> {
    use Builtin::*;
    let text = to_js_string(&operands[0])?;
    let len = text.len();
    let parts = match which {
        StringPrototypeSubstring | StringPrototypeSlice => {
            let position = |v: &Value| -> Result<usize, Fault> {
                let n = to_number(v)?;
                let n = if n.is_nan() { 0.0 } else { n.trunc() };
                let n = if which == StringPrototypeSlice && n < 0.0 {
                    len as f64 + n
                } else {
                    n
                };
                Ok(n.clamp(0.0, len as f64) as usize)
            };
            let mut start = position(&operands[1])?;
            let mut end = match operands.get(2) {
                None => len,
                Some(value) => position(value)?,
            };
            if which == StringPrototypeSubstring && start > end {
                std::mem::swap(&mut start, &mut end);
            }
            vec![(text, start, end.saturating_sub(start))]
        }
        StringPrototypeConcat => operands
            .iter()
            .map(|v| {
                let s = to_js_string(v)?;
                let len = s.len();
                Ok((s, 0, len))
            })
            .collect::<Result<Vec<_>, Fault>>()?,
        StringPrototypePadStart | StringPrototypePadEnd => {
            let target = to_number(&operands[1])?;
            if target.is_nan() || target.trunc() <= len as f64 {
                vec![(text, 0, len)]
            } else {
                let fill = match operands.get(2) {
                    None => JsString::from(" "),
                    Some(value) => to_js_string(value)?,
                };
                if fill.is_empty() {
                    vec![(text, 0, len)]
                } else {
                    if target.trunc() > crate::value::string::MAX_STRING_LEN as f64 {
                        return Err(Fault::StringTooLong);
                    }
                    let padding = (fill, 0, target.trunc() as usize - len);
                    if which == StringPrototypePadStart {
                        vec![padding, (text, 0, len)]
                    } else {
                        vec![(text, 0, len), padding]
                    }
                }
            }
        }
        _ => return Err(Fault::Internal("not a string build operation")),
    };
    parts.iter().try_fold(0usize, |total, (_, _, len)| {
        total
            .checked_add(*len)
            .filter(|total| *total <= crate::value::string::MAX_STRING_LEN)
            .ok_or(Fault::StringTooLong)
    })?;
    if which == StringPrototypeConcat {
        crate::value::string::StringBuild::concatenation_accounted(heap, parts)
    } else {
        crate::value::string::StringBuild::new_accounted(heap, parts)
    }
}

/// Receiver validation precedes radix coercion; the continuation retains only
/// the validated numeric value and the radix operand.
pub(crate) fn number_to_string(n: f64, radix: &Value) -> Result<Value, Fault> {
    let radix = to_radix(radix)?;
    Ok(Value::String(JsString::from(
        number_to_string_radix(n, radix).as_str(),
    )))
}

pub(crate) fn to_radix(radix: &Value) -> Result<u32, Fault> {
    let radix = if matches!(radix, Value::Undefined) {
        10.0
    } else {
        to_number(radix)?.trunc()
    };
    if !(2.0..=36.0).contains(&radix) {
        return Err(Fault::RangeError(
            "radix must be between 2 and 36".to_string(),
        ));
    }
    Ok(radix as u32)
}

fn number_digit_count(digits: &Value, method: &str) -> Result<f64, Fault> {
    // Bound StringToNumber before it flattens or scans, including a string
    // produced by resumable ToPrimitive. Generic budgeted parsing is -265.
    const MAX_DIGIT_CONVERSION_UNITS: usize = 4096;
    if let Value::String(text) = digits
        && text.len() > MAX_DIGIT_CONVERSION_UNITS
    {
        return Err(Fault::RangeError(format!(
            "{method} digit conversion limit of {MAX_DIGIT_CONVERSION_UNITS} UTF-16 units exceeded"
        )));
    }
    let digits = to_number(digits)?;
    // ToIntegerOrInfinity maps NaN to zero and truncates finite fractions.
    Ok(if digits.is_nan() { 0.0 } else { digits.trunc() })
}

pub(crate) fn number_to_fixed(number: f64, digits: &Value) -> Result<Value, Fault> {
    let digits = number_digit_count(digits, "toFixed")?;
    if !(0.0..=100.0).contains(&digits) {
        return Err(Fault::RangeError("invalid number of digits".into()));
    }
    Ok(Value::String(JsString::from(
        crate::number::to_fixed(number, digits as usize).as_str(),
    )))
}

pub(crate) fn number_to_precision_format(
    which: Builtin,
    number: f64,
    digits: &Value,
    originally_undefined: bool,
) -> Result<Value, Fault> {
    // ToPrecision(undefined) returns ToString directly. Otherwise conversion
    // happens even for a nonfinite receiver, before skipping the range check.
    let precision = which == Builtin::NumberPrototypeToPrecision;
    let count = if precision && originally_undefined {
        0.0
    } else {
        number_digit_count(digits, which.name())?
    };
    let text = if !number.is_finite() || (precision && originally_undefined) {
        crate::value::string::number_to_string(number)
    } else {
        let count = if originally_undefined {
            None
        } else {
            if !(if precision { 1.0 } else { 0.0 }..=100.0).contains(&count) {
                return Err(Fault::RangeError("invalid number of digits".into()));
            }
            Some(count as usize)
        };
        if precision {
            crate::number::to_precision(number, count)
        } else {
            crate::number::to_exponential(number, count)
        }
    };
    Ok(Value::String(JsString::from(text.as_str())))
}

/// thisNumberValue (ECMAScript §21.1.3; QuickJS `js_thisNumberValue`): a
/// number receiver is itself; a Number wrapper yields its [[NumberData]],
/// including `Number.prototype`'s +0. Other receivers throw a TypeError.
pub(crate) fn this_number_value(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    method: &str,
) -> Result<f64, Fault> {
    if let Value::Number(n) = this {
        return Ok(*n);
    }
    match wrapper_data(heap, task, this)? {
        Some(PrimitiveData::Number(n)) => Ok(n),
        _ => Err(Fault::TypeError(format!(
            "Number.prototype.{method} requires that 'this' be a Number, not {}",
            describe_receiver(this)
        ))),
    }
}

/// thisStringValue (ECMAScript §22.1.3; QuickJS `js_thisStringValue`): a
/// string receiver is itself; a String wrapper yields its [[StringData]],
/// including `String.prototype`'s "". Other receivers throw a TypeError.
fn this_string_value(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    method: &str,
) -> Result<JsString, Fault> {
    if let Value::String(s) = this {
        return Ok(s.clone());
    }
    match wrapper_data(heap, task, this)? {
        Some(PrimitiveData::String(s)) => Ok(s),
        _ => Err(Fault::TypeError(format!(
            "String.prototype.{method} requires that 'this' be a String, not {}",
            describe_receiver(this)
        ))),
    }
}

fn symbol_descriptive_string(symbol: &crate::value::JsSymbol) -> Result<JsString, Fault> {
    JsString::from("Symbol(")
        .concat(symbol.description().unwrap_or(&JsString::from("")))
        .and_then(|s| s.concat(&JsString::from(")")))
        .ok_or(Fault::StringTooLong)
}

/// Brand checks inspect only the receiver's own immutable internal data.
fn this_boolean_value(heap: &Heap, task: TaskId, this: &Value) -> Result<bool, Fault> {
    if let Value::Bool(value) = this {
        return Ok(*value);
    }
    match wrapper_data(heap, task, this)? {
        Some(PrimitiveData::Boolean(value)) => Ok(value),
        _ => Err(Fault::TypeError("not a boolean".into())),
    }
}

pub(crate) fn this_bigint_value(
    heap: &Heap,
    task: TaskId,
    this: &Value,
) -> Result<crate::value::JsBigInt, Fault> {
    if let Value::BigInt(value) = this {
        return Ok(value.clone());
    }
    match wrapper_data(heap, task, this)? {
        Some(PrimitiveData::BigInt(value)) => Ok((*value).clone()),
        _ => Err(Fault::TypeError("not a BigInt".into())),
    }
}

/// thisSymbolValue accepts a symbol or its own branded wrapper, never a prototype slot.
fn this_symbol_value(
    heap: &Heap,
    task: TaskId,
    this: &Value,
) -> Result<crate::value::JsSymbol, Fault> {
    if let Value::Symbol(symbol) = this {
        return Ok(symbol.clone());
    }
    match wrapper_data(heap, task, this)? {
        Some(PrimitiveData::Symbol(symbol)) => Ok(symbol),
        _ => Err(Fault::TypeError("not a symbol".into())),
    }
}

/// ToObject(symbol), following the task-owned Number/String wrapper pattern.
fn symbol_wrapper(heap: &Heap, task: TaskId, value: &Value) -> Result<Value, Fault> {
    let Value::Symbol(symbol) = value else {
        return Err(Fault::Internal("not a symbol primitive"));
    };
    let realm = realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    let object = OrdinaryObject::with_prototype(heap, task, realm.symbol_prototype)
        .with_primitive_data(PrimitiveData::Symbol(symbol.clone()));
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

/// Inspect the receiver's own internal slots, never its prototype's slots.
/// Owned wrappers obey the same read boundary as their ordinary properties.
pub(super) fn wrapper_data(
    heap: &Heap,
    task: TaskId,
    this: &Value,
) -> Result<Option<PrimitiveData>, Fault> {
    let Value::Object(id) = this else {
        return Ok(None);
    };
    heap.read(*id, |object| match object {
        HeapObject::Plain(o) => {
            if let Some(owner) = o.owner()
                && owner != task
            {
                return Err(Fault::ForeignObject {
                    owner: Some(owner),
                    caller: task,
                });
            }
            Ok(o.primitive_data().cloned())
        }
        _ => Ok(None),
    })
    .unwrap_or(Err(Fault::Internal("wrapper receiver was reclaimed")))
}

/// ToBigInt after resumable ToPrimitive. StringToBigInt runs in a native frame;
/// unlike the BigInt constructor, this conversion never accepts a Number.
pub(crate) fn to_bigint(value: &Value) -> Result<Value, Fault> {
    match value {
        Value::BigInt(_) => Ok(value.clone()),
        Value::Bool(b) => Ok(Value::BigInt(crate::value::JsBigInt::from(i64::from(*b)))),
        Value::String(_) => Err(Fault::Internal("StringToBigInt requires a resumable frame")),
        _ => Err(Fault::TypeError("cannot convert to BigInt".into())),
    }
}

pub(crate) fn bigint_as_n(bits: u64, value: &Value, signed: bool) -> Result<Value, Fault> {
    let Value::BigInt(value) = to_bigint(value)? else {
        unreachable!("ToBigInt returns a BigInt")
    };
    if signed {
        value.checked_as_int_n(bits)
    } else {
        value.checked_as_uint_n(bits)
    }
    .map(Value::BigInt)
    .map_err(|error| Fault::RangeError(error.to_string()))
}

/// Finish a primitive constructor after resumable ToPrimitive. Arguments remain
/// traced on the calling frame's operand stack or in its native continuation.
/// The result replaces them before any safepoint, needing no temporary root.
pub(crate) fn primitive_constructor(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    args: &[Value],
    construct: bool,
) -> Result<Value, Fault> {
    let value = match which {
        Builtin::Boolean => Value::Bool(args.first().is_some_and(Value::truthy)),
        Builtin::BigInt if !construct => match args.first().unwrap_or(&Value::Undefined) {
            Value::Number(n) => Value::BigInt(
                crate::value::JsBigInt::from_integral_number(*n).ok_or_else(|| {
                    Fault::RangeError(
                        "cannot convert non-integral or non-finite Number to BigInt".into(),
                    )
                })?,
            ),
            value => to_bigint(value)?,
        },
        Builtin::Number => Value::Number(match args.first() {
            // Number uses ToNumeric and explicitly permits BigInt. All
            // implicit numeric conversions keep using rejecting ToNumber.
            Some(Value::BigInt(value)) => value.to_number(),
            Some(value) => to_number(value)?,
            None => 0.0,
        }),
        Builtin::String => Value::String(match args.first() {
            Some(Value::Symbol(s)) if !construct => symbol_descriptive_string(s)?,
            Some(value) => to_js_string(value)?,
            None => JsString::from(""),
        }),
        Builtin::Symbol if !construct => {
            Value::Symbol(crate::value::JsSymbol::new(match args.first() {
                None | Some(Value::Undefined) => None,
                Some(value) => Some(to_js_string(value)?),
            }))
        }
        _ => return Err(Fault::Internal("not a primitive constructor")),
    };
    if !construct {
        return Ok(value);
    }
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let realm = realm::intrinsics(heap).ok_or_else(exhausted)?;
    let (prototype, data) = match value {
        Value::Number(n) => (realm.number_prototype, PrimitiveData::Number(n)),
        Value::String(s) => (realm.string_prototype, PrimitiveData::String(s)),
        Value::Bool(b) => (realm.boolean_prototype, PrimitiveData::Boolean(b)),
        _ => return Err(Fault::Internal("not a wrapper primitive")),
    };
    let object = OrdinaryObject::with_prototype(heap, task, prototype).with_primitive_data(data);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// ToObject for generic built-ins and object environments. Primitive payloads
/// are immutable; the wrapper's ordinary properties belong to the task.
pub(crate) fn array_string_receiver(
    heap: &Heap,
    task: TaskId,
    this: &Value,
) -> Result<Value, Fault> {
    match this {
        Value::Object(_) => Ok(this.clone()),
        Value::Number(_) | Value::String(_) => primitive_constructor(
            heap,
            task,
            if matches!(this, Value::Number(_)) {
                Builtin::Number
            } else {
                Builtin::String
            },
            std::slice::from_ref(this),
            true,
        ),
        Value::Undefined | Value::Null => Err(Fault::TypeError(format!(
            "cannot convert {} to object",
            this.type_name()
        ))),
        Value::Bool(_) => primitive_constructor(
            heap,
            task,
            Builtin::Boolean,
            std::slice::from_ref(this),
            true,
        ),
        Value::Symbol(_) => symbol_wrapper(heap, task, this),
        Value::BigInt(value) => {
            let realm =
                realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            let object = OrdinaryObject::with_prototype(heap, task, realm.bigint_prototype)
                .with_primitive_data(PrimitiveData::BigInt(std::sync::Arc::new(value.clone())));
            heap.alloc(HeapObject::Plain(object))
                .map(Value::Object)
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))
        }
    }
}

/// A receiver's kind for a thisNumberValue/thisStringValue message.
fn describe_receiver(v: &Value) -> &'static str {
    match v {
        Value::Undefined => "undefined",
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Symbol(_) => "a symbol",
        Value::BigInt(_) => "a BigInt",
        Value::Object(_) => "an object",
    }
}

/// The call a forwarding built-in (`Function.prototype.call`,
/// `Function.prototype.apply`, `Reflect.apply`) makes in its place (module
/// docs, "Forwarding built-ins").
pub(crate) struct Forward {
    pub(crate) callee: Value,
    pub(crate) this: Value,
    pub(crate) args: Vec<Value>,
}

/// Fold a synchronous `Function.prototype.call` forwarding hop. Apply
/// operations return None and run their list expansion resumably instead.
pub(crate) fn forward(
    heap: &Heap,
    _task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
    _max_args: usize,
) -> Result<Option<Forward>, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    let (callee, this, args) = match which {
        // `call(thisArg, ...args)`.
        Builtin::FunctionPrototypeCall => {
            callable(heap, this)?;
            (this.clone(), arg(0), args.get(1..).unwrap_or(&[]).to_vec())
        }
        // apply reads user properties, so the native trampoline expands
        // its list before requesting the target call.
        _ => return Ok(None),
    };
    Ok(Some(Forward { callee, this, args }))
}

/// IsCallable, failing with the `… is not a function` TypeError.
fn callable(heap: &Heap, v: &Value) -> Result<(), Fault> {
    crate::vm::dispatch::callee(heap, v).map(drop)
}

/// Object's call and direct construction share argument rules. ToObject does
/// not call user coercion hooks: reuse the existing primitive
/// wrappers, retaining object arguments by identity. Operands stay traced in
/// the caller until replacement, before the next safepoint.
pub(crate) fn object_constructor(
    heap: &Heap,
    task: TaskId,
    args: &[Value],
) -> Result<Value, Fault> {
    match args.first().unwrap_or(&Value::Undefined) {
        Value::Undefined | Value::Null => new_object(heap, task),
        value => array_string_receiver(heap, task, value),
    }
}

/// A new ordinary object owned by `task` whose prototype is
/// `Object.prototype` (OrdinaryObjectCreate(%Object.prototype%)).
pub(crate) fn new_object(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let prototype = realm::intrinsics(heap)
        .ok_or_else(exhausted)?
        .object_prototype;
    let object = OrdinaryObject::with_prototype(heap, task, prototype);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// No argument conversion is required by Array: a Number is a length,
/// every other value is an element, including Number wrappers. Copying
/// elements is bounded by the VM's argument/value-stack limit; a numeric
/// length takes constant time and storage. Allocation publishes the
/// complete array before the next safepoint; arguments remain VM roots.
pub(crate) fn array_constructor(heap: &Heap, task: TaskId, args: &[Value]) -> Result<Value, Fault> {
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let prototype = realm::intrinsics(heap)
        .ok_or_else(exhausted)?
        .array_prototype;
    let array = if let [Value::Number(number)] = args {
        let length = *number as u32;
        if f64::from(length) != *number {
            return Err(Fault::RangeError("invalid array length".to_string()));
        }
        ArrayObject::with_length(heap, task, prototype, length)
    } else {
        let mut array = ArrayObject::with_prototype(heap, task, prototype);
        for value in args {
            array.push(Some(value.clone()))?;
        }
        array
    };
    heap.alloc(HeapObject::Array(Box::new(array)))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// Allocate the task-owned target before the resumable iterable consumer.
pub(crate) fn map_constructor(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .map_prototype;
    let map = crate::object::map::MapObject::new(heap, task, prototype);
    heap.alloc(HeapObject::JsMap(Box::new(map)))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

fn map_operation(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let invalid = || {
        Fault::TypeError(format!(
            "Map.prototype.{} requires a Map receiver",
            which.name()
        ))
    };
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    // The frame roots receiver and arguments throughout this leaf operation.
    // The object lock is the same task-owned mutation boundary as plain objects.
    heap.with_object_mut(*id, |object| {
        let HeapObject::JsMap(map) = object else {
            return Err(invalid());
        };
        if map.object().owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: map.object().owner(),
                caller: task,
            });
        }
        let key = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(match which {
            Builtin::MapSet => {
                map.set(key, args.get(1).cloned().unwrap_or(Value::Undefined))
                    .map_err(|message| Fault::RangeError(message.into()))?;
                this.clone()
            }
            Builtin::MapGet => map.get(&key).cloned().unwrap_or(Value::Undefined),
            Builtin::MapHas => Value::Bool(map.get(&key).is_some()),
            Builtin::MapDelete => Value::Bool(map.delete(&key)),
            Builtin::MapClear => {
                map.clear();
                Value::Undefined
            }
            Builtin::MapSize => Value::Number(map.size() as f64),
            _ => return Err(Fault::Internal("not a Map operation")),
        })
    })
    .unwrap_or(Err(Fault::Internal("Map receiver was reclaimed")))
}

/// Array.prototype is an immutable empty-array intrinsic whose property
/// storage remains ordinary. Its realm identity carries the array brand;
/// prototype inheritance alone never makes an ordinary object an array.
pub(crate) fn is_array(heap: &Heap, value: &Value) -> Result<bool, Fault> {
    let Value::Object(id) = value else {
        return Ok(false);
    };
    let target = super::proxy::unwrapped(heap, *id)?;
    let id = &target;
    let intrinsics =
        realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    if *id == intrinsics.array_prototype {
        return Ok(true);
    }
    heap.read(*id, |object| matches!(object, HeapObject::Array(_)))
        .ok_or(Fault::Internal("array test argument was reclaimed"))
}

/// Compute builtinTag before the observable tag read. Only IsArray and
/// IsCallable follow Proxy targets; all other brands belong to the receiver.
pub(super) fn builtin_tag(heap: &Heap, this: &Value) -> Result<&'static str, Fault> {
    if is_array(heap, this)? {
        return Ok("Array");
    }
    match this {
        Value::Undefined => Ok("Undefined"),
        Value::Null => Ok("Null"),
        Value::Bool(_) => Ok("Boolean"),
        Value::Number(_) => Ok("Number"),
        Value::String(_) => Ok("String"),
        Value::Symbol(_) | Value::BigInt(_) => Ok("Object"),
        Value::Object(id) => {
            let target = super::proxy::unwrapped(heap, *id)?;
            // IsCallable needs only the kind, not dispatch's copy of a bound
            // function's potentially large argument list.
            if heap
                .read(target, |object| {
                    matches!(
                        object,
                        HeapObject::Function(_)
                            | HeapObject::Builtin(_)
                            | HeapObject::BoundFunction(_)
                    )
                })
                .ok_or(Fault::Internal("receiver was reclaimed"))?
            {
                return Ok("Function");
            }
            heap.read(*id, |object| match object {
                HeapObject::Plain(o) if o.is_arguments() => "Arguments",
                HeapObject::Plain(o) if o.is_error() => "Error",
                HeapObject::Plain(o) if o.date_value().is_some() => "Date",
                HeapObject::Plain(o) if o.regexp().is_some() => "RegExp",
                HeapObject::Plain(o) => match o.primitive_data() {
                    Some(PrimitiveData::Number(_)) => "Number",
                    Some(PrimitiveData::String(_)) => "String",
                    Some(PrimitiveData::Boolean(_)) => "Boolean",
                    _ => "Object",
                },
                _ => "Object",
            })
            .ok_or(Fault::Internal("receiver was reclaimed"))
        }
    }
}

/// Allocate an empty WeakMap; AddEntriesFromIterable performs user calls.
pub(crate) fn weak_map_constructor(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .weak_map_prototype;
    heap.alloc(HeapObject::WeakMap(Box::new(
        crate::object::weak_map::WeakMapObject::new(heap, task, prototype),
    )))
    .map(Value::Object)
    .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

/// State-dependent leaf call. Every dynamically resolved builtin call uses
/// this entry, including forwarding and callbacks from resumable natives.
/// The running mutator cannot collect between reading a slot and keeping it.
pub(crate) fn call_in_state(
    heap: &Heap,
    state: &mut super::frames::VmState,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    // Primitive constructor callbacks and prototype forwarding can execute on
    // small workers. Keep the large leaf dispatcher's unrelated storage out.
    match which {
        Builtin::Object => return object_constructor(heap, state.diag.task, args),
        Builtin::Boolean
        | Builtin::Number
        | Builtin::String
        | Builtin::Symbol
        | Builtin::BigInt => {
            return primitive_constructor(heap, state.diag.task, which, args, false);
        }
        Builtin::ObjectSetPrototypeOf => {
            return prototype::object_set_prototype_of(heap, state.diag.task, args);
        }
        Builtin::ObjectGetPrototypeOf => {
            return prototype::object_get_prototype_of(heap, state.diag.task, args);
        }
        Builtin::ObjectCreate => return prototype::object_create(heap, state.diag.task, args),
        _ => {}
    }
    if matches!(
        which,
        Builtin::FinalizationRegistryRegister | Builtin::FinalizationRegistryUnregister
    ) {
        return finalization_registry_operation(heap, state, which, this, args);
    }
    if which != Builtin::WeakRefDeref {
        return call(heap, state.diag.task, which, this, args);
    }
    let invalid = || Fault::TypeError("WeakRef.prototype.deref requires a WeakRef receiver".into());
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    let value = heap
        .read(*id, |object| {
            let HeapObject::WeakRef(weak) = object else {
                return Err(invalid());
            };
            if weak.object().owner() != Some(state.diag.task) {
                return Err(Fault::ForeignObject {
                    owner: weak.object().owner(),
                    caller: state.diag.task,
                });
            }
            Ok(weak.deref())
        })
        .unwrap_or(Err(Fault::Internal("WeakRef receiver was reclaimed")))?;
    if let Some(edge) = crate::object::weak_map::weak_key(&value) {
        state
            .kept_objects
            .entry(edge)
            .or_insert_with(|| value.clone());
    }
    Ok(value)
}

pub(crate) fn weak_ref_constructor(
    heap: &Heap,
    state: &mut super::frames::VmState,
    args: &[Value],
) -> Result<Value, Fault> {
    let target = args.first().unwrap_or(&Value::Undefined);
    let edge = crate::object::weak_map::weak_key(target)
        .ok_or_else(|| Fault::TypeError("invalid WeakRef target".into()))?;
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .weak_ref_prototype;
    let id = heap
        .alloc(HeapObject::WeakRef(Box::new(
            crate::object::weak_ref::WeakRefObject::new(
                heap,
                state.diag.task,
                prototype,
                target,
                edge,
            ),
        )))
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    state
        .kept_objects
        .entry(edge)
        .or_insert_with(|| target.clone());
    Ok(Value::Object(id))
}

fn weak_map_operation(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let invalid = || {
        Fault::TypeError(format!(
            "WeakMap.prototype.{} requires a WeakMap receiver",
            which.name()
        ))
    };
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    // Receiver and arguments are rooted by the frame throughout this leaf.
    heap.with_object_mut(*id, |object| {
        let HeapObject::WeakMap(map) = object else {
            return Err(invalid());
        };
        if map.object().owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: map.object().owner(),
                caller: task,
            });
        }
        let key = args.first().and_then(crate::object::weak_map::weak_key);
        Ok(match which {
            Builtin::WeakMapSet => {
                let key = key.ok_or_else(|| Fault::TypeError("invalid WeakMap key".into()))?;
                map.set(key, args.get(1).cloned().unwrap_or(Value::Undefined));
                this.clone()
            }
            Builtin::WeakMapGet => key
                .and_then(|k| map.get(k))
                .cloned()
                .unwrap_or(Value::Undefined),
            Builtin::WeakMapHas => Value::Bool(key.is_some_and(|k| map.get(k).is_some())),
            Builtin::WeakMapDelete => Value::Bool(key.is_some_and(|k| map.delete(k))),
            _ => return Err(Fault::Internal("not a WeakMap operation")),
        })
    })
    .unwrap_or(Err(Fault::Internal("WeakMap receiver was reclaimed")))
}

pub(crate) fn finalization_registry_constructor(
    heap: &Heap,
    state: &super::frames::VmState,
    args: &[Value],
) -> Result<Value, Fault> {
    let callback = args.first().unwrap_or(&Value::Undefined);
    callable(heap, callback)?;
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .finalization_registry_prototype;
    heap.alloc(HeapObject::FinalizationRegistry(Box::new(
        crate::object::finalization_registry::FinalizationRegistryObject::new(
            heap,
            state.diag.task,
            prototype,
            callback.clone(),
            state.jobs.clone(),
        ),
    )))
    .map(Value::Object)
    .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

fn finalization_registry_operation(
    heap: &Heap,
    state: &super::frames::VmState,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let invalid = || {
        Fault::TypeError(
            "FinalizationRegistry method requires a FinalizationRegistry receiver".into(),
        )
    };
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    let inbox = state
        .jobs
        .upgrade()
        .ok_or(Fault::Internal("missing job inbox"))?;
    heap.with_object_mut(*id, |object| {
        let HeapObject::FinalizationRegistry(registry) = object else {
            return Err(invalid());
        };
        if registry.object().owner() != Some(state.diag.task) {
            return Err(Fault::ForeignObject {
                owner: registry.object().owner(),
                caller: state.diag.task,
            });
        }
        let first = args.first().unwrap_or(&Value::Undefined);
        let key = crate::object::weak_map::weak_key(first).ok_or_else(|| {
            Fault::TypeError("invalid finalization target or unregister token".into())
        })?;
        let result = if which == Builtin::FinalizationRegistryRegister {
            let held = args.get(1).unwrap_or(&Value::Undefined);
            if same_value(first, held) {
                return Err(Fault::TypeError("held value cannot be the target".into()));
            }
            let token = match args.get(2) {
                None | Some(Value::Undefined) => None,
                Some(token) => Some(
                    crate::object::weak_map::weak_key(token)
                        .ok_or_else(|| Fault::TypeError("invalid unregister token".into()))?,
                ),
            };
            registry.register(key, held.clone(), token).ok_or_else(|| {
                Fault::RangeError("finalization cell identities exhausted".into())
            })?;
            Value::Undefined
        } else {
            Value::Bool(registry.unregister(key))
        };
        inbox.retain_registry(*id, !registry.is_empty());
        Ok(result)
    })
    .unwrap_or(Err(Fault::Internal(
        "FinalizationRegistry receiver was reclaimed",
    )))
}

fn disposable_stack_method(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    use crate::object::disposable_stack::{self as stack, Hint, Kind, Resource};
    let kind = if matches!(
        which,
        Builtin::AsyncDisposableStackAdopt
            | Builtin::AsyncDisposableStackDefer
            | Builtin::AsyncDisposableStackMove
            | Builtin::AsyncDisposableStackDisposed
    ) {
        Kind::Async
    } else {
        Kind::Sync
    };
    let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
    if matches!(
        which,
        Builtin::DisposableStackDisposed | Builtin::AsyncDisposableStackDisposed
    ) {
        return stack::access(heap, task, this, kind, false, |s| Value::Bool(s.disposed));
    }
    stack::access(heap, task, this, kind, true, |_| ())?;
    if matches!(
        which,
        Builtin::DisposableStackMove | Builtin::AsyncDisposableStackMove
    ) {
        let prototype = kind.prototype(heap)?;
        let mut moved = stack::DisposableStack::new(heap, task, prototype, kind);
        moved.resources = stack::access(heap, task, this, kind, true, |s| {
            s.disposed = true;
            std::mem::take(&mut s.resources)
        })?;
        return heap
            .alloc(HeapObject::DisposableStack(Box::new(moved)))
            .map(Value::Object)
            .ok_or(Fault::OutOfMemory("heap object table exhausted"));
    }
    let (value, method, hint, result) = match which {
        Builtin::DisposableStackAdopt | Builtin::AsyncDisposableStackAdopt => {
            (arg(0), arg(1), Hint::Adopt, arg(0))
        }
        Builtin::DisposableStackDefer | Builtin::AsyncDisposableStackDefer => {
            (Value::Undefined, arg(0), Hint::Defer, Value::Undefined)
        }
        _ => return Err(Fault::Internal("invalid DisposableStack method")),
    };
    stack::callable(heap, &method)?;
    stack::access(heap, task, this, kind, true, |s| {
        s.resources.push(Resource {
            value,
            method,
            hint,
        })
    })?;
    Ok(result)
}

pub(crate) fn set_constructor(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .set_prototype;
    let map = crate::object::map::MapObject::new(heap, task, prototype);
    heap.alloc(HeapObject::JsSet(Box::new(map)))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

fn set_operation(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let invalid = || {
        Fault::TypeError(format!(
            "Set.prototype.{} requires a Set receiver",
            which.name()
        ))
    };
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    // The frame roots receiver and arguments throughout this leaf operation.
    // The object lock is the same task-owned mutation boundary as plain objects.
    heap.with_object_mut(*id, |object| {
        let HeapObject::JsSet(map) = object else {
            return Err(invalid());
        };
        if map.object().owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: map.object().owner(),
                caller: task,
            });
        }
        let key = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(match which {
            Builtin::SetAdd => {
                map.set(key, Value::Undefined)
                    .map_err(|message| Fault::RangeError(message.into()))?;
                this.clone()
            }
            Builtin::SetHas => Value::Bool(map.get(&key).is_some()),
            Builtin::SetDelete => Value::Bool(map.delete(&key)),
            Builtin::SetClear => {
                map.clear();
                Value::Undefined
            }
            Builtin::SetSize => Value::Number(map.size() as f64),
            _ => return Err(Fault::Internal("not a Set operation")),
        })
    })
    .unwrap_or(Err(Fault::Internal("Set receiver was reclaimed")))
}

pub(crate) fn weak_set_constructor(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .weak_set_prototype;
    heap.alloc(HeapObject::WeakSet(Box::new(
        crate::object::weak_map::WeakMapObject::new(heap, task, prototype),
    )))
    .map(Value::Object)
    .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

fn weak_set_operation(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let invalid = || {
        Fault::TypeError(format!(
            "WeakSet.prototype.{} requires a WeakSet receiver",
            which.name()
        ))
    };
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    // Receiver and arguments are rooted by the frame throughout this leaf.
    heap.with_object_mut(*id, |object| {
        let HeapObject::WeakSet(map) = object else {
            return Err(invalid());
        };
        if map.object().owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: map.object().owner(),
                caller: task,
            });
        }
        let key = args.first().and_then(crate::object::weak_map::weak_key);
        Ok(match which {
            Builtin::WeakSetAdd => {
                let key = key.ok_or_else(|| Fault::TypeError("invalid WeakSet key".into()))?;
                map.set(key, Value::Undefined);
                this.clone()
            }
            Builtin::WeakSetHas => Value::Bool(key.is_some_and(|k| map.get(k).is_some())),
            Builtin::WeakSetDelete => Value::Bool(key.is_some_and(|k| map.delete(k))),
            _ => return Err(Fault::Internal("not a WeakSet operation")),
        })
    })
    .unwrap_or(Err(Fault::Internal("WeakSet receiver was reclaimed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(heap: &Heap, this: &Value) -> String {
        format!(
            "[object {}]",
            builtin_tag(heap, this).unwrap_or_else(|_| panic!("invalid receiver"))
        )
    }

    /// Internal builtinTag classification; source fixtures cover dispatch.
    #[test]
    fn to_string_tags_of_other_receivers() {
        let heap = Heap::default();
        let object = Value::Object(realm::intrinsics(&heap).unwrap().object);
        assert_eq!(tag(&heap, &object), "[object Function]");
        let vector = crate::collections::new_vector(&heap, &[]).unwrap();
        assert_eq!(tag(&heap, &vector), "[object Object]");
        for (primitive, expected) in [
            (Value::Number(1.0), "Number"),
            (Value::String(JsString::from("s")), "String"),
        ] {
            assert_eq!(tag(&heap, &primitive), format!("[object {expected}]"));
        }
        assert_eq!(tag(&heap, &Value::Null), "[object Null]");
        assert_eq!(tag(&heap, &Value::Bool(true)), "[object Boolean]");
    }

    #[test]
    fn wrapper_internal_data_checks_the_owner() {
        let heap = Heap::default();
        let owner = TaskId::from_raw(1);
        let caller = TaskId::from_raw(2);
        for (constructor, method, argument) in [
            (
                Builtin::Boolean,
                Builtin::BooleanPrototypeValueOf,
                Value::Bool(true),
            ),
            (
                Builtin::Number,
                Builtin::NumberPrototypeValueOf,
                Value::Number(7.0),
            ),
            (
                Builtin::String,
                Builtin::StringPrototypeToString,
                Value::String(JsString::from("abc")),
            ),
        ] {
            let wrapper = primitive_constructor(&heap, owner, constructor, &[argument], true)
                .unwrap_or_else(|_| panic!("construction failed"));
            assert!(matches!(
                call(&heap, caller, method, &wrapper, &[]),
                Err(Fault::ForeignObject { owner: Some(id), caller: task }) if id == owner && task == caller
            ));
            assert!(call(&heap, owner, method, &wrapper, &[]).is_ok());
        }
        let wrapper = array_string_receiver(&heap, owner, &Value::BigInt(7.into()))
            .unwrap_or_else(|_| panic!("boxing failed"));
        assert!(matches!(
            this_bigint_value(&heap, caller, &wrapper),
            Err(Fault::ForeignObject { owner: Some(id), caller: task }) if id == owner && task == caller
        ));
        assert_eq!(
            this_bigint_value(&heap, owner, &wrapper).unwrap_or_else(|_| panic!("unboxing failed")),
            7.into()
        );
    }
}
