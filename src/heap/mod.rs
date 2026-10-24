//! The shared heap: object storage, stable object identity, and roots.
//!
//! **Temporary non-reclaiming heap.** This is an explicit stub: objects are
//! appended to an object table and never freed, so no handle can dangle, and
//! no collection is implemented or claimed. Pellet clj-quickjs-2 replaces it
//! with a precise nonmoving mark/sweep collector (`docs/design.md` §11).
//!
//! What already holds and later work must keep:
//!
//! * One logical heap per [`Runtime`](crate::Runtime), shared by every task
//!   (no per-task or per-worker heaps).
//! * Stable identity: an object is named by its `ObjectId`, which never
//!   changes for the object's lifetime (the heap is nonmoving).
//! * Raw `ObjectId`s are crate-private. Embedders reach heap values only
//!   through persistent roots ([`Rooted`]) or scoped handles ([`Local`]); see
//!   [`roots`].
//! * Everything that holds raw values is traceable through `gc::Trace`, and
//!   every root source is listed in `gc::RootCategory`.
//!
//! Locking: allocation and root registration take short internal locks
//! (metadata only). No lock is held while bytecode executes.

pub mod gc;
pub mod roots;

use std::sync::{Mutex, MutexGuard, PoisonError};

use roots::RootTable;
pub use roots::{HandleError, Local, Rooted, Scope};

/// Stable identity of a heap object. Crate-private: see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ObjectId(u64);

impl ObjectId {
    #[cfg(test)]
    pub(crate) fn from_index(index: u64) -> Self {
        ObjectId(index)
    }
}

/// Kinds of heap object. Append-only registry: later pellets add variants at
/// the end and handle them in every exhaustive match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObjectKind {
    /// A JavaScript object with no properties. Property storage is deferred;
    /// this kind exists so object identity is observable (`===`, handles).
    PlainObject,
}

/// Counters describing heap usage, for tests and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapStats {
    /// Objects allocated so far. The stub heap never frees, so this only grows.
    pub objects: usize,
    /// Live persistent roots ([`Rooted`] handles, including task results).
    pub persistent_roots: usize,
}

/// The heap shared by all tasks of one runtime.
#[derive(Debug, Default)]
pub(crate) struct Heap {
    /// Object table indexed by `ObjectId`. Append-only in this stub.
    objects: Mutex<Vec<ObjectKind>>,
    /// Persistent root slots.
    roots: Mutex<RootTable>,
}

/// Lock a metadata mutex, recovering from poisoning. The protected data is
/// plain bookkeeping that every critical section leaves consistent, so a
/// panic elsewhere cannot leave it half-updated.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Heap {
    /// Allocate a new object and return its identity.
    pub(crate) fn alloc(&self, kind: ObjectKind) -> ObjectId {
        let mut objects = lock(&self.objects);
        let id = ObjectId(objects.len() as u64);
        objects.push(kind);
        id
    }

    #[cfg(test)]
    /// The kind of a live object, or `None` if the id was never allocated by
    /// this heap.
    pub(crate) fn kind(&self, id: ObjectId) -> Option<ObjectKind> {
        let index = usize::try_from(id.0).ok()?;
        lock(&self.objects).get(index).copied()
    }

    pub(crate) fn stats(&self) -> HeapStats {
        HeapStats {
            objects: lock(&self.objects).len(),
            persistent_roots: lock(&self.roots).live(),
        }
    }

    pub(crate) fn roots(&self) -> MutexGuard<'_, RootTable> {
        lock(&self.roots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_gives_distinct_stable_identities() {
        let heap = Heap::default();
        let a = heap.alloc(ObjectKind::PlainObject);
        let b = heap.alloc(ObjectKind::PlainObject);
        assert_ne!(a, b);
        assert_eq!(heap.kind(a), Some(ObjectKind::PlainObject));
        assert_eq!(heap.kind(ObjectId(99)), None);
        assert_eq!(heap.stats().objects, 2);
    }
}
