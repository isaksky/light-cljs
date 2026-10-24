//! Embedding handle boundaries: persistent roots and scoped access.
//!
//! A Rust local holding a raw value is not a collector root. The embedder
//! therefore never sees raw values; it holds one of two handle kinds:
//!
//! * [`Rooted`]: a **persistent root**. Owning one registers its value in the
//!   heap's root table until the handle is dropped. It is `Send + Sync`, can be
//!   stored anywhere, and is how task results are returned
//!   ([`RunOutcome::Complete`](crate::RunOutcome::Complete)). Cloning registers
//!   a second root for the same value.
//! * [`Local<'s>`]: a **scoped access handle**, valid only inside the
//!   [`Runtime::scope`](crate::Runtime::scope) callback that created it. The
//!   scope owns the values (its local list is a root category for the future
//!   collector) and the invariant lifetime brand `'s` makes it a compile error
//!   to let a `Local` outlive or escape its scope:
//!
//! ```compile_fail
//! use clj_quickjs::Runtime;
//! let rt = Runtime::new();
//! // error: the returned Local would outlive its scope.
//! let escaped = rt.scope(|s| s.new_object());
//! ```
//!
//! ```compile_fail,E0521
//! use clj_quickjs::Runtime;
//! let rt = Runtime::new();
//! let mut stash = None;
//! // error: borrowed data escapes the closure.
//! rt.scope(|s| stash = Some(s.new_object()));
//! ```
//!
//! Handles from different scopes cannot be mixed either (the brands differ):
//!
//! ```compile_fail,E0521
//! use clj_quickjs::Runtime;
//! let rt = Runtime::new();
//! rt.scope(|outer| {
//!     let o = outer.new_object();
//!     rt.scope(|inner| inner.view(o));
//! });
//! ```
//!
//! Escaping a scope correctly means persisting: `scope.persist(local)` returns
//! a [`Rooted`]. Handles are checked at runtime against the runtime they are
//! used with; a handle from another runtime is rejected with
//! [`HandleError::ForeignRuntime`].

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use super::gc::Trace;
use super::{Heap, ObjectId, ObjectKind};
use crate::value::{Primitive, Value, ValueView};

/// Persistent root slots. Freed slots are reused; a slot holds the rooted
/// value while its [`Rooted`] handle is alive.
#[derive(Debug, Default)]
pub(crate) struct RootTable {
    slots: Vec<Option<Value>>,
    free: Vec<usize>,
}

impl RootTable {
    fn insert(&mut self, value: Value) -> usize {
        match self.free.pop() {
            Some(slot) => {
                self.slots[slot] = Some(value);
                slot
            }
            None => {
                self.slots.push(Some(value));
                self.slots.len() - 1
            }
        }
    }

    fn remove(&mut self, slot: usize) {
        if let Some(entry) = self.slots.get_mut(slot)
            && entry.take().is_some()
        {
            self.free.push(slot);
        }
    }

    pub(crate) fn live(&self) -> usize {
        self.slots.len() - self.free.len()
    }
}

impl Trace for RootTable {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.slots.iter().flatten().for_each(|v| v.trace(visit));
    }
}

/// Error from using a handle with the wrong runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleError {
    /// The handle belongs to a different [`Runtime`](crate::Runtime).
    ForeignRuntime,
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandleError::ForeignRuntime => f.write_str("handle belongs to a different runtime"),
        }
    }
}

impl std::error::Error for HandleError {}

/// A persistent root: keeps one value alive until dropped. See the module docs.
pub struct Rooted {
    heap: Arc<Heap>,
    slot: usize,
    /// Cached copy of the rooted value. Valid because the heap is nonmoving
    /// and the root-table entry keeps the object alive while `self` exists.
    value: Value,
}

impl Rooted {
    pub(crate) fn new(heap: &Arc<Heap>, value: Value) -> Rooted {
        let slot = heap.roots().insert(value);
        Rooted {
            heap: Arc::clone(heap),
            slot,
            value,
        }
    }

    /// Root-free view of the value.
    pub fn view(&self) -> ValueView {
        self.value.view()
    }

    /// True only when both handles reference the same heap object (identity,
    /// not equality). Primitives are never "the same object".
    pub fn same_object(&self, other: &Rooted) -> bool {
        Arc::ptr_eq(&self.heap, &other.heap) && self.value.same_object(other.value)
    }

    /// JavaScript `===` between two rooted values of the same runtime.
    pub fn strict_equals(&self, other: &Rooted) -> Result<bool, HandleError> {
        let other = other.value_in(&self.heap)?;
        Ok(self.value.strict_equals(other))
    }

    /// The raw value, after checking the handle belongs to `heap`.
    pub(crate) fn value_in(&self, heap: &Arc<Heap>) -> Result<Value, HandleError> {
        if Arc::ptr_eq(&self.heap, heap) {
            Ok(self.value)
        } else {
            Err(HandleError::ForeignRuntime)
        }
    }
}

impl Clone for Rooted {
    fn clone(&self) -> Self {
        Rooted::new(&self.heap, self.value)
    }
}

impl Drop for Rooted {
    fn drop(&mut self) {
        self.heap.roots().remove(self.slot);
    }
}

impl fmt::Debug for Rooted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rooted")
            .field("slot", &self.slot)
            .field("value", &self.value)
            .finish()
    }
}

/// Invariant lifetime brand shared by a [`Scope`] and its [`Local`]s.
type Brand<'s> = PhantomData<fn(&'s ()) -> &'s ()>;

/// Scoped heap access, created by [`Runtime::scope`](crate::Runtime::scope).
///
/// Holds every [`Local`] value created in it; all are released when the scope
/// callback returns. Not `Sync`: a scope is used by the thread that opened it.
pub struct Scope<'s> {
    heap: &'s Arc<Heap>,
    locals: RefCell<Vec<Value>>,
    _brand: Brand<'s>,
}

/// A value handle valid only within its [`Scope`]. See the module docs.
#[derive(Clone, Copy, Debug)]
pub struct Local<'s> {
    index: usize,
    _brand: Brand<'s>,
}

impl<'s> Scope<'s> {
    pub(crate) fn new(heap: &'s Arc<Heap>) -> Self {
        Scope {
            heap,
            locals: RefCell::new(Vec::new()),
            _brand: PhantomData,
        }
    }

    fn push(&self, value: Value) -> Local<'s> {
        let mut locals = self.locals.borrow_mut();
        locals.push(value);
        Local {
            index: locals.len() - 1,
            _brand: PhantomData,
        }
    }

    fn get(&self, local: Local<'s>) -> Value {
        // The brand guarantees `local` was created by this scope, and the
        // local list only grows while the scope is open, so the index exists.
        self.locals.borrow()[local.index]
    }

    /// Allocate a new (property-less) plain object.
    pub fn new_object(&self) -> Local<'s> {
        let id = self.heap.alloc(ObjectKind::PlainObject);
        self.push(Value::Object(id))
    }

    /// Wrap a primitive as a local handle.
    pub fn primitive(&self, p: Primitive) -> Local<'s> {
        self.push(p.into())
    }

    /// Access a persistent root's value within this scope.
    pub fn local(&self, rooted: &Rooted) -> Result<Local<'s>, HandleError> {
        let value = rooted.value_in(self.heap)?;
        Ok(self.push(value))
    }

    /// Create a persistent root for a local value so it can leave the scope.
    pub fn persist(&self, local: Local<'s>) -> Rooted {
        Rooted::new(self.heap, self.get(local))
    }

    /// Root-free view of a local value.
    pub fn view(&self, local: Local<'s>) -> ValueView {
        self.get(local).view()
    }

    /// True only when both locals reference the same heap object.
    pub fn same_object(&self, a: Local<'s>, b: Local<'s>) -> bool {
        self.get(a).same_object(self.get(b))
    }

    /// JavaScript `===` between two locals.
    pub fn strict_equals(&self, a: Local<'s>, b: Local<'s>) -> bool {
        self.get(a).strict_equals(self.get(b))
    }
}

impl Trace for Scope<'_> {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.locals.borrow().iter().for_each(|v| v.trace(visit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooted_handles_register_and_release_roots() {
        let heap = Arc::new(Heap::default());
        let a = Rooted::new(&heap, Value::Number(1.0));
        let b = a.clone();
        assert_eq!(heap.stats().persistent_roots, 2);
        drop(a);
        assert_eq!(heap.stats().persistent_roots, 1);
        let c = Rooted::new(&heap, Value::Null);
        assert_eq!(heap.stats().persistent_roots, 2);
        drop((b, c));
        assert_eq!(heap.stats().persistent_roots, 0);
    }

    #[test]
    fn root_table_traces_live_objects_only() {
        let heap = Arc::new(Heap::default());
        let id = heap.alloc(ObjectKind::PlainObject);
        let dead = heap.alloc(ObjectKind::PlainObject);
        let _keep = Rooted::new(&heap, Value::Object(id));
        drop(Rooted::new(&heap, Value::Object(dead)));
        let mut seen = Vec::new();
        heap.roots().trace(&mut |o| seen.push(o));
        assert_eq!(seen, vec![id]);
    }

    #[test]
    fn scope_locals_are_traceable_and_compare_by_identity() {
        let heap = Arc::new(Heap::default());
        let scope = Scope::new(&heap);
        let a = scope.new_object();
        let b = scope.new_object();
        let a2 = scope.persist(a);
        let a3 = scope.local(&a2).unwrap();
        assert!(scope.same_object(a, a3));
        assert!(!scope.same_object(a, b));
        assert!(scope.strict_equals(a, a3));
        let mut seen = 0;
        scope.trace(&mut |_| seen += 1);
        assert_eq!(seen, 3);
    }

    #[test]
    fn handles_from_another_runtime_are_rejected() {
        let heap1 = Arc::new(Heap::default());
        let heap2 = Arc::new(Heap::default());
        let foreign = Rooted::new(&heap2, Value::Number(1.0));
        let scope = Scope::new(&heap1);
        assert_eq!(
            scope.local(&foreign).unwrap_err(),
            HandleError::ForeignRuntime
        );
        let mine = Rooted::new(&heap1, Value::Number(1.0));
        assert_eq!(
            mine.strict_equals(&foreign),
            Err(HandleError::ForeignRuntime)
        );
    }
}
