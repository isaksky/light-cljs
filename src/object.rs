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
//!   position of the first definition; `delete` (clj-quickjs-124,
//!   [`OrdinaryObject::delete`]) removes the entry, so a property defined
//!   again comes last. Each property holds a data value or an accessor
//!   getter/setter pair, with [`PropertyFlags`]. Assignment and
//!   literals create properties with all three attributes;
//!   [`OrdinaryObject::define_own_property`] (`Object.defineProperty`,
//!   clj-quickjs-119) creates and validates any combination.
//! * **`[[Prototype]]`**, an object or null: `Object.prototype` for an
//!   object literal or an `Object()` result (clj-quickjs-117,
//!   [`crate::realm`]), null for an object made by the `NewObject`
//!   instruction or by the embedder, and for `Object.prototype` itself;
//!   whatever `Object.create`, `Object.setPrototypeOf`, `o.__proto__ = p`
//!   or a literal's `__proto__: p` gives it (clj-quickjs-121,
//!   `crate::vm::prototype`). Reads walk it; a write of a property the object does
//!   not have itself consults it (`crate::vm::property`).
//! * **Intrinsic**: true only for the realm's shared, immutable intrinsic
//!   objects ([`crate::realm`]): `Object.prototype` and the property storage
//!   of built-in functions. An intrinsic object has no owner, every task
//!   reads it, nothing changes it after the realm is built, and it is
//!   classified [`Shareability::Persistent`](crate::value::share::Shareability).
//! * **`[[Extensible]]`**: true until `Object.preventExtensions` or
//!   `Object.freeze` (clj-quickjs-122, `crate::vm::integrity`) clears it,
//!   for good; adding a property to a non-extensible object fails.
//!   Freezing also makes every own property non-writable and
//!   non-configurable; it does not change the owner.
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
pub(crate) mod map;
pub(crate) mod weak_map;
pub(crate) mod weak_ref;

pub(crate) mod array_buffer;
pub(crate) mod typed_array;

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use crate::atom::same_value;
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::value::{JsString, JsSymbol, Value};

/// Interned identity of a property name: a private id issued by
/// [`PropertyKeys`]. Equal names in one runtime always have the same key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PropertyKey(u32, Option<u64>);

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
    names: Vec<StoredKey>,
    symbols: HashMap<JsSymbol, PropertyKey>,
}

#[derive(Debug)]
enum StoredKey {
    Name(JsString),
    Symbol(JsSymbol),
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
        let key = PropertyKey(u32::try_from(table.names.len()).ok()?, None);
        table.names.push(StoredKey::Name(name.clone()));
        table.ids.insert(name.clone(), key);
        Some(key)
    }

    /// The name `key` was interned for.
    pub(crate) fn name(&self, key: PropertyKey) -> Option<JsString> {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        match table.names.get(key.0 as usize)? {
            StoredKey::Name(name) => Some(name.clone()),
            StoredKey::Symbol(_) => None,
        }
    }

    /// Symbols occupy the same private u32 key space as names, without
    /// entering the name index. Entries retain identity and description
    /// until heap drop, like interned names (docs/architecture.md).
    pub(crate) fn intern_symbol(&self, symbol: &JsSymbol) -> Option<PropertyKey> {
        let mut table = self.table.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(&key) = table.symbols.get(symbol) {
            return Some(key);
        }
        let key = PropertyKey(
            u32::try_from(table.names.len()).ok()?,
            Some(symbol.identity()),
        );
        table.names.push(StoredKey::Symbol(symbol.clone()));
        table.symbols.insert(symbol.clone(), key);
        Some(key)
    }

    pub(crate) fn lookup_symbol(&self, symbol: &JsSymbol) -> Option<PropertyKey> {
        self.table
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .symbols
            .get(symbol)
            .copied()
    }

    pub(crate) fn symbol(&self, key: PropertyKey) -> Option<JsSymbol> {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        match table.names.get(key.0 as usize)? {
            StoredKey::Symbol(symbol) => Some(symbol.clone()),
            StoredKey::Name(_) => None,
        }
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
    /// Present for an accessor; `value` is then undefined and writable is false.
    pub(crate) accessor: Option<(Value, Value)>,
}

/// Objects with more own properties than this get a key index; smaller ones
/// are searched linearly (fewer allocations and cache misses for the common
/// small object).
pub(crate) const INDEX_THRESHOLD: usize = 8;

/// What [`OrdinaryObject::set`] did.
#[derive(Clone, Debug, PartialEq)]
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
    /// Call this setter with the original receiver, outside the object lock.
    Setter(Value),
}

/// The internal data of a Number or String wrapper (clj-quickjs-72).
/// Neither variant has heap edges: strings own immutable `JsString` storage.
#[derive(Clone, Debug)]
pub(crate) enum PrimitiveData {
    Number(f64),
    String(JsString),
}

/// Keep service slots behind one pointer so adding a service does not enlarge
/// every object by an inline cached Value (including dispatcher temporaries).
#[derive(Debug)]
struct DateTimeFormatSlots {
    options: crate::intl::date_time_format::DateTimeFormat,
    bound: Option<Value>,
}

#[derive(Debug)]
struct CollatorSlots {
    options: crate::intl::collator::Collator,
    bound: Option<Value>,
}

/// Storage of an ordinary object, optionally with immutable primitive data.
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
    /// `[[NumberData]]` / `[[StringData]]`, initialized before allocation.
    primitive_data: Option<PrimitiveData>,
    /// `[[ErrorData]]`: independent of properties and prototype changes.
    error_data: bool,
    /// Engine-only diagnostic and original cause, independent of properties.
    engine_error: Option<Box<crate::vm::exception::ErrorObject>>,
    /// Intl.Locale's immutable internal record; contains no heap edges.
    locale_data: Option<Box<crate::intl::tag::LanguageTag>>,

    /// `[[DateValue]]`: clipped epoch milliseconds or NaN, never a heap edge.
    date_value: Option<f64>,
    /// Immutable RegExp code has no heap edges; mutable state is lastIndex.
    regexp: Option<std::sync::Arc<crate::regexp::object::Compiled>>,

    number_format: Option<Box<crate::intl::number_format::NumberFormat>>,
    number_format_bound: Option<Value>,
    /// Task-owned cursor of a built-in iterator. Never exposed as properties.
    iterator: Option<Box<crate::vm::native::iterator::Data>>,

    plural_rules: Option<Box<crate::intl::plural_rules::PluralRules>>,
    collator: Option<Box<CollatorSlots>>,
    date_time_format: Option<Box<DateTimeFormatSlots>>,
}

impl OrdinaryObject {
    pub(crate) fn with_iterator(mut self, data: crate::vm::native::iterator::Data) -> Self {
        self.iterator = Some(Box::new(data));
        self
    }

    pub(crate) fn iterator(&mut self) -> Option<&mut crate::vm::native::iterator::Data> {
        self.iterator.as_deref_mut()
    }

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
            primitive_data: None,
            error_data: false,
            engine_error: None,
            locale_data: None,

            date_value: None,
            regexp: None,

            number_format: None,
            number_format_bound: None,
            iterator: None,

            plural_rules: None,
            collator: None,
            date_time_format: None,
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

    /// Initialize a wrapper before publishing its identity to the heap.
    pub(crate) fn with_primitive_data(mut self, data: PrimitiveData) -> OrdinaryObject {
        self.primitive_data = Some(data);
        self
    }

    /// A wrapper's internal data, independent of its JavaScript prototype.
    pub(crate) fn primitive_data(&self) -> Option<&PrimitiveData> {
        self.primitive_data.as_ref()
    }

    pub(crate) fn with_locale_data(mut self, data: crate::intl::tag::LanguageTag) -> Self {
        self.locale_data = Some(Box::new(data));
        self
    }

    pub(crate) fn locale_data(&self) -> Option<&crate::intl::tag::LanguageTag> {
        self.locale_data.as_deref()
    }

    pub(crate) fn with_regexp(
        mut self,
        compiled: std::sync::Arc<crate::regexp::object::Compiled>,
    ) -> Self {
        self.regexp = Some(compiled);
        self
    }

    pub(crate) fn regexp(&self) -> Option<&std::sync::Arc<crate::regexp::object::Compiled>> {
        self.regexp.as_ref()
    }

    pub(crate) fn with_plural_rules(
        mut self,
        data: crate::intl::plural_rules::PluralRules,
    ) -> Self {
        self.plural_rules = Some(Box::new(data));
        self
    }
    pub(crate) fn with_date_time_format(
        mut self,
        data: crate::intl::date_time_format::DateTimeFormat,
    ) -> Self {
        self.date_time_format = Some(Box::new(DateTimeFormatSlots {
            options: data,
            bound: None,
        }));
        self
    }
    pub(crate) fn date_time_format(
        &self,
    ) -> Option<&crate::intl::date_time_format::DateTimeFormat> {
        self.date_time_format.as_ref().map(|s| &s.options)
    }
    pub(crate) fn date_time_format_bound(&self) -> Option<&Value> {
        self.date_time_format
            .as_ref()
            .and_then(|s| s.bound.as_ref())
    }
    pub(crate) fn set_date_time_format_bound(&mut self, value: Value) {
        self.date_time_format
            .as_mut()
            .expect("checked DateTimeFormat receiver")
            .bound = Some(value);
    }
    pub(crate) fn with_collator(mut self, data: crate::intl::collator::Collator) -> Self {
        self.collator = Some(Box::new(CollatorSlots {
            options: data,
            bound: None,
        }));
        self
    }
    pub(crate) fn collator(&self) -> Option<&crate::intl::collator::Collator> {
        self.collator.as_ref().map(|slots| &slots.options)
    }
    pub(crate) fn collator_bound(&self) -> Option<&Value> {
        self.collator
            .as_ref()
            .and_then(|slots| slots.bound.as_ref())
    }
    pub(crate) fn set_collator_bound(&mut self, value: Value) {
        self.collator
            .as_mut()
            .expect("Collator receiver checked")
            .bound = Some(value);
    }
    pub(crate) fn plural_rules(&self) -> Option<&crate::intl::plural_rules::PluralRules> {
        self.plural_rules.as_deref()
    }
    pub(crate) fn with_number_format(
        mut self,
        data: crate::intl::number_format::NumberFormat,
    ) -> Self {
        self.number_format = Some(Box::new(data));
        self
    }
    pub(crate) fn number_format(&self) -> Option<&crate::intl::number_format::NumberFormat> {
        self.number_format.as_deref()
    }
    pub(crate) fn number_format_bound(&self) -> Option<&Value> {
        self.number_format_bound.as_ref()
    }
    pub(crate) fn set_number_format_bound(&mut self, value: Value) {
        self.number_format_bound = Some(value);
    }

    pub(crate) fn with_error_data(mut self) -> Self {
        self.error_data = true;
        self
    }

    pub(crate) fn is_error(&self) -> bool {
        self.error_data
    }

    pub(crate) fn with_engine_error(mut self, error: crate::vm::exception::ErrorObject) -> Self {
        self.error_data = true;
        self.engine_error = Some(Box::new(error));
        self
    }

    pub(crate) fn engine_error(&self) -> Option<&crate::vm::exception::ErrorObject> {
        self.engine_error.as_deref()
    }

    /// Initialize Date's own slot before publishing its heap identity.
    pub(crate) fn with_date_value(mut self, time: f64) -> Self {
        self.date_value = Some(time);
        self
    }

    pub(crate) fn date_value(&self) -> Option<f64> {
        self.date_value
    }

    /// Caller validates the brand, ownership and intrinsic mutation boundary.
    pub(crate) fn set_date_value(&mut self, time: f64) {
        debug_assert!(self.date_value.is_some());
        self.date_value = Some(time);
    }

    /// The `[[Prototype]]`: an object, or `None` for null.
    pub(crate) fn prototype(&self) -> Option<ObjectId> {
        self.prototype
    }

    /// The `[[Extensible]]` flag.
    pub(crate) fn extensible(&self) -> bool {
        self.extensible
    }

    /// `[[PreventExtensions]]` (QuickJS `JS_PreventExtensions`): clear the
    /// `[[Extensible]]` flag. It is never set again; existing properties
    /// keep their attributes (clj-quickjs-122).
    pub(crate) fn prevent_extensions(&mut self) {
        self.extensible = false;
    }

    /// SetIntegrityLevel(frozen) (`Object.freeze`, clj-quickjs-122; QuickJS
    /// `js_object_seal` with its freeze flag): not extensible, and every own
    /// data property non-configurable and non-writable (`enumerable`
    /// kept). Accessors become only non-configurable; their unused
    /// writable flag is always false, and their setter stays callable.
    pub(crate) fn freeze(&mut self) {
        self.extensible = false;
        for (_, property) in &mut self.entries {
            property.flags.writable = false;
            property.flags.configurable = false;
        }
    }

    /// TestIntegrityLevel(frozen) (`Object.isFrozen`, clj-quickjs-122): not
    /// extensible, and no own property is configurable or writable.
    pub(crate) fn is_frozen(&self) -> bool {
        !self.extensible
            && self
                .entries
                .iter()
                .all(|(_, p)| !p.flags.configurable && !p.flags.writable)
    }

    /// Set the `[[Prototype]]` (`Object.setPrototypeOf`, `Object.create`,
    /// `__proto__`; clj-quickjs-121). The caller has made the checks of
    /// OrdinarySetPrototypeOf (owner, extensibility, no cycle:
    /// `crate::vm::prototype`); nothing changes a realm intrinsic's.
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
        let property = Property {
            value,
            flags,
            accessor: None,
        };
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
                if let Some((_, setter)) = &property.accessor {
                    return SetOutcome::Setter(setter.clone());
                }
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
                    accessor: None,
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
            accessor: None,
            value,
            flags: PropertyFlags::DATA,
        };
        self.push(key, property);
        true
    }

    /// `[[Delete]]` (ECMAScript OrdinaryDelete, clj-quickjs-124; QuickJS
    /// `delete_property`): remove the own property `key` if it is
    /// configurable. The other properties keep their creation order, so a
    /// property added again later comes last. Extensibility does not
    /// matter: a non-extensible object still loses configurable properties.
    pub(crate) fn delete(&mut self, key: PropertyKey) -> DeleteOutcome {
        let Some(i) = self.position(key) else {
            return DeleteOutcome::Absent;
        };
        if !self.entries[i].1.flags.configurable {
            return DeleteOutcome::NonConfigurable;
        }
        self.entries.remove(i);
        if let Some(index) = &mut self.index {
            index.remove(&key);
            for position in index.values_mut() {
                if *position as usize > i {
                    *position -= 1;
                }
            }
        }
        DeleteOutcome::Deleted
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

/// ValidateAndApplyPropertyDescriptor, for data and accessor properties.
/// Absent fields preserve the current property; a kind change resets the
/// kind-specific fields. The caller checks extensibility.
pub(crate) fn validate_and_apply(
    current: Option<&Property>,
    desc: &PropertyDescriptor,
) -> Result<Property, DefineOutcome> {
    let empty = Property {
        value: Value::Undefined,
        accessor: desc
            .is_accessor()
            .then_some((Value::Undefined, Value::Undefined)),
        flags: PropertyFlags::FIXED,
    };
    let is_new = current.is_none();
    let current = current.unwrap_or(&empty);
    let flags = current.flags;
    if !is_new && !flags.configurable {
        if desc.configurable == Some(true) {
            return Err(DefineOutcome::Rejected("it is not configurable"));
        }
        if desc.enumerable.is_some_and(|e| e != flags.enumerable) {
            return Err(DefineOutcome::Rejected(
                "it is not configurable, so its enumerable attribute cannot change",
            ));
        }
        if (desc.is_accessor() && current.accessor.is_none())
            || (desc.is_data() && current.accessor.is_some())
        {
            return Err(DefineOutcome::Rejected(
                "it is not configurable, so its property kind cannot change",
            ));
        }
        if let Some((get, set)) = &current.accessor {
            if desc.get.as_ref().is_some_and(|v| !same_value(v, get))
                || desc.set.as_ref().is_some_and(|v| !same_value(v, set))
            {
                return Err(DefineOutcome::Rejected(
                    "it is not configurable, so its accessors cannot change",
                ));
            }
        } else if !flags.writable {
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
    }
    let accessor = if desc.is_accessor() {
        let (get, set) = current
            .accessor
            .clone()
            .unwrap_or((Value::Undefined, Value::Undefined));
        Some((
            desc.get.clone().unwrap_or(get),
            desc.set.clone().unwrap_or(set),
        ))
    } else if desc.is_data() {
        None
    } else {
        current.accessor.clone()
    };
    let keep_data = current.accessor.is_none() && accessor.is_none();
    Ok(Property {
        value: if accessor.is_some() {
            Value::Undefined
        } else {
            desc.value.clone().unwrap_or_else(|| {
                if keep_data {
                    current.value.clone()
                } else {
                    Value::Undefined
                }
            })
        },
        flags: PropertyFlags {
            writable: accessor.is_none() && desc.writable.unwrap_or(keep_data && flags.writable),
            enumerable: desc.enumerable.unwrap_or(flags.enumerable),
            configurable: desc.configurable.unwrap_or(flags.configurable),
        },
        accessor,
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
    /// throws a TypeError); the reason, for the message. Ordinary properties
    /// stay unchanged; ArraySetLength may already have removed higher
    /// elements before reaching a non-configurable one.
    Rejected(&'static str),
}

/// What a `[[Delete]]` of an own property did ([`OrdinaryObject::delete`],
/// `ArrayObject::delete_element`; clj-quickjs-124).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeleteOutcome {
    /// The configurable property was removed.
    Deleted,
    /// There was no such own property (`delete` gives true).
    Absent,
    /// The property is not configurable and stays (`delete` gives false in
    /// sloppy code and throws a TypeError in strict code).
    NonConfigurable,
}

/// The collector traces every property value and the prototype.
impl Trace for OrdinaryObject {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(iterator) = &self.iterator {
            iterator.trace(visit);
        }
        // PrimitiveData and DateValue contain only f64 / JsString, with no heap edges.
        // locale_data owns Rust strings and also contains no heap edges.
        // NumberFormat options own strings/static data; the cached bound
        // callable is a heap edge, including its cycle back to this object.
        if let Some(bound) = &self.number_format_bound {
            bound.trace(visit);
        }
        // Immutable Intl options contain no heap edges. Cached bound
        // functions form traced cycles through their receivers.
        if let Some(bound) = self.date_time_format_bound() {
            bound.trace(visit);
        }
        if let Some(bound) = self.collator_bound() {
            bound.trace(visit);
        }
        if let Some(error) = &self.engine_error {
            error.trace(visit);
        }
        for (key, property) in &self.entries {
            if let Some(id) = key.1 {
                visit(crate::heap::gc::TraceEdge::Symbol(id));
            }
            property.value.trace(visit);
            if let Some((get, set)) = &property.accessor {
                get.trace(visit);
                set.trace(visit);
            }
        }
        if let Some(prototype) = self.prototype {
            visit(prototype.into());
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

    /// `delete` keeps the creation order of the rest, below and above the
    /// index threshold, and leaves non-configurable properties.
    #[test]
    fn delete_keeps_the_order_of_the_other_properties() {
        let keys = PropertyKeys::default();
        let names: Vec<PropertyKey> = (0..12)
            .map(|i| keys.intern(&s(&format!("p{i}"))).unwrap())
            .collect();
        for count in [4, 12] {
            let mut o = OrdinaryObject::new(None);
            for (i, &k) in names[..count].iter().enumerate() {
                assert!(o.define(k, Value::Number(i as f64)));
            }
            assert_eq!(o.delete(names[1]), DeleteOutcome::Deleted);
            assert_eq!(o.delete(names[1]), DeleteOutcome::Absent);
            let mut expected: Vec<PropertyKey> = names[..count].to_vec();
            expected.remove(1);
            let order: Vec<PropertyKey> = o.properties().map(|(k, _)| k).collect();
            assert_eq!(order, expected);
            for (i, &k) in names[..count].iter().enumerate().skip(2) {
                assert_eq!(o.get_own(k).unwrap().value, Value::Number(i as f64));
            }
            // Added again, it comes last.
            assert!(o.define(names[1], Value::Null));
            assert_eq!(o.properties().last().map(|(k, _)| k), Some(names[1]));
            o.freeze();
            assert_eq!(o.delete(names[0]), DeleteOutcome::NonConfigurable);
            assert!(o.get_own(names[0]).is_some());
        }
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
