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
//! | `Object(v)` | `undefined`/`null`/absent: a new ordinary object owned by the running task, with prototype `Object.prototype`; an object: `v` itself; a boolean, number or string: `UnsupportedOperand` (wrapper objects are clj-quickjs-72) |
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
//! | `String.prototype.substring`, `.slice`, `.concat`, `.padStart`, `.padEnd` (clj-quickjs-145) | generic ToString receivers and resumable argument coercion; UTF-16 extraction/combination, with at most 256 output units copied per native step; length above `2^30 - 1`: catchable `RangeError` ("string too long") |
//! | `Number(...)`, `String(...)`, `new Number(...)`, `new String(...)` (clj-quickjs-72) | convert the argument (absent: 0 / ""); `new` allocates a task-owned ordinary object with [[NumberData]] / [[StringData]]; object arguments use resumable ToPrimitive with number / string hint |
//! | global `isNaN`, `isFinite`; `Number.isNaN`, `.isFinite`, `.isInteger`, `.isSafeInteger` (clj-quickjs-147) | globals apply ToNumber (objects through a resumable native conversion); Number methods return false for non-numbers without coercion; all ignore `this` and use only the first argument (absent: undefined) |

//! | `parseInt`, `parseFloat` (clj-quickjs-148) | resumable ToString and, for parseInt, ToInt32(radix); bounded prefix scanning in native::NumberParse; `Number` exposes the same functions |
//! | `Function.prototype(...)` (clj-quickjs-102) | `undefined`, whatever the receiver and arguments |
//! | `Function(...)` (clj-quickjs-102; `Function.prototype.constructor`) | `UnsupportedOperand` until the Function constructor exists (clj-quickjs-111) |
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
    /// `Function` (clj-quickjs-102): the global name and
    /// `Function.prototype.constructor`; fails when called until the
    /// Function constructor exists (clj-quickjs-111).
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
}

impl NativeErrorKind {
    pub(crate) const ALL: [Self; 7] = [
        Self::Type,
        Self::Range,
        Self::Reference,
        Self::Syntax,
        Self::Eval,
        Self::Uri,
        Self::Internal,
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
        }
    }
}

impl Builtin {
    /// The function's `name` property.
    pub(crate) fn name(self) -> &'static str {
        match self {
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
            Builtin::BigInt => "BigInt",
            Builtin::BigIntAsIntN => "asIntN",
            Builtin::BigIntAsUintN => "asUintN",

            Builtin::TypedArraySet => "set",
            Builtin::TypedArrayFill => "fill",
            Builtin::TypedArrayJoin => "join",

            Builtin::BigIntPrototypeToString => "toString",
            Builtin::BigIntPrototypeValueOf => "valueOf",

            Builtin::IntlPluralRules => "PluralRules",
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
            Builtin::GeneratorNext => "next",
            Builtin::PromiseCatch => "catch",
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
            Builtin::StringPrototypeSplit => "split",
            Builtin::Object => "Object",
            Builtin::ObjectIs => "is",
            Builtin::ObjectPrototypeToString => "toString",
            Builtin::ObjectPrototypeValueOf => "valueOf",
            Builtin::ArrayPrototypeToString => "toString",
            Builtin::Array => "Array",
            Builtin::ObjectDefineProperty => "defineProperty",
            Builtin::ObjectGetOwnPropertyDescriptor => "getOwnPropertyDescriptor",
            Builtin::ObjectGetPrototypeOf => "getPrototypeOf",
            Builtin::ObjectSetPrototypeOf => "setPrototypeOf",
            Builtin::ObjectCreate => "create",
            Builtin::ObjectPreventExtensions => "preventExtensions",
            Builtin::ObjectIsExtensible => "isExtensible",
            Builtin::ObjectFreeze => "freeze",
            Builtin::ObjectIsFrozen => "isFrozen",
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
            Builtin::Map => "Map",
            Builtin::WeakMap => "WeakMap",
            Builtin::SimGc => "gc",
            Builtin::SimSetTimeout => "setTimeout",
            Builtin::SimClearTimeout => "clearTimeout",
            Builtin::WeakMapSet => "set",
            Builtin::WeakMapGet => "get",
            Builtin::WeakMapHas => "has",
            Builtin::WeakMapDelete => "delete",
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
            Builtin::IntlCollator
            | Builtin::IntlCollatorGetCompare
            | Builtin::IntlCollatorResolvedOptions => 0,
            Builtin::IntlCollatorBound => 2,
            Builtin::IntlCollatorSupportedLocales => 1,
            Builtin::IntlNumberFormat
            | Builtin::IntlNumberFormatBound
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
            | Builtin::TypedArrayLength
            | Builtin::TypedArrayByteLength
            | Builtin::TypedArrayByteOffset => 0,

            Builtin::IntlLocaleToString
            | Builtin::IntlLocaleGet(_)
            | Builtin::IntlLocaleMaximize
            | Builtin::IntlLocaleMinimize => 0,
            Builtin::RegExp => 2,
            Builtin::StringPrototypeReplace | Builtin::StringPrototypeReplaceAll => 2,
            Builtin::RegExpExec | Builtin::RegExpTest => 1,
            Builtin::RegExpToString
            | Builtin::RegExpSource
            | Builtin::RegExpFlags
            | Builtin::RegExpFlag(_) => 0,

            Builtin::PromiseThen => 2,
            Builtin::GeneratorNext => 1,
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
            | Builtin::ObjectIsFrozen => 1,
            Builtin::ObjectKeys | Builtin::ObjectGetOwnPropertyNames => 1,
            Builtin::Number
            | Builtin::String
            | Builtin::NumberPrototypeToString
            | Builtin::NumberPrototypeToExponential
            | Builtin::NumberPrototypeToPrecision
            | Builtin::NumberPrototypeToFixed => 1,
            Builtin::NumberPrototypeValueOf
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
            Builtin::Map
            | Builtin::MapGet
            | Builtin::MapHas
            | Builtin::MapDelete
            | Builtin::MapForEach => 1,
            Builtin::MapSet | Builtin::WeakMapSet => 2,
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
        Builtin::SimGc | Builtin::SimSetTimeout | Builtin::SimClearTimeout => Err(Fault::Internal(
            "simulated host binding requires task state",
        )),
        Builtin::ArrayPrototypeSort => Err(Fault::Internal("sort requires a resumable frame")),
        Builtin::ArrayFrom => Err(Fault::Internal("Array.from requires a resumable frame")),
        Builtin::MathSumPrecise => Err(Fault::Internal("sumPrecise requires a resumable frame")),
        Builtin::ArrayValues => {
            super::native::iterator::create(heap, task, this, super::native::iterator::Kind::Array)
        }
        Builtin::TypedArrayValues => super::native::iterator::create(
            heap,
            task,
            this,
            super::native::iterator::Kind::TypedArray,
        ),
        Builtin::IteratorIdentity => Ok(this.clone()),
        Builtin::StringIterator | Builtin::IteratorNext(_) | Builtin::MapForEach => Err(
            Fault::Internal("iterator callback requires a resumable frame"),
        ),
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
        Builtin::IntlPluralRules => Err(Fault::TypeError("Intl.PluralRules requires new".into())),
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
        Builtin::RegExp
        | Builtin::StringPrototypeReplace
        | Builtin::StringPrototypeReplaceAll
        | Builtin::RegExpExec
        | Builtin::RegExpTest
        | Builtin::RegExpToString
        | Builtin::RegExpFlags => Err(Fault::Internal("RegExp requires a resumable frame")),
        Builtin::RegExpSource | Builtin::RegExpFlag(_) => {
            crate::vm::native::regexp::getter(heap, task, which, this)
        }

        Builtin::GeneratorNext => Err(Fault::Internal("generator next requires VM state")),
        Builtin::Promise
        | Builtin::PromiseResolve
        | Builtin::PromiseReject
        | Builtin::PromiseThen
        | Builtin::PromiseCatch
        | Builtin::PromiseResolver(_, _)
        | Builtin::PromiseReaction(_)
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
        Builtin::WeakMap => Err(Fault::TypeError("WeakMap requires new".to_string())),
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
        Builtin::Object => match arg(0) {
            Value::Undefined | Value::Null => new_object(heap, task),
            v @ Value::Object(_) => Ok(v),
            v => Err(Fault::Unsupported(format!(
                "Object() of a {} needs a wrapper object, which is not supported",
                v.type_name()
            ))),
        },
        Builtin::ObjectIs => Ok(Value::Bool(same_value(&arg(0), &arg(1)))),
        Builtin::MathFloor
        | Builtin::MathCeil
        | Builtin::MathAbs
        | Builtin::MathImul
        | Builtin::MathPow
        | Builtin::MathFround
        | Builtin::MathSqrt
        | Builtin::MathLog2 => math(which, args),
        Builtin::MathHypot => Err(Fault::Internal("hypot requires a budgeted native frame")),
        Builtin::ObjectPrototypeToString => {
            let tag = to_string_tag(heap, task, this)?;
            Ok(Value::String(JsString::from(
                format!("[object {tag}]").as_str(),
            )))
        }
        Builtin::ObjectPrototypeValueOf => match this {
            Value::Undefined | Value::Null => Err(Fault::TypeError(format!(
                "cannot convert {} to object",
                this.type_name()
            ))),
            Value::Object(_) => Ok(this.clone()),
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
        Builtin::ObjectDefineProperty => Err(Fault::Internal(
            "defineProperty must run through the native trampoline",
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
        Builtin::ObjectKeys => keys::object_keys(heap, task, args, KeyFilter::Enumerable),
        Builtin::ObjectGetOwnPropertyNames => keys::object_keys(heap, task, args, KeyFilter::All),
        Builtin::FunctionPrototype => Ok(Value::Undefined),
        Builtin::FunctionPrototypeBind => Err(Fault::Internal(
            "bind must run through the native trampoline",
        )),
        Builtin::Function => Err(Fault::Unsupported(
            "the Function constructor is not supported".to_string(),
        )),
        // Every call site resolves these with `forward` first.
        Builtin::FunctionPrototypeCall
        | Builtin::FunctionPrototypeApply
        | Builtin::ReflectApply => {
            Err(Fault::Internal("a forwarding built-in was called directly"))
        }
        Builtin::Number | Builtin::String | Builtin::Symbol | Builtin::BigInt => {
            primitive_constructor(heap, task, which, args, false)
        }
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
        Builtin::BigIntPrototypeToString => Err(Fault::Internal(
            "BigInt formatting requires a resumable frame",
        )),
        Builtin::BigIntAsIntN | Builtin::BigIntAsUintN => Err(Fault::Internal(
            "BigInt wrapping requires a resumable frame",
        )),
        Builtin::BigIntPrototypeValueOf => match this {
            Value::BigInt(_) => Ok(this.clone()),
            _ => Err(Fault::TypeError("not a BigInt".into())),
        },
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
        Builtin::StringFromCharCode | Builtin::StringFromCodePoint => {
            string_from_codes(which, args)
        }
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

/// Leaf construction: O(args + total primitive-string conversion work),
/// at most two output units per argument. The stack limit bounds args but
/// does not bound aggregate string scans; resumable coercion is clj-quickjs-265.
/// No JavaScript conversion is attempted on this Rust stack.
fn string_from_codes(which: Builtin, args: &[Value]) -> Result<Value, Fault> {
    let mut units = Vec::new();
    for arg in args {
        let n = crate::vm::dispatch::to_number(arg)?;
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
            validate_code_point(n)?;
            let point = n as u32;
            if point <= 0xffff {
                units.push(point as u16);
            } else {
                let offset = point - 0x10000;
                units.push(0xd800 | (offset >> 10) as u16);
                units.push(0xdc00 | (offset & 0x3ff) as u16);
            }
        }
        if units.len() > crate::value::string::MAX_STRING_LEN {
            return Err(Fault::OutOfMemory("string too long"));
        }
    }
    JsString::from_code_units(units)
        .map(Value::String)
        .ok_or(Fault::OutOfMemory("string too long"))
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
                        return Err(Fault::RangeError("string too long".to_string()));
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
    (if which == StringPrototypeConcat {
        crate::value::string::StringBuild::concatenation(parts)
    } else {
        crate::value::string::StringBuild::new(parts)
    })
    .ok_or_else(|| Fault::RangeError("string too long".to_string()))
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
            Some(Value::Symbol(s)) if !construct => {
                let prefix = JsString::from("Symbol(");
                prefix
                    .concat(s.description().unwrap_or(&JsString::from("")))
                    .and_then(|s| s.concat(&JsString::from(")")))
                    .ok_or(Fault::OutOfMemory("string too long"))?
            }
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
        _ => return Err(Fault::Internal("not a wrapper primitive")),
    };
    let object = OrdinaryObject::with_prototype(task, prototype).with_primitive_data(data);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// ToObject for the generic array string methods. Boolean wrappers remain
/// unsupported until Boolean.prototype exists; never give them a wrong tag.
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
        Value::Bool(_) => Err(Fault::Unsupported(
            "Boolean wrapper objects are not supported".into(),
        )),
        Value::Symbol(_) => Err(Fault::Unsupported(
            "Symbol wrapper objects are not supported".into(),
        )),
        Value::BigInt(_) => Err(Fault::Unsupported(
            "BigInt wrapper objects are not supported".into(),
        )),
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

/// A new ordinary object owned by `task` whose prototype is
/// `Object.prototype` (OrdinaryObjectCreate(%Object.prototype%)).
pub(crate) fn new_object(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let prototype = realm::intrinsics(heap)
        .ok_or_else(exhausted)?
        .object_prototype;
    let object = OrdinaryObject::with_prototype(task, prototype);
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
        ArrayObject::with_length(task, prototype, length)
    } else {
        let mut array = ArrayObject::with_prototype(task, prototype);
        for value in args {
            if !array.push(Some(value.clone())) {
                return Err(Fault::RangeError("invalid array length".to_string()));
            }
        }
        array
    };
    heap.alloc(HeapObject::Array(Box::new(array)))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// Nullish arguments create an empty Map. Iterable construction needs a
/// resumable iterator frame and is deliberately rejected until supported.
pub(crate) fn map_constructor(heap: &Heap, task: TaskId, args: &[Value]) -> Result<Value, Fault> {
    if !matches!(args.first(), None | Some(Value::Undefined | Value::Null)) {
        return Err(Fault::TypeError(
            "Map iterable construction is not supported".to_string(),
        ));
    }
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .map_prototype;
    let map = crate::object::map::MapObject::new(task, prototype);
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
                map.set(key, args.get(1).cloned().unwrap_or(Value::Undefined));
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
fn is_array(heap: &Heap, value: &Value) -> Result<bool, Fault> {
    let Value::Object(id) = value else {
        return Ok(false);
    };
    let intrinsics =
        realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    if *id == intrinsics.array_prototype {
        return Ok(true);
    }
    heap.read(*id, |object| matches!(object, HeapObject::Array(_)))
        .ok_or(Fault::Internal("array test argument was reclaimed"))
}

/// The builtinTag of `Object.prototype.toString`. Reject an own or
/// inherited `@@toStringTag` until resumable tag consumption is supported.
fn to_string_tag(heap: &Heap, task: TaskId, this: &Value) -> Result<&'static str, Fault> {
    // The shared Intl namespace has a fixed data descriptor and cannot be
    // mutated. Its own tag needs no getter, coercion, or resumable JS call.
    if let Value::Object(id) = this
        && *id
            == realm::intrinsics(heap)
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
                .intl
    {
        return Ok("Intl");
    }
    // HasProperty walks outside object-table locks and never invokes a
    // getter. The receiver and its prototype chain stay rooted by the frame.
    if matches!(this, Value::Object(_))
        && crate::vm::property::has_property(
            heap,
            task,
            this,
            &crate::vm::property::Key::Symbol(crate::value::symbol::well_known("toStringTag")),
        )?
    {
        return Err(Fault::Unsupported(
            "Object.prototype.toString: Symbol.toStringTag lookup is not supported".to_string(),
        ));
    }
    if is_array(heap, this)? {
        return Ok("Array");
    }
    let unsupported = |what: &str| {
        Err(Fault::Unsupported(format!(
            "Object.prototype.toString of {what} is not supported"
        )))
    };
    match this {
        Value::Undefined => Ok("Undefined"),
        Value::Null => Ok("Null"),
        Value::Bool(_) => unsupported("a boolean"),
        Value::Number(_) => unsupported("a number"),
        Value::String(_) => unsupported("a string"),
        Value::Symbol(_) => unsupported("a symbol"),
        Value::BigInt(_) => unsupported("a BigInt"),
        Value::Object(id) => heap
            .read(*id, |object| match object {
                HeapObject::Function(_) | HeapObject::BoundFunction(_) | HeapObject::Builtin(_) => {
                    Ok("Function")
                }
                HeapObject::Array(_) => Ok("Array"),
                HeapObject::Native(_) | HeapObject::JsMap(_) | HeapObject::WeakMap(_) => {
                    Ok("Object")
                }
                HeapObject::ArrayBuffer(_) => Ok("ArrayBuffer"),
                HeapObject::TypedArray(a) => Ok(a.view.kind.name()),
                HeapObject::Plain(o) if o.is_error() => Ok("Error"),
                HeapObject::Plain(o) if o.date_value().is_some() => Ok("Date"),
                HeapObject::Plain(o) if o.regexp().is_some() => Ok("RegExp"),
                HeapObject::Plain(o) => Ok(match o.primitive_data() {
                    Some(PrimitiveData::Number(_)) => "Number",
                    Some(PrimitiveData::String(_)) => "String",
                    None => "Object",
                }),
                HeapObject::Vector(_)
                | HeapObject::Map(_)
                | HeapObject::Occurrence(_)
                | HeapObject::Builder(_)
                | HeapObject::Cell(_)
                | HeapObject::Atom(_)
                | HeapObject::PromiseReaction(_)
                | HeapObject::PromiseResolving(_) => Ok("Object"),
                HeapObject::Promise(_) => Ok("Promise"),
                HeapObject::Generator(_) | HeapObject::AsyncState(_) => Ok("Object"),
            })
            // The receiver is an operand of the running frame.
            .unwrap_or(Err(Fault::Internal("receiver was reclaimed"))),
    }
}

/// Same explicit iterable gap as Map: no iterator callbacks in a leaf native.
pub(crate) fn weak_map_constructor(
    heap: &Heap,
    task: TaskId,
    args: &[Value],
) -> Result<Value, Fault> {
    if !matches!(args.first(), None | Some(Value::Undefined | Value::Null)) {
        return Err(Fault::TypeError(
            "WeakMap iterable construction is not supported".into(),
        ));
    }
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .weak_map_prototype;
    heap.alloc(HeapObject::WeakMap(Box::new(
        crate::object::weak_map::WeakMapObject::new(task, prototype),
    )))
    .map(Value::Object)
    .ok_or(Fault::OutOfMemory("heap object table exhausted"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(heap: &Heap, this: &Value) -> String {
        match call(
            heap,
            TaskId::from_raw(1),
            Builtin::ObjectPrototypeToString,
            this,
            &[],
        ) {
            Ok(Value::String(s)) => s.to_string(),
            Ok(other) => panic!("not a string: {other:?}"),
            Err(_) => "unsupported".to_string(),
        }
    }

    /// The receivers source code cannot pass yet (`Function.prototype.call`
    /// is clj-quickjs-102).
    #[test]
    fn to_string_tags_of_other_receivers() {
        let heap = Heap::default();
        let object = Value::Object(realm::intrinsics(&heap).unwrap().object);
        assert_eq!(tag(&heap, &object), "[object Function]");
        let vector = crate::collections::new_vector(&heap, &[]).unwrap();
        assert_eq!(tag(&heap, &vector), "[object Object]");
        for primitive in [
            Value::Bool(true),
            Value::Number(1.0),
            Value::String(JsString::from("s")),
        ] {
            assert_eq!(tag(&heap, &primitive), "unsupported");
        }
        assert_eq!(tag(&heap, &Value::Null), "[object Null]");
    }

    #[test]
    fn wrapper_internal_data_checks_the_owner() {
        let heap = Heap::default();
        let owner = TaskId::from_raw(1);
        let caller = TaskId::from_raw(2);
        for (constructor, method, argument) in [
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
    }
}
