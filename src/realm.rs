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
//!
//! Property attributes are the standard ones (ECMAScript §18): methods and
//! `constructor` are writable, non-enumerable and configurable; `length` and
//! `name` are non-writable, non-enumerable and configurable; a constructor's
//! `prototype` has no attribute set.
//!
//! # Shared and immutable
//!
//! An intrinsic object has no owner task and is marked intrinsic
//! ([`OrdinaryObject::intrinsic`]). Every task reads it without a lock of
//! its own (each read takes the heap's one short object-table lock, like any
//! property read) and no task changes it: a write to any of its properties
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
use crate::object::{OrdinaryObject, PropertyFlags};
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
    /// Every intrinsic object (the collector's roots).
    all: Vec<ObjectId>,
}

impl Intrinsics {
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
    Some(Intrinsics {
        object_prototype,
        object,
        all,
    })
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
        assert_eq!(first.all.len(), 5);
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
}
