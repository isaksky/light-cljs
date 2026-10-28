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
//! Reads and writes take only the object's own access word in the heap
//! (one short critical section per object visited), never the object-table
//! lock (`crate::heap::storage`): only the owning task's run ever touches an
//! owned object's properties, so the word is uncontended and stays with
//! that task's worker. A frozen realm intrinsic is read with no lock.
//!
//! # Property keys
//!
//! A [`PropertyKey`] is a private 32-bit identity issued by the runtime-wide
//! [`PropertyKeys`] table, which interns each property name once. Keys
//! compare and hash by identity (derived Rust equality over the private id),
//! never by any user-defined comparison. A symbol key (clj-quickjs-127) is a
//! fresh id with no name entry. A name is interned when a property with it
//! is first defined or a named executable operand binds to this runtime.
//! Generic reads of unknown computed names do not grow the table.
//! The collector reclaims untraced
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
mod shape;
pub(crate) mod typed_array;
pub(crate) use shape::InlineCache;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
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

impl PropertyKey {
    /// The key's dense slot in [`PropertyKeys`]: at most one live key
    /// occupies a slot at a time (the collector's key marks use it).
    pub(crate) fn slot(self) -> u32 {
        self.0
    }
}

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
///
/// [`PropertyKeys::lookup`] first consults a bounded thread-local cache of
/// found names, so concurrent tasks do not all write the read lock's shared
/// counter on every property access. An entry is tagged with this table's
/// never-reused identity and the sweep epoch read under the lock. Only a
/// sweep removes a name or frees a key for reuse, and it advances the epoch
/// under the write lock before mutators resume, which invalidates every
/// cached entry; interning only adds names, which a cache never denies.
#[derive(Debug)]
pub(crate) struct PropertyKeys {
    budget: std::sync::Arc<crate::heap::budget::ByteBudget>,
    table: RwLock<KeyTable>,
    identity: u64,
    epoch: AtomicU64,
    binding_stamp: AtomicU64,
}

fn next_binding_stamp() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Default for PropertyKeys {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        PropertyKeys {
            budget: Default::default(),
            table: RwLock::default(),
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
            epoch: AtomicU64::new(0),
            binding_stamp: AtomicU64::new(next_binding_stamp()),
        }
    }
}

/// Found names of one key table on this thread (see [`PropertyKeys`]).
#[derive(Default)]
struct KeyCache {
    table: u64,
    epoch: u64,
    keys: HashMap<JsString, PropertyKey>,
}

/// Entries kept per thread before the cache starts over.
const KEY_CACHE_LIMIT: usize = 4096;

thread_local! {
    static KEY_CACHE: RefCell<KeyCache> = RefCell::new(KeyCache::default());
}

#[derive(Debug, Default)]
struct KeyTable {
    ids: HashMap<JsString, PropertyKey>,
    names: Vec<Option<StoredKey>>,
    symbols: HashMap<JsSymbol, PropertyKey>,
    free: Vec<u32>,
    reservation: crate::heap::budget::ByteReservation,
}

#[derive(Debug)]
enum StoredKey {
    Name(JsString),
    Symbol(JsSymbol),
}

impl KeyTable {
    fn insert(
        &mut self,
        entry: StoredKey,
        budget: &std::sync::Arc<crate::heap::budget::ByteBudget>,
    ) -> Option<PropertyKey> {
        if self.free.is_empty() {
            let Some(additional) = budget.reserve(512) else {
                budget.note_denial();
                return None;
            };
            self.reservation.merge(additional);
        }
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
    pub(crate) fn with_budget(budget: std::sync::Arc<crate::heap::budget::ByteBudget>) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }
    /// Runtime/collection identity in one word. Overflow disables caching;
    /// it never aliases a previous runtime or a reclaimed key slot.
    pub(crate) fn bind_name(&self, name: &JsString, cache: &AtomicU64) -> Option<PropertyKey> {
        self.bind_cached_name(cache, || self.intern(name))
    }

    /// A non-interning binding for a native's fixed property name. Absence
    /// cannot be cached: interning may add the name without changing the stamp.
    pub(crate) fn lookup_bound_name(
        &self,
        name: &JsString,
        cache: &AtomicU64,
    ) -> Option<PropertyKey> {
        self.bind_cached_name(cache, || self.lookup(name))
    }

    fn bind_cached_name(
        &self,
        cache: &AtomicU64,
        resolve: impl FnOnce() -> Option<PropertyKey>,
    ) -> Option<PropertyKey> {
        let stamp = self.binding_stamp.load(Ordering::Acquire);
        let cached = cache.load(Ordering::Relaxed);
        if stamp <= u64::from(u32::MAX) && cached >> 32 == stamp {
            return Some(PropertyKey(cached as u32, None));
        }
        let key = resolve()?;
        if stamp <= u64::from(u32::MAX) {
            cache.store((stamp << 32) | u64::from(key.0), Ordering::Relaxed);
        }
        Some(key)
    }

    /// The key of `name` if it is currently interned: from this thread's
    /// cache when valid (type docs), otherwise under a shared read lock.
    pub(crate) fn lookup(&self, name: &JsString) -> Option<PropertyKey> {
        let epoch = self.epoch.load(Ordering::Acquire);
        // A thread tearing down its locals simply takes the lock.
        let cached = KEY_CACHE
            .try_with(|cache| {
                let cache = cache.borrow();
                if cache.table == self.identity && cache.epoch == epoch {
                    cache.keys.get(name).copied()
                } else {
                    None
                }
            })
            .ok()
            .flatten();
        if cached.is_some() {
            return cached;
        }
        let (key, epoch) = {
            let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
            (
                table.ids.get(name).copied()?,
                self.epoch.load(Ordering::Relaxed),
            )
        };
        let _ = KEY_CACHE.try_with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.table != self.identity
                || cache.epoch != epoch
                || cache.keys.len() >= KEY_CACHE_LIMIT
            {
                cache.table = self.identity;
                cache.epoch = epoch;
                cache.keys.clear();
            }
            cache.keys.insert(name.clone(), key);
        });
        Some(key)
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
        let key = table.insert(StoredKey::Name(name.clone()), &self.budget)?;
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
        let key = table.insert(StoredKey::Symbol(symbol.clone()), &self.budget)?;
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
        // Invalidate every thread's cached names before any is removed.
        self.epoch.fetch_add(1, Ordering::Release);
        self.binding_stamp
            .store(next_binding_stamp(), Ordering::Release);
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
    HeapLimit,
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
    budget: std::sync::Arc<crate::heap::budget::ByteBudget>,
    charged_entries: usize,
    owner: Option<TaskId>,
    /// A realm intrinsic (module docs): ownerless, shared and immutable.
    intrinsic: bool,
    prototype: Option<ObjectId>,
    extensible: bool,
    entries: Vec<(PropertyKey, Property)>,
    shape: Option<std::sync::Arc<shape::ShapeHandle>>,
    /// Key to position in `entries`; present once there are more than
    /// [`INDEX_THRESHOLD`] entries.
    index: Option<HashMap<PropertyKey, u32>>,
    /// `[[ErrorData]]`: independent of properties and prototype changes.
    error_data: bool,
    /// Arguments exotic brand, independent of user properties/prototype.
    arguments_data: bool,
    /// Cold internal state stays boxed so allocation frames do not grow with
    /// each new language feature (in particular on 64 KiB VM workers).
    internal: Option<Box<InternalSlots>>,
    /// Specialized built-in payloads do not enlarge every ordinary object,
    /// function and binding cell through the HeapObject enum.
    data: Option<Box<ObjectData>>,
    reservation: crate::heap::budget::ByteReservation,
}

#[derive(Debug, Default)]
struct ObjectData {
    /// `[[NumberData]]` / `[[StringData]]` / `[[SymbolData]]`.
    primitive_data: Option<PrimitiveData>,
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

/// Charged per boxed internal slot beyond the box itself: the bounded
/// strings and vectors such a record owns (a locale tag is under 512 bytes).
const BOXED_SLOT_EXTRA: usize = 1024;

impl OrdinaryObject {
    /// The fixed metadata in this object's boxed internal slots
    /// (`HeapObject::boxed_bytes`). Every slot is installed before the
    /// object is allocated except the private-element record, whose
    /// elements are charged as they are added (`crate::vm::private`).
    pub(crate) fn boxed_bytes(&self) -> usize {
        fn boxed<T: ?Sized>(slot: &Option<Box<T>>) -> usize {
            slot.as_deref()
                .map_or(0, |body| std::mem::size_of_val(body) + BOXED_SLOT_EXTRA)
        }
        let internal = self.internal.as_deref().map_or(0, |internal| {
            std::mem::size_of_val(internal)
                + boxed(&internal.regexp_iterator)
                + boxed(&internal.engine_error)
        });
        internal
            + self.data.as_deref().map_or(0, |data| {
                std::mem::size_of_val(data)
                    + boxed(&data.locale_data)
                    + boxed(&data.number_format)
                    + boxed(&data.iterator)
                    + boxed(&data.plural_rules)
                    + boxed(&data.collator)
                    + boxed(&data.date_time_format)
                    + boxed(&data.list_format)
                    + boxed(&data.intl_service)
                    + boxed(&data.segmenter)
                    + boxed(&data.duration_format)
            })
    }

    fn data_mut(&mut self) -> &mut ObjectData {
        self.data.get_or_insert_with(Default::default)
    }

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
        self.data_mut().intl_service = Some(Box::new(IntlServiceSlots::DisplayNames(data)));
        self
    }
    pub(crate) fn display_names(&self) -> Option<&crate::intl::display_names::DisplayNames> {
        match self.data.as_deref()?.intl_service.as_deref() {
            Some(IntlServiceSlots::DisplayNames(data)) => Some(data),
            _ => None,
        }
    }

    pub(crate) fn with_duration_format(
        mut self,
        data: crate::intl::duration_format::DurationFormat,
    ) -> Self {
        self.data_mut().duration_format = Some(Box::new(data));
        self
    }
    pub(crate) fn duration_format(&self) -> Option<&crate::intl::duration_format::DurationFormat> {
        self.data.as_deref()?.duration_format.as_deref()
    }

    pub(crate) fn with_segmenter(mut self, data: crate::intl::segmenter::Data) -> Self {
        self.data_mut().segmenter = Some(Box::new(data));
        self
    }
    pub(crate) fn segmenter(&mut self) -> Option<&mut crate::intl::segmenter::Data> {
        self.data.as_deref_mut()?.segmenter.as_deref_mut()
    }
    pub(crate) fn with_list_format(mut self, data: crate::intl::list_format::ListFormat) -> Self {
        self.data_mut().list_format = Some(Box::new(data));
        self
    }

    pub(crate) fn list_format(&self) -> Option<&crate::intl::list_format::ListFormat> {
        self.data.as_deref()?.list_format.as_deref()
    }

    pub(crate) fn with_iterator(mut self, data: crate::vm::native::iterator::Data) -> Self {
        self.data_mut().iterator = Some(Box::new(data));
        self
    }

    pub(crate) fn iterator(&mut self) -> Option<&mut crate::vm::native::iterator::Data> {
        self.data.as_deref_mut()?.iterator.as_deref_mut()
    }

    /// An empty, extensible object with a null prototype, owned by `owner`
    /// (`None`: created by the embedder).
    pub(crate) fn new(heap: &crate::heap::Heap, owner: Option<TaskId>) -> OrdinaryObject {
        Self::with_budget(heap.byte_budget(), owner)
    }

    fn with_budget(
        budget: std::sync::Arc<crate::heap::budget::ByteBudget>,
        owner: Option<TaskId>,
    ) -> OrdinaryObject {
        OrdinaryObject {
            budget,
            reservation: Default::default(),
            charged_entries: 0,
            owner,
            intrinsic: false,
            prototype: None,
            extensible: true,
            entries: Vec::new(),
            shape: None,
            index: None,
            error_data: false,
            arguments_data: false,
            internal: None,
            data: None,
        }
    }

    /// A copy of a frozen intrinsic's plain property store, for an attempted
    /// change that the heap must not apply to the shared original
    /// (`crate::heap::storage`). `None` if internal slots are present: no
    /// frozen object carries them.
    pub(crate) fn stand_in(&self) -> Option<OrdinaryObject> {
        let internal = self.internal.is_some()
            || self.data.as_ref().is_some_and(|data| {
                data.locale_data.is_some()
                    || data.number_format.is_some()
                    || data.iterator.is_some()
                    || data.plural_rules.is_some()
                    || data.collator.is_some()
                    || data.date_time_format.is_some()
                    || data.list_format.is_some()
                    || data.intl_service.is_some()
                    || data.segmenter.is_some()
                    || data.duration_format.is_some()
            });
        (!internal).then(|| OrdinaryObject {
            owner: self.owner,
            intrinsic: self.intrinsic,
            prototype: self.prototype,
            extensible: self.extensible,
            entries: self.entries.clone(),
            index: self.index.clone(),
            error_data: self.error_data,
            arguments_data: self.arguments_data,
            data: self.data.as_ref().map(|data| {
                Box::new(ObjectData {
                    primitive_data: data.primitive_data.clone(),
                    date_value: data.date_value,
                    regexp: data.regexp.clone(),
                    number_format_bound: data.number_format_bound.clone(),
                    ..Default::default()
                })
            }),
            ..OrdinaryObject::with_budget(self.budget.clone(), None)
        })
    }

    /// An empty, extensible object owned by `owner` whose prototype is
    /// `prototype` (`Object.prototype` for literals and `Object()`).
    pub(crate) fn with_prototype(
        heap: &crate::heap::Heap,
        owner: TaskId,
        prototype: ObjectId,
    ) -> OrdinaryObject {
        OrdinaryObject {
            prototype: Some(prototype),
            ..OrdinaryObject::new(heap, Some(owner))
        }
    }

    /// An empty realm intrinsic with `prototype` (`crate::realm`). The realm
    /// defines its properties with [`OrdinaryObject::define_with`] before
    /// any task can reach it; nothing changes it afterwards.
    pub(crate) fn intrinsic(
        heap: &crate::heap::Heap,
        prototype: Option<ObjectId>,
    ) -> OrdinaryObject {
        OrdinaryObject {
            intrinsic: true,
            prototype,
            ..OrdinaryObject::new(heap, None)
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
        self.data_mut().primitive_data = Some(data);
        self
    }

    /// A wrapper's internal data, independent of its JavaScript prototype.
    pub(crate) fn primitive_data(&self) -> Option<&PrimitiveData> {
        self.data.as_deref()?.primitive_data.as_ref()
    }

    pub(crate) fn with_locale_data(mut self, data: crate::intl::tag::LanguageTag) -> Self {
        self.data_mut().locale_data = Some(Box::new(data));
        self
    }

    pub(crate) fn locale_data(&self) -> Option<&crate::intl::tag::LanguageTag> {
        self.data.as_deref()?.locale_data.as_deref()
    }

    pub(crate) fn with_regexp(
        mut self,
        compiled: std::sync::Arc<crate::regexp::object::Compiled>,
    ) -> Self {
        self.data_mut().regexp = Some(compiled);
        self
    }

    pub(crate) fn regexp(&self) -> Option<&std::sync::Arc<crate::regexp::object::Compiled>> {
        self.data.as_deref()?.regexp.as_ref()
    }

    pub(crate) fn with_relative_time_format(
        mut self,
        data: crate::intl::relative_time_format::RelativeTimeFormat,
    ) -> Self {
        self.data_mut().intl_service = Some(Box::new(IntlServiceSlots::RelativeTimeFormat(data)));
        self
    }
    pub(crate) fn relative_time_format(
        &self,
    ) -> Option<&crate::intl::relative_time_format::RelativeTimeFormat> {
        match self.data.as_deref()?.intl_service.as_deref() {
            Some(IntlServiceSlots::RelativeTimeFormat(data)) => Some(data),
            _ => None,
        }
    }
    pub(crate) fn with_plural_rules(
        mut self,
        data: crate::intl::plural_rules::PluralRules,
    ) -> Self {
        self.data_mut().plural_rules = Some(Box::new(data));
        self
    }
    pub(crate) fn with_date_time_format(
        mut self,
        data: crate::intl::date_time_format::DateTimeFormat,
    ) -> Self {
        self.data_mut().date_time_format = Some(Box::new(DateTimeFormatSlots {
            options: data,
            bound: None,
        }));
        self
    }
    pub(crate) fn date_time_format(
        &self,
    ) -> Option<&crate::intl::date_time_format::DateTimeFormat> {
        self.data
            .as_deref()?
            .date_time_format
            .as_ref()
            .map(|s| &s.options)
    }
    pub(crate) fn date_time_format_bound(&self) -> Option<&Value> {
        self.data
            .as_deref()?
            .date_time_format
            .as_ref()
            .and_then(|s| s.bound.as_ref())
    }
    pub(crate) fn set_date_time_format_bound(&mut self, value: Value) {
        self.data_mut()
            .date_time_format
            .as_mut()
            .expect("checked DateTimeFormat receiver")
            .bound = Some(value);
    }
    pub(crate) fn with_collator(mut self, data: crate::intl::collator::Collator) -> Self {
        self.data_mut().collator = Some(Box::new(CollatorSlots {
            options: data,
            bound: None,
        }));
        self
    }
    pub(crate) fn collator(&self) -> Option<&crate::intl::collator::Collator> {
        self.data
            .as_deref()?
            .collator
            .as_ref()
            .map(|slots| &slots.options)
    }
    pub(crate) fn collator_bound(&self) -> Option<&Value> {
        self.data
            .as_deref()?
            .collator
            .as_ref()
            .and_then(|slots| slots.bound.as_ref())
    }
    pub(crate) fn set_collator_bound(&mut self, value: Value) {
        self.data_mut()
            .collator
            .as_mut()
            .expect("Collator receiver checked")
            .bound = Some(value);
    }
    pub(crate) fn plural_rules(&self) -> Option<&crate::intl::plural_rules::PluralRules> {
        self.data.as_deref()?.plural_rules.as_deref()
    }
    pub(crate) fn with_number_format(
        mut self,
        data: crate::intl::number_format::NumberFormat,
    ) -> Self {
        self.data_mut().number_format = Some(Box::new(data));
        self
    }
    pub(crate) fn number_format(&self) -> Option<&crate::intl::number_format::NumberFormat> {
        self.data.as_deref()?.number_format.as_deref()
    }
    pub(crate) fn number_format_bound(&self) -> Option<&Value> {
        self.data.as_deref()?.number_format_bound.as_ref()
    }
    pub(crate) fn set_number_format_bound(&mut self, value: Value) {
        self.data_mut().number_format_bound = Some(value);
    }

    pub(crate) fn with_error_data(mut self) -> Self {
        self.error_data = true;
        self
    }

    pub(crate) fn is_error(&self) -> bool {
        self.error_data
    }

    pub(crate) fn with_arguments_data(mut self) -> Self {
        self.arguments_data = true;
        self
    }

    pub(crate) fn is_arguments(&self) -> bool {
        self.arguments_data
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
        self.data_mut().date_value = Some(time);
        self
    }

    pub(crate) fn date_value(&self) -> Option<f64> {
        self.data.as_deref()?.date_value
    }

    /// Caller validates the brand, ownership and intrinsic mutation boundary.
    pub(crate) fn set_date_value(&mut self, time: f64) {
        debug_assert!(self.date_value().is_some());
        self.data_mut().date_value = Some(time);
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

    fn cached_position(&self, key: PropertyKey, cache: &InlineCache) -> Option<usize> {
        // The first slot needs only its authoritative key check. In
        // particular, single-property records need no shape indirection or
        // shared atomic load on their entire named-access path.
        if self.entries.first().is_some_and(|(k, _)| *k == key) {
            return Some(0);
        }
        let Some(shape) = &self.shape else {
            return self.position(key);
        };
        if let Some(slot) = cache.slot(shape.id)
            && self.entries.get(slot).is_some_and(|(k, _)| *k == key)
        {
            return Some(slot);
        }
        let slot = self.position(key)?;
        cache.remember(shape.id, slot);
        Some(slot)
    }

    /// Immutable realm views retain the same layout hint as their store.
    pub(crate) fn shape_id(&self) -> u32 {
        self.shape.as_ref().map_or(0, |shape| shape.id)
    }

    pub(crate) fn get_own_cached(
        &self,
        key: PropertyKey,
        cache: &InlineCache,
    ) -> Option<&Property> {
        self.cached_position(key, cache).map(|i| &self.entries[i].1)
    }

    /// The own property `key`, if any.
    pub(crate) fn get_own(&self, key: PropertyKey) -> Option<&Property> {
        self.position(key).map(|i| &self.entries[i].1)
    }

    /// Own properties in creation order.
    pub(crate) fn properties(&self) -> impl Iterator<Item = (PropertyKey, &Property)> {
        self.entries.iter().map(|(k, p)| (*k, p))
    }

    fn push(&mut self, key: PropertyKey, property: Property) -> bool {
        // Include geometric property Vec capacity, the optional key index,
        // and bounded shape metadata. Keep the capacity charge after deletes.
        if self.entries.len() >= self.charged_entries {
            let Some(additional) = self.budget.reserve(512) else {
                self.budget.note_denial();
                return false;
            };
            self.reservation.merge(additional);
            self.charged_entries += 1;
        }
        self.shape = match &self.shape {
            Some(shape) => shape.append(key),
            None if self.entries.is_empty() => shape::Shape::append_empty(key),
            None => None,
        };
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
        true
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
            None if self.extensible => return self.push(key, property),
            None => return false,
        }
        true
    }

    /// OrdinarySet on this object as the receiver, for the cases that do
    /// not depend on the prototype chain: update an own writable data
    /// property, or add a new one to an extensible object with a null
    /// prototype.
    pub(crate) fn set(&mut self, key: PropertyKey, value: Value) -> SetOutcome {
        self.set_at(key, value, self.position(key))
    }

    pub(crate) fn set_cached(
        &mut self,
        key: PropertyKey,
        value: Value,
        cache: &InlineCache,
    ) -> SetOutcome {
        self.set_at(key, value, self.cached_position(key, cache))
    }

    fn set_at(&mut self, key: PropertyKey, value: Value, position: Option<usize>) -> SetOutcome {
        match position {
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
                if self.push(key, property) {
                    SetOutcome::Done
                } else {
                    SetOutcome::HeapLimit
                }
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
        self.push(key, property)
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
        self.shape = None;
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
            None if !self.push(key, property) => return DefineOutcome::HeapLimit,
            None => {}
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
    HeapLimit,
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
        if let Some(data) = &self.data {
            data.trace(visit);
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

impl Trace for ObjectData {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(segmenter) = &self.segmenter {
            segmenter.trace(visit);
        }
        if let Some(iterator) = &self.iterator {
            iterator.trace(visit);
        }
        if let Some(PrimitiveData::Symbol(symbol)) = &self.primitive_data {
            visit(crate::heap::gc::TraceEdge::Symbol(symbol.identity()));
        }
        // Options, DateValue, RegExp code and other wrapper data have no
        // heap edges. Cached bound callables form cycles with their receiver.
        if let Some(bound) = &self.number_format_bound {
            bound.trace(visit);
        }
        if let Some(bound) = self
            .date_time_format
            .as_ref()
            .and_then(|s| s.bound.as_ref())
        {
            bound.trace(visit);
        }
        if let Some(bound) = self.collator.as_ref().and_then(|s| s.bound.as_ref()) {
            bound.trace(visit);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn primitive_wrapper_slots_stay_compact_for_small_workers() {
        // Keep wrapper metadata small even in its optional payload: larger
        // construction temporaries have overflowed 64 KiB VM paths.
        assert!(std::mem::size_of::<super::PrimitiveData>() <= 16);
    }

    #[test]
    fn specialized_object_slots_do_not_enlarge_every_heap_object() {
        // HeapObject is the allocation size for functions, cells and forms
        // too. Optional built-in payloads previously inflated it to 304 bytes
        // on 64-bit targets, multiplying each task's bootstrap/catalog cost.
        assert!(std::mem::size_of::<crate::heap::HeapObject>() <= 192);
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

    /// The thread-local lookup cache (light-cljs-471) never outlives a
    /// sweep that removed or reused a key, and never answers for another
    /// table on the same thread.
    #[test]
    fn cached_lookups_follow_sweeps_and_stay_within_their_table() {
        let keys = PropertyKeys::default();
        let dead = keys.intern(&s("dead")).unwrap();
        let live = keys.intern(&s("live")).unwrap();
        // Cached now, on this thread.
        assert_eq!(keys.lookup(&s("dead")), Some(dead));
        assert_eq!(keys.lookup(&s("live")), Some(live));
        keys.sweep(|key| key == live);
        assert_eq!(keys.lookup(&s("dead")), None);
        assert_eq!(keys.lookup(&s("live")), Some(live));
        // The freed slot now names something else; the old name is new.
        let reused = keys.intern(&s("other")).unwrap();
        assert_eq!(reused.0, dead.0);
        assert_eq!(keys.lookup(&s("other")), Some(reused));
        assert_eq!(keys.lookup(&s("dead")), None);
        let again = keys.intern(&s("dead")).unwrap();
        assert_ne!(again, reused);
        assert_eq!(keys.lookup(&s("dead")), Some(again));

        let second = PropertyKeys::default();
        assert_eq!(second.lookup(&s("live")), None);
        let first = second.intern(&s("first")).unwrap();
        second.intern(&s("padding")).unwrap();
        let theirs = second.intern(&s("live")).unwrap();
        assert_ne!(theirs, live);
        assert_eq!(second.lookup(&s("live")), Some(theirs));
        assert_eq!(keys.lookup(&s("live")), Some(live));
        assert_eq!(second.lookup(&s("first")), Some(first));
    }

    #[test]
    fn static_names_bind_only_found_keys_and_rebind_after_sweep_or_table_change() {
        let cache = AtomicU64::new(0);
        let name = s("static-name-probe");
        let keys = PropertyKeys::default();
        assert_eq!(keys.lookup_bound_name(&name, &cache), None);
        assert_eq!(keys.len(), 0);
        let key = keys.intern(&name).unwrap();
        assert_eq!(keys.lookup_bound_name(&name, &cache), Some(key));
        keys.sweep(|_| false);
        let reused = keys.intern(&s("different-name")).unwrap();
        assert_eq!(reused, key);
        assert_eq!(keys.lookup_bound_name(&name, &cache), None);
        let new = keys.intern(&name).unwrap();
        assert_ne!(new, reused);
        assert_eq!(keys.lookup_bound_name(&name, &cache), Some(new));

        let other = PropertyKeys::default();
        other.intern(&s("padding")).unwrap();
        other.intern(&s("more-padding")).unwrap();
        let theirs = other.intern(&name).unwrap();
        assert_ne!(theirs, new);
        assert_eq!(other.lookup_bound_name(&name, &cache), Some(theirs));
        assert_eq!(keys.lookup_bound_name(&name, &cache), Some(new));
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
            let mut object = OrdinaryObject::new(&crate::heap::Heap::default(), None);
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
            let mut object = OrdinaryObject::new(&crate::heap::Heap::default(), None);
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
        let mut o = OrdinaryObject::new(&crate::heap::Heap::default(), None);
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
            let mut o = OrdinaryObject::new(&crate::heap::Heap::default(), None);
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
        let mut o = OrdinaryObject::new(&crate::heap::Heap::default(), Some(TaskId::from_raw(1)));
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
