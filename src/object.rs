//! Ordinary JavaScript objects (clj-quickjs-114): task-owned mutable heap
//! state with own properties keyed by interned property keys.
//!
//! The decision record (representation, the alternatives considered and
//! their consequences) is `docs/architecture.md`, "Ordinary objects".
//!
//! # Representation
//!
//! An ordinary object is [`HeapObject::Plain`](crate::heap::HeapObject)
//! holding an [`OrdinaryObject`]:
//!
//! * **Owner.** The [`TaskId`] whose run created it, or `None` for an object
//!   the embedder created ([`Scope::new_object`](crate::Scope::new_object)).
//!   Only the owning task changes the object, and only the owning task reads
//!   its properties (as only the owner uses a binding cell): another task
//!   gets [`VmErrorKind::ForeignObject`](crate::VmErrorKind::ForeignObject).
//!   An embedder-created object has no owner task: every task may read it and
//!   none may change it. The object is classified
//!   [`Shareability::TaskOwned`](crate::value::share::Shareability), so the
//!   publication check rejects any value that reaches it.
//! * **Own properties**, in creation order: a vector of (key, property)
//!   entries, plus a hash index from key to position once the object has
//!   more than [`INDEX_THRESHOLD`] properties. A redefinition keeps the
//!   position of the first definition. Each property is a data property
//!   (a [`Value`] and its `writable` / `enumerable` / `configurable`
//!   [`PropertyFlags`]); accessor properties (clj-quickjs-120) replace the
//!   value with a getter/setter pair in the same entry. Assignment and
//!   literals create properties with all three attributes;
//!   [`OrdinaryObject::define_own_property`] (`Object.defineProperty`,
//!   clj-quickjs-119) creates and validates any combination.
//! * **`[[Prototype]]`**, an object or null: `Object.prototype` for an
//!   object literal or an `Object()` result (clj-quickjs-117,
//!   [`crate::realm`]), null for an object made by the `NewObject`
//!   instruction or by the embedder, and for `Object.prototype` itself. `Object.setPrototypeOf` and `__proto__` are
//!   clj-quickjs-121. Reads walk it; a write of a property the object does
//!   not have itself consults it (`crate::vm::property`).
//! * **Intrinsic**: true only for the realm's shared, immutable intrinsic
//!   objects ([`crate::realm`]): `Object.prototype` and the property storage
//!   of built-in functions. An intrinsic object has no owner, every task
//!   reads it, nothing changes it after the realm is built, and it is
//!   classified [`Shareability::Persistent`](crate::value::share::Shareability).
//! * **`[[Extensible]]`**: true until `Object.preventExtensions`
//!   (clj-quickjs-122); adding a property to a non-extensible object fails.
//!
//! Reads and writes take only the heap's object-table lock (one short
//! critical section per object visited), no per-object lock: only the owning
//! task's run ever touches an owned object's properties, so there is no
//! contention on the object itself.
//!
//! # Property keys
//!
//! A [`PropertyKey`] is a private 32-bit identity issued by the runtime-wide
//! [`PropertyKeys`] table, which interns each property name once. Keys
//! compare and hash by identity (derived Rust equality over the private id),
//! never by any user-defined comparison. A symbol key (clj-quickjs-127) is a
//! fresh id with no name entry. A name is interned when a property with it
//! is first defined; a read of a name that was never interned cannot find a
//! property and does not grow the table. Interned names are never removed
//! (see the decision record for the bound and the follow-up).
//!
//! These keys are QuickJS's `JSAtom`s (`research/quickjs/quickjs.c`,
//! revision 7d1c13c5d8f8, `__JS_NewAtom`); they are not called atoms here
//! because [`crate::atom`] is the concurrent reference.

pub(crate) mod array;

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use crate::atom::same_value;
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::value::{JsString, Value};

/// Interned identity of a property name: a private id issued by
/// [`PropertyKeys`]. Equal names in one runtime always have the same key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PropertyKey(u32);

/// The runtime-wide property-key table (one per heap). Its lock is a leaf:
/// it is never held while another heap lock is taken, nor taken while one
/// is held, and it is held only to look up or insert one name.
#[derive(Debug, Default)]
pub(crate) struct PropertyKeys {
    table: RwLock<KeyTable>,
}

#[derive(Debug, Default)]
struct KeyTable {
    ids: HashMap<JsString, PropertyKey>,
    names: Vec<JsString>,
}

impl PropertyKeys {
    /// The key of `name` if some property was ever defined with it. A
    /// shared read lock only.
    pub(crate) fn lookup(&self, name: &JsString) -> Option<PropertyKey> {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        table.ids.get(name).copied()
    }

    /// The key of `name`, interning it if new. `None` only when 2^32 names
    /// have been interned.
    pub(crate) fn intern(&self, name: &JsString) -> Option<PropertyKey> {
        if let Some(key) = self.lookup(name) {
            return Some(key);
        }
        let mut table = self.table.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(&key) = table.ids.get(name) {
            return Some(key);
        }
        let key = PropertyKey(u32::try_from(table.names.len()).ok()?);
        table.names.push(name.clone());
        table.ids.insert(name.clone(), key);
        Some(key)
    }

    /// The name `key` was interned for.
    pub(crate) fn name(&self, key: PropertyKey) -> Option<JsString> {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        table.names.get(key.0 as usize).cloned()
    }

    /// Number of interned names (tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.table
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .names
            .len()
    }
}

/// The attributes of a data property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PropertyFlags {
    pub(crate) writable: bool,
    pub(crate) enumerable: bool,
    pub(crate) configurable: bool,
}

impl PropertyFlags {
    /// The attributes of a property created by assignment or an object
    /// literal (CreateDataProperty): all three true.
    pub(crate) const DATA: PropertyFlags = PropertyFlags {
        writable: true,
        enumerable: true,
        configurable: true,
    };

    /// The attributes of a built-in method (ECMAScript §18, "ECMAScript
    /// Standard Built-in Objects"): writable, not enumerable, configurable.
    pub(crate) const METHOD: PropertyFlags = PropertyFlags {
        writable: true,
        enumerable: false,
        configurable: true,
    };

    /// The attributes of a built-in function's `length` and `name`: not
    /// writable, not enumerable, configurable.
    pub(crate) const FUNCTION_INFO: PropertyFlags = PropertyFlags {
        writable: false,
        enumerable: false,
        configurable: true,
    };

    /// The attributes of a constructor's `prototype`: none.
    pub(crate) const FIXED: PropertyFlags = PropertyFlags {
        writable: false,
        enumerable: false,
        configurable: false,
    };
}

/// One own property.
#[derive(Clone, Debug)]
pub(crate) struct Property {
    pub(crate) value: Value,
    pub(crate) flags: PropertyFlags,
}

/// Objects with more own properties than this get a key index; smaller ones
/// are searched linearly (fewer allocations and cache misses for the common
/// small object).
pub(crate) const INDEX_THRESHOLD: usize = 8;

/// What [`OrdinaryObject::set`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SetOutcome {
    /// An own writable property was updated, or a new one was added.
    Done,
    /// The own property is not writable (strict mode: TypeError).
    ReadOnly,
    /// The property is absent and the object is not extensible.
    NotExtensible,
    /// The property is absent and the object has a prototype: the
    /// prototype chain decides (an inherited read-only property forbids the
    /// write, otherwise [`OrdinaryObject::add`] adds an own property; see
    /// `crate::vm::property`).
    Inherited,
}

/// Storage of an ordinary object. See the module docs.
#[derive(Debug)]
pub(crate) struct OrdinaryObject {
    owner: Option<TaskId>,
    /// A realm intrinsic (module docs): ownerless, shared and immutable.
    intrinsic: bool,
    prototype: Option<ObjectId>,
    extensible: bool,
    entries: Vec<(PropertyKey, Property)>,
    /// Key to position in `entries`; present once there are more than
    /// [`INDEX_THRESHOLD`] entries.
    index: Option<HashMap<PropertyKey, u32>>,
}

impl OrdinaryObject {
    /// An empty, extensible object with a null prototype, owned by `owner`
    /// (`None`: created by the embedder).
    pub(crate) fn new(owner: Option<TaskId>) -> OrdinaryObject {
        OrdinaryObject {
            owner,
            intrinsic: false,
            prototype: None,
            extensible: true,
            entries: Vec::new(),
            index: None,
        }
    }

    /// An empty, extensible object owned by `owner` whose prototype is
    /// `prototype` (`Object.prototype` for literals and `Object()`).
    pub(crate) fn with_prototype(owner: TaskId, prototype: ObjectId) -> OrdinaryObject {
        OrdinaryObject {
            prototype: Some(prototype),
            ..OrdinaryObject::new(Some(owner))
        }
    }

    /// An empty realm intrinsic with `prototype` (`crate::realm`). The realm
    /// defines its properties with [`OrdinaryObject::define_with`] before
    /// any task can reach it; nothing changes it afterwards.
    pub(crate) fn intrinsic(prototype: Option<ObjectId>) -> OrdinaryObject {
        OrdinaryObject {
            intrinsic: true,
            prototype,
            ..OrdinaryObject::new(None)
        }
    }

    /// The task that owns the object; `None` if the embedder created it or
    /// it is a realm intrinsic.
    pub(crate) fn owner(&self) -> Option<TaskId> {
        self.owner
    }

    /// True for a realm intrinsic (module docs).
    pub(crate) fn is_intrinsic(&self) -> bool {
        self.intrinsic
    }

    /// The `[[Prototype]]`: an object, or `None` for null.
    pub(crate) fn prototype(&self) -> Option<ObjectId> {
        self.prototype
    }

    /// The `[[Extensible]]` flag.
    pub(crate) fn extensible(&self) -> bool {
        self.extensible
    }

    /// Set the `[[Prototype]]` (tests; no operation of this slice sets it).
    #[cfg(test)]
    pub(crate) fn set_prototype(&mut self, prototype: Option<ObjectId>) {
        self.prototype = prototype;
    }

    fn position(&self, key: PropertyKey) -> Option<usize> {
        match &self.index {
            Some(index) => index.get(&key).map(|&i| i as usize),
            None => self.entries.iter().position(|(k, _)| *k == key),
        }
    }

    /// The own property `key`, if any.
    pub(crate) fn get_own(&self, key: PropertyKey) -> Option<&Property> {
        self.position(key).map(|i| &self.entries[i].1)
    }

    /// Own properties in creation order.
    pub(crate) fn properties(&self) -> impl Iterator<Item = (PropertyKey, &Property)> {
        self.entries.iter().map(|(k, p)| (*k, p))
    }

    fn push(&mut self, key: PropertyKey, property: Property) {
        let at = self.entries.len();
        self.entries.push((key, property));
        if let Some(index) = &mut self.index {
            index.insert(key, at as u32);
        } else if self.entries.len() > INDEX_THRESHOLD {
            let index = self
                .entries
                .iter()
                .enumerate()
                .map(|(i, (k, _))| (*k, i as u32))
                .collect();
            self.index = Some(index);
        }
    }

    /// CreateDataProperty as an object literal uses it: define `key` as a
    /// writable, enumerable, configurable data property with `value`. An
    /// existing property keeps its position and is redefined (every own
    /// property of an object built by a literal is configurable). Returns
    /// false (nothing changed) if `key` is new and the object is not
    /// extensible.
    pub(crate) fn define(&mut self, key: PropertyKey, value: Value) -> bool {
        self.define_with(key, value, PropertyFlags::DATA)
    }

    /// [`OrdinaryObject::define`] with the attributes `flags` (the realm
    /// builds intrinsics with it).
    pub(crate) fn define_with(
        &mut self,
        key: PropertyKey,
        value: Value,
        flags: PropertyFlags,
    ) -> bool {
        let property = Property { value, flags };
        match self.position(key) {
            Some(i) => self.entries[i].1 = property,
            None if self.extensible => self.push(key, property),
            None => return false,
        }
        true
    }

    /// OrdinarySet on this object as the receiver, for the cases that do
    /// not depend on the prototype chain: update an own writable data
    /// property, or add a new one to an extensible object with a null
    /// prototype.
    pub(crate) fn set(&mut self, key: PropertyKey, value: Value) -> SetOutcome {
        match self.position(key) {
            Some(i) => {
                let property = &mut self.entries[i].1;
                if !property.flags.writable {
                    return SetOutcome::ReadOnly;
                }
                property.value = value;
                SetOutcome::Done
            }
            None if self.prototype.is_some() => SetOutcome::Inherited,
            None if !self.extensible => SetOutcome::NotExtensible,
            None => {
                let property = Property {
                    value,
                    flags: PropertyFlags::DATA,
                };
                self.push(key, property);
                SetOutcome::Done
            }
        }
    }

    /// The end of OrdinarySet once the prototype chain allowed the write of
    /// a property this object does not have (`SetOutcome::Inherited`): add
    /// it as a writable, enumerable, configurable data property. False
    /// (nothing changed) if the object is not extensible.
    pub(crate) fn add(&mut self, key: PropertyKey, value: Value) -> bool {
        if !self.extensible {
            return false;
        }
        debug_assert!(self.position(key).is_none(), "add of an own property");
        let property = Property {
            value,
            flags: PropertyFlags::DATA,
        };
        self.push(key, property);
        true
    }

    /// `[[DefineOwnProperty]]` (ECMAScript OrdinaryDefineOwnProperty and
    /// ValidateAndApplyPropertyDescriptor, clj-quickjs-119; QuickJS
    /// `JS_DefineProperty`): create `key` from `desc` (absent fields default
    /// to `undefined`/false) or update the existing property (absent fields
    /// are kept), unless the update is one a non-configurable property
    /// forbids. A rejected define changes nothing.
    pub(crate) fn define_own_property(
        &mut self,
        key: PropertyKey,
        desc: &PropertyDescriptor,
    ) -> DefineOutcome {
        let i = self.position(key);
        if i.is_none() && !self.extensible {
            return DefineOutcome::Rejected("the object is not extensible");
        }
        let current = i.map(|i| &self.entries[i].1);
        let property = match validate_and_apply(current, desc) {
            Ok(property) => property,
            Err(outcome) => return outcome,
        };
        match i {
            Some(i) => self.entries[i].1 = property,
            None => self.push(key, property),
        }
        DefineOutcome::Done
    }
}

/// ValidateAndApplyPropertyDescriptor for data properties (clj-quickjs-119),
/// without the extensibility check, which the caller makes: the property
/// `desc` makes of `current` (absent fields kept) or, for `None`, creates
/// (absent fields `undefined`/false), or why not. A non-configurable
/// `current` rejects becoming configurable, a change of `enumerable`, an
/// accessor, and, when it is also read-only, becoming writable or a value
/// that is not SameValue. An accessor descriptor otherwise is
/// [`DefineOutcome::Accessor`] (clj-quickjs-120). Arrays use it for their
/// elements (`crate::object::array`).
pub(crate) fn validate_and_apply(
    current: Option<&Property>,
    desc: &PropertyDescriptor,
) -> Result<Property, DefineOutcome> {
    let Some(current) = current else {
        if desc.is_accessor() {
            return Err(DefineOutcome::Accessor);
        }
        return Ok(Property {
            value: desc.value.clone().unwrap_or(Value::Undefined),
            flags: PropertyFlags {
                writable: desc.writable.unwrap_or(false),
                enumerable: desc.enumerable.unwrap_or(false),
                configurable: desc.configurable.unwrap_or(false),
            },
        });
    };
    let flags = current.flags;
    if !flags.configurable {
        if desc.configurable == Some(true) {
            return Err(DefineOutcome::Rejected("it is not configurable"));
        }
        if desc.enumerable.is_some_and(|e| e != flags.enumerable) {
            return Err(DefineOutcome::Rejected(
                "it is not configurable, so its enumerable attribute cannot change",
            ));
        }
        if desc.is_accessor() {
            return Err(DefineOutcome::Rejected(
                "it is a non-configurable data property, so it cannot become an accessor",
            ));
        }
        if !flags.writable {
            if desc.writable == Some(true) {
                return Err(DefineOutcome::Rejected(
                    "it is neither writable nor configurable, so it cannot become writable",
                ));
            }
            if desc
                .value
                .as_ref()
                .is_some_and(|v| !same_value(v, &current.value))
            {
                return Err(DefineOutcome::Rejected(
                    "it is neither writable nor configurable, so its value cannot change",
                ));
            }
        }
    } else if desc.is_accessor() {
        return Err(DefineOutcome::Accessor);
    }
    Ok(Property {
        value: desc.value.clone().unwrap_or_else(|| current.value.clone()),
        flags: PropertyFlags {
            writable: desc.writable.unwrap_or(flags.writable),
            enumerable: desc.enumerable.unwrap_or(flags.enumerable),
            configurable: desc.configurable.unwrap_or(flags.configurable),
        },
    })
}

/// A property descriptor as ToPropertyDescriptor reads it (ECMAScript
/// "The Property Descriptor Specification Type"): each field may be absent.
/// A descriptor with `get` or `set` is an accessor descriptor, one with
/// `value` or `writable` a data descriptor, one with neither a generic
/// descriptor; ToPropertyDescriptor rejects one that is both.
#[derive(Clone, Debug, Default)]
pub(crate) struct PropertyDescriptor {
    pub(crate) value: Option<Value>,
    pub(crate) writable: Option<bool>,
    pub(crate) get: Option<Value>,
    pub(crate) set: Option<Value>,
    pub(crate) enumerable: Option<bool>,
    pub(crate) configurable: Option<bool>,
}

impl PropertyDescriptor {
    /// IsAccessorDescriptor: `get` or `set` is present.
    pub(crate) fn is_accessor(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }

    /// IsDataDescriptor: `value` or `writable` is present.
    pub(crate) fn is_data(&self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }
}

/// What [`OrdinaryObject::define_own_property`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DefineOutcome {
    /// The property was created or updated.
    Done,
    /// ValidateAndApplyPropertyDescriptor returned false (`defineProperty`
    /// throws a TypeError); the reason, for the message. Nothing changed.
    Rejected(&'static str),
    /// The define would create an accessor property, or turn a
    /// configurable data property into one (clj-quickjs-120). Nothing
    /// changed.
    Accessor,
}

/// The collector traces every property value and the prototype.
impl Trace for OrdinaryObject {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        for (_, property) in &self.entries {
            property.value.trace(visit);
        }
        if let Some(prototype) = self.prototype {
            visit(prototype);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> JsString {
        JsString::from(text)
    }

    #[test]
    fn keys_are_interned_once_and_lookups_do_not_intern() {
        let keys = PropertyKeys::default();
        assert_eq!(keys.lookup(&s("x")), None);
        assert_eq!(keys.len(), 0, "a lookup interns nothing");
        let x = keys.intern(&s("x")).unwrap();
        let y = keys.intern(&s("y")).unwrap();
        assert_ne!(x, y);
        // A different body with the same contents is the same key.
        let units: Vec<u16> = "x".encode_utf16().collect();
        assert_eq!(
            keys.intern(&JsString::from_code_units(units).unwrap()),
            Some(x)
        );
        assert_eq!(keys.lookup(&s("x")), Some(x));
        assert_eq!(keys.name(y), Some(s("y")));
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn properties_keep_creation_order_and_the_first_position() {
        let keys = PropertyKeys::default();
        let mut o = OrdinaryObject::new(None);
        let names: Vec<PropertyKey> = (0..20)
            .map(|i| keys.intern(&s(&format!("p{i}"))).unwrap())
            .collect();
        for (i, &k) in names.iter().enumerate() {
            assert!(o.define(k, Value::Number(i as f64)));
            // Lookups work below and above the index threshold.
            assert_eq!(o.get_own(k).unwrap().value, Value::Number(i as f64));
            assert_eq!(o.index.is_some(), i + 1 > INDEX_THRESHOLD);
        }
        // Redefining and setting keep the position of the first definition.
        assert!(o.define(names[3], Value::Number(-1.0)));
        assert_eq!(o.set(names[15], Value::Number(-2.0)), SetOutcome::Done);
        let order: Vec<PropertyKey> = o.properties().map(|(k, _)| k).collect();
        assert_eq!(order, names);
        assert_eq!(o.get_own(names[3]).unwrap().value, Value::Number(-1.0));
        assert_eq!(o.get_own(names[15]).unwrap().value, Value::Number(-2.0));
        assert_eq!(o.get_own(names[3]).unwrap().flags, PropertyFlags::DATA);
    }

    #[test]
    fn set_reports_the_cases_it_does_not_decide() {
        let keys = PropertyKeys::default();
        let (a, b) = (keys.intern(&s("a")).unwrap(), keys.intern(&s("b")).unwrap());
        let mut o = OrdinaryObject::new(Some(TaskId::from_raw(1)));
        assert_eq!(o.set(a, Value::Bool(true)), SetOutcome::Done);
        o.entries[0].1.flags.writable = false;
        assert_eq!(o.set(a, Value::Null), SetOutcome::ReadOnly);
        assert_eq!(o.get_own(a).unwrap().value, Value::Bool(true));
        o.extensible = false;
        assert_eq!(o.set(b, Value::Null), SetOutcome::NotExtensible);
        assert!(!o.define(b, Value::Null));
        o.extensible = true;
        o.prototype = Some(ObjectId::from_index(0));
        assert_eq!(o.set(b, Value::Null), SetOutcome::Inherited);
        assert!(o.get_own(b).is_none());
    }
}
