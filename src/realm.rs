//! The realm: the runtime's intrinsic objects and built-in global names
//! (clj-quickjs-117).
//!
//! The decision record (shared immutable intrinsics, the alternatives
//! considered and their consequences) is `docs/architecture.md`, "Realm
//! intrinsics".
//!
//! # One shared realm per runtime
//!
//! Each heap (one per [`Runtime`](crate::Runtime)) has at most one realm,
//! built the first time an instruction needs it (`NewObjectLiteral`, for
//! the prototype of a literal, `GetBuiltinGlobal`, a write of `__proto__`,
//! clj-quickjs-121, or a property read on a number or string, clj-quickjs-73) and kept until the heap
//! is dropped. Every task of the runtime sees the same intrinsic objects,
//! with the same identities (`({}).toString === Object.prototype.toString`
//! in every task, and a closure that captured `Object` in one task calls the
//! same function in another).
//!
//! The intrinsics are:
//!
//! | Object | Kind | Own properties |
//! |--------|------|----------------|
//! | `Object.prototype` | ordinary object (`HeapObject::Plain`), null prototype | `constructor` (`Object`), `toString`, `valueOf`, `__proto__` (stored non-enumerable, configurable accessor, clj-quickjs-261) |
//! | `get __proto__`, `set __proto__` | built-in accessor functions | `length` (0, 1), `name`; inherit from `Function.prototype` |
//! | `Object` | built-in function ([`Builtin::Object`]) | `length` (1), `name`, `prototype` (`Object.prototype`), `is`, `defineProperty`, `getOwnPropertyDescriptor` (clj-quickjs-119), `getPrototypeOf`, `setPrototypeOf`, `create` (clj-quickjs-121), `preventExtensions`, `isExtensible`, `freeze`, `isFrozen` (clj-quickjs-122), `keys`, `getOwnPropertyNames` (clj-quickjs-123) |
//! | `Object.is` | built-in function | `length` (2), `name` |
//! | `Object.defineProperty` | built-in function | `length` (3), `name` |
//! | `Object.getOwnPropertyDescriptor` | built-in function | `length` (2), `name` |
//! | `Object.getPrototypeOf` | built-in function | `length` (1), `name` |
//! | `Object.setPrototypeOf` | built-in function | `length` (2), `name` |
//! | `Object.create` | built-in function | `length` (2), `name` |
//! | `Object.preventExtensions`, `Object.isExtensible`, `Object.freeze`, `Object.isFrozen` (clj-quickjs-122) | built-in functions | `length` (1), `name` |
//! | `Object.keys` | built-in function | `length` (1), `name` |
//! | `Object.getOwnPropertyNames` | built-in function | `length` (1), `name` |
//! | `Object.prototype.toString` | built-in function | `length` (0), `name` |
//! | `Object.prototype.valueOf` | built-in function | `length` (0), `name` |
//! | `Array.prototype` (clj-quickjs-118) | ordinary object, prototype `Object.prototype` | `length` (0), `constructor` (`Array`), `toString`, `push` (clj-quickjs-155), `join` (clj-quickjs-156) |
//! | `Array.prototype.join` | built-in function ([`Builtin::ArrayPrototypeJoin`]) | `length` (1), `name` |
//! | `Array.prototype.toString` | built-in function ([`Builtin::ArrayPrototypeToString`]) | `length` (0), `name` |
//! | `Array.prototype.push` | built-in function ([`Builtin::ArrayPrototypePush`]) | `length` (1), `name` |
//! | `Array` (callable and constructible, clj-quickjs-153) | built-in function ([`Builtin::Array`]) | `length` (1), `name`, `prototype` (`Array.prototype`), `isArray` |
//! | `Array.isArray` | built-in function ([`Builtin::ArrayIsArray`]) | `length` (1), `name` |
//! | `ArrayBuffer` (clj-quickjs-165) | constructible built-in function | `length` (1), `name`, `prototype`; calling without `new` throws |
//! | `ArrayBuffer.prototype` | ordinary intrinsic | `constructor`, `byteLength` and `detached` getters, `transfer`, `slice` |
//! | `Number.prototype`, `String.prototype` (clj-quickjs-73) | ordinary objects, prototype `Object.prototype`; where a property read on a number or string primitive starts ([`Intrinsics::primitive_prototype`]) | `constructor` (`Number` / `String`), `toString`, `valueOf`; `String.prototype` also `length` (0, not writable, enumerable or configurable) |
//! | `Number.prototype.toString`, `.valueOf`; `String.prototype.toString`, `.valueOf` | built-in functions | `length` (1 for `Number.prototype.toString`, else 0), `name` |
//! | `Number`, `String` (callable and constructible, clj-quickjs-72) | built-in functions ([`Builtin::Number`], [`Builtin::String`]) | `length` (1), `name`, `prototype` |
//! | `Symbol.prototype` (clj-quickjs-129) | ordinary intrinsic without primitive data; symbol reads start here | `constructor`, `toString`, `valueOf`, `description` getter, non-writable `[Symbol.toPrimitive]` and `[Symbol.toStringTag]` |
//! | String code-unit methods (clj-quickjs-143) | built-in functions | `String.prototype` has `charAt`, `charCodeAt`, `codePointAt`; `String` has `fromCharCode`, `fromCodePoint`; all have `length` (1), `name` |
//! | String extraction/combination (clj-quickjs-145) | built-in functions | `String.prototype` has `substring`, `slice` (length 2), `concat`, `padStart`, `padEnd` (length 1); each has `name` |
//! | `parseInt`, `parseFloat` (clj-quickjs-148) | built-in functions, also on `Number` with the same identities | `length` (2, 1), `name` |
//! | `Function.prototype` (clj-quickjs-102/-103) | built-in function ([`Builtin::FunctionPrototype`]), prototype `Object.prototype` | `length` (0), `name` (`""`), `constructor` (`Function`), `call`, `apply`, `bind`; `toString`, `arguments` and `caller` are missing and their lookup fails (`crate::vm::function::lacks_inherited`) |
//! | `Function.prototype.call`, `Function.prototype.apply` | built-in functions | `length` (1, 2), `name` |
//! | `Function` (fails when called, clj-quickjs-111) | built-in function ([`Builtin::Function`]) | `length` (1), `name`, `prototype` (`Function.prototype`) |
//! | `Reflect` (clj-quickjs-102) | ordinary object, prototype `Object.prototype` | `apply` |
//! | `Reflect.apply` | built-in function | `length` (3), `name` |
//! | Number predicates and constants (clj-quickjs-147) | built-in functions and primitive values | `Number` has `isNaN`, `isFinite`, `isInteger`, `isSafeInteger` (each function has `length` 1 and `name`), plus `EPSILON`, `MIN_VALUE`, `MAX_VALUE`, `MAX_SAFE_INTEGER`, `MIN_SAFE_INTEGER`, `NaN`, `POSITIVE_INFINITY`, `NEGATIVE_INFINITY` (no attributes set); globals `isNaN` and `isFinite` each have `length` 1 and `name` |
//! | `Math` (clj-quickjs-152, clj-quickjs-164) | ordinary object, prototype `Object.prototype` | `floor`, `ceil`, `abs`, `fround` (length 1), `imul`, `hypot`, `pow` (length 2); each function has `name` |
//! | `Math` additions (clj-quickjs-205) | same namespace | `sqrt`, `log2` (length 1), each with `name` |
//! | `Math` additions (clj-quickjs-158) | same namespace | `sumPrecise` (length 1), exact resumable iterable summation |
//! | `Error`, `Error.prototype` (clj-quickjs-228) | callable/constructible built-in; ordinary prototype inheriting from `Object.prototype` | constructor length 1; prototype `constructor`, `name` ("Error"), `message` (""), `toString` (length 0) |
//! | NativeError constructors and prototypes (clj-quickjs-229) | TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError, InternalError; constructors inherit from Error, prototypes from Error.prototype | constructor length 1 and respective name; prototype owns constructor, name and empty message, inherits toString |
//! | `JSON` (clj-quickjs-159/-160) | ordinary object, prototype `Object.prototype` | `parse` (length 2), `stringify` (length 3), resumable conversion/callbacks and budgeted JSON processing |
//! | `Intl` (clj-quickjs-209) | ordinary object, prototype `Object.prototype` | `Symbol.toStringTag` (`Intl`, non-writable, non-enumerable, configurable); service constructors deferred |

//! | `Map`, `Map.prototype` (clj-quickjs-169, -170) | constructible built-in; ordinary prototype inheriting from `Object.prototype` | constructor length 1; prototype `constructor`, `set` (length 2), `get`, `has`, `delete`, resumable `forEach` (length 1), `clear` (length 0), `size` getter (length 0) |
//! | `Date`, `Date.prototype` (clj-quickjs-179) | constructible built-in; unbranded ordinary prototype | constructor length 7, `UTC` (length 2); prototype `constructor`, `getTime`, `valueOf` (length 0), `setTime` (length 1) |
//! | `Date.prototype.toISOString` (clj-quickjs-180) | leaf built-in | length 0; method flags |
//! | `Date.prototype` UTC fields (clj-quickjs-181) | eight leaf getters, seven resumable setters | getter lengths 0; setter lengths 3, 2, 1, 4, 3, 2, 1; method flags |
//! | `Date.prototype` string formats (clj-quickjs-183) | three local methods, UTC method and identical `toGMTString` alias | lengths 0; method flags; `Date()` formats host time |
//!
//! NativeError constructors inherit from Error. Other built-in functions inherit from `Function.prototype`, which
//! inherits from `Object.prototype` (clj-quickjs-102).
//!
//! Property attributes are the standard ones (ECMAScript §18): methods and
//! `constructor` are writable, non-enumerable and configurable; `length` and
//! `name` are non-writable, non-enumerable and configurable; a constructor's
//! `prototype` has no attribute set.
//!
//! # Shared and immutable
//!
//! An intrinsic object has no owner task and is marked intrinsic
//! ([`OrdinaryObject::intrinsic`]). Every task reads it without taking any
//! lock (see "Lock-free reads") and no task changes it: a write to any of its properties
//! is a catchable `TypeError` (`crate::vm::property`), whatever the
//! property's `writable` attribute says. Immutability comes from sharing,
//! not from the descriptors, so an ordinary object that inherits
//! `toString` can still be given its own (`o.toString = f` adds an own
//! property, because the inherited property is writable).
//!
//! The realm is built completely, then installed: the objects are allocated
//! and their properties defined while the building thread is a registered
//! heap mutator between safepoints (no collection can run), then every
//! object is marked known-shareable and the table is published through a
//! [`OnceLock`]. Afterwards it never changes, so the intrinsics are
//! [`Shareability::Persistent`](crate::value::share::Shareability) objects
//! whose edges never change, and reading the table is one atomic load.
//! Concurrent first uses on two workers serialize on a build lock that is
//! taken only while the realm does not exist yet; it is a leaf lock held
//! across allocations (which take the object-table lock) and never taken
//! by the collector.
//!
//! # Lock-free reads
//!
//! When the realm is complete, before it is installed, `build` copies every
//! intrinsic's own properties and prototype into an immutable
//! [`IntrinsicView`] stored with the [`Intrinsics`]. Property reads
//! (`crate::vm::property`) that reach an intrinsic, as a base or along a
//! prototype chain, use the view instead of the object table: after the
//! one atomic load of the installed realm and a scan of the few intrinsic
//! identities, they take no lock at all, so tasks on every worker read
//! `Object.prototype.toString` without contending. The views are exact
//! because nothing changes an intrinsic after installation, and they hold
//! only intrinsics and primitives, so they need no root report of their own.
//!
//! # Roots
//!
//! Every intrinsic object is a collector root of category
//! [`RootCategory::Intrinsic`](crate::heap::gc::RootCategory) from the
//! moment the realm is installed until the heap is dropped, so intrinsics
//! survive collections with no task running and are never reclaimed while
//! the runtime lives.
//!
//! A separate template-site registry roots immutable arrays under the same
//! category. Its short metadata lock covers lookup and first installation;
//! no JavaScript or safepoint runs under it. GC releases that lock before
//! taking the object table, matching template creation's lock order. Entries
//! retain immutable code identity and arrays until realm teardown.
//!
//! # Built-in global names
//!
//! [`Global`] lists the names a free identifier resolves to after every
//! lexical scope and before the `ReferenceError` (`crate::frontend::lower`):
//! `Object`, `Number` and `String` (clj-quickjs-73), `Function` and `Reflect`
//! (clj-quickjs-102), `isNaN`, `isFinite`, `parseInt`, `parseFloat`, `Math`, `Array` (clj-quickjs-155), `Error` (clj-quickjs-228), and the seven NativeError constructors (clj-quickjs-229). They initialize writable
//! properties of each task's mutable global object (clj-quickjs-104,
//! [`new_global_object`]). Source references always look up those properties;
//! `get_builtin_global` remains an instruction for hand-assembled code.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::object::{
    DefineOutcome, OrdinaryObject, PrimitiveData, Property, PropertyDescriptor, PropertyFlags,
    PropertyKey,
};
use crate::value::{JsString, Value};
use crate::vm::builtin::{Builtin, BuiltinFunction, NativeErrorKind};
use crate::vm::executable::Code;

type TemplateSites = std::collections::HashMap<(usize, usize), (Arc<Code>, ObjectId)>;

/// A built-in global name (the `get_builtin_global` operand). Append-only:
/// the operand values are part of the instruction format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum Global {
    /// `Object`, the Object constructor.
    Object = 0,
    /// `Number`, the Number constructor (clj-quickjs-73: it carries
    /// `Number.prototype`, callable and constructible).
    Number = 1,
    /// `String`, the String constructor (clj-quickjs-73: it carries
    /// `String.prototype`, callable and constructible).
    String = 2,
    /// `Function` (clj-quickjs-102): a stub constructor that fails when
    /// called (clj-quickjs-111), reachable for `Function.prototype`.
    Function = 3,
    /// `Reflect` (clj-quickjs-102): an ordinary object with `apply`.
    Reflect = 4,
    /// `Array` (clj-quickjs-155/-153): callable and constructible.
    Array = 5,
    /// Coercing number predicates (clj-quickjs-147).
    IsNaN = 6,
    IsFinite = 7,

    ParseInt = 8,
    ParseFloat = 9,
    /// The ordinary Math namespace (clj-quickjs-152).
    Math = 10,
    Error = 11,
    TypeError = 12,
    RangeError = 13,
    ReferenceError = 14,
    SyntaxError = 15,
    EvalError = 16,
    URIError = 17,
    InternalError = 18,
    Json = 19,

    Symbol = 20,
    Map = 21,

    ArrayBuffer = 22,

    Intl = 23,
    Int8Array = 24,
    Uint8Array = 25,
    Uint8ClampedArray = 26,
    Int16Array = 27,
    Uint16Array = 28,
    Int32Array = 29,
    Uint32Array = 30,

    Date = 31,
    Float16Array = 32,
    Float32Array = 33,
    Float64Array = 34,
    RegExp = 35,

    BigInt = 36,
    Promise = 37,
    WeakMap = 38,
    Eval = 39,
    WeakRef = 40,
}

impl Global {
    /// Every built-in global, in operand order.
    const ALL: [Global; 41] = [
        Global::Object,
        Global::Number,
        Global::String,
        Global::Function,
        Global::Reflect,
        Global::Array,
        Global::IsNaN,
        Global::IsFinite,
        Global::ParseInt,
        Global::ParseFloat,
        Global::Math,
        Global::Error,
        Global::TypeError,
        Global::RangeError,
        Global::ReferenceError,
        Global::SyntaxError,
        Global::EvalError,
        Global::URIError,
        Global::InternalError,
        Global::Json,
        Global::Symbol,
        Global::Map,
        Global::ArrayBuffer,
        Global::Intl,
        Global::Int8Array,
        Global::Uint8Array,
        Global::Uint8ClampedArray,
        Global::Int16Array,
        Global::Uint16Array,
        Global::Int32Array,
        Global::Uint32Array,
        Global::Date,
        Global::Float16Array,
        Global::Float32Array,
        Global::Float64Array,
        Global::RegExp,
        Global::BigInt,
        Global::Promise,
        Global::WeakMap,
        Global::Eval,
        Global::WeakRef,
    ];

    /// The JavaScript name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Global::BigInt => "BigInt",
            Global::Promise => "Promise",
            Global::Object => "Object",
            Global::Number => "Number",
            Global::String => "String",
            Global::Function => "Function",
            Global::Reflect => "Reflect",
            Global::Array => "Array",
            Global::IsNaN => "isNaN",
            Global::IsFinite => "isFinite",

            Global::ParseInt => "parseInt",
            Global::ParseFloat => "parseFloat",
            Global::Eval => "eval",
            Global::Math => "Math",
            Global::Error => "Error",
            Global::TypeError => "TypeError",
            Global::RangeError => "RangeError",
            Global::ReferenceError => "ReferenceError",
            Global::SyntaxError => "SyntaxError",
            Global::EvalError => "EvalError",
            Global::URIError => "URIError",
            Global::InternalError => "InternalError",
            Global::Json => "JSON",

            Global::Symbol => "Symbol",
            Global::Map => "Map",
            Global::WeakMap => "WeakMap",
            Global::WeakRef => "WeakRef",

            Global::ArrayBuffer => "ArrayBuffer",

            Global::Intl => "Intl",
            Global::Int8Array => "Int8Array",
            Global::Uint8Array => "Uint8Array",
            Global::Uint8ClampedArray => "Uint8ClampedArray",
            Global::Int16Array => "Int16Array",
            Global::Uint16Array => "Uint16Array",
            Global::Int32Array => "Int32Array",
            Global::Uint32Array => "Uint32Array",

            Global::Date => "Date",
            Global::Float16Array => "Float16Array",
            Global::Float32Array => "Float32Array",
            Global::Float64Array => "Float64Array",
            Global::RegExp => "RegExp",
        }
    }

    /// The built-in global named `name`, if any.
    #[cfg(test)]
    pub(crate) fn from_name(name: &str) -> Option<Global> {
        Global::ALL.into_iter().find(|g| g.name() == name)
    }

    /// Decode a `get_builtin_global` operand.
    pub(crate) fn from_u16(operand: u16) -> Option<Global> {
        Global::ALL.get(usize::from(operand)).copied()
    }
}

/// The realm's intrinsic objects. See the module docs.
#[derive(Debug)]
pub(crate) struct Intrinsics {
    /// Template sites in this realm. Code is retained to prevent address
    /// reuse; arrays are Intrinsic roots, independent of task lifetimes.
    /// The leaf lock covers lookup/first allocation/installation only.
    pub(crate) templates: Mutex<TemplateSites>,
    /// `Object.prototype`.
    pub(crate) object_prototype: ObjectId,
    /// The `Object` constructor.
    pub(crate) object: ObjectId,
    /// `Array.prototype`, the prototype of every array (clj-quickjs-118).
    pub(crate) array_prototype: ObjectId,
    /// The `Array` constructor, also reachable as a global.
    array: ObjectId,
    /// `Function.prototype`, the prototype of every function object and
    /// built-in function (clj-quickjs-102).
    pub(crate) function_prototype: ObjectId,
    /// The `Function` stub constructor (clj-quickjs-102).
    function: ObjectId,
    /// The `Reflect` object (clj-quickjs-102).
    reflect: ObjectId,
    /// Every intrinsic object (the collector's roots).
    all: Vec<ObjectId>,
    /// A copy of every intrinsic's own properties and prototype, taken once
    /// the realm is complete: property reads that reach an intrinsic use it
    /// instead of the object table, so they take no lock (module docs,
    /// "Lock-free reads").
    views: Vec<IntrinsicView>,
    /// `Number.prototype`: a property read on a number primitive starts
    /// here (clj-quickjs-73, [`Intrinsics::primitive_prototype`]).
    pub(crate) number_prototype: ObjectId,
    /// `String.prototype`: a property read on a string primitive starts
    /// here (clj-quickjs-73).
    pub(crate) string_prototype: ObjectId,
    /// The `Number` constructor.
    number: ObjectId,
    /// The `String` constructor.
    string: ObjectId,
    /// The coercing global number predicates.
    is_nan: ObjectId,
    is_finite: ObjectId,

    parse_int: ObjectId,
    parse_float: ObjectId,
    eval: ObjectId,
    /// Ordinary, non-callable Math namespace.
    math: ObjectId,
    pub(crate) error_prototype: ObjectId,
    error: ObjectId,
    /// (constructor, prototype) pairs in NativeErrorKind order; rooted in `all`.
    native_errors: [(ObjectId, ObjectId); 7],
    json: ObjectId,

    symbol: ObjectId,
    pub(crate) map_prototype: ObjectId,
    map: ObjectId,
    pub(crate) weak_map_prototype: ObjectId,
    weak_map: ObjectId,
    pub(crate) weak_ref_prototype: ObjectId,
    weak_ref: ObjectId,

    pub(crate) array_buffer_prototype: ObjectId,
    array_buffer: ObjectId,

    pub(crate) intl: ObjectId,
    pub(crate) intl_locale_prototype: ObjectId,
    pub(crate) typed_array_prototypes: [ObjectId; 10],
    typed_array_constructors: [ObjectId; 10],

    pub(crate) date_prototype: ObjectId,
    date: ObjectId,
    pub(crate) regexp_prototype: ObjectId,
    regexp: ObjectId,

    pub(crate) bigint_prototype: ObjectId,
    bigint: ObjectId,
    pub(crate) promise_prototype: ObjectId,
    promise: ObjectId,

    pub(crate) intl_number_format_prototype: ObjectId,
    pub(crate) intl_number_format_bound: ObjectId,
    pub(crate) array_iterator_prototype: ObjectId,
    pub(crate) string_iterator_prototype: ObjectId,

    pub(crate) intl_plural_rules_prototype: ObjectId,
    pub(crate) generator_prototype: ObjectId,
    pub(crate) intl_collator_prototype: ObjectId,
    pub(crate) intl_collator_bound: ObjectId,
    pub(crate) intl_date_time_format_prototype: ObjectId,
    pub(crate) intl_date_time_format_bound: ObjectId,
    pub(crate) symbol_prototype: ObjectId,
}

/// The immutable property view of one intrinsic object.
#[derive(Debug)]
pub(crate) struct IntrinsicView {
    id: ObjectId,
    /// `Function.prototype`, which lacks some of its standard properties
    /// (`toString`, ...): a lookup that reaches it without finding
    /// one of those fails instead of continuing at `Object.prototype`
    /// ([`crate::vm::function::lacks_inherited`]).
    pub(crate) function_prototype: bool,
    /// A built-in function (`Function.prototype` included): the for-in
    /// walk ends at it (`crate::vm::keys`).
    pub(crate) builtin: bool,
    pub(crate) prototype: Option<ObjectId>,
    properties: Vec<(PropertyKey, Property)>,
}

impl IntrinsicView {
    /// The own property `key`, if any.
    pub(crate) fn get_own(&self, key: PropertyKey) -> Option<&Property> {
        self.properties
            .iter()
            .find_map(|(k, p)| (*k == key).then_some(p))
    }

    /// Each own property's key and `enumerable` attribute, in creation
    /// order (`for`-`in`'s prototype walk, clj-quickjs-86,
    /// `crate::vm::keys`).
    pub(crate) fn own_keys(&self) -> impl Iterator<Item = (PropertyKey, bool)> + '_ {
        self.properties
            .iter()
            .map(|(k, p)| (*k, p.flags.enumerable))
    }
}

impl Intrinsics {
    pub(crate) fn native_error_prototype(&self, kind: NativeErrorKind) -> ObjectId {
        self.native_errors[kind as usize].1
    }

    /// The property view of `id` if it is an intrinsic: a scan of the few
    /// intrinsic identities, no lock.
    pub(crate) fn view(&self, id: ObjectId) -> Option<&IntrinsicView> {
        self.views.iter().find(|v| v.id == id)
    }

    /// The prototype a property access on the primitive `v` starts at, as
    /// if on its wrapper object, which is not allocated (clj-quickjs-73;
    /// QuickJS `JS_GetPrototypePrimitive`): `Number.prototype` for a
    /// number, `String.prototype` for a string, `BigInt.prototype` for a BigInt,
    /// `Symbol.prototype` for a symbol.
    /// `None` for anything else
    /// (objects, `undefined`, `null`, and booleans until `Boolean.prototype`
    /// exists, clj-quickjs-39).
    pub(crate) fn primitive_prototype(&self, v: &Value) -> Option<ObjectId> {
        match v {
            Value::Number(_) => Some(self.number_prototype),
            Value::String(_) => Some(self.string_prototype),
            Value::BigInt(_) => Some(self.bigint_prototype),
            Value::Symbol(_) => Some(self.symbol_prototype),
            _ => None,
        }
    }

    /// The value of a built-in global.
    pub(crate) fn global(&self, global: Global) -> Value {
        match global {
            Global::BigInt => Value::Object(self.bigint),
            Global::Promise => Value::Object(self.promise),
            Global::ArrayBuffer => Value::Object(self.array_buffer),
            Global::Int8Array => Value::Object(self.typed_array_constructors[0]),
            Global::Uint8Array => Value::Object(self.typed_array_constructors[1]),
            Global::Uint8ClampedArray => Value::Object(self.typed_array_constructors[2]),
            Global::Int16Array => Value::Object(self.typed_array_constructors[3]),
            Global::Uint16Array => Value::Object(self.typed_array_constructors[4]),
            Global::Int32Array => Value::Object(self.typed_array_constructors[5]),
            Global::Uint32Array => Value::Object(self.typed_array_constructors[6]),
            Global::Float16Array => Value::Object(self.typed_array_constructors[7]),
            Global::Float32Array => Value::Object(self.typed_array_constructors[8]),
            Global::Float64Array => Value::Object(self.typed_array_constructors[9]),
            Global::Object => Value::Object(self.object),
            Global::Map => Value::Object(self.map),
            Global::WeakMap => Value::Object(self.weak_map),
            Global::WeakRef => Value::Object(self.weak_ref),
            Global::Date => Value::Object(self.date),
            Global::Number => Value::Object(self.number),
            Global::String => Value::Object(self.string),
            Global::Function => Value::Object(self.function),
            Global::Reflect => Value::Object(self.reflect),
            Global::Array => Value::Object(self.array),
            Global::IsNaN => Value::Object(self.is_nan),
            Global::IsFinite => Value::Object(self.is_finite),

            Global::ParseInt => Value::Object(self.parse_int),
            Global::ParseFloat => Value::Object(self.parse_float),
            Global::Eval => Value::Object(self.eval),
            Global::Math => Value::Object(self.math),
            Global::Error => Value::Object(self.error),
            Global::TypeError => {
                Value::Object(self.native_errors[NativeErrorKind::Type as usize].0)
            }
            Global::RangeError => {
                Value::Object(self.native_errors[NativeErrorKind::Range as usize].0)
            }
            Global::ReferenceError => {
                Value::Object(self.native_errors[NativeErrorKind::Reference as usize].0)
            }
            Global::SyntaxError => {
                Value::Object(self.native_errors[NativeErrorKind::Syntax as usize].0)
            }
            Global::EvalError => {
                Value::Object(self.native_errors[NativeErrorKind::Eval as usize].0)
            }
            Global::URIError => Value::Object(self.native_errors[NativeErrorKind::Uri as usize].0),
            Global::InternalError => {
                Value::Object(self.native_errors[NativeErrorKind::Internal as usize].0)
            }
            Global::Json => Value::Object(self.json),

            Global::Symbol => Value::Object(self.symbol),
            Global::Intl => Value::Object(self.intl),
            Global::RegExp => Value::Object(self.regexp),
        }
    }
}

/// Report every intrinsic object.
impl Trace for Intrinsics {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        // The views copy property values of these objects, which are
        // intrinsics (listed here) or primitives, so they need no report of
        // their own (`realm_views_hold_only_intrinsics` checks it).
        self.all
            .iter()
            .copied()
            .map(Into::into)
            .for_each(&mut *visit);
        for (_, id) in self
            .templates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
        {
            visit((*id).into());
        }
    }
}

/// The realm slot of a heap.
#[derive(Debug, Default)]
pub(crate) struct Realm {
    intrinsics: OnceLock<Intrinsics>,
    /// Serializes building; held only while no realm exists (module docs).
    build: Mutex<()>,
}

impl Realm {
    /// The installed intrinsics, if the realm was built.
    pub(crate) fn get(&self) -> Option<&Intrinsics> {
        self.intrinsics.get()
    }

    /// Drop the realm (tests: its objects become unreachable).
    #[cfg(test)]
    pub(crate) fn take(&mut self) -> Option<Intrinsics> {
        self.intrinsics.take()
    }
}

/// The realm of `heap`, building it on first use. `None` only when the
/// heap's object table is exhausted (nothing is installed then, and a later
/// call tries again).
///
/// The caller must be a registered heap mutator of this thread that does
/// not reach a safepoint during the call (a VM instruction, or a test with
/// no collector running): the objects are rooted only once installed.
pub(crate) fn intrinsics(heap: &Heap) -> Option<&Intrinsics> {
    let realm = heap.realm();
    if let Some(intrinsics) = realm.get() {
        return Some(intrinsics);
    }
    let _build = realm.build.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(intrinsics) = realm.get() {
        return Some(intrinsics);
    }
    let built = build(heap)?;
    heap.mark_shareable(&built.all);
    Some(realm.intrinsics.get_or_init(|| built))
}

/// A task's mutable global object. Intrinsics stay shared and immutable;
/// their global bindings are ordinary writable properties of this object.
/// The caller is a registered mutator and must install the result in traced
/// task state before the next safepoint.
pub(crate) fn new_global_object(heap: &Heap, task: crate::ids::TaskId) -> Option<ObjectId> {
    let intrinsics = intrinsics(heap)?;
    let mut object = OrdinaryObject::with_prototype(task, intrinsics.object_prototype);
    for global in Global::ALL {
        let key = heap.keys().intern(&JsString::from(global.name()))?;
        object.define_with(key, intrinsics.global(global), PropertyFlags::METHOD);
    }
    for (name, value) in [
        ("undefined", Value::Undefined),
        ("NaN", Value::Number(f64::NAN)),
        ("Infinity", Value::Number(f64::INFINITY)),
    ] {
        let key = heap.keys().intern(&JsString::from(name))?;
        object.define_with(key, value, PropertyFlags::FIXED);
    }
    let key = heap.keys().intern(&JsString::from("globalThis"))?;
    let id = heap.alloc(HeapObject::Plain(object))?;
    heap.with_object_mut(id, |object| {
        if let HeapObject::Plain(object) = object {
            object.define_with(key, Value::Object(id), PropertyFlags::METHOD);
        }
    })?;
    Some(id)
}

/// GlobalDeclarationInstantiation for a script's object bindings. Lexical
/// redeclarations are checked by the compiler; there is one script per task.
#[cfg(test)]
pub(crate) fn declare_global(
    heap: &Heap,
    id: ObjectId,
    name: &JsString,
    function: bool,
) -> Result<(), crate::vm::dispatch::Fault> {
    declare_global_with(heap, id, name, function, false)
}

pub(crate) fn declare_global_with(
    heap: &Heap,
    id: ObjectId,
    name: &JsString,
    function: bool,
    configurable: bool,
) -> Result<(), crate::vm::dispatch::Fault> {
    use crate::vm::dispatch::Fault;
    let declaration_error = |message| {
        if configurable {
            Fault::TypeError(message)
        } else {
            Fault::SyntaxError(message)
        }
    };
    let key = heap
        .keys()
        .intern(name)
        .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
    heap.with_object_mut(id, |object| {
        let HeapObject::Plain(object) = object else {
            return Err(Fault::Internal("global object is not ordinary"));
        };
        if let Some(prior) = object.get_own(key) {
            if !function {
                return Ok(());
            }
            if !prior.flags.configurable && (!prior.flags.writable || !prior.flags.enumerable) {
                return Err(declaration_error(format!(
                    "cannot declare global function '{name}'"
                )));
            }
            if !prior.flags.configurable {
                return Ok(());
            }
        }
        let flags = PropertyFlags {
            writable: true,
            enumerable: true,
            configurable,
        };
        if !object.define_with(key, Value::Undefined, flags) {
            return Err(declaration_error(format!("cannot declare global '{name}'")));
        }
        Ok(())
    })
    .ok_or(Fault::Internal("global object was collected"))?
}

/// Declaration preflight performs no property mutation and invokes no user
/// code. Eval checks all names before any declaration creates or resets one.
pub(crate) fn check_global_declaration(
    heap: &Heap,
    id: ObjectId,
    name: &JsString,
    function: bool,
    eval: bool,
) -> Result<(), crate::vm::dispatch::Fault> {
    use crate::vm::dispatch::Fault;
    let key = heap
        .keys()
        .intern(name)
        .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
    heap.read(id, |object| {
        let HeapObject::Plain(object) = object else {
            return Err(Fault::Internal("global object is not ordinary"));
        };
        let allowed = if let Some(prior) = object.get_own(key) {
            !function
                || prior.flags.configurable
                || (prior.flags.writable && prior.flags.enumerable && prior.accessor.is_none())
        } else {
            object.extensible()
        };
        if allowed {
            Ok(())
        } else {
            let message = format!("cannot declare global '{name}'");
            Err(if eval {
                Fault::TypeError(message)
            } else {
                Fault::SyntaxError(message)
            })
        }
    })
    .ok_or(Fault::Internal("global object was collected"))?
}

/// Allocate and connect the intrinsics (module docs, table).
///
/// The order follows the prototype edges: `Object.prototype` first (its
/// methods are added once they exist), then `Function.prototype`, the
/// prototype of every built-in function created after it.
fn build(heap: &Heap) -> Option<Intrinsics> {
    let mut b = Build {
        heap,
        all: Vec::new(),
        function_prototype: None,
    };
    let object_prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(None)))?;
    b.all.push(object_prototype);
    let function_prototype = b.builtin(Some(object_prototype), Builtin::FunctionPrototype, &[])?;
    b.function_prototype = Some(function_prototype);

    let to_string = b.function(Builtin::ObjectPrototypeToString, &[])?;
    let value_of = b.function(Builtin::ObjectPrototypeValueOf, &[])?;
    let is = b.function(Builtin::ObjectIs, &[])?;
    let define_property = b.function(Builtin::ObjectDefineProperty, &[])?;
    let get_own_property_descriptor = b.function(Builtin::ObjectGetOwnPropertyDescriptor, &[])?;
    let get_prototype_of = b.function(Builtin::ObjectGetPrototypeOf, &[])?;
    let set_prototype_of = b.function(Builtin::ObjectSetPrototypeOf, &[])?;
    let create = b.function(Builtin::ObjectCreate, &[])?;
    let prevent_extensions = b.function(Builtin::ObjectPreventExtensions, &[])?;
    let is_extensible = b.function(Builtin::ObjectIsExtensible, &[])?;
    let freeze = b.function(Builtin::ObjectFreeze, &[])?;
    let is_frozen = b.function(Builtin::ObjectIsFrozen, &[])?;
    let keys = b.function(Builtin::ObjectKeys, &[])?;
    let get_own_property_names = b.function(Builtin::ObjectGetOwnPropertyNames, &[])?;
    let object = b.function(
        Builtin::Object,
        &[
            ("prototype", object_prototype, PropertyFlags::FIXED),
            ("is", is, PropertyFlags::METHOD),
            ("defineProperty", define_property, PropertyFlags::METHOD),
            (
                "getOwnPropertyDescriptor",
                get_own_property_descriptor,
                PropertyFlags::METHOD,
            ),
            ("getPrototypeOf", get_prototype_of, PropertyFlags::METHOD),
            ("setPrototypeOf", set_prototype_of, PropertyFlags::METHOD),
            ("create", create, PropertyFlags::METHOD),
            (
                "preventExtensions",
                prevent_extensions,
                PropertyFlags::METHOD,
            ),
            ("isExtensible", is_extensible, PropertyFlags::METHOD),
            ("freeze", freeze, PropertyFlags::METHOD),
            ("isFrozen", is_frozen, PropertyFlags::METHOD),
            ("keys", keys, PropertyFlags::METHOD),
            (
                "getOwnPropertyNames",
                get_own_property_names,
                PropertyFlags::METHOD,
            ),
        ],
    )?;
    // `Object.prototype`'s properties; `constructor` is the cycle
    // `Object.prototype.constructor === Object`.
    for (name, value) in [
        ("toString", to_string),
        ("valueOf", value_of),
        ("constructor", object),
    ] {
        b.add(object_prototype, name, value, PropertyFlags::METHOD)?;
    }
    let proto_getter = b.function(Builtin::ObjectPrototypeProtoGetter, &[])?;
    let proto_setter = b.function(Builtin::ObjectPrototypeProtoSetter, &[])?;
    let proto_key = heap.keys().intern(&JsString::from("__proto__"))?;
    let accessor = PropertyDescriptor {
        get: Some(Value::Object(proto_getter)),
        set: Some(Value::Object(proto_setter)),
        enumerable: Some(false),
        configurable: Some(true),
        ..PropertyDescriptor::default()
    };
    let defined = heap.with_object_mut(object_prototype, |o| match o {
        HeapObject::Plain(o) => o.define_own_property(proto_key, &accessor),
        _ => unreachable!("Object.prototype is an ordinary intrinsic"),
    })?;
    if defined != DefineOutcome::Done {
        return None;
    }

    // `Function.prototype.call` and `apply`, and the `Function` stub
    // (`Function.prototype.constructor`, clj-quickjs-102).
    let call = b.function(Builtin::FunctionPrototypeCall, &[])?;
    let apply = b.function(Builtin::FunctionPrototypeApply, &[])?;
    let bind = b.function(Builtin::FunctionPrototypeBind, &[])?;
    b.add(function_prototype, "bind", bind, PropertyFlags::METHOD)?;
    let function = b.function(
        Builtin::Function,
        &[("prototype", function_prototype, PropertyFlags::FIXED)],
    )?;
    for (name, value) in [("call", call), ("apply", apply), ("constructor", function)] {
        b.add(function_prototype, name, value, PropertyFlags::METHOD)?;
    }

    // `Reflect`: an ordinary object with `apply` (clj-quickjs-102).
    let reflect_apply = b.function(Builtin::ReflectApply, &[])?;
    let mut reflect = OrdinaryObject::intrinsic(Some(object_prototype));
    define(
        heap,
        &mut reflect,
        "apply",
        reflect_apply,
        PropertyFlags::METHOD,
    )?;
    let reflect = heap.alloc(HeapObject::Plain(reflect))?;
    b.all.push(reflect);

    let (array_prototype, array) = array_prototype(&mut b, object_prototype)?;
    let (number_prototype, number) = primitive_prototype(
        &mut b,
        object_prototype,
        [
            Builtin::Number,
            Builtin::NumberPrototypeToString,
            Builtin::NumberPrototypeValueOf,
        ],
    )?;
    let to_fixed = b.function(Builtin::NumberPrototypeToFixed, &[])?;
    b.add(number_prototype, "toFixed", to_fixed, PropertyFlags::METHOD)?;
    for which in [
        Builtin::NumberPrototypeToExponential,
        Builtin::NumberPrototypeToPrecision,
    ] {
        let method = b.function(which, &[])?;
        b.add(
            number_prototype,
            which.name(),
            method,
            PropertyFlags::METHOD,
        )?;
    }
    let (string_prototype, string) = primitive_prototype(
        &mut b,
        object_prototype,
        [
            Builtin::String,
            Builtin::StringPrototypeToString,
            Builtin::StringPrototypeValueOf,
        ],
    )?;
    for which in [
        Builtin::StringPrototypeCharAt,
        Builtin::StringPrototypeCharCodeAt,
        Builtin::StringPrototypeCodePointAt,
        Builtin::StringPrototypeIndexOf,
        Builtin::StringPrototypeLastIndexOf,
        Builtin::StringPrototypeSubstring,
        Builtin::StringPrototypeSlice,
        Builtin::StringPrototypeConcat,
        Builtin::StringPrototypePadStart,
        Builtin::StringPrototypePadEnd,
        Builtin::StringPrototypeSplit,
        Builtin::StringPrototypeReplace,
        Builtin::StringPrototypeReplaceAll,
    ] {
        let method = b.function(which, &[])?;
        b.add(
            string_prototype,
            which.name(),
            method,
            PropertyFlags::METHOD,
        )?;
    }
    for which in [
        Builtin::StringFromCharCode,
        Builtin::StringFromCodePoint,
        Builtin::StringRaw,
    ] {
        let method = b.function(which, &[])?;
        b.add(string, which.name(), method, PropertyFlags::METHOD)?;
    }
    for which in [
        Builtin::NumberIsNaN,
        Builtin::NumberIsFinite,
        Builtin::NumberIsInteger,
        Builtin::NumberIsSafeInteger,
    ] {
        let method = b.function(which, &[])?;
        b.add(number, which.name(), method, PropertyFlags::METHOD)?;
    }
    for (name, value) in [
        ("EPSILON", f64::EPSILON),
        ("MAX_SAFE_INTEGER", 9_007_199_254_740_991.0),
        ("MIN_SAFE_INTEGER", -9_007_199_254_740_991.0),
        ("MAX_VALUE", f64::MAX),
        ("MIN_VALUE", f64::from_bits(1)),
        ("NaN", f64::NAN),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
    ] {
        b.add_value(number, name, Value::Number(value), PropertyFlags::FIXED)?;
    }
    let is_nan = b.function(Builtin::GlobalIsNaN, &[])?;
    let is_finite = b.function(Builtin::GlobalIsFinite, &[])?;

    let parse_int = b.function(Builtin::ParseInt, &[])?;
    let parse_float = b.function(Builtin::ParseFloat, &[])?;
    b.add(number, "parseInt", parse_int, PropertyFlags::METHOD)?;
    b.add(number, "parseFloat", parse_float, PropertyFlags::METHOD)?;
    let math = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(math);
    for which in [
        Builtin::MathFloor,
        Builtin::MathCeil,
        Builtin::MathAbs,
        Builtin::MathImul,
        Builtin::MathFround,
        Builtin::MathHypot,
        Builtin::MathPow,
        Builtin::MathSqrt,
        Builtin::MathLog2,
        Builtin::MathSumPrecise,
    ] {
        let method = b.function(which, &[])?;
        b.add(math, which.name(), method, PropertyFlags::METHOD)?;
    }
    let error_prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(error_prototype);
    let error = b.function(
        Builtin::Error,
        &[("prototype", error_prototype, PropertyFlags::FIXED)],
    )?;
    let error_to_string = b.function(Builtin::ErrorPrototypeToString, &[])?;
    b.add(error_prototype, "constructor", error, PropertyFlags::METHOD)?;
    b.add(
        error_prototype,
        "toString",
        error_to_string,
        PropertyFlags::METHOD,
    )?;
    for (name, text) in [("name", "Error"), ("message", "")] {
        b.add_value(
            error_prototype,
            name,
            Value::String(JsString::from(text)),
            PropertyFlags::METHOD,
        )?;
    }
    let mut native_errors = Vec::with_capacity(NativeErrorKind::ALL.len());
    for kind in NativeErrorKind::ALL {
        let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
            error_prototype,
        ))))?;
        b.all.push(prototype);
        let constructor = b.builtin(
            Some(error),
            Builtin::NativeError(kind),
            &[("prototype", prototype, PropertyFlags::FIXED)],
        )?;
        b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
        for (name, text) in [("name", kind.name()), ("message", "")] {
            b.add_value(
                prototype,
                name,
                Value::String(JsString::from(text)),
                PropertyFlags::METHOD,
            )?;
        }
        native_errors.push((constructor, prototype));
    }
    let native_errors = native_errors.try_into().ok()?;
    let json = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(json);
    let parse = b.function(Builtin::JsonParse, &[])?;
    b.add(json, "parse", parse, PropertyFlags::METHOD)?;

    let symbol_prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(symbol_prototype);
    let symbol = b.function(
        Builtin::Symbol,
        &[("prototype", symbol_prototype, PropertyFlags::FIXED)],
    )?;
    b.add(
        symbol_prototype,
        "constructor",
        symbol,
        PropertyFlags::METHOD,
    )?;
    for which in [
        Builtin::SymbolPrototypeToString,
        Builtin::SymbolPrototypeValueOf,
    ] {
        let method = b.function(which, &[])?;
        b.add(
            symbol_prototype,
            which.name(),
            method,
            PropertyFlags::METHOD,
        )?;
    }
    let getter = b.function(Builtin::SymbolPrototypeDescription, &[])?;
    let description = heap.keys().intern(&JsString::from("description"))?;
    let to_primitive = b.function(Builtin::SymbolPrototypeToPrimitive, &[])?;
    let to_primitive_key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toPrimitive"))?;
    let tag_key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    heap.with_object_mut(symbol_prototype, |object| {
        let HeapObject::Plain(o) = object else {
            return false;
        };
        o.define_own_property(
            description,
            &PropertyDescriptor {
                get: Some(Value::Object(getter)),
                set: Some(Value::Undefined),
                enumerable: Some(false),
                configurable: Some(true),
                ..PropertyDescriptor::default()
            },
        ) == DefineOutcome::Done
            && o.define_with(
                to_primitive_key,
                Value::Object(to_primitive),
                PropertyFlags::FUNCTION_INFO,
            )
            && o.define_with(
                tag_key,
                Value::String(JsString::from("Symbol")),
                PropertyFlags::FUNCTION_INFO,
            )
    })?
    .then_some(())?;
    let symbol_for = b.function(Builtin::SymbolFor, &[])?;
    b.add(symbol, "for", symbol_for, PropertyFlags::METHOD)?;
    let symbol_key_for = b.function(Builtin::SymbolKeyFor, &[])?;
    b.add(symbol, "keyFor", symbol_key_for, PropertyFlags::METHOD)?;
    for name in ["iterator", "toPrimitive", "hasInstance", "toStringTag"] {
        b.add_value(
            symbol,
            name,
            Value::Symbol(crate::value::symbol::well_known(name)),
            PropertyFlags::FIXED,
        )?;
    }
    let stringify = b.function(Builtin::JsonStringify, &[])?;
    b.add(json, "stringify", stringify, PropertyFlags::METHOD)?;

    let (map_prototype, map) = map_intrinsics(&mut b, object_prototype)?;
    let (weak_map_prototype, weak_map) = weak_map_intrinsics(&mut b, object_prototype)?;
    let (weak_ref_prototype, weak_ref) = weak_ref_intrinsics(&mut b, object_prototype)?;

    let (array_buffer_prototype, array_buffer) = array_buffer_intrinsics(&mut b, object_prototype)?;

    let intl = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(intl);
    let tag = crate::value::symbol::well_known("toStringTag");
    let tag_key = heap.keys().intern_symbol(&tag)?;
    heap.with_object_mut(intl, |object| match object {
        HeapObject::Plain(o) => o.define_with(
            tag_key,
            Value::String(JsString::from("Intl")),
            PropertyFlags::FUNCTION_INFO,
        ),
        _ => false,
    })?
    .then_some(())?;
    let intl_locale_prototype = intl_locale_intrinsics(&mut b, object_prototype, intl)?;
    let (typed_array_prototypes, typed_array_constructors) =
        typed_array_intrinsics(&mut b, object_prototype)?;
    for method in [Builtin::IntlLocaleMaximize, Builtin::IntlLocaleMinimize] {
        let function = b.function(method, &[])?;
        b.add(
            intl_locale_prototype,
            method.name(),
            function,
            PropertyFlags::METHOD,
        )?;
    }

    let (date_prototype, date) = date_intrinsics(&mut b, object_prototype)?;
    let (regexp_prototype, regexp) = regexp_intrinsics(&mut b, object_prototype)?;
    let bigint_prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(bigint_prototype);
    let bigint = b.function(
        Builtin::BigInt,
        &[("prototype", bigint_prototype, PropertyFlags::FIXED)],
    )?;
    b.add(
        bigint_prototype,
        "constructor",
        bigint,
        PropertyFlags::METHOD,
    )?;
    // Append methods after the existing intrinsic allocation sequence. Share
    // Array.prototype.toString's actual function object.
    let shared = heap.read(typed_array_prototypes[0], |o| match o {
        HeapObject::Plain(o) => o.prototype(),
        _ => None,
    })??;
    let key = heap.keys().intern(&JsString::from("toString"))?;
    let to_string = heap.read(array_prototype, |o| match o {
        HeapObject::Plain(o) => o.get_own(key).map(|p| p.value.clone()),
        _ => None,
    })??;
    b.add_value(shared, "toString", to_string, PropertyFlags::METHOD)?;
    for method in [
        Builtin::TypedArraySet,
        Builtin::TypedArrayFill,
        Builtin::TypedArrayJoin,
    ] {
        let function = b.function(method, &[])?;
        b.add(shared, method.name(), function, PropertyFlags::METHOD)?;
    }
    for method in [
        Builtin::BigIntPrototypeToString,
        Builtin::BigIntPrototypeValueOf,
    ] {
        let function = b.function(method, &[])?;
        b.add(
            bigint_prototype,
            method.name(),
            function,
            PropertyFlags::METHOD,
        )?;
    }
    let promise_prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(promise_prototype);
    let promise = b.function(
        Builtin::Promise,
        &[("prototype", promise_prototype, PropertyFlags::FIXED)],
    )?;
    b.add(
        promise_prototype,
        "constructor",
        promise,
        PropertyFlags::METHOD,
    )?;
    for (name, which, target) in [
        ("resolve", Builtin::PromiseResolve, promise),
        ("reject", Builtin::PromiseReject, promise),
        ("then", Builtin::PromiseThen, promise_prototype),
        ("catch", Builtin::PromiseCatch, promise_prototype),
    ] {
        let function = b.function(which, &[])?;
        b.add(target, name, function, PropertyFlags::METHOD)?;
    }

    let (intl_number_format_prototype, intl_number_format_bound) =
        intl_number_format_intrinsics(&mut b, object_prototype, intl)?;
    date_string_intrinsics(&mut b, date_prototype)?;
    let (iterator_prototype, array_iterator_prototype, string_iterator_prototype) =
        iterator_intrinsics(
            &mut b,
            object_prototype,
            array_prototype,
            string_prototype,
            typed_array_prototypes[0],
        )?;

    for method in [Builtin::BigIntAsIntN, Builtin::BigIntAsUintN] {
        let function = b.function(method, &[])?;
        b.add(bigint, method.name(), function, PropertyFlags::METHOD)?;
    }

    let intl_plural_rules_prototype = intl_plural_rules_intrinsics(&mut b, object_prototype, intl)?;

    let date_parse = b.function(Builtin::DateParse, &[])?;
    b.add(date, "parse", date_parse, PropertyFlags::METHOD)?;
    let array_from = b.function(Builtin::ArrayFrom, &[])?;
    b.add(array, "from", array_from, PropertyFlags::METHOD)?;

    let mut generator = OrdinaryObject::intrinsic(Some(iterator_prototype));
    let tag = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    generator
        .define_with(
            tag,
            Value::String("Generator".into()),
            PropertyFlags::FUNCTION_INFO,
        )
        .then_some(())?;
    let generator_prototype = heap.alloc(HeapObject::Plain(generator))?;
    b.all.push(generator_prototype);
    let next = b.function(Builtin::GeneratorNext, &[])?;
    b.add(generator_prototype, "next", next, PropertyFlags::METHOD)?;

    let array_sort = b.function(Builtin::ArrayPrototypeSort, &[])?;
    b.add(array_prototype, "sort", array_sort, PropertyFlags::METHOD)?;
    let (intl_collator_prototype, intl_collator_bound) =
        intl_collator_intrinsics(&mut b, object_prototype, intl)?;
    let (intl_date_time_format_prototype, intl_date_time_format_bound) =
        intl_date_time_format_intrinsics(&mut b, object_prototype, intl)?;
    let eval = b.function(Builtin::Eval, &[])?;
    let all = b.all;
    // Nothing changes the intrinsics from here on, so the views stay exact.
    let views = all
        .iter()
        .map(|&id| {
            heap.read(id, |object| {
                let (store, builtin) = match object {
                    HeapObject::Plain(o) => (o, false),
                    HeapObject::Builtin(f) => (&f.properties, true),
                    _ => return None,
                };
                Some(IntrinsicView {
                    id,
                    function_prototype: id == function_prototype,
                    builtin,
                    prototype: store.prototype(),
                    properties: store.properties().map(|(k, p)| (k, p.clone())).collect(),
                })
            })
            .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Intrinsics {
        array_iterator_prototype,
        string_iterator_prototype,
        bigint,
        bigint_prototype,
        typed_array_prototypes,
        typed_array_constructors,
        regexp_prototype,
        regexp,
        array_buffer_prototype,
        array_buffer,
        templates: Mutex::new(std::collections::HashMap::new()),
        object_prototype,
        object,
        array_prototype,
        array,
        function_prototype,
        function,
        reflect,
        all,
        views,
        number_prototype,
        string_prototype,
        number,
        string,
        is_nan,
        is_finite,

        parse_int,
        parse_float,
        eval,
        math,
        error_prototype,
        error,
        native_errors,
        json,

        symbol,
        map_prototype,
        map,
        weak_map_prototype,
        weak_map,
        weak_ref_prototype,
        weak_ref,
        intl,
        intl_locale_prototype,

        date_prototype,
        date,

        promise_prototype,
        promise,

        intl_number_format_prototype,
        intl_number_format_bound,
        intl_plural_rules_prototype,
        generator_prototype,
        intl_collator_prototype,
        intl_collator_bound,
        intl_date_time_format_prototype,
        intl_date_time_format_bound,
        symbol_prototype,
    })
}

/// `Number.prototype` or `String.prototype` (clj-quickjs-73) and its
/// constructor, from `[constructor, toString, valueOf]`: an intrinsic
/// ordinary object inheriting from `Object.prototype` with `constructor`,
/// `toString` and `valueOf`. ECMAScript makes `Number.prototype` a Number
/// object holding `+0` and `String.prototype` a String object holding `""`;
/// these primitives live in the ordinary store's internal data slots, and
/// `String.prototype` has the own `length` 0 of the String object for `""`
/// (not writable, enumerable or configurable). The constructor is a
/// callable, constructible built-in function (clj-quickjs-72).
fn primitive_prototype(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
    [constructor, to_string, value_of]: [Builtin; 3],
) -> Option<(ObjectId, ObjectId)> {
    let heap = b.heap;
    let to_string_id = b.function(to_string, &[])?;
    let value_of = b.function(value_of, &[])?;
    let mut prototype = OrdinaryObject::intrinsic(Some(object_prototype));
    prototype = prototype.with_primitive_data(match constructor {
        Builtin::Number => PrimitiveData::Number(0.0),
        Builtin::String => PrimitiveData::String(JsString::from("")),
        _ => return None,
    });
    if to_string == Builtin::StringPrototypeToString {
        let length = heap.keys().intern(&JsString::from("length"))?;
        let flags = PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: false,
        };
        prototype
            .define_with(length, Value::Number(0.0), flags)
            .then_some(())?;
    }
    let to_string = to_string_id;
    define(
        heap,
        &mut prototype,
        "toString",
        to_string,
        PropertyFlags::METHOD,
    )?;
    define(
        heap,
        &mut prototype,
        "valueOf",
        value_of,
        PropertyFlags::METHOD,
    )?;
    let id = heap.alloc(HeapObject::Plain(prototype))?;
    b.all.push(id);
    let constructor = b.function(constructor, &[("prototype", id, PropertyFlags::FIXED)])?;
    b.add(id, "constructor", constructor, PropertyFlags::METHOD)?;
    Some((id, constructor))
}

/// `Array.prototype` (clj-quickjs-118): an intrinsic ordinary object
/// inheriting from `Object.prototype`, with an own `length` of 0 (writable,
/// not enumerable, not configurable, as an array's), `join` and `toString`
/// (clj-quickjs-156). ECMAScript makes it an array exotic object itself;
/// its immutable empty-array representation uses ordinary property storage,
/// with the array brand recognized by realm identity (clj-quickjs-153).
/// Its `constructor` is the callable and constructible global `Array`.
fn array_prototype(b: &mut Build<'_>, object_prototype: ObjectId) -> Option<(ObjectId, ObjectId)> {
    let heap = b.heap;
    let to_string = b.function(Builtin::ArrayPrototypeToString, &[])?;
    let push = b.function(Builtin::ArrayPrototypePush, &[])?;
    let join = b.function(Builtin::ArrayPrototypeJoin, &[])?;
    let mut prototype = OrdinaryObject::intrinsic(Some(object_prototype));
    let length = heap.keys().intern(&JsString::from("length"))?;
    let length_flags = PropertyFlags {
        writable: true,
        enumerable: false,
        configurable: false,
    };
    prototype
        .define_with(length, Value::Number(0.0), length_flags)
        .then_some(())?;
    define(
        heap,
        &mut prototype,
        "toString",
        to_string,
        PropertyFlags::METHOD,
    )?;
    define(heap, &mut prototype, "push", push, PropertyFlags::METHOD)?;
    define(heap, &mut prototype, "join", join, PropertyFlags::METHOD)?;
    let id = heap.alloc(HeapObject::Plain(prototype))?;
    b.all.push(id);
    // Close the constructor/prototype cycle before publishing the realm.
    let array = b.function(Builtin::Array, &[("prototype", id, PropertyFlags::FIXED)])?;
    b.add(id, "constructor", array, PropertyFlags::METHOD)?;
    let is_array = b.function(Builtin::ArrayIsArray, &[])?;
    b.add(array, "isArray", is_array, PropertyFlags::METHOD)?;
    Some((id, array))
}

/// Shared iterator prototypes and the iterable methods (clj-quickjs-237).
fn iterator_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
    array: ObjectId,
    string: ObjectId,
    typed_array: ObjectId,
) -> Option<(ObjectId, ObjectId, ObjectId)> {
    let heap = b.heap;
    let key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("iterator"))?;
    let tag = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    let identity = b.function(Builtin::IteratorIdentity, &[])?;
    let mut iterator = OrdinaryObject::intrinsic(Some(object_prototype));
    iterator
        .define_with(key, Value::Object(identity), PropertyFlags::METHOD)
        .then_some(())?;
    let iterator = heap.alloc(HeapObject::Plain(iterator))?;
    b.all.push(iterator);
    let mut prototypes = Vec::new();
    for (name, kind) in [
        ("Array Iterator", crate::vm::native::iterator::Kind::Array),
        ("String Iterator", crate::vm::native::iterator::Kind::String),
    ] {
        let next = b.function(Builtin::IteratorNext(kind), &[])?;
        let mut prototype = OrdinaryObject::intrinsic(Some(iterator));
        define(heap, &mut prototype, "next", next, PropertyFlags::METHOD)?;
        prototype
            .define_with(
                tag,
                Value::String(JsString::from(name)),
                PropertyFlags::FUNCTION_INFO,
            )
            .then_some(())?;
        let id = heap.alloc(HeapObject::Plain(prototype))?;
        b.all.push(id);
        prototypes.push(id);
    }
    let shared_typed = heap.read(typed_array, |o| match o {
        HeapObject::Plain(o) => o.prototype(),
        _ => None,
    })??;
    for (prototype, which, values) in [
        (array, Builtin::ArrayValues, true),
        (string, Builtin::StringIterator, false),
        (shared_typed, Builtin::TypedArrayValues, true),
    ] {
        let function = b.function(which, &[])?;
        heap.with_object_mut(prototype, |o| match o {
            HeapObject::Plain(o) => {
                o.define_with(key, Value::Object(function), PropertyFlags::METHOD)
            }
            _ => false,
        })?
        .then_some(())?;
        if values {
            b.add(prototype, "values", function, PropertyFlags::METHOD)?;
        }
    }
    Some((iterator, prototypes[0], prototypes[1]))
}

/// Append the Map constructor/prototype cycle and its ordinary accessor.
fn map_intrinsics(b: &mut Build<'_>, object_prototype: ObjectId) -> Option<(ObjectId, ObjectId)> {
    let prototype = b
        .heap
        .alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
            object_prototype,
        ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::Map,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    for which in [
        Builtin::MapSet,
        Builtin::MapGet,
        Builtin::MapHas,
        Builtin::MapDelete,
        Builtin::MapClear,
        Builtin::MapForEach,
    ] {
        let method = b.function(which, &[])?;
        b.add(prototype, which.name(), method, PropertyFlags::METHOD)?;
    }
    let getter = b.function(Builtin::MapSize, &[])?;
    let tag = crate::value::symbol::well_known("toStringTag");
    let tag_key = b.heap.keys().intern_symbol(&tag)?;
    b.heap
        .with_object_mut(prototype, |object| match object {
            HeapObject::Plain(o) => o.define_with(
                tag_key,
                Value::String(JsString::from("Map")),
                PropertyFlags::FUNCTION_INFO,
            ),
            _ => false,
        })?
        .then_some(())?;
    let key = b.heap.keys().intern(&JsString::from("size"))?;
    let desc = PropertyDescriptor {
        get: Some(Value::Object(getter)),
        set: Some(Value::Undefined),
        enumerable: Some(false),
        configurable: Some(true),
        ..PropertyDescriptor::default()
    };
    b.heap
        .with_object_mut(prototype, |object| match object {
            HeapObject::Plain(o) => o.define_own_property(key, &desc) == DefineOutcome::Done,
            _ => false,
        })?
        .then_some(())?;

    Some((prototype, constructor))
}

/// ArrayBuffer's constructor, ordinary prototype, and branded accessors.
fn array_buffer_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
) -> Option<(ObjectId, ObjectId)> {
    let heap = b.heap;
    let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::ArrayBuffer,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    for (name, which) in [
        ("transfer", Builtin::ArrayBufferTransfer),
        ("slice", Builtin::ArrayBufferSlice),
    ] {
        let function = b.function(which, &[])?;
        b.add(prototype, name, function, PropertyFlags::METHOD)?;
    }
    for (name, which) in [
        ("byteLength", Builtin::ArrayBufferByteLength),
        ("detached", Builtin::ArrayBufferDetached),
    ] {
        let getter = b.function(which, &[])?;
        let key = heap.keys().intern(&JsString::from(name))?;
        let descriptor = PropertyDescriptor {
            get: Some(Value::Object(getter)),
            enumerable: Some(false),
            configurable: Some(true),
            ..PropertyDescriptor::default()
        };
        let outcome = heap.with_object_mut(prototype, |o| match o {
            HeapObject::Plain(o) => o.define_own_property(key, &descriptor),
            _ => unreachable!(),
        })?;
        if outcome != DefineOutcome::Done {
            return None;
        }
    }
    Some((prototype, constructor))
}

/// Append the Locale constructor/prototype and canonical locale-list operation.
fn intl_locale_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
    intl: ObjectId,
) -> Option<ObjectId> {
    let heap = b.heap;
    let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::IntlLocale,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    b.add(intl, "Locale", constructor, PropertyFlags::METHOD)?;
    let canonical = b.function(Builtin::IntlGetCanonicalLocales, &[])?;
    b.add(
        intl,
        "getCanonicalLocales",
        canonical,
        PropertyFlags::METHOD,
    )?;
    let to_string = b.function(Builtin::IntlLocaleToString, &[])?;
    b.add(prototype, "toString", to_string, PropertyFlags::METHOD)?;
    for field in crate::intl::locale::Field::ALL {
        let getter = b.function(Builtin::IntlLocaleGet(field), &[])?;
        let key = heap.keys().intern(&JsString::from(field.name()))?;
        let descriptor = PropertyDescriptor {
            get: Some(Value::Object(getter)),
            enumerable: Some(false),
            configurable: Some(true),
            ..PropertyDescriptor::default()
        };
        if heap.with_object_mut(prototype, |o| match o {
            HeapObject::Plain(o) => o.define_own_property(key, &descriptor),
            _ => unreachable!(),
        })? != DefineOutcome::Done
        {
            return None;
        }
    }
    let key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_with(
            key,
            Value::String(JsString::from("Intl.Locale")),
            PropertyFlags::FUNCTION_INFO,
        ),
        _ => false,
    })?
    .then_some(())?;
    Some(prototype)
}

/// Date.prototype is ordinary and unbranded; only constructed Dates have a slot.
fn date_intrinsics(b: &mut Build<'_>, object_prototype: ObjectId) -> Option<(ObjectId, ObjectId)> {
    let prototype = b
        .heap
        .alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
            object_prototype,
        ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::Date,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    for which in [
        Builtin::DateGetTime,
        Builtin::DateValueOf,
        Builtin::DateSetTime,
    ] {
        let method = b.function(which, &[])?;
        b.add(prototype, which.name(), method, PropertyFlags::METHOD)?;
    }
    let utc = b.function(Builtin::DateUtc, &[])?;
    b.add(constructor, "UTC", utc, PropertyFlags::METHOD)?;
    let iso = b.function(Builtin::DateToISOString, &[])?;
    b.add(prototype, "toISOString", iso, PropertyFlags::METHOD)?;
    for which in (0..8)
        .map(Builtin::DateGetUtcField)
        .chain((0..7).map(Builtin::DateSetUtcField))
    {
        let method = b.function(which, &[])?;
        b.add(prototype, which.name(), method, PropertyFlags::METHOD)?;
    }
    let now = b.function(Builtin::DateNow, &[])?;
    b.add(constructor, "now", now, PropertyFlags::METHOD)?;
    for which in (0..8)
        .map(Builtin::DateGetLocalField)
        .chain((0..7).map(Builtin::DateSetLocalField))
        .chain([Builtin::DateTimezoneOffset])
    {
        let method = b.function(which, &[])?;
        b.add(prototype, which.name(), method, PropertyFlags::METHOD)?;
    }
    Some((prototype, constructor))
}

/// Append formatting functions after every existing realm allocation.
fn date_string_intrinsics(b: &mut Build<'_>, prototype: ObjectId) -> Option<()> {
    for part in 1..=3 {
        let which = Builtin::DateToLocalString(part);
        let method = b.function(which, &[])?;
        b.add(prototype, which.name(), method, PropertyFlags::METHOD)?;
    }
    let utc_string = b.function(Builtin::DateToUtcString, &[])?;
    b.add(prototype, "toUTCString", utc_string, PropertyFlags::METHOD)?;
    b.add(prototype, "toGMTString", utc_string, PropertyFlags::METHOD)?;
    Some(())
}

/// Append NumberFormat after all previously allocated realm identities.
fn intl_number_format_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
    intl: ObjectId,
) -> Option<(ObjectId, ObjectId)> {
    let heap = b.heap;
    let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::IntlNumberFormat,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    b.add(intl, "NumberFormat", constructor, PropertyFlags::METHOD)?;
    let supported = b.function(Builtin::IntlNumberFormatSupportedLocales, &[])?;
    b.add(
        constructor,
        "supportedLocalesOf",
        supported,
        PropertyFlags::METHOD,
    )?;
    let resolved = b.function(Builtin::IntlNumberFormatResolvedOptions, &[])?;
    b.add(
        prototype,
        "resolvedOptions",
        resolved,
        PropertyFlags::METHOD,
    )?;
    let getter = b.function(Builtin::IntlNumberFormatGetFormat, &[])?;
    let key = heap.keys().intern(&JsString::from("format"))?;
    let descriptor = PropertyDescriptor {
        get: Some(Value::Object(getter)),
        enumerable: Some(false),
        configurable: Some(true),
        ..PropertyDescriptor::default()
    };
    if heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_own_property(key, &descriptor),
        _ => unreachable!(),
    })? != DefineOutcome::Done
    {
        return None;
    }
    let key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_with(
            key,
            Value::String(JsString::from("Intl.NumberFormat")),
            PropertyFlags::FUNCTION_INFO,
        ),
        _ => false,
    })?
    .then_some(())?;
    let bound = b.function(Builtin::IntlNumberFormatBound, &[])?;
    Some((prototype, bound))
}

/// Define `name` on an intrinsic under construction.
fn define(
    heap: &Heap,
    object: &mut OrdinaryObject,
    name: &str,
    value: ObjectId,
    flags: PropertyFlags,
) -> Option<()> {
    let key = heap.keys().intern(&JsString::from(name))?;
    object
        .define_with(key, Value::Object(value), flags)
        .then_some(())
}

/// The realm under construction.
struct Build<'h> {
    heap: &'h Heap,
    /// Every intrinsic allocated so far.
    all: Vec<ObjectId>,
    /// `Function.prototype`, once allocated.
    function_prototype: Option<ObjectId>,
}

impl Build<'_> {
    /// A built-in function inheriting from `Function.prototype`.
    fn function(
        &mut self,
        which: Builtin,
        extra: &[(&str, ObjectId, PropertyFlags)],
    ) -> Option<ObjectId> {
        let prototype = self.function_prototype?;
        self.builtin(Some(prototype), which, extra)
    }

    /// Allocate the built-in function `which` with prototype `prototype`,
    /// its `length` and `name` and the further own properties `extra`.
    fn builtin(
        &mut self,
        prototype: Option<ObjectId>,
        which: Builtin,
        extra: &[(&str, ObjectId, PropertyFlags)],
    ) -> Option<ObjectId> {
        let heap = self.heap;
        let mut properties = OrdinaryObject::intrinsic(prototype);
        let info = [
            ("length", Value::Number(f64::from(which.length()))),
            ("name", Value::String(JsString::from(which.name()))),
        ];
        for (name, value) in info {
            let key = heap.keys().intern(&JsString::from(name))?;
            properties
                .define_with(key, value, PropertyFlags::FUNCTION_INFO)
                .then_some(())?;
        }
        for &(name, value, flags) in extra {
            define(heap, &mut properties, name, value, flags)?;
        }
        let id = heap.alloc(HeapObject::Builtin(BuiltinFunction { which, properties }))?;
        self.all.push(id);
        Some(id)
    }

    /// Define `name` on the allocated intrinsic `id` (an ordinary object
    /// or a built-in function), for the edges that close a cycle.
    fn add(&self, id: ObjectId, name: &str, value: ObjectId, flags: PropertyFlags) -> Option<()> {
        self.add_value(id, name, Value::Object(value), flags)
    }

    /// Define a primitive or intrinsic value before the realm is published.
    fn add_value(
        &self,
        id: ObjectId,
        name: &str,
        value: Value,
        flags: PropertyFlags,
    ) -> Option<()> {
        let key = self.heap.keys().intern(&JsString::from(name))?;
        let defined = self.heap.with_object_mut(id, |o| match o {
            HeapObject::Plain(o) => o.define_with(key, value, flags),
            HeapObject::Builtin(f) => f.properties.define_with(key, value, flags),
            _ => false,
        });
        (defined == Some(true)).then_some(())
    }
}

/// The abstract %TypedArray% and its shared accessor prototype, followed by
/// one constructor/prototype pair for each element type.
fn typed_array_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
) -> Option<([ObjectId; 10], [ObjectId; 10])> {
    use crate::object::typed_array::ElementType;
    let heap = b.heap;
    let shared = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(shared);
    let abstract_constructor = b.function(
        Builtin::TypedArray,
        &[("prototype", shared, PropertyFlags::FIXED)],
    )?;
    b.add(
        shared,
        "constructor",
        abstract_constructor,
        PropertyFlags::METHOD,
    )?;
    for (name, which) in [
        ("buffer", Builtin::TypedArrayBuffer),
        ("length", Builtin::TypedArrayLength),
        ("byteLength", Builtin::TypedArrayByteLength),
        ("byteOffset", Builtin::TypedArrayByteOffset),
    ] {
        let getter = b.function(which, &[])?;
        let key = heap.keys().intern(&JsString::from(name))?;
        let desc = PropertyDescriptor {
            get: Some(Value::Object(getter)),
            enumerable: Some(false),
            configurable: Some(true),
            ..PropertyDescriptor::default()
        };
        if heap.with_object_mut(shared, |o| match o {
            HeapObject::Plain(o) => o.define_own_property(key, &desc),
            _ => unreachable!(),
        })? != DefineOutcome::Done
        {
            return None;
        }
    }
    let mut prototypes = [shared; 10];
    let mut constructors = [abstract_constructor; 10];
    for (index, kind) in ElementType::ALL.into_iter().enumerate() {
        let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(shared))))?;
        b.all.push(prototype);
        let constructor = b.builtin(
            Some(abstract_constructor),
            Builtin::IntegerTypedArray(kind),
            &[("prototype", prototype, PropertyFlags::FIXED)],
        )?;
        b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
        for id in [constructor, prototype] {
            b.add_value(
                id,
                "BYTES_PER_ELEMENT",
                Value::Number(kind.size() as f64),
                PropertyFlags::FIXED,
            )?;
        }
        prototypes[index] = prototype;
        constructors[index] = constructor;
    }
    Some((prototypes, constructors))
}
/// RegExp's shared immutable prototype and constructor (clj-quickjs-188).
fn regexp_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
) -> Option<(ObjectId, ObjectId)> {
    let prototype = b
        .heap
        .alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
            object_prototype,
        ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::RegExp,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    for (name, which) in [
        ("exec", Builtin::RegExpExec),
        ("test", Builtin::RegExpTest),
        ("toString", Builtin::RegExpToString),
    ] {
        let f = b.function(which, &[])?;
        b.add(prototype, name, f, PropertyFlags::METHOD)?;
    }
    let mut getters = vec![
        ("source", Builtin::RegExpSource),
        ("flags", Builtin::RegExpFlags),
    ];
    getters.extend(
        crate::regexp::object::FLAG_NAMES
            .iter()
            .map(|&(name, c)| (name, Builtin::RegExpFlag(c))),
    );
    for (name, which) in getters {
        let f = b.function(which, &[])?;
        let key = b.heap.keys().intern(&JsString::from(name))?;
        let desc = PropertyDescriptor {
            get: Some(Value::Object(f)),
            enumerable: Some(false),
            configurable: Some(true),
            ..PropertyDescriptor::default()
        };
        let ok = b.heap.with_object_mut(prototype, |o| match o {
            HeapObject::Plain(o) => o.define_own_property(key, &desc) == DefineOutcome::Done,
            _ => false,
        })?;
        if !ok {
            return None;
        }
    }
    Some((prototype, constructor))
}

fn intl_plural_rules_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
    intl: ObjectId,
) -> Option<ObjectId> {
    let heap = b.heap;
    let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::IntlPluralRules,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    b.add(intl, "PluralRules", constructor, PropertyFlags::METHOD)?;
    let supported = b.function(Builtin::IntlPluralRulesSupportedLocales, &[])?;
    b.add(
        constructor,
        "supportedLocalesOf",
        supported,
        PropertyFlags::METHOD,
    )?;
    for (name, which) in [
        ("select", Builtin::IntlPluralRulesSelect),
        ("resolvedOptions", Builtin::IntlPluralRulesResolvedOptions),
    ] {
        let method = b.function(which, &[])?;
        b.add(prototype, name, method, PropertyFlags::METHOD)?;
    }
    let key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_with(
            key,
            Value::String(JsString::from("Intl.PluralRules")),
            PropertyFlags::FUNCTION_INFO,
        ),
        _ => false,
    })?
    .then_some(())?;
    Some(prototype)
}

fn intl_collator_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
    intl: ObjectId,
) -> Option<(ObjectId, ObjectId)> {
    let heap = b.heap;
    let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::IntlCollator,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    b.add(intl, "Collator", constructor, PropertyFlags::METHOD)?;
    let supported = b.function(Builtin::IntlCollatorSupportedLocales, &[])?;
    b.add(
        constructor,
        "supportedLocalesOf",
        supported,
        PropertyFlags::METHOD,
    )?;
    let resolved = b.function(Builtin::IntlCollatorResolvedOptions, &[])?;
    b.add(
        prototype,
        "resolvedOptions",
        resolved,
        PropertyFlags::METHOD,
    )?;
    let getter = b.function(Builtin::IntlCollatorGetCompare, &[])?;
    let key = heap.keys().intern(&JsString::from("compare"))?;
    let descriptor = PropertyDescriptor {
        get: Some(Value::Object(getter)),
        enumerable: Some(false),
        configurable: Some(true),
        ..PropertyDescriptor::default()
    };
    if heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_own_property(key, &descriptor),
        _ => unreachable!(),
    })? != DefineOutcome::Done
    {
        return None;
    }
    let key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_with(
            key,
            Value::String(JsString::from("Intl.Collator")),
            PropertyFlags::FUNCTION_INFO,
        ),
        _ => false,
    })?
    .then_some(())?;
    let bound = b.function(Builtin::IntlCollatorBound, &[])?;
    Some((prototype, bound))
}

/// WeakMap has an ordinary prototype, no size, iteration or clear method.
fn weak_map_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
) -> Option<(ObjectId, ObjectId)> {
    let prototype = b
        .heap
        .alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
            object_prototype,
        ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::WeakMap,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    for which in [
        Builtin::WeakMapSet,
        Builtin::WeakMapGet,
        Builtin::WeakMapHas,
        Builtin::WeakMapDelete,
    ] {
        let method = b.function(which, &[])?;
        b.add(prototype, which.name(), method, PropertyFlags::METHOD)?;
    }
    let tag_key = b
        .heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    b.heap
        .with_object_mut(prototype, |o| match o {
            HeapObject::Plain(o) => o.define_with(
                tag_key,
                Value::String(JsString::from("WeakMap")),
                PropertyFlags::FUNCTION_INFO,
            ),
            _ => false,
        })?
        .then_some(())?;
    Some((prototype, constructor))
}

fn intl_date_time_format_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
    intl: ObjectId,
) -> Option<(ObjectId, ObjectId)> {
    let heap = b.heap;
    let prototype = heap.alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
        object_prototype,
    ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::IntlDateTimeFormat,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    b.add(intl, "DateTimeFormat", constructor, PropertyFlags::METHOD)?;
    let supported = b.function(Builtin::IntlDateTimeFormatSupportedLocales, &[])?;
    b.add(
        constructor,
        "supportedLocalesOf",
        supported,
        PropertyFlags::METHOD,
    )?;
    let resolved = b.function(Builtin::IntlDateTimeFormatResolvedOptions, &[])?;
    b.add(
        prototype,
        "resolvedOptions",
        resolved,
        PropertyFlags::METHOD,
    )?;
    let getter = b.function(Builtin::IntlDateTimeFormatGetFormat, &[])?;
    let key = heap.keys().intern(&JsString::from("format"))?;
    let descriptor = PropertyDescriptor {
        get: Some(Value::Object(getter)),
        enumerable: Some(false),
        configurable: Some(true),
        ..PropertyDescriptor::default()
    };
    if heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_own_property(key, &descriptor),
        _ => unreachable!(),
    })? != DefineOutcome::Done
    {
        return None;
    }
    let key = heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    heap.with_object_mut(prototype, |o| match o {
        HeapObject::Plain(o) => o.define_with(
            key,
            Value::String(JsString::from("Intl.DateTimeFormat")),
            PropertyFlags::FUNCTION_INFO,
        ),
        _ => false,
    })?
    .then_some(())?;
    let bound = b.function(Builtin::IntlDateTimeFormatBound, &[])?;
    Some((prototype, bound))
}

/// WeakRef's ordinary prototype is not itself a branded WeakRef.
fn weak_ref_intrinsics(
    b: &mut Build<'_>,
    object_prototype: ObjectId,
) -> Option<(ObjectId, ObjectId)> {
    let prototype = b
        .heap
        .alloc(HeapObject::Plain(OrdinaryObject::intrinsic(Some(
            object_prototype,
        ))))?;
    b.all.push(prototype);
    let constructor = b.function(
        Builtin::WeakRef,
        &[("prototype", prototype, PropertyFlags::FIXED)],
    )?;
    b.add(prototype, "constructor", constructor, PropertyFlags::METHOD)?;
    let deref = b.function(Builtin::WeakRefDeref, &[])?;
    b.add(prototype, "deref", deref, PropertyFlags::METHOD)?;
    let tag_key = b
        .heap
        .keys()
        .intern_symbol(&crate::value::symbol::well_known("toStringTag"))?;
    b.heap
        .with_object_mut(prototype, |o| match o {
            HeapObject::Plain(o) => o.define_with(
                tag_key,
                Value::String(JsString::from("WeakRef")),
                PropertyFlags::FUNCTION_INFO,
            ),
            _ => false,
        })?
        .then_some(())?;
    Some((prototype, constructor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::share::{Shareability, classify};

    #[test]
    fn global_declarations_validate_and_preserve_properties() {
        use crate::ids::TaskId;
        use crate::vm::dispatch::Fault;
        let heap = Heap::default();
        let id = new_global_object(&heap, TaskId::from_raw(1)).unwrap();
        let name = JsString::from("locked");
        let key = heap.keys().intern(&name).unwrap();
        heap.with_object_mut(id, |object| {
            let HeapObject::Plain(object) = object else {
                panic!()
            };
            object.define_with(key, Value::Number(7.0), PropertyFlags::FIXED);
        })
        .unwrap();
        assert!(declare_global(&heap, id, &name, false).is_ok());
        assert!(matches!(
            declare_global(&heap, id, &name, true),
            Err(Fault::SyntaxError(_))
        ));
        heap.read(id, |object| {
            let HeapObject::Plain(object) = object else {
                panic!()
            };
            let property = object.get_own(key).unwrap();
            assert_eq!(property.value, Value::Number(7.0));
            assert_eq!(property.flags, PropertyFlags::FIXED);
        })
        .unwrap();
        let name = JsString::from("Object");
        assert!(declare_global(&heap, id, &name, true).is_ok());
        let key = heap.keys().lookup(&name).unwrap();
        heap.with_object_mut(id, |object| {
            let HeapObject::Plain(object) = object else {
                panic!()
            };
            let property = object.get_own(key).unwrap();
            assert!(property.flags.writable && property.flags.enumerable);
            assert!(!property.flags.configurable);
            object.prevent_extensions();
        })
        .unwrap();
        assert!(matches!(
            declare_global(&heap, id, &JsString::from("absent"), false),
            Err(Fault::SyntaxError(_))
        ));
    }

    #[test]
    fn global_declarations_preserve_or_replace_accessors() {
        use crate::object::{DefineOutcome, PropertyDescriptor};
        use crate::vm::dispatch::Fault;
        let heap = Heap::default();
        let id = new_global_object(&heap, crate::ids::TaskId::from_raw(1)).unwrap();
        for configurable in [false, true] {
            let name = JsString::from(if configurable { "replaceable" } else { "fixed" });
            let key = heap.keys().intern(&name).unwrap();
            heap.with_object_mut(id, |object| {
                let HeapObject::Plain(object) = object else {
                    panic!()
                };
                assert_eq!(
                    object.define_own_property(
                        key,
                        &PropertyDescriptor {
                            get: Some(Value::Undefined),
                            enumerable: Some(true),
                            configurable: Some(configurable),
                            ..PropertyDescriptor::default()
                        }
                    ),
                    DefineOutcome::Done
                );
            })
            .unwrap();
            assert!(declare_global(&heap, id, &name, false).is_ok());
            assert!(
                heap.read(id, |object| {
                    let HeapObject::Plain(object) = object else {
                        panic!()
                    };
                    object.get_own(key).unwrap().accessor.is_some()
                })
                .unwrap(),
                "var preserves an existing accessor"
            );
            let result = declare_global(&heap, id, &name, true);
            if configurable {
                assert!(result.is_ok());
                heap.read(id, |object| {
                    let HeapObject::Plain(object) = object else {
                        panic!()
                    };
                    let p = object.get_own(key).unwrap();
                    assert!(p.accessor.is_none());
                    assert!(p.flags.writable && p.flags.enumerable && !p.flags.configurable);
                })
                .unwrap();
            } else {
                assert!(matches!(result, Err(Fault::SyntaxError(_))));
            }
        }
    }

    #[test]
    fn the_realm_is_built_once_and_its_objects_are_shareable() {
        let heap = Heap::default();
        assert!(heap.realm().get().is_none());
        let first = intrinsics(&heap).unwrap();
        let (prototype, object) = (first.object_prototype, first.object);
        let again = intrinsics(&heap).unwrap();
        assert_eq!((again.object_prototype, again.object), (prototype, object));
        assert_eq!(first.all.len(), 284); // Includes WeakRef (-176) and Symbol.prototype with four methods (-129).
        for &id in &first.all {
            assert_eq!(heap.known_shareable(id), Some(true));
            let category = heap.read(id, classify).unwrap();
            assert_eq!(category, Shareability::Persistent);
        }
        assert_eq!(Global::from_name("Object"), Some(Global::Object));
        assert_eq!(Global::from_name("object"), None);
        assert_eq!(Global::from_name("Function"), Some(Global::Function));
        assert_eq!(Global::from_name("Reflect"), Some(Global::Reflect));
        assert_eq!(Global::from_u16(0), Some(Global::Object));
        assert_eq!(Global::from_name("Number"), Some(Global::Number));
        assert_eq!(Global::from_name("String"), Some(Global::String));
        assert_eq!(Global::from_u16(2), Some(Global::String));
        assert_eq!(Global::from_u16(3), Some(Global::Function));
        assert_eq!(Global::from_u16(4), Some(Global::Reflect));
        assert_eq!(Global::from_name("Array"), Some(Global::Array));
        assert_eq!(Global::from_u16(5), Some(Global::Array));
        assert_eq!(Global::from_name("isNaN"), Some(Global::IsNaN));
        assert_eq!(Global::from_name("isFinite"), Some(Global::IsFinite));
        assert_eq!(Global::from_u16(6), Some(Global::IsNaN));
        assert_eq!(Global::from_u16(7), Some(Global::IsFinite));

        assert_eq!(Global::from_name("parseInt"), Some(Global::ParseInt));
        assert_eq!(Global::from_name("parseFloat"), Some(Global::ParseFloat));
        assert_eq!(Global::from_u16(8), Some(Global::ParseInt));
        assert_eq!(Global::from_u16(9), Some(Global::ParseFloat));
        assert_eq!(Global::from_name("Math"), Some(Global::Math));
        assert_eq!(Global::from_u16(10), Some(Global::Math));
        assert_eq!(Global::from_name("Error"), Some(Global::Error));
        assert_eq!(Global::from_u16(11), Some(Global::Error));
        for (offset, kind) in NativeErrorKind::ALL.into_iter().enumerate() {
            assert_eq!(
                Global::from_u16(12 + offset as u16),
                Global::from_name(kind.name())
            );
        }
        assert_eq!(Global::from_u16(19), Some(Global::Json));

        assert_eq!(Global::from_name("Symbol"), Some(Global::Symbol));
        assert_eq!(Global::from_u16(20), Some(Global::Symbol));
        assert_eq!(Global::from_name("Map"), Some(Global::Map));
        assert_eq!(Global::from_u16(21), Some(Global::Map));

        assert_eq!(Global::from_u16(22), Some(Global::ArrayBuffer));

        assert_eq!(Global::from_name("Intl"), Some(Global::Intl));
        assert_eq!(Global::from_u16(23), Some(Global::Intl));
        assert_eq!(Global::from_u16(24), Some(Global::Int8Array));
        assert_eq!(Global::from_u16(30), Some(Global::Uint32Array));

        assert_eq!(Global::from_name("Date"), Some(Global::Date));
        assert_eq!(Global::from_u16(31), Some(Global::Date));
        assert_eq!(Global::from_u16(32), Some(Global::Float16Array));
        assert_eq!(Global::from_u16(33), Some(Global::Float32Array));
        assert_eq!(Global::from_u16(34), Some(Global::Float64Array));
        assert_eq!(Global::from_name("RegExp"), Some(Global::RegExp));
        assert_eq!(Global::from_u16(35), Some(Global::RegExp));

        assert_eq!(Global::from_u16(36), Some(Global::BigInt));
        assert_eq!(Global::from_name("Promise"), Some(Global::Promise));
        assert_eq!(Global::from_u16(37), Some(Global::Promise));
        assert_eq!(Global::from_name("WeakMap"), Some(Global::WeakMap));
        assert_eq!(Global::from_u16(38), Some(Global::WeakMap));
        assert_eq!(Global::from_name("eval"), Some(Global::Eval));
        assert_eq!(Global::from_u16(39), Some(Global::Eval));
        assert_eq!(Global::from_u16(40), Some(Global::WeakRef));
        assert_eq!(Global::from_name("WeakRef"), Some(Global::WeakRef));
        assert_eq!(Global::from_u16(41), None);
    }

    /// Built-ins inherit from Function.prototype, except NativeErrors whose
    /// constructor prototype is Error (clj-quickjs-229).
    #[test]
    fn built_in_functions_inherit_from_function_prototype() {
        let heap = Heap::default();
        let intrinsics = intrinsics(&heap).unwrap();
        let fp = intrinsics.function_prototype;
        for view in &intrinsics.views {
            let builtin = heap
                .read(view.id, |o| matches!(o, HeapObject::Builtin(_)))
                .unwrap();
            let expected = if view.id == fp {
                Some(intrinsics.object_prototype)
            } else if intrinsics
                .native_errors
                .iter()
                .any(|&(id, _)| id == view.id)
            {
                Some(intrinsics.error)
            } else if intrinsics.typed_array_constructors.contains(&view.id) {
                intrinsics
                    .view(intrinsics.typed_array_constructors[0])
                    .unwrap()
                    .prototype
            } else if builtin {
                Some(fp)
            } else {
                view.prototype
            };
            assert_eq!(view.prototype, expected, "{view:?}");
            assert_eq!(view.function_prototype, view.id == fp);
        }
    }

    #[test]
    fn realm_views_hold_only_intrinsics() {
        let heap = Heap::default();
        let intrinsics = intrinsics(&heap).unwrap();
        assert_eq!(intrinsics.views.len(), intrinsics.all.len());
        for view in &intrinsics.views {
            assert!(intrinsics.all.contains(&view.id));
            for (_, property) in &view.properties {
                if let Value::Object(id) = property.value {
                    assert!(intrinsics.all.contains(&id), "{view:?}");
                }
                if let Some((get, set)) = &property.accessor {
                    for value in [get, set] {
                        if let Value::Object(id) = value {
                            assert!(intrinsics.all.contains(id), "{view:?}");
                        }
                    }
                }
            }
            // The view equals the object's own properties.
            let count = heap
                .read(view.id, |o| match o {
                    HeapObject::Plain(o) => o.properties().count(),
                    HeapObject::Builtin(f) => f.properties.properties().count(),
                    _ => usize::MAX,
                })
                .unwrap();
            assert_eq!(count, view.properties.len());
        }
    }
}
