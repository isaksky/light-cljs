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
//! property and does not grow the table. The collector reclaims untraced
//! keys and reuses their slots after a full mark (clj-quickjs-259).
//!
//! These keys are QuickJS's `JSAtom`s (`research/quickjs/quickjs.c`,
//! revision 7d1c13c5d8f8, `__JS_NewAtom`); they are not called atoms here
//! because [`crate::atom`] is the concurrent reference.

pub(crate) mod array;
pub(crate) mod disposable_stack;
pub(crate) mod finalization_registry;
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

impl Trace for PropertyKey {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        visit(crate::heap::gc::TraceEdge::PropertyKey(*self));
        if let Some(id) = self.1 {
            visit(crate::heap::gc::TraceEdge::Symbol(id));
        }
    }
}

/// The runtime-wide property-key table (one per heap). Mutators take its
/// leaf lock only outside other heap locks. The stopped collector sweeps it
/// with only the rendezvous lock held. Raw keys must be traced across a
/// safepoint; operation-local caches must end before the next safepoint.
#[derive(Debug, Default)]
pub(crate) struct PropertyKeys {
    table: RwLock<KeyTable>,
}

#[derive(Debug, Default)]
struct KeyTable {
    ids: HashMap<JsString, PropertyKey>,
    names: Vec<Option<StoredKey>>,
    symbols: HashMap<JsSymbol, PropertyKey>,
    free: Vec<u32>,
}

#[derive(Debug)]
enum StoredKey {
    Name(JsString),
    Symbol(JsSymbol),
}

impl KeyTable {
    fn insert(&mut self, entry: StoredKey) -> Option<PropertyKey> {
        let index = match self.free.pop() {
            Some(index) => index,
            None => u32::try_from(self.names.len()).ok()?,
        };
        let key = PropertyKey(
            index,
            match &entry {
                StoredKey::Name(_) => None,
                StoredKey::Symbol(symbol) => Some(symbol.identity()),
            },
        );
        if index as usize == self.names.len() {
            self.names.push(Some(entry));
        } else {
            self.names[index as usize] = Some(entry);
        }
        Some(key)
    }
}

impl PropertyKeys {
    /// The key of `name` if it is currently interned. A
    /// shared read lock only.
    pub(crate) fn lookup(&self, name: &JsString) -> Option<PropertyKey> {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        table.ids.get(name).copied()
    }

    /// The key of `name`, interning it if new. `None` only when all 2^32
    /// slots are occupied. The caller stays in heap access without a
    /// safepoint until the key is installed in traced storage or discarded.
    pub(crate) fn intern(&self, name: &JsString) -> Option<PropertyKey> {
        if let Some(key) = self.lookup(name) {
            return Some(key);
        }
        let mut table = self.table.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(&key) = table.ids.get(name) {
            return Some(key);
        }
        let key = table.insert(StoredKey::Name(name.clone()))?;
        table.ids.insert(name.clone(), key);
        Some(key)
    }

    /// The name `key` was interned for.
    pub(crate) fn name(&self, key: PropertyKey) -> Option<JsString> {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        match table.names.get(key.0 as usize)?.as_ref()? {
            StoredKey::Name(name) => Some(name.clone()),
            StoredKey::Symbol(_) => None,
        }
    }

    /// Symbols occupy the same private u32 key space as names, without
    /// entering the name index. Like names, their table entries live only
    /// while traced keys hold them. Symbol value identity is independent
    /// of the reusable property-key slot.
    pub(crate) fn intern_symbol(&self, symbol: &JsSymbol) -> Option<PropertyKey> {
        let mut table = self.table.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(&key) = table.symbols.get(symbol) {
            return Some(key);
        }
        let key = table.insert(StoredKey::Symbol(symbol.clone()))?;
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
        match table.names.get(key.0 as usize)?.as_ref()? {
            StoredKey::Symbol(symbol) => Some(symbol.clone()),
            StoredKey::Name(_) => None,
        }
    }

    /// Reclaim unused names and symbols after the complete strong/ephemeron
    /// mark. Only the stopped collector calls this, before mutators resume.
    pub(crate) fn sweep(&self, is_live: impl Fn(PropertyKey) -> bool) {
        let mut table = self.table.write().unwrap_or_else(PoisonError::into_inner);
        for index in 0..table.names.len() {
            let Some(entry) = &table.names[index] else {
                continue;
            };
            let key = PropertyKey(
                index as u32,
                match entry {
                    StoredKey::Name(_) => None,
                    StoredKey::Symbol(symbol) => Some(symbol.identity()),
                },
            );
            if is_live(key) {
                continue;
            }
            match table.names[index].take().unwrap() {
                StoredKey::Name(name) => {
                    table.ids.remove(&name);
                }
                StoredKey::Symbol(symbol) => {
                    table.symbols.remove(&symbol);
                }
            }
            table.free.push(index as u32);
        }
        // Release hash-index capacity after a large transient batch too.
        if table.ids.capacity() > table.ids.len().saturating_mul(4) {
            table.ids.shrink_to_fit();
        }
        if table.symbols.capacity() > table.symbols.len().saturating_mul(4) {
            table.symbols.shrink_to_fit();
        }
    }

    /// Number of occupied name and symbol slots (tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        table.names.len() - table.free.len()
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

/// Immutable internal wrapper data. Symbols report identity edges for weak-key GC;
/// BigInt limbs use shared immutable Rust storage and have no heap edges.
#[derive(Clone, Debug)]
pub(crate) enum PrimitiveData {
    Number(f64),
    String(JsString),
    Symbol(crate::value::JsSymbol),
    Boolean(bool),
    // Keep the wrapper slot pointer-sized: inline BigInt storage would enlarge
    // every OrdinaryObject and heap-allocation temporary on small VM workers.
    BigInt(std::sync::Arc<crate::value::JsBigInt>),
}

#[derive(Debug)]
enum IntlServiceSlots {
    RelativeTimeFormat(crate::intl::relative_time_format::RelativeTimeFormat),
    DisplayNames(crate::intl::display_names::DisplayNames),
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
    /// `[[NumberData]]` / `[[StringData]]` / `[[SymbolData]]`, initialized before allocation.
    primitive_data: Option<PrimitiveData>,
    /// `[[ErrorData]]`: independent of properties and prototype changes.
    error_data: bool,
    /// Cold internal state stays boxed so allocation frames do not grow with
    /// each new language feature (in particular on 64 KiB VM workers).
    internal: Option<Box<InternalSlots>>,
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
    /// Immutable options have no heap edges; this ordinary object remains task-owned.
    list_format: Option<Box<crate::intl::list_format::ListFormat>>,

    // Immutable Rust strings/static tables only; no additional GC edges.
    intl_service: Option<Box<IntlServiceSlots>>,
    segmenter: Option<Box<crate::intl::segmenter::Data>>,
    duration_format: Option<Box<crate::intl::duration_format::DurationFormat>>,
}

#[derive(Debug, Default)]
struct InternalSlots {
    regexp_iterator: Option<Box<crate::vm::native::string_regexp::IteratorData>>,
    engine_error: Option<Box<crate::vm::exception::ErrorObject>>,
    private: crate::vm::private::Elements,
}

impl OrdinaryObject {
    pub(crate) fn with_regexp_iterator(
        mut self,
        data: crate::vm::native::string_regexp::IteratorData,
    ) -> Self {
        self.internal
            .get_or_insert_with(Default::default)
            .regexp_iterator = Some(Box::new(data));
        self
    }
    pub(crate) fn regexp_iterator(
        &mut self,
    ) -> Option<&mut crate::vm::native::string_regexp::IteratorData> {
        self.internal.as_deref_mut()?.regexp_iterator.as_deref_mut()
    }
    pub(crate) fn with_display_names(
        mut self,
        data: crate::intl::display_names::DisplayNames,
    ) -> Self {
        self.intl_service = Some(Box::new(IntlServiceSlots::DisplayNames(data)));
        self
    }
    pub(crate) fn display_names(&self) -> Option<&crate::intl::display_names::DisplayNames> {
        match self.intl_service.as_deref() {
            Some(IntlServiceSlots::DisplayNames(data)) => Some(data),
            _ => None,
        }
    }

    pub(crate) fn with_duration_format(
        mut self,
        data: crate::intl::duration_format::DurationFormat,
    ) -> Self {
        self.duration_format = Some(Box::new(data));
        self
    }
    pub(crate) fn duration_format(&self) -> Option<&crate::intl::duration_format::DurationFormat> {
        self.duration_format.as_deref()
    }

    pub(crate) fn with_segmenter(mut self, data: crate::intl::segmenter::Data) -> Self {
        self.segmenter = Some(Box::new(data));
        self
    }
    pub(crate) fn segmenter(&mut self) -> Option<&mut crate::intl::segmenter::Data> {
        self.segmenter.as_deref_mut()
    }
    pub(crate) fn with_list_format(mut self, data: crate::intl::list_format::ListFormat) -> Self {
        self.list_format = Some(Box::new(data));
        self
    }

    pub(crate) fn list_format(&self) -> Option<&crate::intl::list_format::ListFormat> {
        self.list_format.as_deref()
    }

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
            internal: None,
            locale_data: None,

            date_value: None,
            regexp: None,

            number_format: None,
            number_format_bound: None,
            iterator: None,

            plural_rules: None,
            collator: None,
            date_time_format: None,
            list_format: None,

            intl_service: None,
            segmenter: None,
            duration_format: None,
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

    pub(crate) fn with_relative_time_format(
        mut self,
        data: crate::intl::relative_time_format::RelativeTimeFormat,
    ) -> Self {
        self.intl_service = Some(Box::new(IntlServiceSlots::RelativeTimeFormat(data)));
        self
    }
    pub(crate) fn relative_time_format(
        &self,
    ) -> Option<&crate::intl::relative_time_format::RelativeTimeFormat> {
        match self.intl_service.as_deref() {
            Some(IntlServiceSlots::RelativeTimeFormat(data)) => Some(data),
            _ => None,
        }
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
        self.internal
            .get_or_insert_with(Default::default)
            .engine_error = Some(Box::new(error));
        self
    }

    pub(crate) fn engine_error(&self) -> Option<&crate::vm::exception::ErrorObject> {
        self.internal.as_ref()?.engine_error.as_deref()
    }

    /// Private elements ignore public extensibility/freeze flags and are
    /// allocated only for a receiver on which a private definition runs.
    pub(crate) fn private_elements(
        &mut self,
        create: bool,
    ) -> Option<&mut crate::vm::private::Elements> {
        if create {
            self.internal.get_or_insert_with(Default::default);
        }
        self.internal.as_mut().map(|internal| &mut internal.private)
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

/// The collector traces every property key, value and the prototype.
impl Trace for OrdinaryObject {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(internal) = &self.internal {
            internal.private.trace(visit);
            if let Some(iterator) = &internal.regexp_iterator {
                iterator.trace(visit);
            }
        }
        if let Some(segmenter) = &self.segmenter {
            segmenter.trace(visit);
        }
        if let Some(iterator) = &self.iterator {
            iterator.trace(visit);
        }
        if let Some(PrimitiveData::Symbol(symbol)) = &self.primitive_data {
            visit(crate::heap::gc::TraceEdge::Symbol(symbol.identity()));
        }
        // Other primitive data and DateValue have no heap edges.
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
        if let Some(error) = self.engine_error() {
            error.trace(visit);
        }
        for (key, property) in &self.entries {
            key.trace(visit);
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
    #[test]
    fn primitive_wrapper_slots_stay_compact_for_small_workers() {
        // These slots occur inline in every ordinary object. Growing them
        // over their original 16 bytes overflows existing 64 KiB VM paths.
        assert!(std::mem::size_of::<super::PrimitiveData>() <= 16);
    }

    use super::*;

    fn s(text: &str) -> JsString {
        JsString::from(text)
    }

    /// Synthetic JavaScript fixture, not light-cljs output. The returned
    /// object is the only surviving root after the script task is dropped.
    #[test]
    fn collector_reclaims_computed_keys_and_preserves_live_keys() {
        use crate::{RunOutcome, Runtime};

        fn run(rt: &Runtime, source: &str) -> crate::Rooted {
            let code = crate::frontend::compile_script("key-collection", source).unwrap();
            let task = rt.spawn(&code, &[]).unwrap();
            match task.run().unwrap() {
                RunOutcome::Complete(value) => value,
                other => panic!("{other:?}"),
            }
        }

        let rt = Runtime::new();
        drop(run(&rt, "({})"));
        rt.collect().unwrap();
        let baseline = rt.heap().keys().len();
        let live = run(&rt, "({'259-live': 73})");
        rt.collect().unwrap();
        let live_key = rt.heap().keys().lookup(&s("259-live")).unwrap();
        assert_eq!(rt.heap().keys().len(), baseline + 1);
        for batch in 0..3 {
            drop(run(
                &rt,
                &format!(
                    "for (let i = 0; i < 300; i++) {{ let dead = {{}}; dead['259-garbage-{batch}-' + i] = i; }} 0"
                ),
            ));
            assert!(rt.heap().keys().len() >= baseline + 301);
            rt.collect().unwrap();
            assert_eq!(rt.heap().keys().len(), baseline + 1);
            assert_eq!(rt.heap().keys().lookup(&s("259-live")), Some(live_key));
            let Value::Object(id) = live.value_in(rt.heap()).unwrap() else {
                panic!("expected object")
            };
            rt.heap()
                .read(id, |object| {
                    let crate::heap::HeapObject::Plain(object) = object else {
                        panic!("expected ordinary object")
                    };
                    assert_eq!(object.get_own(live_key).unwrap().value, Value::Number(73.0));
                })
                .unwrap();
        }
        drop(live);
        rt.collect().unwrap();
        assert_eq!(rt.heap().keys().len(), baseline);
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
    fn reclaimed_name_and_symbol_slots_are_reused() {
        let keys = PropertyKeys::default();
        let live = keys.intern(&s("live")).unwrap();
        let dead_name_slot = keys.intern(&s("dead")).unwrap().0;
        let symbol = JsSymbol::new(Some(s("description")));
        let dead_symbol_slot = keys.intern_symbol(&symbol).unwrap().0;
        keys.sweep(|key| key == live);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys.lookup(&s("dead")), None);
        assert_eq!(keys.lookup_symbol(&symbol), None);
        let replacement_name = keys.intern(&s("replacement")).unwrap();
        let replacement_symbol = keys.intern_symbol(&symbol).unwrap();
        assert_eq!(replacement_name.0, dead_symbol_slot);
        assert_eq!(replacement_symbol.0, dead_name_slot);
        assert_eq!(keys.intern(&s("live")), Some(live));
        assert_eq!(keys.name(live), Some(s("live")));
        assert_eq!(keys.name(replacement_name), Some(s("replacement")));
        assert_eq!(keys.symbol(replacement_symbol), Some(symbol));
        assert_eq!(keys.table.read().unwrap().names.len(), 3);
    }

    /// Synthetic fixture, not light-cljs output. A key used only by an
    /// unmarked object must not retain a symbol in the property table.
    #[test]
    fn collector_reclaims_symbol_keys() {
        use crate::heap::{Heap, HeapObject, Rooted};
        use std::sync::Arc;
        let heap = Arc::new(Heap::default());
        let live_symbol = JsSymbol::new(Some(s("live")));
        let live = {
            let mut object = OrdinaryObject::new(None);
            object.define(
                heap.keys().intern_symbol(&live_symbol).unwrap(),
                Value::Number(42.0),
            );
            Rooted::new(
                &heap,
                Value::Object(heap.alloc(HeapObject::Plain(object)).unwrap()),
            )
        };
        for i in 0..300 {
            let symbol = JsSymbol::new(Some(s(&format!("garbage-{i}"))));
            let mut object = OrdinaryObject::new(None);
            object.define(heap.keys().intern_symbol(&symbol).unwrap(), Value::Null);
            heap.alloc(HeapObject::Plain(object)).unwrap();
        }
        assert_eq!(heap.keys().len(), 301);
        heap.collect().unwrap();
        assert_eq!(heap.keys().len(), 1);
        {
            let key = heap.keys().lookup_symbol(&live_symbol).unwrap();
            assert_eq!(heap.keys().symbol(key), Some(live_symbol.clone()));
            let Value::Object(id) = live.value_in(&heap).unwrap() else {
                unreachable!()
            };
            heap.read(id, |object| {
                let HeapObject::Plain(object) = object else {
                    unreachable!()
                };
                assert_eq!(object.get_own(key).unwrap().value, Value::Number(42.0));
            })
            .unwrap();
        }
        // A symbol value alone holds no raw property-key id. Reinterning it
        // after its property's death still preserves symbol value identity.
        let symbol_root = Rooted::new(&heap, Value::Symbol(live_symbol.clone()));
        drop(live);
        heap.collect().unwrap();
        assert_eq!(heap.keys().len(), 0);
        assert_eq!(heap.keys().lookup_symbol(&live_symbol), None);
        let key = heap.keys().intern_symbol(&live_symbol).unwrap();
        assert_eq!(heap.keys().symbol(key), Some(live_symbol));
        drop(symbol_root);
    }

    /// Synthetic fixture, not light-cljs output. The final value and its
    /// property keys are reached only after two conditional edges activate.
    #[test]
    fn collector_preserves_keys_reached_through_ephemerons() {
        use crate::heap::HeapObject;
        use crate::{RunOutcome, Runtime};

        let rt = Runtime::new();
        crate::realm::intrinsics(rt.heap()).unwrap();
        rt.collect().unwrap();
        let baseline = rt.heap().keys().len();
        let code = crate::frontend::compile_function(
            r#"
            function make() {
                const a = new WeakMap(), b = new WeakMap(), key = {}, middle = {};
                const value = {}, symbol = Symbol('259-conditional-symbol');
                value['259-conditional-name'] = 42;
                value[symbol] = 73;
                a.set(key, middle);
                b.set(middle, value);
                return __rt.vector(a, b, key);
            }
        "#,
        )
        .unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(result) = task.run().unwrap() else {
            panic!("expected completed fixture")
        };
        let (a, b, key) = rt.scope(|scope| {
            let result = scope.local(&result).unwrap();
            (
                scope.persist(scope.nth(result, 0).unwrap()),
                scope.persist(scope.nth(result, 1).unwrap()),
                scope.persist(scope.nth(result, 2).unwrap()),
            )
        });
        drop((task, result, code));
        let object_id = |root: &crate::Rooted| {
            let Value::Object(id) = root.value_in(rt.heap()).unwrap() else {
                unreachable!()
            };
            id
        };
        let weak_value = |map, key: crate::heap::ObjectId| {
            rt.heap()
                .read(map, |object| {
                    let HeapObject::WeakMap(map) = object else {
                        unreachable!()
                    };
                    let Value::Object(value) = map.get(key.into()).unwrap() else {
                        unreachable!()
                    };
                    *value
                })
                .unwrap()
        };
        let middle = weak_value(object_id(&a), object_id(&key));
        let value = weak_value(object_id(&b), middle);
        let symbol = {
            let name_key = rt.heap().keys().lookup(&s("259-conditional-name")).unwrap();
            let symbol_key = rt
                .heap()
                .read(value, |object| {
                    let HeapObject::Plain(object) = object else {
                        unreachable!()
                    };
                    object
                        .properties()
                        .find(|(key, _)| key.1.is_some())
                        .unwrap()
                        .0
                })
                .unwrap();
            let symbol = rt.heap().keys().symbol(symbol_key).unwrap();
            for _ in 0..3 {
                rt.collect().unwrap();
                assert_eq!(rt.heap().keys().len(), baseline + 2);
                assert_eq!(
                    rt.heap().keys().lookup(&s("259-conditional-name")),
                    Some(name_key)
                );
                assert_eq!(rt.heap().keys().lookup_symbol(&symbol), Some(symbol_key));
                assert_eq!(weak_value(object_id(&b), middle), value);
                rt.heap()
                    .read(value, |object| {
                        let HeapObject::Plain(object) = object else {
                            unreachable!()
                        };
                        assert_eq!(object.get_own(name_key).unwrap().value, Value::Number(42.0));
                        assert_eq!(
                            object.get_own(symbol_key).unwrap().value,
                            Value::Number(73.0)
                        );
                    })
                    .unwrap();
            }
            symbol
        };
        drop(key);
        rt.collect().unwrap();
        assert!(!rt.heap().is_live(middle));
        assert!(!rt.heap().is_live(value));
        assert_eq!(a.weak_map_live_entry_count(), Some(0));
        assert_eq!(b.weak_map_live_entry_count(), Some(0));
        assert_eq!(rt.heap().keys().lookup(&s("259-conditional-name")), None);
        assert_eq!(rt.heap().keys().lookup_symbol(&symbol), None);
        assert_eq!(rt.heap().keys().len(), baseline);
    }

    /// Synthetic fixture, not light-cljs output. Collect at every budget
    /// boundary, including accessor calls and lazy function-store creation.
    #[test]
    fn property_stores_and_resumable_keys_survive_collection() {
        use crate::{Budget, RunOutcome, Runtime, ValueView};
        let source = r#"
            let symbol = Symbol('259-key');
            let objects = [{}, [], new Map(), new WeakMap(), new Set(), new WeakSet(), function() {},
                (function() {}).bind(null), new ArrayBuffer(8), new Uint8Array(2),
                (function*() { yield 1; })(), new WeakRef({}),
                new FinalizationRegistry(() => {}), new DisposableStack()];
            for (let i = 0; i < objects.length; i++) {
                objects[i]['259-store-' + i] = i;
                objects[i][symbol] = i + 100;
                Object.defineProperty(objects[i], '259-accessor', {
                    get() { return this[symbol]; }
                });
            }
            class C { ['259-field'] = 21; ['259-method']() { return this['259-field']; } }
            let ok = true;
            for (let i = 0; i < objects.length; i++) {
                ok = ok && objects[i]['259-store-' + i] === i
                    && objects[i][symbol] === i + 100
                    && objects[i]['259-accessor'] === i + 100;
            }
            let deleted = {};
            deleted['259-deleted'] = 1;
            delete deleted['259-deleted'];
            try { Object.defineProperty(Object.preventExtensions({}), '259-failed', {value: 1}); } catch(e) {}
            ok && new C()['259-method']() === 21
        "#;
        let rt = Runtime::new();
        let code = crate::frontend::compile_script("key-safepoints", source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        for _ in 0..5000 {
            match task
                .run_with(Budget::instructions(1), &mut crate::host::NoHost)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true));
                    rt.collect().unwrap();
                    assert_eq!(rt.heap().keys().lookup(&s("259-deleted")), None);
                    assert_eq!(rt.heap().keys().lookup(&s("259-failed")), None);
                    return;
                }
                other => panic!("{other:?}"),
            }
        }
        panic!("fixture exceeded instruction bound");
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
