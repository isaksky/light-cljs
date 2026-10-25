//! The shared heap: object storage, stable object identity, roots, and the
//! precise nonmoving mark/sweep collector (`docs/design.md` §11).
//!
//! * One logical heap per [`Runtime`](crate::Runtime), shared by every task
//!   (no per-task or per-worker heaps).
//! * **Nonmoving, stable identity.** An object is named by an `ObjectId`
//!   (slot index + generation) that never changes for the object's lifetime,
//!   and its storage is a separate allocation (`Box<HeapObject>`) that is
//!   never moved: growing the object table moves slot headers, not objects.
//! * **Reclamation.** [`gc`] implements a precise stop-the-world mark/sweep.
//!   Anything not reachable from a root is reclaimed, including cycles. A
//!   task's allocations are not tied to the task: finishing a task drops its
//!   frames, and only what is still reachable (for example from a returned
//!   [`Rooted`] result) survives.
//! * **Stale identities are rejected.** Sweeping a slot bumps its generation
//!   before the slot is reused, and every object lookup compares the
//!   generation, so an `ObjectId` naming a reclaimed object resolves to
//!   nothing (never to the slot's new occupant). A slot whose generation
//!   would wrap is retired instead of reused. Embedders see this as
//!   [`HandleError::Reclaimed`] from a [`WeakHandle`].
//! * Raw `ObjectId`s are crate-private. Embedders reach heap values only
//!   through persistent roots ([`Rooted`]), scoped handles ([`Local`]), or
//!   non-rooting [`WeakHandle`]s; see [`roots`].
//!
//! **Locks.** Every heap lock is a short metadata lock; none is held while
//! bytecode executes. Lock order (a thread only ever acquires in this order):
//! `gc` (collector state and the per-thread mutator registry; see [`gc`] for
//! the safepoint rendezvous and its condition variable) → `roots` → `scopes` → `sources`
//! → task bodies (collector only) → `objects`. The collector holds all of
//! them for the duration of one stop-the-world collection. A run holds its
//! own task body while it takes `gc` (to poll, or for a nested registration
//! such as an atom operation); that is no cycle because the collector never
//! locks the body of an executing task.
//!
//! **No unsafe code.** Objects are only reached through the `objects` lock,
//! so the collector needs no raw pointers; `Box` provides address stability.
//!
//! **Storage outside the heap.** [`Executable`](crate::Executable) code is
//! independently owned immutable `Arc` storage, shared by tasks and not
//! managed by the collector. Its constants are traced through every live task
//! running it (`RootCategory::ExecutableConstant`); they are primitives only
//! in this slice. String values (`crate::value::JsString`) are immutable
//! `Arc` storage outside the table as well: a string has no heap edges, so
//! the collector neither traces nor sweeps it. Task VM state is owned by the task and traced as a root
//! source while the task handle exists.
//! Persistent collections ([`crate::collections`]) are heap objects whose
//! bodies are immutable `Arc` trie nodes outside the object table, shared
//! between versions; the owning object's `Trace` reports every heap edge in
//! its trie, and reads take the `objects` lock only to clone the body's `Arc`.
//! Atoms ([`crate::atom`]) likewise hold an `Arc` cell outside the table;
//! their operations use Rust atomics on it after cloning the `Arc`, and the
//! atom module (not the heap) contains the crate's only `unsafe` code.
//!
//! **Publication.** Each slot also carries a sticky known-shareable bit,
//! set when a persistent object is allocated from known-shareable parts or
//! proven shareable by the publication check (`value::share`).

pub mod gc;
#[cfg(test)]
mod inventory;
pub mod roots;
mod stats;

use std::any::Any;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};

pub use gc::{
    CollectError, CollectStats, CollectionObserver, CollectionReport, MutatorState,
    ThreadRegistration,
};
use gc::{GcState, RootSource, SourceRegistry, Trace};
pub use roots::{HandleError, Local, Rooted, Scope, WeakHandle};
use roots::{RootTable, ScopeTable};
pub use stats::{HeapUsage, ObjectCounts};

use crate::atom::AtomCell;
use crate::collections::{Builder, Occurrence, PMap, PVector};
use crate::object::array::ArrayObject;
use crate::object::{OrdinaryObject, PropertyKeys};
use crate::realm::Realm;
use crate::value::Value;
use crate::value::share::{self, Shareability};
use crate::vm::builtin::BuiltinFunction;
use crate::vm::function::{BindingCell, BoundFunction, FunctionObject};

/// Stable identity of a heap object: slot index plus the slot's generation
/// at allocation time. Crate-private: see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ObjectId {
    index: u32,
    generation: u32,
}

impl ObjectId {
    /// The identity as one integer: stable for the object's lifetime and
    /// never reused for another object (the slot generation differs).
    pub(crate) fn to_bits(self) -> u64 {
        (u64::from(self.index) << 32) | u64::from(self.generation)
    }

    #[cfg(test)]
    pub(crate) fn from_index(index: u32) -> Self {
        ObjectId {
            index,
            generation: 0,
        }
    }
}

/// Kinds of heap object. Append-only registry: later pellets add variants at
/// the end and handle them in every exhaustive match.
/// Inspected by tests today; property access and native operations dispatch
/// on it in later pellets.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObjectKind {
    /// An ordinary JavaScript object (`crate::object`): task-owned mutable
    /// own properties, a prototype slot and an extensible flag.
    PlainObject,
    /// An object carrying a Rust [`NativePayload`] whose heap edges are
    /// reported by its [`Trace`] implementation (the analogue of a QuickJS
    /// class `gc_mark` hook).
    Native,
    /// A persistent vector ([`crate::collections`]).
    Vector,
    /// A persistent map ([`crate::collections`]).
    Map,
    /// A syntax occurrence: a value plus its source provenance
    /// ([`crate::collections`]).
    Occurrence,
    /// A task-owned transient builder (`crate::collections::builder`).
    Builder,
    /// A function object (`crate::vm::function`): callable by `Call`.
    Function,
    /// A binding cell (`crate::vm::function`): the task-owned storage of a
    /// mutable binding captured by closures.
    Cell,
    /// An atomic reference to a shareable value (`crate::atom`).
    Atom,
    /// A built-in function of the realm (`crate::vm::builtin`): callable
    /// Rust code with an immutable property store, shared by every task.
    Builtin,
    /// A JavaScript array object (`crate::object::array`): task-owned, with
    /// element storage and an automatic `length` beside the properties of
    /// an ordinary object.
    Array,
    /// A callable with immutable bound target, receiver and arguments.
    BoundFunction,
    /// Task-owned JavaScript Map with insertion-ordered entries.
    JsMap,

    ArrayBuffer,
    TypedArray,
    Promise,
    PromiseReaction,
    PromiseResolving,
    Generator,
    WeakMap,
    AsyncState,
    WeakRef,
    FinalizationRegistry,
    ModuleNamespace,
    DisposableStack,
    Proxy,
    JsSet,
    WeakSet,
}

/// Rust data stored inside a heap object, for native operations.
///
/// The payload's [`Trace`] must report every strong [`ObjectId`] it stores;
/// weak slots and ephemeron tables must instead be enumerated by
/// [`Trace::trace_conditional`]. Such mutable conditional payloads are
/// task-owned, and their callback/held-data fields remain strong edges. An
/// unreported edge is a dangling reference after the next collection (seen as
/// a stale identity, never as memory unsafety). A payload must not own a
/// [`Rooted`] or a cleared-slot notification: those roots would keep their
/// targets (and any cycles through them) alive forever. Notification queues
/// belong outside the heap. Reclaimed payloads are dropped after the collector
/// has released every heap lock. Payloads are reached only under the heap's object lock,
/// so they need `Send` but not `Sync`.
///
/// Every payload type must also declare its publication category
/// ([`NativePayload::shareability`]; see [`crate::value::share`]).
pub(crate) trait NativePayload: Trace + Any + Send + fmt::Debug {
    /// The payload's publication category. It must not change during the
    /// payload's lifetime, and a [`Shareability::Persistent`] payload must
    /// never change the edges its [`Trace`] reports: the publication check
    /// walks those edges and caches a positive answer in the object's slot.
    /// Called under the object lock: it must only return the category (same
    /// restrictions as [`Trace`]).
    fn shareability(&self) -> Shareability;
}

/// Storage of one heap object.
#[derive(Debug)]
pub(crate) enum HeapObject {
    /// An ordinary JavaScript object: task-owned and mutable by its owning
    /// task's run only, in place under the object lock (`crate::object`),
    /// or a realm intrinsic (`Object.prototype`), immutable once installed.
    Plain(OrdinaryObject),
    /// Constructed by tests today; resumable native continuations
    /// (clj-quickjs-12) and later native kinds construct it.
    #[allow(dead_code)]
    Native(Box<dyn NativePayload>),
    /// Persistent collections and occurrences: immutable once allocated.
    /// Their bodies are independently owned immutable `Arc` storage whose
    /// heap edges are reported by `Trace` (see [`crate::collections`]).
    Vector(PVector),
    Map(PMap),
    Occurrence(Occurrence),
    /// A task-owned transient builder: the one mutable collection kind. Only
    /// its owning task's run mutates it, in place under the object lock; an
    /// open builder's `Trace` reports its draft's contents.
    Builder(Builder),
    /// A function object over shared immutable code and its captured values
    /// (`crate::vm::function`).
    Function(FunctionObject),
    /// A binding cell: mutable, task-owned; only its owning task's run
    /// changes it, in place under the object lock (`crate::vm::function`).
    Cell(BindingCell),
    /// An atom (`crate::atom`): an explicit concurrent reference. Its cell is
    /// independently owned `Arc` storage so operations run on Rust atomics
    /// without holding the object lock; its `Trace` reports the current value.
    Atom(Arc<AtomCell>),
    /// A built-in function (`crate::vm::builtin`): a realm intrinsic,
    /// immutable after the realm is built and shared by every task.
    Builtin(BuiltinFunction),
    /// A JavaScript array object: task-owned and mutable by its owning
    /// task's run only, in place under the object lock
    /// (`crate::object::array`). Boxed: its element storage would otherwise
    /// enlarge every heap object.
    Array(Box<ArrayObject>),
    /// A bound callable (`crate::vm::function`, clj-quickjs-103).
    BoundFunction(BoundFunction),
    /// A task-owned JavaScript Map (distinct from the persistent HAMT).
    JsMap(Box<crate::object::map::MapObject>),

    ArrayBuffer(Box<crate::object::array_buffer::ArrayBuffer>),
    TypedArray(Box<crate::object::typed_array::TypedArray>),
    Promise(crate::vm::promise::Promise),
    PromiseReaction(crate::vm::promise::Reaction),
    PromiseResolving(crate::vm::promise::Resolving),
    Generator(Box<crate::vm::generator::Generator>),
    WeakMap(Box<crate::object::weak_map::WeakMapObject>),
    AsyncState(Box<crate::vm::asynchronous::AsyncState>),
    WeakRef(Box<crate::object::weak_ref::WeakRefObject>),
    FinalizationRegistry(Box<crate::object::finalization_registry::FinalizationRegistryObject>),
    ModuleNamespace(Box<crate::module::namespace::Namespace>),
    DisposableStack(Box<crate::object::disposable_stack::DisposableStack>),
    Proxy(crate::vm::proxy::Proxy),
    JsSet(Box<crate::object::map::MapObject>),
    WeakSet(Box<crate::object::weak_map::WeakMapObject>),
}

impl HeapObject {
    /// A new empty ordinary object created by the embedder (no owner task;
    /// see `crate::object`).
    pub(crate) fn host_object() -> HeapObject {
        HeapObject::Plain(OrdinaryObject::new(None))
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> ObjectKind {
        match self {
            HeapObject::Plain(_) => ObjectKind::PlainObject,
            HeapObject::Native(_) => ObjectKind::Native,
            HeapObject::Vector(_) => ObjectKind::Vector,
            HeapObject::Map(_) => ObjectKind::Map,
            HeapObject::Occurrence(_) => ObjectKind::Occurrence,
            HeapObject::Builder(_) => ObjectKind::Builder,
            HeapObject::Function(_) => ObjectKind::Function,
            HeapObject::Cell(_) => ObjectKind::Cell,
            HeapObject::Atom(_) => ObjectKind::Atom,
            HeapObject::Builtin(_) => ObjectKind::Builtin,
            HeapObject::Array(_) => ObjectKind::Array,
            HeapObject::JsMap(_) => ObjectKind::JsMap,
            HeapObject::JsSet(_) => ObjectKind::JsSet,
            HeapObject::WeakSet(_) => ObjectKind::WeakSet,
            HeapObject::WeakMap(_) => ObjectKind::WeakMap,
            HeapObject::WeakRef(_) => ObjectKind::WeakRef,
            HeapObject::FinalizationRegistry(_) => ObjectKind::FinalizationRegistry,
            HeapObject::DisposableStack(_) => ObjectKind::DisposableStack,
            HeapObject::BoundFunction(_) => ObjectKind::BoundFunction,
            HeapObject::ArrayBuffer(_) => ObjectKind::ArrayBuffer,
            HeapObject::TypedArray(_) => ObjectKind::TypedArray,
            HeapObject::Promise(_) => ObjectKind::Promise,
            HeapObject::PromiseReaction(_) => ObjectKind::PromiseReaction,
            HeapObject::PromiseResolving(_) => ObjectKind::PromiseResolving,
            HeapObject::Generator(_) => ObjectKind::Generator,
            HeapObject::AsyncState(_) => ObjectKind::AsyncState,
            HeapObject::ModuleNamespace(_) => ObjectKind::ModuleNamespace,
            HeapObject::Proxy(_) => ObjectKind::Proxy,
        }
    }
}

impl Trace for HeapObject {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            HeapObject::Plain(o) => o.trace(visit),
            HeapObject::Native(payload) => payload.trace(visit),
            HeapObject::Vector(v) => v.trace(visit),
            HeapObject::Map(m) => m.trace(visit),
            HeapObject::Occurrence(o) => o.trace(visit),
            HeapObject::Builder(b) => b.trace(visit),
            HeapObject::Function(f) => f.trace(visit),
            HeapObject::Cell(c) => c.trace(visit),
            HeapObject::Atom(a) => a.trace(visit),
            HeapObject::Builtin(f) => f.trace(visit),
            HeapObject::Array(a) => a.trace(visit),
            HeapObject::JsMap(m) | HeapObject::JsSet(m) => m.trace(visit),
            HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => m.trace(visit),
            HeapObject::WeakRef(m) => m.trace(visit),
            HeapObject::FinalizationRegistry(m) => m.trace(visit),
            HeapObject::DisposableStack(m) => m.trace(visit),
            HeapObject::BoundFunction(f) => f.trace(visit),
            HeapObject::ArrayBuffer(b) => b.trace(visit),
            HeapObject::TypedArray(a) => a.trace(visit),
            HeapObject::Promise(p) => p.trace(visit),
            HeapObject::PromiseReaction(r) => r.trace(visit),
            HeapObject::PromiseResolving(r) => r.trace(visit),
            HeapObject::Generator(g) => g.trace(visit),
            HeapObject::AsyncState(a) => a.trace(visit),
            HeapObject::ModuleNamespace(ns) => ns.trace(visit),
            HeapObject::Proxy(p) => p.trace(visit),
        }
    }

    fn trace_conditional(&mut self, visit: &mut dyn gc::ConditionalVisitor) {
        match self {
            HeapObject::Plain(o) => o.trace_conditional(visit),
            HeapObject::Native(p) => p.trace_conditional(visit),
            HeapObject::Vector(v) => v.trace_conditional(visit),
            HeapObject::Map(m) => m.trace_conditional(visit),
            HeapObject::Occurrence(o) => o.trace_conditional(visit),
            HeapObject::Builder(b) => b.trace_conditional(visit),
            HeapObject::Function(f) => f.trace_conditional(visit),
            HeapObject::Cell(c) => c.trace_conditional(visit),
            // Atom contents are exclusively strong, read through its Arc.
            HeapObject::Atom(_) => {}
            HeapObject::Builtin(f) => f.trace_conditional(visit),
            HeapObject::Array(a) => a.trace_conditional(visit),
            HeapObject::BoundFunction(f) => f.trace_conditional(visit),
            HeapObject::JsMap(m) | HeapObject::JsSet(m) => m.trace_conditional(visit),
            HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => m.trace_conditional(visit),
            HeapObject::WeakRef(m) => m.trace_conditional(visit),
            HeapObject::FinalizationRegistry(m) => m.trace_conditional(visit),
            HeapObject::DisposableStack(m) => m.trace_conditional(visit),
            HeapObject::ArrayBuffer(b) => b.trace_conditional(visit),
            HeapObject::TypedArray(a) => a.trace_conditional(visit),
            HeapObject::Promise(p) => p.trace_conditional(visit),
            HeapObject::PromiseReaction(r) => r.trace_conditional(visit),
            HeapObject::PromiseResolving(r) => r.trace_conditional(visit),
            HeapObject::Generator(g) => g.trace_conditional(visit),
            HeapObject::AsyncState(a) => a.trace_conditional(visit),
            HeapObject::ModuleNamespace(_) => {}
            HeapObject::Proxy(_) => {}
        }
    }
}

/// One object-table slot.
#[derive(Debug)]
struct Slot {
    /// Incremented each time the slot's object is reclaimed.
    generation: u32,
    /// Mark bit; only set during a collection.
    marked: bool,
    /// The object is known to be transitively shareable
    /// ([`crate::value::share`]): set at allocation or by a successful
    /// publication check, never cleared while the object lives (the sticky
    /// bit invariant documented there), and rewritten when the slot is reused.
    shareable: bool,
    object: Option<Box<HeapObject>>,
}

/// The object table: slots with generation-checked lookup and a free list.
#[derive(Debug, Default)]
struct ObjectTable {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: usize,
}

impl ObjectTable {
    fn alloc(&mut self, object: HeapObject, shareable: bool) -> Option<ObjectId> {
        let object = Some(Box::new(object));
        let index = match self.free.pop() {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                slot.object = object;
                slot.shareable = shareable;
                index
            }
            None => {
                let index = u32::try_from(self.slots.len()).ok()?;
                self.slots.push(Slot {
                    generation: 0,
                    marked: false,
                    shareable,
                    object,
                });
                index
            }
        };
        self.live += 1;
        Some(ObjectId {
            index,
            generation: self.slots[index as usize].generation,
        })
    }

    /// True when `v` is a primitive or a live object known to be transitively
    /// shareable: its sticky bit is set, or it is an explicit concurrent
    /// reference (shareable without descending into it).
    fn known_shareable(&self, v: &Value) -> bool {
        let Value::Object(id) = *v else {
            return true;
        };
        self.slot(id).is_some_and(|slot| {
            slot.shareable
                || slot
                    .object
                    .as_deref()
                    .is_some_and(|o| share::classify(o) == Shareability::ConcurrentReference)
        })
    }

    /// The live slot named by `id`, or `None` if `id` is stale or foreign.
    fn slot(&self, id: ObjectId) -> Option<&Slot> {
        self.slots
            .get(id.index as usize)
            .filter(|s| s.generation == id.generation && s.object.is_some())
    }

    fn get(&self, id: ObjectId) -> Option<&HeapObject> {
        self.slot(id)?.object.as_deref()
    }

    fn get_mut(&mut self, id: ObjectId) -> Option<&mut HeapObject> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.object.as_deref_mut()
    }

    /// Set the mark bit of a live object and report its children. Returns
    /// `false` (and reports nothing) for stale ids and already-marked objects.
    fn mark(&mut self, id: ObjectId, children: &mut dyn FnMut(crate::heap::gc::TraceEdge)) -> bool {
        let Some(slot) = self.slots.get_mut(id.index as usize) else {
            return false;
        };
        if slot.generation != id.generation || slot.marked {
            return false;
        }
        let Some(object) = slot.object.as_deref() else {
            return false;
        };
        slot.marked = true;
        object.trace(children);
        true
    }

    /// The values of the retired boxes of every live atom (`crate::atom`), taken
    /// while the world is stopped so the caller can drop them after releasing
    /// the heap locks. Each atom's retire list lock is a leaf lock.
    fn take_atom_retirements(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for slot in &self.slots {
            if let Some(HeapObject::Atom(cell)) = slot.object.as_deref() {
                out.extend(cell.take_retired());
            }
        }
        out
    }

    /// Free every unmarked object and clear the marks. The freed objects are
    /// returned so the caller can drop them after releasing the heap locks.
    fn sweep(&mut self) -> Vec<HeapObject> {
        let mut garbage = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.marked {
                slot.marked = false;
                continue;
            }
            let Some(object) = slot.object.take() else {
                continue;
            };
            garbage.push(*object);
            // A slot whose generation would wrap is retired, never reused, so
            // an old id can never match a new occupant.
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                // `index` fits u32: slots are only created with u32 indices.
                self.free.push(index as u32);
            }
        }
        self.live -= garbage.len();
        garbage
    }
}

/// Counters describing heap usage, for tests and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapStats {
    /// Live (allocated and not yet reclaimed) objects.
    pub objects: usize,
    /// Live persistent roots ([`Rooted`] handles, including task results).
    pub persistent_roots: usize,
    /// Completed collections.
    pub collections: u64,
}

/// The object-table lock held by a test ([`Heap::hold_objects`]).
#[cfg(test)]
pub(crate) struct ObjectsHeld<'a>(#[allow(dead_code)] MutexGuard<'a, ObjectTable>);

/// The heap shared by all tasks of one runtime.
#[derive(Debug, Default)]
pub(crate) struct Heap {
    /// Immutable host zone, shared by every task/world of this runtime.
    pub(crate) date_zone: std::sync::OnceLock<crate::host::TimeZone>,
    /// Collector state: the per-thread mutator registry, pending
    /// collection requests, the collection counter.
    gc: Mutex<GcState>,
    /// Notified when a thread stops at a safepoint or leaves heap access,
    /// and when a collection request ends ([`gc`] module docs).
    gc_changed: Condvar,
    /// In-flight task collection-point batch; no heap edges. This metadata
    /// lock is never held across a rendezvous, notification or wait.
    point_batch: Mutex<Weak<gc::PointBatch>>,
    /// All post-pause deliveries, including explicit embedder collections.
    /// Weak entries carry no heap edges and are pruned on registration.
    notifications: Mutex<Vec<Weak<gc::NotificationDelivery>>>,
    /// True while a collection request is pending: what VM polls read.
    /// Written only under the `gc` lock.
    requested: AtomicBool,
    /// Persistent root slots.
    roots: Mutex<RootTable>,
    /// Locals of open scopes.
    scopes: Mutex<ScopeTable>,
    /// Registered root sources (tasks, and later cells, atoms, ...).
    sources: Mutex<SourceRegistry>,
    /// The object table.
    objects: Mutex<ObjectTable>,
    /// The runtime-wide property-key table (`crate::object`). Mutators take
    /// its leaf lock outside other heap locks; the stopped collector sweeps
    /// it with only `gc` held, after releasing the other tracing locks.
    keys: PropertyKeys,
    /// Collection observers ([`gc::CollectionObserver`]). A leaf lock: never
    /// held with another heap lock or while an observer runs.
    observers: Mutex<Vec<Weak<dyn gc::CollectionObserver>>>,
    /// The realm's intrinsic objects (`crate::realm`), built on first use;
    /// their identities are roots of category `Intrinsic`. Its build lock
    /// is a leaf taken only before the realm exists.
    realm: Realm,
    /// Runtime-owned immutable symbol leaves; a leaf metadata lock,
    /// independent of GC because neither keys nor symbols have heap edges.
    pub(crate) symbols: crate::value::symbol::SymbolRegistry,
}

/// Lock a metadata mutex, recovering from poisoning. The protected data is
/// plain bookkeeping that every critical section leaves consistent, so a
/// panic elsewhere cannot leave it half-updated.
fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Heap {
    /// Allocate a new object and return its identity, or `None` when the
    /// object table has no index left (2^32 slots).
    ///
    /// The caller must be a registered mutator of this thread (a run guard,
    /// an unreleased scope, or a native operation; see [`gc`]) and must make
    /// the returned id reachable from a root before its next safepoint: a
    /// collection cannot start while it is between safepoints. Every
    /// allocation path reachable from outside the crate goes through such a
    /// registration; a released scope cannot be reached at all.
    pub(crate) fn alloc(&self, object: HeapObject) -> Option<ObjectId> {
        lock(&self.objects).alloc(object, false)
    }

    /// [`Heap::alloc`] for a persistent object, recording at allocation
    /// whether it is already known to be transitively shareable
    /// ([`crate::value::share`]): it is when every value in `parts` is
    /// known shareable. `parts` must cover every direct edge of `object`
    /// that is not inside a part (for an update: the old collection plus the
    /// values it adds); `None` means "every direct edge of `object`".
    /// One object-table lock; the work is O(parts).
    pub(crate) fn alloc_persistent(
        &self,
        object: HeapObject,
        parts: Option<&[Value]>,
    ) -> Option<ObjectId> {
        debug_assert_eq!(share::classify(&object), Shareability::Persistent);
        let mut table = lock(&self.objects);
        let shareable = match parts {
            Some(parts) => parts.iter().all(|v| table.known_shareable(v)),
            None => {
                let mut all = true;
                object.trace(&mut |edge| {
                    if let gc::TraceEdge::Object(id) = edge {
                        all = all && table.known_shareable(&Value::Object(id));
                    }
                });
                all
            }
        };
        table.alloc(object, shareable)
    }

    /// Run `f` on a live object and its known-shareable bit under the object
    /// lock; `None` if `id` is stale or foreign. Same rules as [`Heap::read`].
    pub(crate) fn read_shareable<R>(
        &self,
        id: ObjectId,
        f: impl FnOnce(&HeapObject, bool) -> R,
    ) -> Option<R> {
        let table = lock(&self.objects);
        let slot = table.slot(id)?;
        slot.object.as_deref().map(|o| f(o, slot.shareable))
    }

    /// Set the known-shareable bit of every live object in `ids`: the caller
    /// has just proved each one transitively shareable. Stale ids are
    /// ignored. One object-table lock.
    pub(crate) fn mark_shareable(&self, ids: &[ObjectId]) {
        let mut table = lock(&self.objects);
        for &id in ids {
            let Some(slot) = table.slots.get_mut(id.index as usize) else {
                continue;
            };
            if slot.generation == id.generation && slot.object.is_some() {
                slot.shareable = true;
            }
        }
    }

    /// [`Heap::mark_shareable`] for the result of a publication walk, unless
    /// one of the objects changed since the walk read it: in one object-table
    /// critical section, if `changed` holds for any live object in
    /// `ids`, nothing is marked and the result is false (the caller walks
    /// again); otherwise every live object in `ids` is marked and the result
    /// is true. The one change it guards against is a function object
    /// gaining its own-property record (`crate::vm::function`, "Own
    /// properties"), which takes the same lock and refuses a known function.
    pub(crate) fn mark_shareable_unless(
        &self,
        ids: &[ObjectId],
        changed: impl Fn(&HeapObject) -> bool,
    ) -> bool {
        let mut table = lock(&self.objects);
        if ids.iter().any(|&id| table.get(id).is_some_and(&changed)) {
            return false;
        }
        for &id in ids {
            let Some(slot) = table.slots.get_mut(id.index as usize) else {
                continue;
            };
            if slot.generation == id.generation && slot.object.is_some() {
                slot.shareable = true;
            }
        }
        true
    }

    /// Run `f` on a live object and its known-shareable bit under the object
    /// lock, with mutable access; `None` if `id` is stale. Same rules as
    /// [`Heap::with_object_mut`]; used to attach a function's own-property
    /// record only while the function is not known shareable
    /// (`crate::vm::function`).
    pub(crate) fn with_object_mut_known<R>(
        &self,
        id: ObjectId,
        f: impl FnOnce(&mut HeapObject, bool) -> R,
    ) -> Option<R> {
        let mut table = lock(&self.objects);
        let slot = table.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let known = slot.shareable;
        slot.object.as_deref_mut().map(|o| f(o, known))
    }

    /// The known-shareable bit of a live object (tests).
    #[cfg(test)]
    pub(crate) fn known_shareable(&self, id: ObjectId) -> Option<bool> {
        lock(&self.objects).slot(id).map(|s| s.shareable)
    }

    /// True when `id` names a live (not reclaimed) object of this heap.
    pub(crate) fn is_live(&self, id: ObjectId) -> bool {
        lock(&self.objects).slot(id).is_some()
    }

    /// Run `f` on a live object under the object lock; `None` if `id` is
    /// stale or foreign. `f` must not touch the heap (it would deadlock) and
    /// should only copy out what it needs (for collections: an `Arc` clone of
    /// the immutable body), so the lock is held briefly.
    pub(crate) fn read<R>(&self, id: ObjectId, f: impl FnOnce(&HeapObject) -> R) -> Option<R> {
        lock(&self.objects).slot(id)?.object.as_deref().map(f)
    }

    /// The kind of a live object, or `None` if `id` is stale or foreign.
    #[cfg(test)]
    pub(crate) fn kind(&self, id: ObjectId) -> Option<ObjectKind> {
        lock(&self.objects).get(id).map(HeapObject::kind)
    }

    /// Run `f` on a live object under the object lock; `None` if `id` is
    /// stale. `f` must not touch the heap (it would deadlock). Only mutable
    /// kinds may be changed: transient builders, binding cells and ordinary
    /// objects (each by its owning task's run), a realm intrinsic while its
    /// realm is being built, before anything can reach it (`crate::realm`),
    /// and, in tests, native payloads. Persistent kinds never change.
    pub(crate) fn with_object_mut<R>(
        &self,
        id: ObjectId,
        f: impl FnOnce(&mut HeapObject) -> R,
    ) -> Option<R> {
        lock(&self.objects).get_mut(id).map(f)
    }

    /// Address of a live object's storage, to test that the heap never moves
    /// objects.
    #[cfg(test)]
    pub(crate) fn address(&self, id: ObjectId) -> Option<usize> {
        lock(&self.objects)
            .get(id)
            .map(|o| std::ptr::from_ref(o) as usize)
    }

    pub(crate) fn stats(&self) -> HeapStats {
        // One lock at a time (separate statements), so the lock order holds.
        let collections = lock(&self.gc).collections;
        let persistent_roots = lock(&self.roots).live();
        let objects = lock(&self.objects).live;
        HeapStats {
            objects,
            persistent_roots,
            collections,
        }
    }

    /// The runtime-wide property-key table (`crate::object`).
    pub(crate) fn keys(&self) -> &PropertyKeys {
        &self.keys
    }

    /// The realm slot (`crate::realm`); use `realm::intrinsics` to build
    /// or read it.
    pub(crate) fn realm(&self) -> &Realm {
        &self.realm
    }

    /// Hold the object-table lock until the guard drops (tests: proves a
    /// path takes no object lock).
    #[cfg(test)]
    pub(crate) fn hold_objects(&self) -> ObjectsHeld<'_> {
        ObjectsHeld(lock(&self.objects))
    }

    /// The realm slot, to drop the realm (tests).
    #[cfg(test)]
    pub(crate) fn realm_mut(&mut self) -> &mut Realm {
        &mut self.realm
    }

    pub(crate) fn roots(&self) -> MutexGuard<'_, RootTable> {
        lock(&self.roots)
    }

    pub(crate) fn scopes(&self) -> MutexGuard<'_, ScopeTable> {
        lock(&self.scopes)
    }

    /// Register a root source. It is traced at every collection while it is
    /// alive and forgotten once the `Weak` no longer upgrades.
    pub(crate) fn register_source(&self, source: Weak<dyn RootSource>) {
        lock(&self.sources).register(source);
    }

    /// Persistently root `value` if it is still live, atomically with respect
    /// to collection (the roots lock is held across the liveness check).
    /// Returns the root slot.
    pub(crate) fn root_if_live(&self, value: crate::value::Value) -> Option<usize> {
        let mut roots = lock(&self.roots);
        if let crate::value::Value::Object(id) = value
            && !self.is_live(id)
        {
            return None;
        }
        Some(roots.insert(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_gives_distinct_stable_identities() {
        let heap = Heap::default();
        let a = heap.alloc(HeapObject::host_object()).unwrap();
        let b = heap.alloc(HeapObject::host_object()).unwrap();
        assert_ne!(a, b);
        assert_eq!(heap.kind(a), Some(ObjectKind::PlainObject));
        assert_eq!(heap.kind(ObjectId::from_index(99)), None);
        assert_eq!(heap.stats().objects, 2);
    }

    #[test]
    fn a_reused_slot_does_not_inherit_the_known_shareable_bit() {
        let heap = Heap::default();
        let known = heap
            .alloc_persistent(HeapObject::Vector(PVector::from_values(&[])), Some(&[]))
            .unwrap();
        assert_eq!(heap.known_shareable(known), Some(true));
        assert_eq!(heap.collect().unwrap().reclaimed, 1);
        // The plain object takes over the slot, with a new generation.
        let plain = heap.alloc(HeapObject::host_object()).unwrap();
        assert_eq!(plain.index, known.index);
        assert_eq!(heap.known_shareable(plain), Some(false));
        assert_eq!(heap.known_shareable(known), None, "stale id");
        // A persistent object built on it is not known either, and marking
        // the stale id does not touch the new occupant.
        heap.mark_shareable(&[known]);
        assert_eq!(heap.known_shareable(plain), Some(false));
        let parts = [Value::Object(plain)];
        let holder = heap
            .alloc_persistent(
                HeapObject::Vector(PVector::from_values(&parts)),
                Some(&parts),
            )
            .unwrap();
        assert_eq!(heap.known_shareable(holder), Some(false));
    }

    #[test]
    fn slots_whose_generation_would_wrap_are_retired() {
        let heap = Heap::default();
        let id = heap.alloc(HeapObject::host_object()).unwrap();
        lock(&heap.objects).slots[id.index as usize].generation = u32::MAX;
        let worn = ObjectId {
            index: id.index,
            generation: u32::MAX,
        };
        assert!(heap.is_live(worn));
        assert_eq!(heap.collect().unwrap().reclaimed, 1);
        let next = heap.alloc(HeapObject::host_object()).unwrap();
        assert_ne!(next.index, id.index, "a retired slot is never reused");
        assert!(!heap.is_live(worn));
        assert!(lock(&heap.objects).free.is_empty());
    }
}
