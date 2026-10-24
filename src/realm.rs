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
//! | `Object.prototype` | ordinary object (`HeapObject::Plain`), null prototype | `constructor` (`Object`), `toString`, `valueOf`; the `__proto__` accessor is applied by the property walk, not stored (clj-quickjs-121, `crate::vm::prototype`) |
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
//! | `Array.prototype` (clj-quickjs-118) | ordinary object, prototype `Object.prototype` | `length` (0), `constructor` (`Array`), `toString`, `push` (clj-quickjs-155) |
//! | `Array.prototype.toString` | built-in function ([`Builtin::ArrayPrototypeToString`]) | `length` (0), `name` |
//! | `Array.prototype.push` | built-in function ([`Builtin::ArrayPrototypePush`]) | `length` (1), `name` |
//! | `Array` (global name, clj-quickjs-155; fails when called, clj-quickjs-39) | built-in function ([`Builtin::Array`]) | `length` (1), `name`, `prototype` (`Array.prototype`) |
//! | `Number.prototype`, `String.prototype` (clj-quickjs-73) | ordinary objects, prototype `Object.prototype`; where a property read on a number or string primitive starts ([`Intrinsics::primitive_prototype`]) | `constructor` (`Number` / `String`), `toString`, `valueOf`; `String.prototype` also `length` (0, not writable, enumerable or configurable) |
//! | `Number.prototype.toString`, `.valueOf`; `String.prototype.toString`, `.valueOf` | built-in functions | `length` (1 for `Number.prototype.toString`, else 0), `name` |
//! | `Number`, `String` (callable and constructible, clj-quickjs-72) | built-in functions ([`Builtin::Number`], [`Builtin::String`]) | `length` (1), `name`, `prototype` |
//! | String code-unit methods (clj-quickjs-143) | built-in functions | `String.prototype` has `charAt`, `charCodeAt`, `codePointAt`; `String` has `fromCharCode`, `fromCodePoint`; all have `length` (1), `name` |
//! | `Function.prototype` (clj-quickjs-102/-103) | built-in function ([`Builtin::FunctionPrototype`]), prototype `Object.prototype` | `length` (0), `name` (`""`), `constructor` (`Function`), `call`, `apply`, `bind`; `toString`, `arguments` and `caller` are missing and their lookup fails (`crate::vm::function::lacks_inherited`) |
//! | `Function.prototype.call`, `Function.prototype.apply` | built-in functions | `length` (1, 2), `name` |
//! | `Function` (fails when called, clj-quickjs-111) | built-in function ([`Builtin::Function`]) | `length` (1), `name`, `prototype` (`Function.prototype`) |
//! | `Reflect` (clj-quickjs-102) | ordinary object, prototype `Object.prototype` | `apply` |
//! | `Reflect.apply` | built-in function | `length` (3), `name` |
//!
//! Every built-in function inherits from `Function.prototype`, which
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
//! # Built-in global names
//!
//! [`Global`] lists the names a free identifier resolves to after every
//! lexical scope and before the `ReferenceError` (`crate::frontend::lower`):
//! `Object`, `Number` and `String` (clj-quickjs-73), `Function` and `Reflect` (clj-quickjs-102), and `Array` (clj-quickjs-155). The compiler emits `get_builtin_global i` for such a
//! name. When the global object exists (clj-quickjs-104) these become its
//! properties; until then assigning one is a compile-time `Unsupported`
//! error.

use std::sync::{Mutex, OnceLock, PoisonError};

use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::object::{OrdinaryObject, PrimitiveData, Property, PropertyFlags, PropertyKey};
use crate::value::{JsString, Value};
use crate::vm::builtin::{Builtin, BuiltinFunction};

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
    /// `Array` (clj-quickjs-155): exposes its prototype; calls remain unsupported.
    Array = 5,
}

impl Global {
    /// Every built-in global, in operand order.
    const ALL: [Global; 6] = [
        Global::Object,
        Global::Number,
        Global::String,
        Global::Function,
        Global::Reflect,
        Global::Array,
    ];

    /// The JavaScript name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Global::Object => "Object",
            Global::Number => "Number",
            Global::String => "String",
            Global::Function => "Function",
            Global::Reflect => "Reflect",
            Global::Array => "Array",
        }
    }

    /// The built-in global named `name`, if any.
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
    /// `Object.prototype`.
    pub(crate) object_prototype: ObjectId,
    /// The `Object` constructor.
    pub(crate) object: ObjectId,
    /// `Array.prototype`, the prototype of every array (clj-quickjs-118).
    pub(crate) array_prototype: ObjectId,
    /// The `Array` stub constructor, also reachable as a global.
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
    /// The interned key `__proto__`: the name of `Object.prototype`'s
    /// `__proto__` accessor, which the property walk applies
    /// (clj-quickjs-121, `crate::vm::prototype`).
    proto_key: PropertyKey,
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
    /// The property view of `id` if it is an intrinsic: a scan of the few
    /// intrinsic identities, no lock.
    pub(crate) fn view(&self, id: ObjectId) -> Option<&IntrinsicView> {
        self.views.iter().find(|v| v.id == id)
    }

    /// True when a property access of `key` that reached the intrinsic `id`
    /// without finding an own property first meets `Object.prototype`'s
    /// `__proto__` accessor (`crate::vm::prototype`, "`Object.prototype.__proto__`").
    pub(crate) fn is_proto_accessor(&self, id: ObjectId, key: PropertyKey) -> bool {
        id == self.object_prototype && key == self.proto_key
    }

    /// True when `key` is the interned name `__proto__`.
    pub(crate) fn is_proto_key(&self, key: PropertyKey) -> bool {
        key == self.proto_key
    }

    /// The prototype a property access on the primitive `v` starts at, as
    /// if on its wrapper object, which is not allocated (clj-quickjs-73;
    /// QuickJS `JS_GetPrototypePrimitive`): `Number.prototype` for a
    /// number, `String.prototype` for a string. `None` for anything else
    /// (objects, `undefined`, `null`, and booleans until `Boolean.prototype`
    /// exists, clj-quickjs-39).
    pub(crate) fn primitive_prototype(&self, v: &Value) -> Option<ObjectId> {
        match v {
            Value::Number(_) => Some(self.number_prototype),
            Value::String(_) => Some(self.string_prototype),
            _ => None,
        }
    }

    /// The value of a built-in global.
    pub(crate) fn global(&self, global: Global) -> Value {
        match global {
            Global::Object => Value::Object(self.object),
            Global::Number => Value::Object(self.number),
            Global::String => Value::Object(self.string),
            Global::Function => Value::Object(self.function),
            Global::Reflect => Value::Object(self.reflect),
            Global::Array => Value::Object(self.array),
        }
    }
}

/// Report every intrinsic object.
impl Trace for Intrinsics {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        // The views copy property values of these objects, which are
        // intrinsics (listed here) or primitives, so they need no report of
        // their own (`realm_views_hold_only_intrinsics` checks it).
        self.all.iter().copied().for_each(visit);
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
    ] {
        let method = b.function(which, &[])?;
        b.add(
            string_prototype,
            which.name(),
            method,
            PropertyFlags::METHOD,
        )?;
    }
    for which in [Builtin::StringFromCharCode, Builtin::StringFromCodePoint] {
        let method = b.function(which, &[])?;
        b.add(string, which.name(), method, PropertyFlags::METHOD)?;
    }
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
    let proto_key = heap.keys().intern(&JsString::from("__proto__"))?;
    Some(Intrinsics {
        object_prototype,
        object,
        array_prototype,
        array,
        function_prototype,
        function,
        reflect,
        all,
        views,
        proto_key,
        number_prototype,
        string_prototype,
        number,
        string,
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
/// not enumerable, not configurable, as an array's) and a `toString` that
/// fails explicitly until `Array.prototype.join` exists (clj-quickjs-39), so
/// `[].toString()` never answers `Object.prototype.toString`'s
/// `"[object Array]"`. ECMAScript makes it an array exotic object itself;
/// that is not observable while no code can write to it. Its
/// `constructor` is the `Array` constructor stub (also a global name,
/// clj-quickjs-155). `push` is supported; calling the constructor and the
/// remaining methods belong to clj-quickjs-39 and -153..-156.
fn array_prototype(b: &mut Build<'_>, object_prototype: ObjectId) -> Option<(ObjectId, ObjectId)> {
    let heap = b.heap;
    let to_string = b.function(Builtin::ArrayPrototypeToString, &[])?;
    let push = b.function(Builtin::ArrayPrototypePush, &[])?;
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
    let id = heap.alloc(HeapObject::Plain(prototype))?;
    b.all.push(id);
    // `Array.prototype.constructor` is the `Array` constructor, a stub
    // that fails when called until clj-quickjs-39 (so `[].constructor` is
    // not the inherited `Object`).
    let array = b.function(Builtin::Array, &[("prototype", id, PropertyFlags::FIXED)])?;
    b.add(id, "constructor", array, PropertyFlags::METHOD)?;
    Some((id, array))
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
        let key = self.heap.keys().intern(&JsString::from(name))?;
        let defined = self.heap.with_object_mut(id, |o| match o {
            HeapObject::Plain(o) => o.define_with(key, Value::Object(value), flags),
            HeapObject::Builtin(f) => f.properties.define_with(key, Value::Object(value), flags),
            _ => false,
        });
        (defined == Some(true)).then_some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::share::{Shareability, classify};

    #[test]
    fn the_realm_is_built_once_and_its_objects_are_shareable() {
        let heap = Heap::default();
        assert!(heap.realm().get().is_none());
        let first = intrinsics(&heap).unwrap();
        let (prototype, object) = (first.object_prototype, first.object);
        let again = intrinsics(&heap).unwrap();
        assert_eq!((again.object_prototype, again.object), (prototype, object));
        assert_eq!(first.all.len(), 40);
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
        assert_eq!(Global::from_u16(6), None);
    }

    /// Every built-in function inherits from `Function.prototype`, which
    /// inherits from `Object.prototype` (clj-quickjs-102).
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
