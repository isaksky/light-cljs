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
//! the prototype of a literal, or `GetBuiltinGlobal`) and kept until the heap
//! is dropped. Every task of the runtime sees the same intrinsic objects,
//! with the same identities (`({}).toString === Object.prototype.toString`
//! in every task, and a closure that captured `Object` in one task calls the
//! same function in another).
//!
//! The intrinsics are:
//!
//! | Object | Kind | Own properties |
//! |--------|------|----------------|
//! | `Object.prototype` | ordinary object (`HeapObject::Plain`), null prototype | `constructor` (`Object`), `toString`, `valueOf` |
//! | `Object` | built-in function ([`Builtin::Object`]) | `length` (1), `name`, `prototype` (`Object.prototype`), `is` |
//! | `Object.is` | built-in function | `length` (2), `name` |
//! | `Object.prototype.toString` | built-in function | `length` (0), `name` |
//! | `Object.prototype.valueOf` | built-in function | `length` (0), `name` |
//! | `Array.prototype` (clj-quickjs-118) | ordinary object, prototype `Object.prototype` | `length` (0), `constructor` (`Array`), `toString` |
//! | `Array.prototype.toString` | built-in function ([`Builtin::ArrayPrototypeToString`]) | `length` (0), `name` |
//! | `Array` (only as `Array.prototype.constructor`; fails when called, clj-quickjs-39) | built-in function ([`Builtin::Array`]) | `length` (1), `name`, `prototype` (`Array.prototype`) |
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
//! today only `Object`. The compiler emits `get_builtin_global i` for such a
//! name. When the global object exists (clj-quickjs-104) these become its
//! properties; until then assigning one is a compile-time `Unsupported`
//! error.

use std::sync::{Mutex, OnceLock, PoisonError};

use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::object::{OrdinaryObject, Property, PropertyFlags, PropertyKey};
use crate::value::{JsString, Value};
use crate::vm::builtin::{Builtin, BuiltinFunction};

/// A built-in global name (the `get_builtin_global` operand). Append-only:
/// the operand values are part of the instruction format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum Global {
    /// `Object`, the Object constructor.
    Object = 0,
}

impl Global {
    /// Every built-in global, in operand order.
    const ALL: [Global; 1] = [Global::Object];

    /// The JavaScript name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Global::Object => "Object",
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
    /// Every intrinsic object (the collector's roots).
    all: Vec<ObjectId>,
    /// A copy of every intrinsic's own properties and prototype, taken once
    /// the realm is complete: property reads that reach an intrinsic use it
    /// instead of the object table, so they take no lock (module docs,
    /// "Lock-free reads").
    views: Vec<IntrinsicView>,
}

/// The immutable property view of one intrinsic object.
#[derive(Debug)]
pub(crate) struct IntrinsicView {
    id: ObjectId,
    /// A built-in function rather than `Object.prototype`: it has only its
    /// own properties (`Function.prototype` does not exist yet).
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
}

impl Intrinsics {
    /// The property view of `id` if it is an intrinsic: a scan of the few
    /// intrinsic identities, no lock.
    pub(crate) fn view(&self, id: ObjectId) -> Option<&IntrinsicView> {
        self.views.iter().find(|v| v.id == id)
    }

    /// The value of a built-in global.
    pub(crate) fn global(&self, global: Global) -> Value {
        match global {
            Global::Object => Value::Object(self.object),
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
fn build(heap: &Heap) -> Option<Intrinsics> {
    let mut all = Vec::new();
    let to_string = builtin(heap, &mut all, Builtin::ObjectPrototypeToString, &[])?;
    let value_of = builtin(heap, &mut all, Builtin::ObjectPrototypeValueOf, &[])?;
    let is = builtin(heap, &mut all, Builtin::ObjectIs, &[])?;

    let mut prototype = OrdinaryObject::intrinsic(None);
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
    let object_prototype = heap.alloc(HeapObject::Plain(prototype))?;
    all.push(object_prototype);

    let object = builtin(
        heap,
        &mut all,
        Builtin::Object,
        &[
            ("prototype", object_prototype, PropertyFlags::FIXED),
            ("is", is, PropertyFlags::METHOD),
        ],
    )?;
    // The one cycle: `Object.prototype.constructor` is `Object`.
    let key = heap.keys().intern(&JsString::from("constructor"))?;
    let defined = heap.with_object_mut(object_prototype, |o| match o {
        HeapObject::Plain(o) => o.define_with(key, Value::Object(object), PropertyFlags::METHOD),
        _ => false,
    });
    if defined != Some(true) {
        return None;
    }
    let array_prototype = array_prototype(heap, &mut all, object_prototype)?;
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
                    builtin,
                    prototype: store.prototype(),
                    properties: store.properties().map(|(k, p)| (k, p.clone())).collect(),
                })
            })
            .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Intrinsics {
        object_prototype,
        object,
        array_prototype,
        all,
        views,
    })
}

/// `Array.prototype` (clj-quickjs-118): an intrinsic ordinary object
/// inheriting from `Object.prototype`, with an own `length` of 0 (writable,
/// not enumerable, not configurable, as an array's) and a `toString` that
/// fails explicitly until `Array.prototype.join` exists (clj-quickjs-39), so
/// `[].toString()` never answers `Object.prototype.toString`'s
/// `"[object Array]"`. ECMAScript makes it an array exotic object itself;
/// that is not observable while no code can write to it. Its
/// `constructor` is the `Array` constructor, which is not a global name
/// yet and fails when called; it and the methods are clj-quickjs-39 and
/// -153..-156.
fn array_prototype(
    heap: &Heap,
    all: &mut Vec<ObjectId>,
    object_prototype: ObjectId,
) -> Option<ObjectId> {
    let to_string = builtin(heap, all, Builtin::ArrayPrototypeToString, &[])?;
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
    let id = heap.alloc(HeapObject::Plain(prototype))?;
    all.push(id);
    // `Array.prototype.constructor` is the `Array` constructor, a stub
    // that fails when called until clj-quickjs-39 (so `[].constructor` is
    // not the inherited `Object`).
    let array = builtin(
        heap,
        all,
        Builtin::Array,
        &[("prototype", id, PropertyFlags::FIXED)],
    )?;
    let key = heap.keys().intern(&JsString::from("constructor"))?;
    let defined = heap.with_object_mut(id, |o| match o {
        HeapObject::Plain(o) => o.define_with(key, Value::Object(array), PropertyFlags::METHOD),
        _ => false,
    });
    (defined == Some(true)).then_some(id)
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

/// Allocate the built-in function `which` with its `length` and `name` and
/// the further own properties `extra`.
fn builtin(
    heap: &Heap,
    all: &mut Vec<ObjectId>,
    which: Builtin,
    extra: &[(&str, ObjectId, PropertyFlags)],
) -> Option<ObjectId> {
    // Its [[Prototype]] is Function.prototype once that exists (module docs
    // of `crate::vm::builtin`).
    let mut properties = OrdinaryObject::intrinsic(None);
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
    all.push(id);
    Some(id)
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
        assert_eq!(first.all.len(), 8);
        for &id in &first.all {
            assert_eq!(heap.known_shareable(id), Some(true));
            let category = heap.read(id, classify).unwrap();
            assert_eq!(category, Shareability::Persistent);
        }
        assert_eq!(Global::from_name("Object"), Some(Global::Object));
        assert_eq!(Global::from_name("object"), None);
        assert_eq!(Global::from_u16(0), Some(Global::Object));
        assert_eq!(Global::from_u16(1), None);
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
