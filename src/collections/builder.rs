//! Task-owned transient builders (`docs/design.md` §4: "Build intermediate
//! values privately"; "Transient builders belong to an exclusively executing
//! task, not permanently to its original OS thread").
//!
//! # Lifecycle
//!
//! ```text
//!   new_vector_builder / new_map_builder        builder_conj / builder_assoc
//!               │                                  (owner only, in place)
//!               ▼                                        │
//!            ┌──────┐ ◄──────────────────────────────────┘
//!            │ Open │
//!            └──────┘
//!       freeze │   │ owning task completes or fails
//!   (owner)    ▼   ▼
//!       ┌────────┐ ┌──────────┐
//!       │ Frozen │ │ Released │      both terminal; the draft is dropped
//!       └────────┘ └──────────┘
//! ```
//!
//! * **Create.** A builder is a heap object with its own identity, owned by
//!   the [`TaskId`] of the task that created it. Ownership follows the task,
//!   not an OS thread: a task that yields and is resumed on another thread
//!   keeps using its builders.
//! * **Mutate.** `builder_conj` (vector builders) and `builder_assoc` change
//!   the builder's private draft in place and return the same builder. A
//!   vector draft is a plain `Vec`, so appends are amortized O(1); a map draft
//!   is a [`PMap`] updated by path copying (in-place node editing is not
//!   implemented). Each operation takes the heap's object-table lock once.
//! * **Freeze.** `freeze` moves the draft out, allocates a new persistent
//!   vector or map from it (a separate object with its own identity), and
//!   leaves the builder `Frozen`. Nothing else ever sees the draft, so the
//!   persistent result can never change afterwards.
//! * **Release.** When the owning task completes or fails, every builder it
//!   created that is still open becomes `Released` and its draft is dropped.
//!   A task tracks its builders in a private, non-rooting list
//!   ([`OwnedBuilders`]).
//!
//! # Errors
//!
//! A builder operation checks, in this order:
//!
//! 1. the operand is a builder of the required kind, else
//!    [`CollectionError::WrongType`] (`found` names `vector builder`,
//!    `map builder`, `vector`, `number`, ...);
//! 2. the builder is not `Released`, else [`BuilderError::OwnerFinished`];
//! 3. the running task is the owner, else [`BuilderError::NotOwner`];
//! 4. the builder is not `Frozen`, else [`BuilderError::Frozen`];
//! 5. operation-specific operands (`builder_assoc` on a vector builder: an
//!    index value `<= count` by the collection index contract, else
//!    [`CollectionError::InvalidIndex`] or
//!    [`CollectionError::IndexOutOfBounds`]).
//!
//! A failed check fails the running task; it never panics and never changes
//! the builder.
//!
//! # Tracing
//!
//! An open builder's draft is reported by the builder object's `Trace`, so
//! its contents stay alive exactly as long as the builder object is
//! reachable (for example from the owning task's frame across a yield).
//! Frozen and released builders hold no edges. The owner's list of builders
//! is not a root.
//!
//! Builders are not shareable values: a builder handle that escapes its task
//! (as a task result, or inside a persistent collection) can be passed
//! around and compared by identity, but no other task can mutate or freeze
//! it. The publication check (`crate::value::share`) rejects any value
//! that reaches a builder, in any state. A task dropped without finishing does not release its
//! builders; they remain unusable by every other task.

use std::fmt;
use std::mem;

use super::{CollectionError, PMap, PVector, index_from_value, object_name};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::Value;

/// Why a builder operation was rejected. See [`crate::vm::intrinsics`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuilderError {
    /// The running task does not own the builder.
    NotOwner { owner: TaskId, caller: TaskId },
    /// The builder was already frozen; its persistent value was returned by
    /// the `freeze` that froze it.
    Frozen,
    /// The task that owned the builder has completed or failed.
    OwnerFinished { owner: TaskId },
}

impl fmt::Display for BuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuilderError::NotOwner { owner, caller } => {
                write!(f, "builder is owned by {owner}, not {caller}")
            }
            BuilderError::Frozen => f.write_str("builder has already been frozen"),
            BuilderError::OwnerFinished { owner } => {
                write!(f, "builder's owning {owner} has finished")
            }
        }
    }
}

impl std::error::Error for BuilderError {}

/// A builder operation failure: a type/index error by the collection
/// contract, or a lifecycle/ownership rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuildFault {
    Collection(CollectionError),
    Builder(BuilderError),
    /// A broken builder invariant (never expected).
    Internal(&'static str),
}

impl From<CollectionError> for BuildFault {
    fn from(e: CollectionError) -> Self {
        BuildFault::Collection(e)
    }
}

impl From<BuilderError> for BuildFault {
    fn from(e: BuilderError) -> Self {
        BuildFault::Builder(e)
    }
}

/// What a builder builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuilderKind {
    Vector,
    Map,
}

impl BuilderKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            BuilderKind::Vector => "vector builder",
            BuilderKind::Map => "map builder",
        }
    }
}

/// The private contents of an open builder.
#[derive(Debug)]
enum Draft {
    Vector(Vec<Value>),
    Map(PMap),
}

#[derive(Debug)]
enum State {
    Open(Draft),
    Frozen,
    Released,
}

/// A builder heap object (`HeapObject::Builder`).
#[derive(Debug)]
pub(crate) struct Builder {
    owner: TaskId,
    kind: BuilderKind,
    state: State,
}

impl Builder {
    pub(crate) fn owner(&self) -> TaskId {
        self.owner
    }

    pub(crate) fn kind(&self) -> BuilderKind {
        self.kind
    }

    fn is_open(&self) -> bool {
        matches!(self.state, State::Open(_))
    }

    /// The draft, if `caller` may use this builder now. Checks 2–4 of the
    /// module docs.
    fn draft(&mut self, caller: TaskId) -> Result<&mut Draft, BuilderError> {
        let owner = self.owner;
        match &mut self.state {
            State::Released => Err(BuilderError::OwnerFinished { owner }),
            _ if owner != caller => Err(BuilderError::NotOwner { owner, caller }),
            State::Frozen => Err(BuilderError::Frozen),
            State::Open(draft) => Ok(draft),
        }
    }
}

impl Trace for Builder {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match &self.state {
            State::Open(Draft::Vector(items)) => items.trace(visit),
            State::Open(Draft::Map(map)) => map.trace(visit),
            State::Frozen | State::Released => {}
        }
    }
}

/// The builders a task created, so they can be released when it finishes.
/// Not a root: entries are identities only, and a reclaimed builder's stale
/// identity resolves to nothing.
#[derive(Debug, Default)]
pub(crate) struct OwnedBuilders {
    ids: Vec<ObjectId>,
    /// Drop entries that are no longer open when the list reaches this
    /// length, so a task that creates many builders keeps a bounded list.
    prune_at: usize,
}

impl OwnedBuilders {
    fn register(&mut self, heap: &Heap, id: ObjectId) {
        if self.ids.len() >= self.prune_at {
            self.ids.retain(|&id| {
                heap.read(id, |o| matches!(o, HeapObject::Builder(b) if b.is_open()))
                    .unwrap_or(false)
            });
            self.prune_at = (self.ids.len() * 2).max(16);
        }
        self.ids.push(id);
    }

    /// Release every still-open builder `owner` created (the owner has
    /// completed or failed). Drafts are dropped after the object lock is
    /// released.
    pub(crate) fn release_all(&mut self, heap: &Heap, owner: TaskId) {
        for id in self.ids.drain(..) {
            let _draft = heap.with_object_mut(id, |o| match o {
                HeapObject::Builder(b) if b.owner == owner && b.is_open() => {
                    Some(mem::replace(&mut b.state, State::Released))
                }
                _ => None,
            });
        }
    }
}

/// A new open builder owned by `owner`, recorded in its owner's list. The
/// caller is the owner's run (a registered mutator) and stores the result
/// on its operand stack before the instruction ends.
pub(crate) fn new_builder(
    heap: &Heap,
    owner: TaskId,
    kind: BuilderKind,
    owned: &mut OwnedBuilders,
) -> Result<Value, CollectionError> {
    let draft = match kind {
        BuilderKind::Vector => Draft::Vector(Vec::new()),
        BuilderKind::Map => Draft::Map(PMap::default()),
    };
    let builder = Builder {
        owner,
        kind,
        state: State::Open(draft),
    };
    let id = heap
        .alloc(HeapObject::Builder(builder))
        .ok_or(CollectionError::OutOfMemory)?;
    owned.register(heap, id);
    Ok(Value::Object(id))
}

/// Run `f` on builder `b` under one object-table lock. Check 1 of the module
/// docs: `b` must be a builder, of kind `expected` when given.
fn with_builder<R>(
    heap: &Heap,
    b: Value,
    expected: Option<BuilderKind>,
    f: impl FnOnce(&mut Builder) -> Result<R, BuildFault>,
) -> Result<R, BuildFault> {
    let wrong = |found| -> BuildFault {
        CollectionError::WrongType {
            expected: expected.map_or("builder", BuilderKind::name),
            found,
        }
        .into()
    };
    let Value::Object(id) = b else {
        return Err(wrong(b.type_name()));
    };
    heap.with_object_mut(id, |object| match object {
        HeapObject::Builder(builder) if expected.is_none_or(|k| k == builder.kind) => f(builder),
        other => Err(wrong(object_name(other))),
    })
    .unwrap_or_else(|| Err(wrong("reclaimed object")))
}

/// Append `value` to vector builder `b` in place.
pub(crate) fn conj(heap: &Heap, caller: TaskId, b: Value, value: Value) -> Result<(), BuildFault> {
    with_builder(heap, b, Some(BuilderKind::Vector), |builder| {
        match builder.draft(caller)? {
            Draft::Vector(items) => items.push(value),
            Draft::Map(_) => return Err(INCONSISTENT),
        }
        Ok(())
    })
}

/// Set `key` to `value` in builder `b` (a vector or map builder) in place.
/// Vector keys follow the index contract and may be `<= count` (append).
pub(crate) fn assoc(
    heap: &Heap,
    caller: TaskId,
    b: Value,
    key: Value,
    value: Value,
) -> Result<(), BuildFault> {
    with_builder(heap, b, None, |builder| {
        match builder.draft(caller)? {
            Draft::Vector(items) => {
                let index = index_from_value(key)?;
                let count = items.len();
                match items.get_mut(index) {
                    Some(slot) => *slot = value,
                    None if index == count => items.push(value),
                    None => return Err(CollectionError::IndexOutOfBounds { index, count }.into()),
                }
            }
            Draft::Map(map) => {
                if let Some(updated) = map.assoc(key, value) {
                    *map = updated;
                }
            }
        }
        Ok(())
    })
}

/// Freeze builder `b` (either kind): a new persistent vector or map with its
/// contents. The builder becomes `Frozen`.
pub(crate) fn freeze(heap: &Heap, caller: TaskId, b: Value) -> Result<Value, BuildFault> {
    let draft = with_builder(heap, b, None, |builder| {
        builder.draft(caller)?;
        match mem::replace(&mut builder.state, State::Frozen) {
            State::Open(draft) => Ok(draft),
            State::Frozen | State::Released => Err(INCONSISTENT),
        }
    })?;
    // The draft's values are held only here until the new object exists;
    // the caller is a registered mutator, so no collection can intervene.
    let object = match draft {
        Draft::Vector(items) => HeapObject::Vector(PVector::from_values(&items)),
        Draft::Map(map) => HeapObject::Map(map),
    };
    // Known shareable at once when every drafted value already is.
    heap.alloc_persistent(object, None)
        .map(Value::Object)
        .ok_or(BuildFault::Collection(CollectionError::OutOfMemory))
}

/// A draft whose shape disagrees with its builder's kind, or a state that
/// `Builder::draft` already ruled out: a broken invariant, reported as an
/// internal failure instead of a panic.
const INCONSISTENT: BuildFault = BuildFault::Internal("inconsistent builder state");

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::heap::Scope;

    #[test]
    fn owned_list_stays_bounded_and_release_skips_frozen_builders() {
        let heap = Arc::new(Heap::default());
        let _mutator = Scope::new(&heap);
        let owner = TaskId::from_raw(1);
        let mut owned = OwnedBuilders::default();
        for _ in 0..1000 {
            let b = new_builder(&heap, owner, BuilderKind::Vector, &mut owned).unwrap();
            freeze(&heap, owner, b).unwrap();
        }
        assert!(owned.ids.len() <= 32, "pruned: {}", owned.ids.len());

        let frozen = new_builder(&heap, owner, BuilderKind::Map, &mut owned).unwrap();
        freeze(&heap, owner, frozen).unwrap();
        let open = new_builder(&heap, owner, BuilderKind::Map, &mut owned).unwrap();
        owned.release_all(&heap, owner);
        assert!(owned.ids.is_empty());
        let finished = BuildFault::Builder(BuilderError::OwnerFinished { owner });
        assert_eq!(freeze(&heap, owner, open), Err(finished));
        let frozen_err = BuildFault::Builder(BuilderError::Frozen);
        assert_eq!(freeze(&heap, owner, frozen), Err(frozen_err));
    }
}
