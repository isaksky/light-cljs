//! The shared heap: object storage, stable object identity, roots, and the
//! precise nonmoving mark/sweep collector (`docs/design.md` §11).
//!
//! * One logical heap per [`Runtime`](crate::Runtime), shared by every task
//!   (no per-task or per-worker heaps).
//! * **Nonmoving, stable identity.** An object is named by an `ObjectId`
//!   (slot index + generation) that never changes for the object's lifetime,
//!   and its storage is a separate allocation (`Box<HeapObject>`) that is
//!   never moved. Slot headers also stay put in fixed-size segments.
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
//! → task bodies (collector only) → `objects` → per-object access words
//! (several only in ascending slot order; see [`storage`]). The collector
//! holds all of the named locks for the duration of one stop-the-world
//! collection and takes access words one at a time. A run holds its
//! own task body while it takes `gc` (to poll, or for a nested registration
//! such as an atom operation); that is no cycle because the collector never
//! locks the body of an executing task.
//!
//! **Allocation.** Registered mutators reserve empty slots in batches under
//! `objects`, then initialize them without that lock. [`storage`] confines the
//! unsafe boundary: stable segments, exclusive reservations, Release/Acquire
//! publication and atomic generation/liveness/shareability. Unused
//! reservations survive safepoints and return to the free list when the
//! thread leaves its last mutator access for this heap.
//!
//! **Access.** Existing objects are not reached through `objects`
//! (light-cljs-471). A frozen body (persistent collections, atom handles,
//! and known-shareable functions, built-ins and intrinsics) never changes
//! and is read with no lock. Every other body is read or changed under its
//! own slot's access word, which its owning task's run holds without
//! contention. Owner and generation checks are unchanged; memory safety does
//! not rely on task identity or scheduling. Sweep holds the stopped world,
//! `objects` and each reclaimed slot's word. The rules, including how
//! publication marks objects against a concurrent function-record attach,
//! are in [`storage`] and `docs/architecture.md`.
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
//! its trie, and reads take no lock to clone the frozen body's `Arc`.
//! Atoms ([`crate::atom`]) likewise hold an `Arc` cell outside the table;
//! their operations use Rust atomics on it after cloning the `Arc`, and the
//! atom module contains its own separate `unsafe` lifetime boundary.
//!
//! **Publication.** Each slot also carries a sticky known-shareable bit,
//! set when a persistent object is allocated from known-shareable parts or
//! proven shareable by the publication check (`value::share`).

pub(crate) mod budget;
pub mod gc;
#[cfg(test)]
mod inventory;
pub mod roots;
mod stats;
mod storage;

use storage::ObjectTable;

use std::any::Any;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};

use crate::ids::TaskId;

pub use gc::{
    CollectError, CollectStats, CollectionObserver, CollectionReport, MutatorState,
    ThreadRegistration,
};
use gc::{GcState, RootSource, SourceRegistry, Trace};
pub use roots::{HandleError, Local, ObjectIdentity, Rooted, Scope, WeakHandle};
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
    /// Slot address for immutable side tables; identity still includes generation.
    pub(crate) fn slot(self) -> usize {
        self.index as usize
    }

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
    /// A native Clojure form value: keyword, symbol, list, vector or map
    /// ([`crate::collections::form`]).
    Form,
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
/// has released every heap lock. Payloads are never frozen: they are reached
/// only under their slot's access word, so they need `Send` but not `Sync`.
///
/// Every payload type must also declare its publication category
/// ([`NativePayload::shareability`]; see [`crate::value::share`]).
pub(crate) trait NativePayload: Trace + Any + Send + fmt::Debug {
    /// The payload's publication category. It must not change during the
    /// payload's lifetime, and a [`Shareability::Persistent`] payload must
    /// never change the edges its [`Trace`] reports: the publication check
    /// walks those edges and caches a positive answer in the object's slot.
    /// Called under the object's access word: it must only return the category (same
    /// restrictions as [`Trace`]).
    fn shareability(&self) -> Shareability;
}

/// Storage of one heap object.
#[derive(Debug)]
pub(crate) enum HeapObject {
    /// An ordinary JavaScript object: task-owned and mutable by its owning
    /// task's run only, in place under its access word (`crate::object`),
    /// or a realm intrinsic (`Object.prototype`), frozen once installed.
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
    /// its owning task's run mutates it, in place under its access word; an
    /// open builder's `Trace` reports its draft's contents.
    Builder(Builder),
    /// A function object over shared immutable code and its captured values
    /// (`crate::vm::function`).
    Function(FunctionObject),
    /// A binding cell: mutable, task-owned; only its owning task's run
    /// changes it, in place under its access word (`crate::vm::function`).
    Cell(BindingCell),
    /// An atom (`crate::atom`): an explicit concurrent reference. Its cell is
    /// independently owned `Arc` storage so operations run on Rust atomics
    /// on a frozen heap object; its `Trace` reports the current value.
    Atom(Arc<AtomCell>),
    /// A built-in function (`crate::vm::builtin`): a realm intrinsic,
    /// immutable after the realm is built and shared by every task.
    Builtin(BuiltinFunction),
    /// A JavaScript array object: task-owned and mutable by its owning
    /// task's run only, in place under its access word
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
    /// A native Clojure form value (`crate::collections::form`): immutable
    /// once allocated, like the persistent collections; its `Trace`
    /// reports its elements, keys, values and metadata.
    Form(Arc<crate::collections::form::FormObject>),
}

impl HeapObject {
    /// A new empty ordinary object created by the embedder (no owner task;
    /// see `crate::object`).
    pub(crate) fn host_object(heap: &Heap) -> HeapObject {
        HeapObject::Plain(OrdinaryObject::new(heap, None))
    }

    /// What allocating an object boxed as `T` (with no boxed internal
    /// slots) charges: the slot and the box. For tests sizing exact headroom.
    #[cfg(test)]
    pub(crate) const fn charged_with_box<T>() -> usize {
        std::mem::size_of::<HeapObject>() + std::mem::size_of::<T>()
    }

    /// Fixed metadata this object owns through boxes, beyond the slot's own
    /// `size_of::<HeapObject>()`: a boxed body and an ordinary object's boxed
    /// internal slots (`OrdinaryObject::boxed_bytes`). Charged with the slot
    /// at allocation. Data-sized storage inside (properties, elements,
    /// entries, buffers, payload strings) is charged where it grows instead.
    pub(crate) fn boxed_bytes(&self) -> usize {
        use std::mem::size_of_val;
        match self {
            HeapObject::Plain(o) => o.boxed_bytes(),
            HeapObject::Native(p) => size_of_val(&**p),
            HeapObject::Array(a) => size_of_val(&**a) + a.object().boxed_bytes(),
            HeapObject::JsMap(m) | HeapObject::JsSet(m) => {
                size_of_val(&**m) + m.object().boxed_bytes()
            }
            HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => {
                size_of_val(&**m) + m.object().boxed_bytes()
            }
            HeapObject::WeakRef(m) => size_of_val(&**m) + m.object().boxed_bytes(),
            HeapObject::FinalizationRegistry(m) => size_of_val(&**m) + m.object().boxed_bytes(),
            HeapObject::DisposableStack(m) => size_of_val(&**m) + m.object().boxed_bytes(),
            HeapObject::ArrayBuffer(b) => size_of_val(&**b) + b.object.boxed_bytes(),
            HeapObject::TypedArray(a) => size_of_val(&**a) + a.object.boxed_bytes(),
            HeapObject::Generator(g) => size_of_val(&**g) + g.object.boxed_bytes(),
            HeapObject::AsyncState(s) => size_of_val(&**s),
            HeapObject::ModuleNamespace(n) => size_of_val(&**n),
            HeapObject::Promise(p) => p
                .properties
                .as_deref()
                .map_or(0, |o| size_of_val(o) + o.boxed_bytes()),
            HeapObject::Builtin(f) => f.properties.boxed_bytes(),
            // The shared body and its `Arc` counts; its trie nodes carry
            // their own charges (`crate::collections::NodeCharge`).
            HeapObject::Form(f) => 2 * std::mem::size_of::<usize>() + size_of_val(&**f),
            HeapObject::Vector(_)
            | HeapObject::Map(_)
            | HeapObject::Occurrence(_)
            | HeapObject::Builder(_)
            | HeapObject::Function(_)
            | HeapObject::Cell(_)
            | HeapObject::Atom(_)
            | HeapObject::BoundFunction(_)
            | HeapObject::PromiseReaction(_)
            | HeapObject::PromiseResolving(_)
            | HeapObject::Proxy(_) => 0,
        }
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
            HeapObject::Form(_) => ObjectKind::Form,
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
            HeapObject::Form(f) => f.trace(visit),
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
            // Form contents are exclusively strong.
            HeapObject::Form(_) => {}
        }
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
    /// Objects reachable after the last collection.
    pub last_live_objects: usize,
    /// Total time with all mutators stopped (excludes rendezvous and observers).
    pub pause_total: std::time::Duration,
    /// Longest stop-the-world pause.
    pub pause_max: std::time::Duration,
    /// Accounted bytes currently admitted under
    /// [`HeapPolicy::hard_limit_bytes`], including garbage not yet swept.
    /// Zero when the runtime has no byte limit (nothing is accounted then).
    pub managed_bytes: usize,
    /// Highest [`managed_bytes`](Self::managed_bytes) seen so far.
    pub peak_managed_bytes: usize,
    /// Accounted bytes still admitted once the last collection had released
    /// its garbage: the live set, plus whatever other workers admitted after
    /// the pause. Zero before the first collection or without a byte limit.
    pub last_live_bytes: usize,
    /// Total bytes ever admitted under the byte limit (allocation volume).
    pub admitted_bytes_total: usize,
    /// The configured [`HeapPolicy::hard_limit_bytes`].
    pub byte_limit: Option<usize>,
}

/// One collection policy shared by every task and worker of a runtime.
///
/// Both hard limits are aggregate: every task, worker thread and host scope
/// of the runtime draws on the same counters, so one worker's denial can be
/// caused by the others' live data. Construct it with `..Default::default()`
/// so later fields keep their defaults.
#[derive(Clone, Copy, Debug)]
pub struct HeapPolicy {
    /// Minimum allocations between automatic collections. `None` disables
    /// automatic collection, including collection driven by
    /// [`hard_limit_bytes`](Self::hard_limit_bytes) pressure. The actual
    /// threshold grows with the live set.
    pub allocation_threshold: Option<usize>,
    /// Aggregate allocated-object admission limit, including unreachable
    /// objects until swept. This is an object count, not a byte or RSS cap.
    pub hard_limit_objects: Option<usize>,
    /// Aggregate accounted-byte admission limit (`docs/architecture.md`,
    /// "Byte accounting"): object slots and their boxed bodies, property and
    /// element storage, strings, BigInts, buffers, native collections and
    /// forms, saved continuations and retained native-operation scratch.
    /// Each is charged conservatively before it is allocated, so a denial
    /// leaves the requesting operation's inputs unchanged. Includes the
    /// realm's intrinsics (use [`crate::Runtime::try_with_heap_policy`] to
    /// learn at startup whether they fit) and garbage until it is swept.
    /// Accounted bytes approximate, and do not bound, process RSS:
    /// allocator overhead, code, compiler caches, Rust stacks and the
    /// embedder's own data are outside it. A denial fails the requesting
    /// operation with catchable [`VmErrorKind::HeapLimit`](crate::VmErrorKind::HeapLimit)
    /// (the primitive string `heap byte limit exceeded`). `None` (the
    /// default) accounts nothing and costs no atomic per allocation.
    pub hard_limit_bytes: Option<usize>,
}

impl Default for HeapPolicy {
    fn default() -> Self {
        Self {
            allocation_threshold: Some(16_384),
            hard_limit_objects: None,
            hard_limit_bytes: None,
        }
    }
}

impl HeapPolicy {
    fn threshold(self, live: usize) -> usize {
        let growth = self.allocation_threshold.unwrap_or(usize::MAX).max(live);
        growth
            .min(
                self.hard_limit_objects
                    .map_or(usize::MAX, |limit| limit.saturating_sub(live) / 2),
            )
            .max(1)
    }
}

/// The object-table lock held by a test ([`Heap::hold_objects`]).
#[cfg(test)]
pub(crate) struct ObjectsHeld<'a>(#[allow(dead_code)] MutexGuard<'a, ObjectTable>);

/// The heap shared by all tasks of one runtime.
#[derive(Debug, Default)]
pub(crate) struct Heap {
    byte_budget: Arc<budget::ByteBudget>,
    policy: HeapPolicy,
    allocations_since_collection: AtomicUsize,
    allocated_objects: AtomicUsize,
    next_collection: AtomicUsize,
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
    /// Stable, atomically published slot-header directory.
    slot_directory: storage::Directory,
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
    /// The quoted global or sticky regex cells each task holds a
    /// `lastIndex` entry in (`crate::regexp::shared`), released when the
    /// task terminates; a leaf metadata lock with no heap edges.
    regex_task_cells:
        Mutex<std::collections::HashMap<TaskId, Vec<Weak<crate::regexp::shared::RegexCell>>>>,
}

/// Lock a metadata mutex, recovering from poisoning. The protected data is
/// plain bookkeeping that every critical section leaves consistent, so a
/// panic elsewhere cannot leave it half-updated.
fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Heap {
    /// Record that `task` holds a `lastIndex` entry in `cell`
    /// (`crate::regexp::shared`).
    pub(crate) fn note_regex_task_state(
        &self,
        task: TaskId,
        cell: Weak<crate::regexp::shared::RegexCell>,
    ) {
        lock(&self.regex_task_cells)
            .entry(task)
            .or_default()
            .push(cell);
    }

    /// Release every regex `lastIndex` entry of `task`, which has
    /// terminated, with its byte charge (`crate::regexp::shared`).
    pub(crate) fn release_regex_task_state(&self, task: TaskId) {
        let cells = lock(&self.regex_task_cells).remove(&task);
        for cell in cells.into_iter().flatten() {
            if let Some(cell) = cell.upgrade() {
                cell.release_task(task);
            }
        }
    }

    pub(crate) fn has_byte_limit(&self) -> bool {
        self.byte_budget.is_limited()
    }
    pub(crate) fn byte_starved(&self) -> bool {
        self.byte_budget.starved()
    }
    /// `HeapPolicy::allocation_threshold` is set: the runtime may collect
    /// on its own, including after a byte-limit denial.
    pub(crate) fn automatic_collection_enabled(&self) -> bool {
        self.policy.allocation_threshold.is_some()
    }
    pub(crate) fn take_byte_denial(&self) -> bool {
        self.byte_budget.take_denial()
    }
    pub(crate) fn byte_budget(&self) -> Arc<budget::ByteBudget> {
        self.byte_budget.clone()
    }
    pub(crate) fn reserve_bytes(&self, bytes: usize) -> Option<budget::ByteReservation> {
        self.byte_budget.reserve(bytes)
    }

    #[cfg(test)]
    pub(crate) fn with_byte_limit(bytes: usize) -> Self {
        Self::with_test_byte_headroom(bytes, false)
    }

    #[cfg(test)]
    pub(crate) fn with_test_byte_headroom(bytes: usize, global: bool) -> Self {
        let budget = budget::ByteBudget::new(Some(usize::MAX));
        let heap = Self {
            keys: PropertyKeys::with_budget(budget.clone()),
            byte_budget: budget,
            ..Self::default()
        };
        // Tests select headroom above the fixed intrinsic inventory, so a
        // quota failure happens in their requested operation, after catch
        // handlers have been installed.
        crate::realm::intrinsics(&heap).expect("test realm");
        let global_bytes = if global {
            crate::realm::new_global_object(&heap, crate::ids::TaskId::from_raw(0))
                .expect("test global");
            let occupied = heap.byte_budget.used();
            heap.collect().unwrap();
            occupied - heap.byte_budget.used()
        } else {
            0
        };
        heap.byte_budget.set_limit(
            heap.byte_budget
                .used()
                .checked_add(bytes)
                .unwrap()
                .checked_add(global_bytes)
                .unwrap(),
        );
        heap
    }

    pub(crate) fn with_policy(policy: HeapPolicy) -> Self {
        let budget = budget::ByteBudget::new(policy.hard_limit_bytes);
        Self {
            keys: PropertyKeys::with_budget(budget.clone()),
            byte_budget: budget,
            policy,
            next_collection: AtomicUsize::new(policy.threshold(0)),
            ..Self::default()
        }
    }

    fn allocated(&self) {
        self.allocations_since_collection
            .fetch_add(1, Ordering::Relaxed);
    }

    fn admit_allocation(&self) -> bool {
        let Some(limit) = self.policy.hard_limit_objects else {
            return true;
        };
        let mut current = self.allocated_objects.load(Ordering::Relaxed);
        loop {
            if current >= limit {
                self.note_limit_failure();
                return false;
            }
            match self.allocated_objects.compare_exchange_weak(
                current,
                current + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.take_limit_failure();
                    return true;
                }
                Err(actual) => current = actual,
            }
        }
    }

    fn allocate_accounted(&self, object: HeapObject, shareable: bool) -> Option<ObjectId> {
        if self.byte_budget.denied() {
            return None;
        }
        // The slot's boxed body and fixed internal slots are charged with it;
        // without a byte limit nothing is measured.
        let bytes = if self.byte_budget.is_limited() {
            std::mem::size_of::<HeapObject>() + object.boxed_bytes()
        } else {
            0
        };
        let Some(reservation) = self.reserve_bytes(bytes) else {
            self.byte_budget.note_denial();
            return None;
        };
        if !self.admit_allocation() {
            return None;
        }
        let id = self.allocate(object, shareable, reservation);
        if id.is_some() {
            self.allocated();
        } else if self.policy.hard_limit_objects.is_some() {
            self.allocated_objects.fetch_sub(1, Ordering::Relaxed);
        }
        id
    }

    /// Allocation only records pressure. The runner releases its run token
    /// after all new identities have reached traced frames, then rendezvous.
    pub(crate) fn automatic_collection_due(&self) -> bool {
        let threshold = self.next_collection.load(Ordering::Relaxed);
        let threshold = if threshold == 0 {
            self.policy.threshold(0)
        } else {
            threshold
        };
        if self.policy.allocation_threshold.is_none()
            || (self.allocations_since_collection.load(Ordering::Relaxed) < threshold
                && !self.byte_budget.collection_due())
        {
            return false;
        }
        self.can_collect_at_point()
    }
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
        self.allocate_accounted(object, false)
    }

    /// [`Heap::alloc`] for a persistent object, recording at allocation
    /// whether it is already known to be transitively shareable
    /// ([`crate::value::share`]): it is when every value in `parts` is
    /// known shareable. `parts` must cover every direct edge of `object`
    /// that is not inside a part (for an update: the old collection plus the
    /// values it adds); `None` means "every direct edge of `object`".
    /// Atomic header reads only; the work is O(parts).
    pub(crate) fn alloc_persistent(
        &self,
        object: HeapObject,
        parts: Option<&[Value]>,
    ) -> Option<ObjectId> {
        debug_assert_eq!(share::classify(&object), Shareability::Persistent);
        let shareable = match parts {
            Some(parts) => parts.iter().all(|v| self.slot_directory.known_shareable(v)),
            None => {
                let mut all = true;
                object.trace(&mut |edge| {
                    if let gc::TraceEdge::Object(id) = edge {
                        all = all && self.slot_directory.known_shareable(&Value::Object(id));
                    }
                });
                all
            }
        };
        self.allocate_accounted(object, shareable)
    }

    /// Run `f` on a live object and its known-shareable bit; `None` if `id`
    /// is stale or foreign. Same rules as [`Heap::read`].
    pub(crate) fn read_shareable<R>(
        &self,
        id: ObjectId,
        f: impl FnOnce(&HeapObject, bool) -> R,
    ) -> Option<R> {
        self.slot_directory.read(id, f)
    }

    /// Set the known-shareable bit of every live object in `ids`: the caller
    /// has just proved each one transitively shareable. Stale ids are
    /// ignored. Takes each object's access word in turn ([`storage`]).
    pub(crate) fn mark_shareable(&self, ids: &[ObjectId]) {
        self.slot_directory.mark_shareable(ids);
    }

    /// [`Heap::mark_shareable`] for the result of a publication walk, unless
    /// one of the objects changed since the walk read it: while holding the
    /// access words of all of `ids`, if `changed` holds for any live object
    /// in `ids`, nothing is marked and the result is false (the caller walks
    /// again); otherwise every live object in `ids` is marked and the result
    /// is true. The one change it guards against is a function object
    /// gaining its own-property record (`crate::vm::function`, "Own
    /// properties"), which [`Heap::with_unshared_object_mut`] makes under
    /// that function's access word and refuses on a known function.
    pub(crate) fn mark_shareable_unless(
        &self,
        ids: &[ObjectId],
        changed: impl Fn(&HeapObject) -> bool,
    ) -> bool {
        self.slot_directory.mark_shareable_unless(ids, changed)
    }

    /// [`Heap::with_object_mut`] unless the object is known shareable:
    /// `None` if `id` is stale, `Some(None)` if it is known shareable (`f`
    /// does not run), otherwise `f`'s result. Check and change happen under
    /// the object's access word; used to attach a function's own-property
    /// record or computed name only while the function is not known
    /// shareable (`crate::vm::function`).
    pub(crate) fn with_unshared_object_mut<R>(
        &self,
        id: ObjectId,
        f: impl FnOnce(&mut HeapObject) -> R,
    ) -> Option<Option<R>> {
        self.slot_directory.write(id, true, f)
    }

    /// The known-shareable bit of a live object (tests).
    #[cfg(test)]
    pub(crate) fn known_shareable(&self, id: ObjectId) -> Option<bool> {
        self.slot_directory.known(id)
    }

    /// True when `id` names a live (not reclaimed) object of this heap.
    /// One atomic header load; no lock.
    pub(crate) fn is_live(&self, id: ObjectId) -> bool {
        self.slot_directory.is_live(id)
    }

    /// Run `f` on a live object; `None` if `id` is stale or foreign. Frozen
    /// objects (persistent collections, atoms, and known-shareable
    /// functions, built-ins and intrinsics) are read without any lock;
    /// other objects under their own access word, never the object-table
    /// lock ([`storage`]). `f` must not touch the heap (it could deadlock)
    /// and should only copy out what it needs (for collections: an `Arc`
    /// clone of the immutable body), so the word is held briefly. The
    /// caller is a registered mutator or keeps `id` reachable.
    #[inline]
    pub(crate) fn read<R>(&self, id: ObjectId, f: impl FnOnce(&HeapObject) -> R) -> Option<R> {
        self.slot_directory.read(id, |object, _| f(object))
    }

    /// The kind of a live object, or `None` if `id` is stale or foreign.
    #[cfg(test)]
    pub(crate) fn kind(&self, id: ObjectId) -> Option<ObjectKind> {
        self.read(id, HeapObject::kind)
    }

    /// Run `f` on a live object under its access word; `None` if `id` is
    /// stale. `f` must not touch the heap (it could deadlock). Only mutable
    /// kinds may be changed: transient builders, binding cells and ordinary
    /// objects (each by its owning task's run), a realm intrinsic while its
    /// realm is being built, before anything can reach it (`crate::realm`),
    /// and task-owned native payloads. A frozen object is never lent
    /// mutably: `f` inspects a private copy (to reject the change) and
    /// anything it writes there is discarded ([`storage`]).
    #[inline]
    pub(crate) fn with_object_mut<R>(
        &self,
        id: ObjectId,
        f: impl FnOnce(&mut HeapObject) -> R,
    ) -> Option<R> {
        self.slot_directory.write(id, false, f).flatten()
    }

    /// Address of a live object's storage, to test that the heap never moves
    /// objects.
    #[cfg(test)]
    pub(crate) fn address(&self, id: ObjectId) -> Option<usize> {
        self.read(id, |o| std::ptr::from_ref(o) as usize)
    }

    pub(crate) fn stats(&self) -> HeapStats {
        // One lock at a time (separate statements), so the lock order holds.
        let (collections, last_live_objects, pause_total, pause_max) = {
            let gc = lock(&self.gc);
            (
                gc.collections,
                gc.last_live_objects,
                gc.pause_total,
                gc.pause_max,
            )
        };
        let persistent_roots = lock(&self.roots).live();
        let objects = lock(&self.objects).live();
        let bytes = &self.byte_budget;
        HeapStats {
            objects,
            persistent_roots,
            collections,
            last_live_objects,
            pause_total,
            pause_max,
            managed_bytes: bytes.used(),
            peak_managed_bytes: bytes.peak(),
            last_live_bytes: bytes.last_live(),
            admitted_bytes_total: bytes.volume(),
            byte_limit: bytes.limit(),
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
    /// path does not take it).
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
        let a = heap.alloc(HeapObject::host_object(&heap)).unwrap();
        let b = heap.alloc(HeapObject::host_object(&heap)).unwrap();
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
        let plain = heap.alloc(HeapObject::host_object(&heap)).unwrap();
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
}
