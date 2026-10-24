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
//! `gc` (collector state and mutator count) → `roots` → `scopes` → `sources`
//! → task bodies (collector only) → `objects`. The collector holds all of
//! them for the duration of one stop-the-world collection.
//!
//! **No unsafe code.** Objects are only reached through the `objects` lock,
//! so the collector needs no raw pointers; `Box` provides address stability.
//!
//! **Storage outside the heap.** [`Executable`](crate::Executable) code is
//! independently owned immutable `Arc` storage, shared by tasks and not
//! managed by the collector. Its constants are traced through every live task
//! running it (`RootCategory::ExecutableConstant`); they are primitives only
//! in this slice. Task VM state is owned by the task and traced as a root
//! source while the task handle exists.
//! Persistent collections ([`crate::collections`]) are heap objects whose
//! bodies are immutable `Arc` trie nodes outside the object table, shared
//! between versions; the owning object's `Trace` reports every heap edge in
//! its trie, and reads take the `objects` lock only to clone the body's `Arc`.

pub mod gc;
pub mod roots;

use std::any::Any;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError, Weak};

pub use gc::{CollectError, CollectStats};
use gc::{GcState, RootSource, SourceRegistry, Trace};
pub use roots::{HandleError, Local, Rooted, Scope, WeakHandle};
use roots::{RootTable, ScopeTable};

use crate::collections::{Occurrence, PMap, PVector};

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
    /// A JavaScript object with no properties. Property storage is deferred;
    /// this kind exists so object identity is observable (`===`, handles).
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
}

/// Rust data stored inside a heap object, for native operations.
///
/// The payload's [`Trace`] must report every [`ObjectId`] it stores; an
/// unreported edge is a dangling reference after the next collection (seen as
/// a stale identity, never as memory unsafety). A payload must not own a
/// [`Rooted`]: that would keep its target (and any cycle through it) alive
/// forever. Reclaimed payloads are dropped after the collector has released
/// every heap lock. Payloads are reached only under the heap's object lock,
/// so they need `Send` but not `Sync`.
pub(crate) trait NativePayload: Trace + Any + Send + fmt::Debug {}

/// Storage of one heap object.
#[derive(Debug)]
pub(crate) enum HeapObject {
    Plain,
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
}

impl HeapObject {
    #[cfg(test)]
    pub(crate) fn kind(&self) -> ObjectKind {
        match self {
            HeapObject::Plain => ObjectKind::PlainObject,
            HeapObject::Native(_) => ObjectKind::Native,
            HeapObject::Vector(_) => ObjectKind::Vector,
            HeapObject::Map(_) => ObjectKind::Map,
            HeapObject::Occurrence(_) => ObjectKind::Occurrence,
        }
    }
}

impl Trace for HeapObject {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match self {
            HeapObject::Plain => {}
            HeapObject::Native(payload) => payload.trace(visit),
            HeapObject::Vector(v) => v.trace(visit),
            HeapObject::Map(m) => m.trace(visit),
            HeapObject::Occurrence(o) => o.trace(visit),
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
    fn alloc(&mut self, object: HeapObject) -> Option<ObjectId> {
        let object = Some(Box::new(object));
        let index = match self.free.pop() {
            Some(index) => {
                self.slots[index as usize].object = object;
                index
            }
            None => {
                let index = u32::try_from(self.slots.len()).ok()?;
                self.slots.push(Slot {
                    generation: 0,
                    marked: false,
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

    /// The live slot named by `id`, or `None` if `id` is stale or foreign.
    fn slot(&self, id: ObjectId) -> Option<&Slot> {
        self.slots
            .get(id.index as usize)
            .filter(|s| s.generation == id.generation && s.object.is_some())
    }

    #[cfg(test)]
    fn get(&self, id: ObjectId) -> Option<&HeapObject> {
        self.slot(id)?.object.as_deref()
    }

    #[cfg(test)]
    fn get_mut(&mut self, id: ObjectId) -> Option<&mut HeapObject> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.object.as_deref_mut()
    }

    /// Set the mark bit of a live object and report its children. Returns
    /// `false` (and reports nothing) for stale ids and already-marked objects.
    fn mark(&mut self, id: ObjectId, children: &mut dyn FnMut(ObjectId)) -> bool {
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

/// The heap shared by all tasks of one runtime.
#[derive(Debug, Default)]
pub(crate) struct Heap {
    /// Collector state and the registered-mutator count.
    gc: Mutex<GcState>,
    /// Persistent root slots.
    roots: Mutex<RootTable>,
    /// Locals of open scopes.
    scopes: Mutex<ScopeTable>,
    /// Registered root sources (tasks, and later cells, atoms, ...).
    sources: Mutex<SourceRegistry>,
    /// The object table.
    objects: Mutex<ObjectTable>,
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
    /// The caller must make the returned id reachable from a root before the
    /// next collection can start; callers are registered mutators (a run
    /// holding a guard, or a scope on the collecting thread), which is what
    /// prevents a collection in between.
    pub(crate) fn alloc(&self, object: HeapObject) -> Option<ObjectId> {
        lock(&self.objects).alloc(object)
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
    /// stale. `f` must not touch the heap (it would deadlock).
    #[cfg(test)]
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
        let a = heap.alloc(HeapObject::Plain).unwrap();
        let b = heap.alloc(HeapObject::Plain).unwrap();
        assert_ne!(a, b);
        assert_eq!(heap.kind(a), Some(ObjectKind::PlainObject));
        assert_eq!(heap.kind(ObjectId::from_index(99)), None);
        assert_eq!(heap.stats().objects, 2);
    }

    #[test]
    fn slots_whose_generation_would_wrap_are_retired() {
        let heap = Heap::default();
        let id = heap.alloc(HeapObject::Plain).unwrap();
        lock(&heap.objects).slots[id.index as usize].generation = u32::MAX;
        let worn = ObjectId {
            index: id.index,
            generation: u32::MAX,
        };
        assert!(heap.is_live(worn));
        assert_eq!(heap.collect().unwrap().reclaimed, 1);
        let next = heap.alloc(HeapObject::Plain).unwrap();
        assert_ne!(next.index, id.index, "a retired slot is never reused");
        assert!(!heap.is_live(worn));
        assert!(lock(&heap.objects).free.is_empty());
    }
}
