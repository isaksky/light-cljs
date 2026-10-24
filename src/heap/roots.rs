//! Embedding handle boundaries: persistent roots, scoped access, and
//! non-rooting weak handles.
//!
//! A Rust local holding a raw value is not a collector root. The embedder
//! therefore never sees raw values; it holds one of three handle kinds:
//!
//! * [`Rooted`]: a **persistent root**. Owning one registers its value in the
//!   heap's root table until the handle is dropped. It is `Send + Sync`, can be
//!   stored anywhere, and is how task results are returned
//!   ([`RunOutcome::Complete`](crate::RunOutcome::Complete)). Cloning registers
//!   a second root for the same value. **Ownership of results:** the embedder
//!   owns the returned `Rooted`; the value (and everything reachable from it)
//!   survives collections until the last root for it is dropped, independent
//!   of the task that produced it.
//! * [`Local<'s>`]: a **scoped access handle**, valid only inside the
//!   [`Runtime::scope`](crate::Runtime::scope) callback that created it. The
//!   scope's locals are registered in the heap (`RootCategory::ScopeLocal`)
//!   until the callback returns, and the invariant lifetime brand `'s` makes
//!   it a compile error to let a `Local` outlive or escape its scope:
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
//! A scope is used only by the thread that opened it (`Scope` is not `Sync`),
//! so it cannot be handed to a thread that would allocate behind the
//! collecting thread's back:
//!
//! ```compile_fail,E0277
//! use clj_quickjs::Runtime;
//! let rt = Runtime::new();
//! rt.scope(|s| {
//!     std::thread::scope(|t| {
//!         t.spawn(|| {
//!             s.new_object();
//!         });
//!     })
//! });
//! ```
//!
//! * [`WeakHandle`]: an identity reference that does **not** keep its object
//!   alive. It shows that a Rust variable alone is not a root: after the last
//!   root is gone and a collection runs, [`WeakHandle::upgrade`] fails with
//!   [`HandleError::Reclaimed`], even if the object's storage slot has since
//!   been reused by a new object.
//!
//! Escaping a scope correctly means persisting: `scope.persist(local)` returns
//! a [`Rooted`]. Handles are checked at runtime against the runtime they are
//! used with; a handle from another runtime is rejected with
//! [`HandleError::ForeignRuntime`].

use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use super::gc::{Access, MutatorToken, Trace};
use super::{Heap, HeapObject};
use crate::error::VmError;
use crate::value::{JsString, Primitive, Value, ValueView};
use crate::vm::exception::error_object;

/// Persistent root slots. Freed slots are reused; a slot holds the rooted
/// value while its [`Rooted`] handle is alive.
#[derive(Debug, Default)]
pub(crate) struct RootTable {
    slots: Vec<Option<Value>>,
    free: Vec<usize>,
}

impl RootTable {
    pub(crate) fn insert(&mut self, value: Value) -> usize {
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
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.slots.iter().flatten().for_each(|v| v.trace(visit));
    }
}

/// Locals of one open scope.
#[derive(Debug)]
struct ScopeEntry {
    locals: Vec<Value>,
}

/// Locals of every open scope, owned by the heap so the collector can trace
/// them. Freed entries are reused.
#[derive(Debug, Default)]
pub(crate) struct ScopeTable {
    entries: Vec<Option<ScopeEntry>>,
    free: Vec<usize>,
}

impl ScopeTable {
    fn open(&mut self) -> usize {
        let entry = Some(ScopeEntry { locals: Vec::new() });
        match self.free.pop() {
            Some(slot) => {
                self.entries[slot] = entry;
                slot
            }
            None => {
                self.entries.push(entry);
                self.entries.len() - 1
            }
        }
    }

    fn close(&mut self, slot: usize) {
        if let Some(entry) = self.entries.get_mut(slot)
            && entry.take().is_some()
        {
            self.free.push(slot);
        }
    }

    fn locals(&mut self, slot: usize) -> &mut Vec<Value> {
        // A `Scope` owns its entry from `open` until its drop calls `close`,
        // so the entry exists whenever a scope method runs.
        &mut self.entries[slot]
            .as_mut()
            .expect("scope entry exists while the scope is open")
            .locals
    }
}

impl Trace for ScopeTable {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.entries
            .iter()
            .flatten()
            .for_each(|e| e.locals.trace(visit));
    }
}

/// Error from using a handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleError {
    /// The handle belongs to a different [`Runtime`](crate::Runtime).
    ForeignRuntime,
    /// A [`WeakHandle`]'s object has been reclaimed by the collector.
    Reclaimed,
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandleError::ForeignRuntime => f.write_str("handle belongs to a different runtime"),
            HandleError::Reclaimed => f.write_str("the handle's object has been reclaimed"),
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
    /// Root `value`. The caller guarantees `value` is live and cannot be
    /// collected before this returns (it is held by another root, a scope on
    /// this thread, or the task of a run guard this thread holds).
    pub(crate) fn new(heap: &Arc<Heap>, value: Value) -> Rooted {
        let slot = heap.roots().insert(value.clone());
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

    /// Collector test hook: live WeakMap entries, or `None` for another kind.
    /// This diagnostic is never exposed to JavaScript. It takes only the
    /// object-table lock and does not inspect any key or value.
    #[doc(hidden)]
    pub fn weak_map_live_entry_count(&self) -> Option<usize> {
        let Value::Object(id) = self.value else {
            return None;
        };
        self.heap
            .read(id, |object| match object {
                HeapObject::WeakMap(map) => Some(map.len()),
                _ => None,
            })
            .flatten()
    }

    /// The contents of a string value (`None` for any other value). The
    /// string is immutable and lives outside the heap, so the returned
    /// [`JsString`] stays valid after this handle is dropped.
    pub fn string(&self) -> Option<JsString> {
        match &self.value {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// The exact immutable BigInt, independent of this root's lifetime.
    pub fn bigint(&self) -> Option<crate::JsBigInt> {
        match &self.value {
            Value::BigInt(n) => Some(n.clone()),
            _ => None,
        }
    }

    /// True only when both handles reference the same heap object (identity,
    /// not equality). Primitives are never "the same object".
    pub fn same_object(&self, other: &Rooted) -> bool {
        Arc::ptr_eq(&self.heap, &other.heap) && self.value.same_object(&other.value)
    }

    /// JavaScript `===` between two rooted values of the same runtime.
    pub fn strict_equals(&self, other: &Rooted) -> Result<bool, HandleError> {
        let other = other.value_in(&self.heap)?;
        Ok(self.value.strict_equals(&other))
    }

    /// The failure an error object stands for, if the value is one: the
    /// exception value of a catchable run-time error or of a failed or
    /// cancelled dependency raised at a `need` (`crate::vm::exception`).
    /// The internal diagnostic is unchanged by JavaScript property writes.
    /// `None` for any other value, including `new Error(...)` and
    /// `new TypeError(...)` created explicitly by JavaScript.
    pub fn error(&self) -> Option<VmError> {
        error_object(&self.heap, &self.value).map(|(error, _)| error)
    }

    /// The cause of an error object, rooted: for a failed dependency, the
    /// error value its producer published (the same object); otherwise
    /// `undefined`. This is the original cause even if JavaScript changes
    /// or deletes `cause`. `None` for values without an engine diagnostic,
    /// including explicitly constructed JavaScript Error instances.
    pub fn error_cause(&self) -> Option<Rooted> {
        // The cause is an edge of the error object, which `self` keeps
        // alive, so it cannot be reclaimed before it is rooted.
        error_object(&self.heap, &self.value).map(|(_, cause)| Rooted::new(&self.heap, cause))
    }

    /// A non-rooting handle to the same value.
    pub fn downgrade(&self) -> WeakHandle {
        WeakHandle {
            heap: Arc::clone(&self.heap),
            value: self.value.clone(),
        }
    }

    /// The raw value, after checking the handle belongs to `heap`.
    pub(crate) fn value_in(&self, heap: &Arc<Heap>) -> Result<Value, HandleError> {
        if Arc::ptr_eq(&self.heap, heap) {
            Ok(self.value.clone())
        } else {
            Err(HandleError::ForeignRuntime)
        }
    }
}

impl Clone for Rooted {
    fn clone(&self) -> Self {
        Rooted::new(&self.heap, self.value.clone())
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

/// A reference that does not keep its value alive. See the module docs.
///
/// A primitive never becomes stale. An object handle is checked against the
/// object's generation on every use, so a reclaimed object is reported as
/// [`HandleError::Reclaimed`] and never confused with a newer object that
/// reuses its storage slot.
#[derive(Clone)]
pub struct WeakHandle {
    heap: Arc<Heap>,
    value: Value,
}

impl WeakHandle {
    /// True while the value is still live (always true for primitives). A
    /// snapshot: a collection on another thread may reclaim it right after.
    pub fn is_live(&self) -> bool {
        match self.value {
            Value::Object(id) => self.heap.is_live(id),
            Value::Undefined
            | Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::BigInt(_)
            | Value::Symbol(_) => true,
        }
    }

    /// Root the value again, or fail with [`HandleError::Reclaimed`]. The
    /// liveness check and the new root are atomic with respect to collection.
    pub fn upgrade(&self) -> Result<Rooted, HandleError> {
        let slot = self
            .heap
            .root_if_live(self.value.clone())
            .ok_or(HandleError::Reclaimed)?;
        Ok(Rooted {
            heap: Arc::clone(&self.heap),
            slot,
            value: self.value.clone(),
        })
    }
}

impl fmt::Debug for WeakHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakHandle")
            .field("value", &self.value)
            .finish()
    }
}

/// Invariant lifetime brand shared by a [`Scope`] and its [`Local`]s.
type Brand<'s> = PhantomData<fn(&'s ()) -> &'s ()>;

/// Scoped heap access, created by [`Runtime::scope`](crate::Runtime::scope).
///
/// Holds every [`Local`] value created in it; all are released when the scope
/// callback returns. Not `Sync` or `Send`: a scope is used by the thread that
/// opened it.
///
/// A scope is the **no-GC access scope** for host and native code: while it
/// is open (and not [released](Scope::released)) its thread is a registered
/// mutator, so a collection requested on another thread waits until the
/// scope is closed or released, and then sees the scope's locals as roots.
/// A collection started on the scope's own thread runs at once (the locals
/// are roots). A scope never spans a task suspension: it ends when its
/// callback returns. See [`crate::heap::gc`].
///
/// Do not block on another thread while holding a scope: if that thread
/// needs to enter heap access while a collection is requested, it waits for
/// the collection, which waits for this scope. Block inside
/// [`Scope::released`] instead.
pub struct Scope<'s> {
    heap: &'s Arc<Heap>,
    /// This scope's entry in the heap's scope table.
    slot: usize,
    _brand: Brand<'s>,
    _not_sync: PhantomData<Cell<()>>,
    /// The thread's registration as a mutator; dropped after the entry is
    /// closed (fields drop after `Drop::drop`).
    _mutator: MutatorToken<'s>,
}

/// A value handle valid only within its [`Scope`]. See the module docs.
#[derive(Clone, Copy, Debug)]
pub struct Local<'s> {
    index: usize,
    _brand: Brand<'s>,
}

impl<'s> Scope<'s> {
    pub(crate) fn new(heap: &'s Arc<Heap>) -> Self {
        // Register first: entering waits while a collection is requested.
        let mutator = heap.enter_mutator(Access::Scope);
        let slot = heap.scopes().open();
        Scope {
            heap,
            slot,
            _brand: PhantomData,
            _not_sync: PhantomData,
            _mutator: mutator,
        }
    }

    /// The heap this scope accesses (crate-internal native operations).
    pub(crate) fn heap(&self) -> &Heap {
        self.heap
    }

    /// Register `value` as a local of this scope. The caller guarantees it
    /// is live (see [`Scope::new_object`]).
    pub(crate) fn new_local(&self, value: Value) -> Local<'s> {
        let mut scopes = self.heap.scopes();
        let locals = scopes.locals(self.slot);
        locals.push(value);
        Local {
            index: locals.len() - 1,
            _brand: PhantomData,
        }
    }

    /// Register several live values as locals under one lock acquisition.
    pub(crate) fn new_locals(&self, values: impl IntoIterator<Item = Value>) -> Vec<Local<'s>> {
        let mut scopes = self.heap.scopes();
        let locals = scopes.locals(self.slot);
        values
            .into_iter()
            .map(|value| {
                locals.push(value);
                Local {
                    index: locals.len() - 1,
                    _brand: PhantomData,
                }
            })
            .collect()
    }

    /// The raw value of a local of this scope.
    pub(crate) fn local_value(&self, local: Local<'s>) -> Value {
        // The brand guarantees `local` was created by this scope, and the
        // local list only grows while the scope is open, so the index exists.
        self.heap.scopes().locals(self.slot)[local.index].clone()
    }

    /// Allocate a new (property-less) plain object.
    ///
    /// # Panics
    ///
    /// If the heap's object table is exhausted (2^32 slots).
    pub fn new_object(&self) -> Local<'s> {
        let id = self
            .heap
            .alloc(HeapObject::host_object())
            .expect("heap object table exhausted");
        // No collection can run between the allocation and the push: this
        // thread is a registered mutator between safepoints (the scope is
        // open and not released).
        self.new_local(Value::Object(id))
    }

    /// Wrap a primitive as a local handle.
    pub fn primitive(&self, p: Primitive) -> Local<'s> {
        self.new_local(p.into())
    }

    /// Access a persistent root's value within this scope.
    pub fn local(&self, rooted: &Rooted) -> Result<Local<'s>, HandleError> {
        let value = rooted.value_in(self.heap)?;
        Ok(self.new_local(value))
    }

    /// Create a persistent root for a local value so it can leave the scope.
    pub fn persist(&self, local: Local<'s>) -> Rooted {
        Rooted::new(self.heap, self.local_value(local))
    }

    /// A non-rooting handle to a local value.
    pub fn downgrade(&self, local: Local<'s>) -> WeakHandle {
        WeakHandle {
            heap: Arc::clone(self.heap),
            value: self.local_value(local),
        }
    }

    /// Access a weak handle's value within this scope, or fail with
    /// [`HandleError::Reclaimed`] or [`HandleError::ForeignRuntime`].
    pub fn upgrade(&self, weak: &WeakHandle) -> Result<Local<'s>, HandleError> {
        if !Arc::ptr_eq(&weak.heap, self.heap) {
            return Err(HandleError::ForeignRuntime);
        }
        if !weak.is_live() {
            return Err(HandleError::Reclaimed);
        }
        // Still live when pushed: see `new_object`.
        Ok(self.new_local(weak.value.clone()))
    }

    /// Root-free view of a local value.
    pub fn view(&self, local: Local<'s>) -> ValueView {
        self.local_value(local).view()
    }

    /// The contents of a local string value (`None` for any other value).
    /// The returned [`JsString`] is immutable and outlives the scope.
    pub fn string(&self, local: Local<'s>) -> Option<JsString> {
        match self.local_value(local) {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The exact immutable BigInt; the returned value outlives this scope.
    pub fn bigint(&self, local: Local<'s>) -> Option<crate::JsBigInt> {
        match self.local_value(local) {
            Value::BigInt(n) => Some(n),
            _ => None,
        }
    }

    /// True only when both locals reference the same heap object.
    pub fn same_object(&self, a: Local<'s>, b: Local<'s>) -> bool {
        self.local_value(a).same_object(&self.local_value(b))
    }

    /// JavaScript `===` between two locals.
    pub fn strict_equals(&self, a: Local<'s>, b: Local<'s>) -> bool {
        self.local_value(a).strict_equals(&self.local_value(b))
    }

    /// Release heap access for the duration of `f`: a safepoint.
    ///
    /// While `f` runs this thread's scopes (this one and any enclosing
    /// scopes on the thread) no longer delay collections, so `f` may block
    /// on work that needs a collection to finish. `f` cannot use any scope:
    /// it must be `Send`, and a reference to a `Scope` is not, so it cannot
    /// allocate through one or hold an unrooted heap value (it may open a new
    /// scope with [`Runtime::scope`](crate::Runtime::scope), which counts as
    /// reentering). When `f` returns, or unwinds, heap access is reentered,
    /// waiting first while a collection is requested.
    ///
    /// Locals stay valid across a release: they are roots the whole time, so
    /// a collection during `f` keeps their objects (nothing moves).
    ///
    /// A run guard or native operation on this thread is not released: it
    /// still delays collections (see [`crate::heap::gc`]).
    ///
    /// ```compile_fail,E0277
    /// use clj_quickjs::Runtime;
    /// let rt = Runtime::new();
    /// rt.scope(|s| {
    ///     // error: the scope cannot be used while released.
    ///     s.released(|| s.new_object());
    /// });
    /// ```
    pub fn released<R>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.heap.release_scopes(f)
    }

    /// Poll for a requested collection: if one is pending, release heap
    /// access until it has run ([`Scope::released`] with nothing to do).
    /// Native code that keeps a scope open for long should call this
    /// regularly; it costs one atomic load when nothing is requested.
    pub fn safepoint(&self) {
        if self.heap.collection_requested() {
            self.released(|| ());
        }
    }

    /// The raw value of a local (crate tests).
    #[cfg(test)]
    pub(crate) fn raw(&self, local: Local<'s>) -> Value {
        self.local_value(local)
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.heap.scopes().close(self.slot);
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
        let id = heap.alloc(HeapObject::host_object()).unwrap();
        let dead = heap.alloc(HeapObject::host_object()).unwrap();
        let _keep = Rooted::new(&heap, Value::Object(id));
        drop(Rooted::new(&heap, Value::Object(dead)));
        let mut seen = Vec::new();
        heap.roots().trace(&mut |o| seen.push(o));
        assert_eq!(seen, vec![id.into()]);
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
        heap.scopes().trace(&mut |_| seen += 1);
        assert_eq!(seen, 3);
        drop(scope);
        seen = 0;
        heap.scopes().trace(&mut |_| seen += 1);
        assert_eq!(seen, 0, "a closed scope's locals are no longer roots");
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
        assert_eq!(
            scope.upgrade(&foreign.downgrade()).unwrap_err(),
            HandleError::ForeignRuntime
        );
        let mine = Rooted::new(&heap1, Value::Number(1.0));
        assert_eq!(
            mine.strict_equals(&foreign),
            Err(HandleError::ForeignRuntime)
        );
    }

    /// clj-quickjs-65: `Rooted::string` and `Scope::string` return the
    /// contents, which outlive the handle, the scope and the heap.
    #[test]
    fn string_contents_outlive_root_and_scope() {
        let heap = Arc::new(Heap::default());
        // A supplementary character (two units) and a lone surrogate.
        let units = [0x61, 0xD83D, 0xDE00, 0xD800];
        let text = JsString::from_code_units(units.to_vec()).unwrap();
        let rooted = Rooted::new(&heap, Value::String(text));
        assert_eq!(rooted.view(), ValueView::String);
        let from_root = rooted.string().unwrap();
        let (from_scope, not_string) = {
            let scope = Scope::new(&heap);
            let local = scope.local(&rooted).unwrap();
            let number = scope.primitive(Primitive::Number(1.0));
            assert_eq!(scope.view(local), ValueView::String);
            (scope.string(local).unwrap(), scope.string(number))
        };
        assert_eq!(not_string, None);
        assert_eq!(Rooted::new(&heap, Value::Null).string(), None);
        drop(rooted);
        drop(heap);
        for s in [from_root, from_scope] {
            assert_eq!(s.code_units(), units);
            assert_eq!(s.len(), 4);
            assert_eq!(s.to_string_lossy(), "a\u{1F600}\u{FFFD}");
        }
    }
}
