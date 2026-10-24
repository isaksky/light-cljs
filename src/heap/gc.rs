//! The collector: tracing contract, root registration, mutator registration
//! with cooperative safepoints, and a precise nonmoving stop-the-world
//! mark/sweep (`docs/design.md` §11).
//!
//! # Tracing boundary
//!
//! * Every structure that stores raw `Value`s implements [`Trace`], which
//!   reports each heap reference it holds directly. Implemented for values,
//!   heap objects (including [`NativePayload`](super::NativePayload) edges),
//!   the persistent root table, scope locals, executable constants, and task
//!   VM state.
//! * A **root source** ([`RootSource`]) is a structure outside the heap that
//!   holds values the collector must treat as roots. Sources register a
//!   `Weak` with [`Heap::register_source`](super::Heap::register_source) and
//!   are traced at every collection while alive; dropping the owner
//!   unregisters it. Tasks and dependency cells are root sources today;
//!   closure environments and pending native states plug in the same way
//!   (or become heap objects reachable from such a source). Atoms are heap
//!   objects: their current value is an edge reported by their `Trace`.
//! * A **running** task's frames are reported by its runner when it stops at
//!   a safepoint (below), not through the task's root source: the runner's
//!   guard holds the task body for the whole run.
//! * [`RootCategory`] names every root source kind; each root is reported
//!   with its category so tests can inspect the root snapshot.
//!
//! # Mutators
//!
//! A thread is **in heap access** while it holds at least one mutator
//! registration (`MutatorToken`, one per `Access` kind):
//!
//! * a **run guard** ([`RunGuard`](crate::RunGuard)) from acquisition to
//!   drop (`Access::Run`): its task's frames are borrowed by the run and
//!   the dispatch loop may hold values in Rust locals mid-instruction;
//! * an open **scope** ([`Scope`](crate::Scope), `Access::Scope`): the
//!   *no-GC access scope* for native and host code. It may have allocated an
//!   object it has not stored yet, so collection waits for it;
//! * a **native operation** with no safepoint inside (`Access::Op`): every
//!   atom operation, for its duration (its epoch argument depends on it).
//!
//! Registrations are per thread (tokens are not `Send`); a thread may hold
//! several (a scope, then a run, then an atom operation inside the run). A
//! thread that holds none never delays a collection, however long it runs.
//!
//! # Safepoints and the collection rendezvous
//!
//! A thread is **stopped** when it is outside heap access, or at a
//! safepoint: parked in a VM poll with its running frames reported, or
//! blocked in its own collection request. A collection runs only when every
//! other thread is stopped, and the collecting thread itself holds no run
//! guard or native operation (scopes are fine: their locals are roots).
//!
//! * [`Heap::collect`](super::Heap::collect) ([`Runtime::collect`](crate::Runtime::collect))
//!   never waits: if some other thread is in heap access and not stopped it
//!   returns [`CollectError::MutatorActive`].
//! * `Heap::request_collection`
//!   ([`Runtime::request_collection`](crate::Runtime::request_collection))
//!   **requests** a collection and blocks until the rendezvous completes:
//!   it raises the request flag, waits until every other thread is stopped,
//!   collects, and lowers the flag. This is the deterministic hook for tests
//!   and for executors. While a request is pending:
//!   - a thread **entering** heap access (its first registration) waits
//!     until a pending request has been served; a thread already in access registers at
//!     once (the request is waiting for it, so blocking would deadlock);
//!   - a running VM **polls** at its safepoints (below): it reports its
//!     frames, parks until a pending request has been served, and
//!     continues (to its next safepoint, even if another request is
//!     already pending, so back-to-back requests cannot starve it);
//!   - a scope delays the collection until it is closed or **released**
//!     ([`Scope::released`](crate::Scope::released),
//!     [`Scope::safepoint`](crate::Scope::safepoint)). Releasing is a
//!     safepoint: the thread's scope registrations are dropped for the
//!     duration of the release callback, which cannot reach the scope (it
//!     must be `Send` and a `Scope` is not `Sync`), so it cannot allocate or
//!     hold unrooted values through it; reentering waits like entering. The
//!     scope's locals stay in the scope table across the release, so the
//!     collector observes them and they remain valid afterwards.
//!
//! **VM poll points** (`crate::vm::dispatch`), each at an instruction
//! boundary where every value the run holds is in its task-owned frames:
//! run entry, backward branches, `Call` and `Return`, and every instruction
//! that may allocate. Run exit is the guard's deregistration. A run parks
//! only if it is its thread's only run guard and no native operation is in
//! progress on the thread; otherwise the poll returns at once and the
//! collection waits for the thread to leave heap access. A leaf host effect
//! runs inside the run's heap access: a request waits for it to return.
//!
//! **Locks and orderings.** The `gc` mutex guards the per-thread registry,
//! the pending-request count and the collection counter; its condition
//! variable (`gc_changed`) is notified whenever a thread stops, leaves heap
//! access, or a request ends. The request flag (`requested`, an
//! `AtomicBool`) mirrors "a request is pending" so polls cost one load: it
//! is written only under the `gc` lock (`Release`) and read by polls
//! (`Acquire`) as a hint; every decision is re-made under the lock, so a
//! stale read only delays a poll until the next poll point. A collection
//! holds the heap locks (in the order `gc` → `roots` → `scopes` → `sources`
//! → task bodies → `objects`) for one stop-the-world pause; stopped threads
//! hold none of them, and parked runs only hold their own task body, which
//! the collector does not lock (the runner reported those frames).
//!
//! This is not a global interpreter lock or a heap lock held throughout VM
//! execution: runs of different tasks proceed in parallel and only take
//! short metadata locks; the registry only decides when a collection may
//! start.
//!
//! **Atom grace periods** (`crate::atom`) rely on this: an atom operation
//! is an `Access::Op` registration with no safepoint inside, so no
//! collection completes while it runs, and its `MutatorToken::epoch` is
//! constant.
//!
//! Deferred: an automatic collection trigger (allocation never requests a
//! collection), worker-local allocation buffers, and releasing a run's
//! access around a long host effect.
//!
//! QuickJS reference (edge inventory only): `JS_MarkValue` / `mark_children`
//! in `research/quickjs/quickjs.c` and `JSClassGCMark` in
//! `research/quickjs/quickjs.h` (revision 7d1c13c5d8f8). QuickJS's reference
//! counting, cycle removal and running-stack exemption are not used: here
//! every root, running or suspended, is traced and nothing is counted.
//! QuickJS forbids sharing one runtime between threads
//! (`research/quickjs/doc/quickjs.texi`, "Runtime and contexts"); the
//! registry and rendezvous above are what replace that restriction.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard, PoisonError, Weak};
use std::thread::{self, ThreadId};

use super::{Heap, HeapObject, ObjectId, lock};
use crate::value::Value;

/// A source of collector roots. Append-only registry: later pellets add
/// categories at the end (dependency results, atom contents, closure
/// environments, native continuations, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootCategory {
    /// [`Rooted`](crate::Rooted) handles, including task results.
    PersistentHandle,
    /// Values held by an open [`Scope`](crate::Scope).
    ScopeLocal,
    /// Task-owned VM state: locals and operand stack of not-yet-run and
    /// yielded tasks (through the task's root source), and of running tasks
    /// stopped at a safepoint (reported by their runner).
    TaskFrame,
    /// Constants of the executable a live task runs.
    ExecutableConstant,
    /// The `Ready` value or `Failed` error of a completed dependency cell
    /// (`crate::dep`), while the cell exists.
    DependencyOutcome,
}

/// Report every heap reference held by `self`.
///
/// Implementations must visit each [`ObjectId`] stored directly in the
/// structure; they must not allocate on the heap, take heap locks, or run
/// bytecode (the collector calls them while holding the heap locks).
pub(crate) trait Trace {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId));
}

impl Trace for Value {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match *self {
            Value::Object(id) => visit(id),
            Value::Undefined | Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}

impl<T: Trace> Trace for [T] {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.iter().for_each(|v| v.trace(visit));
    }
}

/// A registered structure outside the heap whose values are roots. See the
/// module docs. Same restrictions as [`Trace`]; in addition the collector
/// calls it only when every other thread is stopped (module docs), so it may
/// lock state that mutators lock only briefly, but it must not wait for
/// state a stopped run keeps: a task body held by a run parked at a
/// safepoint is skipped (its runner reported those frames).
pub(crate) trait RootSource: Send + Sync {
    fn trace_roots(&self, visit: &mut dyn FnMut(RootCategory, ObjectId));
}

/// Registered root sources.
#[derive(Debug, Default)]
pub(crate) struct SourceRegistry {
    sources: Vec<Weak<dyn RootSource>>,
    /// Prune dead entries when the list reaches this length.
    prune_at: usize,
}

impl SourceRegistry {
    pub(crate) fn register(&mut self, source: Weak<dyn RootSource>) {
        if self.sources.len() >= self.prune_at {
            self.sources.retain(|s| s.strong_count() > 0);
            self.prune_at = (self.sources.len() * 2).max(16);
        }
        self.sources.push(source);
    }

    /// Strong references to the live sources; dead ones are dropped.
    fn live(&mut self) -> Vec<Arc<dyn RootSource>> {
        let mut live = Vec::with_capacity(self.sources.len());
        self.sources.retain(|s| match s.upgrade() {
            Some(strong) => {
                live.push(strong);
                true
            }
            None => false,
        });
        live
    }
}

/// The kind of a mutator registration (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    /// A run guard: the VM may run, and polls at its safepoints.
    Run,
    /// An open scope: native/host access that delays collection until it
    /// is closed or released.
    Scope,
    /// A short native operation with no safepoint inside (atom operations).
    Op,
}

/// One thread's registrations, while it has any or is stopped somewhere.
#[derive(Debug, Default)]
struct ThreadAccess {
    runs: u32,
    scopes: u32,
    ops: u32,
    /// The frames of the thread's run, reported when it parked at a
    /// safepoint; `Some` exactly while it is parked there.
    parked: Option<Vec<ObjectId>>,
    /// The thread is blocked in its own collection request.
    requesting: bool,
}

impl ThreadAccess {
    fn in_access(&self) -> bool {
        self.runs + self.scopes + self.ops > 0
    }

    fn stopped(&self) -> bool {
        !self.in_access() || self.parked.is_some() || self.requesting
    }

    fn count(&mut self, access: Access) -> &mut u32 {
        match access {
            Access::Run => &mut self.runs,
            Access::Scope => &mut self.scopes,
            Access::Op => &mut self.ops,
        }
    }
}

/// Collector state, behind the heap's `gc` lock.
#[derive(Debug, Default)]
pub(crate) struct GcState {
    /// Threads in heap access or stopped at a safepoint; an entry is removed
    /// when its thread has neither.
    threads: HashMap<ThreadId, ThreadAccess>,
    /// Collection requests whose requester has not finished collecting.
    pending: usize,
    /// Requests served so far. A thread waiting for a request (parked, or
    /// entering heap access) continues once this moves past the value it
    /// saw, even if another request is already pending: it then stops at
    /// its next safepoint, so back-to-back requests cannot starve it.
    served: u64,
    /// Completed collections.
    pub(crate) collections: u64,
}

impl GcState {
    /// Drop `me`'s entry if it no longer records anything.
    fn forget_if_idle(&mut self, me: ThreadId) {
        if self
            .threads
            .get(&me)
            .is_some_and(|t| !t.in_access() && t.parked.is_none() && !t.requesting)
        {
            self.threads.remove(&me);
        }
    }

    fn in_access(&self, me: ThreadId) -> bool {
        self.threads.get(&me).is_some_and(ThreadAccess::in_access)
    }

    /// `me` may collect: it holds no run guard or native operation.
    fn may_collect(&self, me: ThreadId) -> bool {
        self.threads
            .get(&me)
            .is_none_or(|t| t.runs == 0 && t.ops == 0)
    }

    /// Every thread other than `me` is stopped.
    fn others_stopped(&self, me: ThreadId) -> bool {
        self.threads.iter().all(|(&id, t)| id == me || t.stopped())
    }

    /// Frames reported by runs parked at safepoints.
    fn parked_roots(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.threads
            .values()
            .filter_map(|t| t.parked.as_deref())
            .flatten()
            .copied()
    }
}

/// One mutator registration of the current thread; deregisters on drop,
/// including on unwind. Not `Send`: it describes what its thread is doing.
#[derive(Debug)]
pub(crate) struct MutatorToken<'h> {
    heap: &'h Heap,
    access: Access,
    /// Collections completed before this registration.
    epoch: u64,
    _thread_bound: PhantomData<*const ()>,
}

impl MutatorToken<'_> {
    /// Collections completed before this mutator registered. For an
    /// [`Access::Op`] registration (the atom module's reclamation epoch) no
    /// collection completes while the token exists: its thread is in heap
    /// access and never stopped (an operation contains no safepoint, and a
    /// run cannot park while one is in progress).
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for MutatorToken<'_> {
    fn drop(&mut self) {
        let me = thread::current().id();
        let mut gc = lock(&self.heap.gc);
        if let Some(entry) = gc.threads.get_mut(&me) {
            *entry.count(self.access) -= 1;
        }
        gc.forget_if_idle(me);
        if gc.pending > 0 {
            self.heap.gc_changed.notify_all();
        }
    }
}

/// What a completed collection did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectStats {
    /// Root references reported (one object may be reported several times).
    pub roots: usize,
    /// Objects found reachable and kept.
    pub live: usize,
    /// Objects reclaimed.
    pub reclaimed: usize,
}

/// Why a collection did not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectError {
    /// The calling thread holds a run guard (including inside a host
    /// effect) or is inside a native operation; or, for the non-waiting
    /// [`Runtime::collect`](crate::Runtime::collect), another thread is in
    /// heap access and not stopped at a safepoint. See the
    /// [`gc`](crate::heap::gc) module docs.
    MutatorActive,
}

impl std::fmt::Display for CollectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollectError::MutatorActive => f.write_str(
                "cannot collect while this thread runs a task, or another thread is in heap access",
            ),
        }
    }
}

impl std::error::Error for CollectError {}

/// Wait on the heap's condition variable, recovering from poisoning like
/// [`lock`].
fn wait<'g>(heap: &Heap, gc: MutexGuard<'g, GcState>) -> MutexGuard<'g, GcState> {
    heap.gc_changed
        .wait(gc)
        .unwrap_or_else(PoisonError::into_inner)
}

/// The rendezvous of one collection: the `gc` lock with every other thread
/// stopped. Ending it (by drop, including on unwind) withdraws the request,
/// if this rendezvous made one, and lets parked threads continue.
struct Stopped<'h> {
    heap: &'h Heap,
    gc: MutexGuard<'h, GcState>,
    me: ThreadId,
    requested: bool,
}

impl Drop for Stopped<'_> {
    fn drop(&mut self) {
        if !self.requested {
            return;
        }
        self.gc.served += 1;
        self.gc.pending -= 1;
        if self.gc.pending == 0 {
            // Release: pairs with the `Acquire` load in `Heap::safepoint`
            // (a hint only; see the module docs).
            self.heap.requested.store(false, Ordering::Release);
        }
        if let Some(entry) = self.gc.threads.get_mut(&self.me) {
            entry.requesting = false;
        }
        self.gc.forget_if_idle(self.me);
        self.heap.gc_changed.notify_all();
    }
}

impl Heap {
    /// Register the current thread as a mutator of kind `access` (module
    /// docs). Entering heap access waits until a pending collection
    /// request has been served; a nested registration does not wait.
    pub(crate) fn enter_mutator(&self, access: Access) -> MutatorToken<'_> {
        let me = thread::current().id();
        let mut gc = self.wait_to_enter(lock(&self.gc), me);
        *gc.threads.entry(me).or_default().count(access) += 1;
        MutatorToken {
            heap: self,
            access,
            epoch: gc.collections,
            _thread_bound: PhantomData,
        }
    }

    /// Block until `me` may enter heap access: at once if it already is in
    /// access, otherwise once no request is pending or one has been served.
    fn wait_to_enter<'g>(
        &'g self,
        mut gc: MutexGuard<'g, GcState>,
        me: ThreadId,
    ) -> MutexGuard<'g, GcState> {
        let ticket = gc.served;
        while gc.pending > 0 && gc.served == ticket && !gc.in_access(me) {
            gc = wait(self, gc);
        }
        gc
    }

    /// True while a collection request is pending: mutators should reach a
    /// safepoint. A hint (module docs).
    pub(crate) fn collection_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    /// A VM poll point: if a collection is requested, report `frames` (the
    /// running task's VM state) and park until a pending request has been served. The
    /// caller is at an instruction boundary and holds no value outside
    /// `frames`. Returns at once when no request is pending, or when the
    /// thread cannot stop here (module docs).
    #[inline]
    pub(crate) fn safepoint(&self, frames: &dyn Trace) {
        if self.collection_requested() {
            self.park_at_safepoint(frames);
        }
    }

    #[cold]
    fn park_at_safepoint(&self, frames: &dyn Trace) {
        let me = thread::current().id();
        let mut gc = lock(&self.gc);
        if gc.pending == 0 {
            return;
        }
        // Only a thread's single run, with no native operation in progress,
        // can stop here: another run guard's frames would be unreported.
        let Some(entry) = gc
            .threads
            .get_mut(&me)
            .filter(|t| t.runs == 1 && t.ops == 0)
        else {
            return;
        };
        let mut roots = Vec::new();
        frames.trace(&mut |id| roots.push(id));
        entry.parked = Some(roots);
        self.gc_changed.notify_all();
        let ticket = gc.served;
        while gc.pending > 0 && gc.served == ticket {
            gc = wait(self, gc);
        }
        if let Some(entry) = gc.threads.get_mut(&me) {
            entry.parked = None;
        }
    }

    /// Run `f` with the current thread's scopes released (module docs):
    /// collections may run meanwhile; on return (or unwind) the scopes are
    /// reentered, waiting for a pending request to be served if the thread has no
    /// other heap access. `f` cannot reach a scope (`Send` bound at the
    /// public entry point, [`Scope::released`](crate::Scope::released)).
    pub(crate) fn release_scopes<R>(&self, f: impl FnOnce() -> R) -> R {
        struct Reenter<'h> {
            heap: &'h Heap,
            me: ThreadId,
            scopes: u32,
        }
        impl Drop for Reenter<'_> {
            fn drop(&mut self) {
                let gc = lock(&self.heap.gc);
                let mut gc = self.heap.wait_to_enter(gc, self.me);
                gc.threads.entry(self.me).or_default().scopes += self.scopes;
            }
        }
        let me = thread::current().id();
        let scopes = {
            let mut gc = lock(&self.gc);
            let scopes = gc
                .threads
                .get_mut(&me)
                .map_or(0, |t| std::mem::take(&mut t.scopes));
            gc.forget_if_idle(me);
            if gc.pending > 0 {
                self.gc_changed.notify_all();
            }
            scopes
        };
        let _reenter = Reenter {
            heap: self,
            me,
            scopes,
        };
        f()
    }

    /// Reach the rendezvous: the `gc` lock with every other thread stopped.
    /// With `request`, raise a collection request and wait for it; without,
    /// fail instead of waiting.
    fn stop_the_world(&self, request: bool) -> Result<Stopped<'_>, CollectError> {
        let me = thread::current().id();
        let mut gc = lock(&self.gc);
        if !gc.may_collect(me) {
            return Err(CollectError::MutatorActive);
        }
        if gc.others_stopped(me) {
            return Ok(Stopped {
                heap: self,
                gc,
                me,
                requested: false,
            });
        }
        if !request {
            return Err(CollectError::MutatorActive);
        }
        gc.pending += 1;
        self.requested.store(true, Ordering::Release);
        gc.threads.entry(me).or_default().requesting = true;
        // Other requesters waiting for a rendezvous see this thread stopped.
        self.gc_changed.notify_all();
        // Nothing here can unwind before `Stopped` takes over the request
        // (waiting recovers from poisoning).
        while !gc.others_stopped(me) {
            gc = wait(self, gc);
        }
        Ok(Stopped {
            heap: self,
            gc,
            me,
            requested: true,
        })
    }

    /// Report every root with its category, holding the root, scope and
    /// source locks; `parked` are the frames of runs stopped at safepoints.
    fn trace_all_roots(
        roots: &super::RootTable,
        scopes: &super::ScopeTable,
        sources: &[Arc<dyn RootSource>],
        parked: impl Iterator<Item = ObjectId>,
        visit: &mut dyn FnMut(RootCategory, ObjectId),
    ) {
        roots.trace(&mut |id| visit(RootCategory::PersistentHandle, id));
        scopes.trace(&mut |id| visit(RootCategory::ScopeLocal, id));
        parked.for_each(|id| visit(RootCategory::TaskFrame, id));
        for source in sources {
            source.trace_roots(visit);
        }
    }

    /// Stop-the-world mark/sweep now, or fail without waiting. See the
    /// module docs for when it may run.
    pub(crate) fn collect(&self) -> Result<CollectStats, CollectError> {
        self.stop_the_world(false)
            .map(|world| self.mark_and_sweep(world))
    }

    /// Request a collection and block until the rendezvous completes and
    /// the collection has run (module docs). Fails at once, without
    /// requesting, if the calling thread holds a run guard or is inside a
    /// native operation.
    pub(crate) fn request_collection(&self) -> Result<CollectStats, CollectError> {
        self.stop_the_world(true)
            .map(|world| self.mark_and_sweep(world))
    }

    fn mark_and_sweep(&self, mut world: Stopped<'_>) -> CollectStats {
        let roots = lock(&self.roots);
        let scopes = lock(&self.scopes);
        // Strong references keep sources alive while traced; they are dropped
        // after every heap lock is released.
        let sources = lock(&self.sources).live();

        let mut grey = Vec::new();
        let mut root_count = 0;
        Self::trace_all_roots(
            &roots,
            &scopes,
            &sources,
            world.gc.parked_roots(),
            &mut |_, id| {
                root_count += 1;
                grey.push(id);
            },
        );

        let mut objects = lock(&self.objects);
        let mut live = 0;
        while let Some(id) = grey.pop() {
            if objects.mark(id, &mut |child| grey.push(child)) {
                live += 1;
            }
        }
        let garbage: Vec<HeapObject> = objects.sweep();
        let stats = CollectStats {
            roots: root_count,
            live,
            reclaimed: garbage.len(),
        };
        world.gc.collections += 1;
        drop((objects, scopes, roots, world));
        // Reclaimed payloads and possibly-last source references are dropped
        // with no heap lock held, so their destructors cannot deadlock.
        drop((garbage, sources));
        stats
    }

    /// The current roots with their categories (tests and diagnostics),
    /// taken at a rendezvous like a collection: without `request` it fails
    /// instead of waiting ([`Heap::collect`]), with it it requests one and
    /// waits ([`Heap::request_collection`]).
    #[cfg(test)]
    pub(crate) fn root_snapshot_at(
        &self,
        request: bool,
    ) -> Result<Vec<(RootCategory, ObjectId)>, CollectError> {
        let world = self.stop_the_world(request)?;
        let roots = lock(&self.roots);
        let scopes = lock(&self.scopes);
        let sources = lock(&self.sources).live();
        let mut out = Vec::new();
        Self::trace_all_roots(
            &roots,
            &scopes,
            &sources,
            world.gc.parked_roots(),
            &mut |c, id| out.push((c, id)),
        );
        drop((scopes, roots, world));
        drop(sources);
        Ok(out)
    }

    /// [`Heap::root_snapshot_at`] without waiting.
    #[cfg(test)]
    pub(crate) fn root_snapshot(&self) -> Result<Vec<(RootCategory, ObjectId)>, CollectError> {
        self.root_snapshot_at(false)
    }
}

/// Collector tests over the crate-internal heap API (native payloads, raw
/// identities, root categories). Programs are hand-assembled bytecode: early
/// VM tests, not JavaScript source support.
#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::heap::{NativePayload, ObjectKind, Rooted};
    use crate::value::Primitive;
    use crate::value::share::Shareability;
    use crate::vm::executable::Code;
    use crate::vm::instr::Instr;
    use crate::vm::{Executable, Opcode};
    use crate::{RunOutcome, Runtime};

    /// Test payload: a mutable list of edges, plus an optional drop counter.
    #[derive(Debug, Default)]
    struct Edges {
        edges: Vec<Value>,
        drops: Option<Arc<AtomicUsize>>,
    }

    impl Trace for Edges {
        fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
            self.edges.trace(visit);
        }
    }

    impl NativePayload for Edges {
        // Mutable (`set_edges`), so task-owned.
        fn shareability(&self) -> Shareability {
            Shareability::TaskOwned
        }
    }

    impl Drop for Edges {
        fn drop(&mut self) {
            if let Some(d) = &self.drops {
                d.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn native(heap: &Heap, edges: Vec<Value>) -> ObjectId {
        let payload = Edges { edges, drops: None };
        heap.alloc(HeapObject::Native(Box::new(payload))).unwrap()
    }

    fn plain(heap: &Heap) -> ObjectId {
        heap.alloc(HeapObject::Plain).unwrap()
    }

    fn set_edges(heap: &Heap, id: ObjectId, edges: Vec<Value>) {
        heap.with_object_mut(id, |o| match o {
            HeapObject::Native(p) => {
                let any: &mut dyn Any = &mut **p;
                any.downcast_mut::<Edges>().unwrap().edges = edges;
            }
            HeapObject::Plain
            | HeapObject::Vector(_)
            | HeapObject::Map(_)
            | HeapObject::Occurrence(_)
            | HeapObject::Builder(_)
            | HeapObject::Function(_)
            | HeapObject::Cell(_) => panic!("not a native object"),
            HeapObject::Atom(_) => panic!("not a native object"),
        })
        .expect("live object");
    }

    fn obj(id: ObjectId) -> Value {
        Value::Object(id)
    }

    fn object_id(value: Value) -> ObjectId {
        match value {
            Value::Object(id) => id,
            other => panic!("expected an object, got {other:?}"),
        }
    }

    #[test]
    fn reachable_objects_keep_identity_and_address() {
        let heap = Arc::new(Heap::default());
        let kept = plain(&heap);
        let _root = Rooted::new(&heap, obj(kept));
        let address = heap.address(kept).unwrap();
        let garbage: Vec<ObjectId> = (0..10).map(|_| plain(&heap)).collect();

        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (1, 10));
        // Refill the freed slots and grow the table well past its old size.
        let fresh: Vec<ObjectId> = (0..1000).map(|_| plain(&heap)).collect();
        heap.collect().unwrap();

        assert!(heap.is_live(kept));
        assert_eq!(heap.address(kept), Some(address), "objects never move");
        assert_eq!(heap.kind(kept), Some(ObjectKind::PlainObject));
        assert!(garbage.iter().all(|&g| !heap.is_live(g)));
        assert!(fresh.iter().all(|&f| !heap.is_live(f)));
        assert_eq!(heap.stats().objects, 1);
        assert_eq!(heap.stats().collections, 2);
    }

    #[test]
    fn unreachable_cycles_are_reclaimed() {
        let heap = Arc::new(Heap::default());
        let a = native(&heap, vec![]);
        let b = native(&heap, vec![obj(a)]);
        set_edges(&heap, a, vec![obj(b)]); // a <-> b
        let selfish = native(&heap, vec![]);
        set_edges(&heap, selfish, vec![obj(selfish)]); // selfish -> selfish

        let root = Rooted::new(&heap, obj(a));
        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (2, 1), "self-cycle freed");
        assert!(heap.is_live(a) && heap.is_live(b));

        drop(root);
        let stats = heap.collect().unwrap();
        assert_eq!(
            (stats.live, stats.reclaimed),
            (0, 2),
            "two-node cycle freed"
        );
        assert!(!heap.is_live(a) && !heap.is_live(b));
        assert_eq!(heap.stats().objects, 0);
    }

    #[test]
    fn native_payload_edges_are_traced() {
        let heap = Arc::new(Heap::default());
        let leaf = plain(&heap);
        let deep = plain(&heap);
        let inner = native(&heap, vec![obj(deep), Value::Number(1.0)]);
        let outer = native(&heap, vec![obj(leaf), obj(inner)]);
        let _root = Rooted::new(&heap, obj(outer));

        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (4, 0));
        assert!([leaf, deep, inner, outer].iter().all(|&o| heap.is_live(o)));
        assert_eq!(heap.kind(outer), Some(ObjectKind::Native));

        // Dropping an edge from the payload makes its subtree unreachable.
        set_edges(&heap, outer, vec![obj(leaf)]);
        let stats = heap.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (2, 2));
        assert!(heap.is_live(leaf) && heap.is_live(outer));
        assert!(!heap.is_live(inner) && !heap.is_live(deep));
    }

    #[test]
    fn reclaimed_payloads_are_dropped() {
        let heap = Arc::new(Heap::default());
        let drops = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let payload = Edges {
                edges: vec![],
                drops: Some(Arc::clone(&drops)),
            };
            heap.alloc(HeapObject::Native(Box::new(payload))).unwrap();
        }
        assert_eq!(heap.collect().unwrap().reclaimed, 3);
        assert_eq!(drops.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn stale_ids_are_rejected_after_slot_reuse() {
        let heap = Arc::new(Heap::default());
        let old = native(&heap, vec![]);
        heap.collect().unwrap();
        let new = plain(&heap);
        assert_eq!(new.index, old.index, "the freed slot is reused");
        assert_ne!(new.generation, old.generation);

        assert!(!heap.is_live(old));
        assert_eq!(heap.kind(old), None);
        assert_eq!(heap.address(old), None);
        assert!(heap.with_object_mut(old, |_| ()).is_none());
        assert_eq!(heap.root_if_live(obj(old)), None, "cannot be re-rooted");
        assert_eq!(heap.kind(new), Some(ObjectKind::PlainObject));

        // Even a stale id that reached a root is ignored, never followed or
        // confused with the slot's new occupant.
        let _stale_root = Rooted::new(&heap, obj(old));
        let stats = heap.collect().unwrap();
        assert_eq!((stats.roots, stats.live, stats.reclaimed), (1, 0, 1));
        assert!(!heap.is_live(new));
    }

    /// `return constant[0]`, with an object constant. Loaded code only has
    /// primitive constants in this slice; this builds `Code` directly to
    /// exercise the constant-pool edge.
    fn spawn_with_constant(rt: &Runtime, constant: ObjectId, args: &[&Rooted]) -> crate::Task {
        let code = Code {
            name: "object-constant".into(),
            params: 1,
            locals: 1,
            constants: Box::new([obj(constant)]),
            functions: Box::new([]),
            captures: 0,
            offsets: Box::new([0, 3]),
            instrs: Box::new([Instr::Const(0), Instr::Return]),
            max_stack: 1,
            debug: None,
        };
        rt.spawn(&Executable(Arc::new(code)), args).unwrap()
    }

    #[test]
    fn root_snapshot_reports_every_category() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let held = rt.scope(|s| {
            let o = s.new_object();
            s.persist(o)
        });
        let held_id = object_id(held.value_in(heap).unwrap());
        let constant = plain(heap);
        let frame_obj = plain(heap);
        let arg = Rooted::new(heap, obj(frame_obj));
        let task = spawn_with_constant(&rt, constant, &[&arg]);
        drop(arg);
        rt.scope(|s| {
            let local_id = object_id(s.raw(s.new_object()));
            let mut snapshot = heap.root_snapshot().unwrap();
            snapshot.sort_by_key(|&(_, id)| id);
            let mut expected = vec![
                (RootCategory::PersistentHandle, held_id),
                (RootCategory::ScopeLocal, local_id),
                (RootCategory::ExecutableConstant, constant),
                (RootCategory::TaskFrame, frame_obj),
            ];
            expected.sort_by_key(|&(_, id)| id);
            assert_eq!(snapshot, expected);
        });
        drop(task);
    }

    #[test]
    fn executable_constants_are_roots_while_a_task_holds_the_code() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let constant = plain(heap);
        let task = spawn_with_constant(&rt, constant, &[]);
        rt.collect().unwrap();
        assert!(heap.is_live(constant));
        let result = match task.run().unwrap() {
            RunOutcome::Complete(r) => r,
            other => panic!("{other:?}"),
        };
        drop(task);
        rt.collect().unwrap();
        assert!(heap.is_live(constant), "now held by the result root");
        drop(result);
        rt.collect().unwrap();
        assert!(!heap.is_live(constant));
    }

    #[test]
    fn finished_and_dropped_tasks_stop_being_roots() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let mut asm = crate::Assembler::new();
        asm.op(Opcode::NewObject).set_local(0);
        asm.constant(Primitive::Null).op(Opcode::Return);
        let exe = asm.finish("drop-local", 0, 1).unwrap();

        // Finished: the frames are released at completion.
        let task = rt.spawn(&exe, &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Complete(_)));
        assert_eq!(heap.stats().objects, 1);
        assert_eq!(rt.collect().unwrap().reclaimed, 1);

        // Dropped while yielded: the source registry forgets the task.
        let task = rt.spawn(&exe, &[]).unwrap();
        let outcome = task
            .run_with(crate::Budget::instructions(2), &mut crate::host::NoHost)
            .unwrap();
        assert!(matches!(outcome, RunOutcome::Yielded));
        assert_eq!(rt.collect().unwrap().reclaimed, 0, "held by saved locals");
        drop(task);
        assert_eq!(rt.collect().unwrap().reclaimed, 1);
        assert!(heap.root_snapshot().unwrap().is_empty());
    }

    /// Host for [`spin_until_stopped`]: reports the first iteration, then
    /// continues until `stop` is set (or fails after a bound).
    struct SpinHost {
        entered: Option<std::sync::mpsc::Sender<()>>,
        stop: Arc<std::sync::atomic::AtomicBool>,
        deadline: std::time::Instant,
    }

    impl crate::Host for SpinHost {
        fn effect(&mut self, _: crate::HostCall) -> Result<crate::Primitive, String> {
            if let Some(tx) = self.entered.take() {
                tx.send(()).unwrap();
            }
            if std::time::Instant::now() > self.deadline {
                return Err("test loop ran past its deadline".to_string());
            }
            Ok(crate::Primitive::Bool(!self.stop.load(Ordering::Acquire)))
        }
    }

    /// `function(kept) { do { new Object(); } while (host_effect(0, null)); return kept; }`
    fn spin_until_stopped() -> Executable {
        let mut asm = crate::Assembler::new();
        let (top, done) = (asm.label(), asm.label());
        asm.bind(top);
        asm.op(Opcode::NewObject).op(Opcode::Pop);
        asm.constant(Primitive::Null).host_effect(0);
        asm.jump_if_false(done).jump(top);
        asm.bind(done);
        asm.get_local(0).op(Opcode::Return);
        asm.finish("spin-until-stopped", 1, 1).unwrap()
    }

    /// The rendezvous root snapshot, with two tasks running on two other
    /// threads (parked at VM safepoints), a third thread holding a released
    /// scope, and a host root: it has each running task's frame object as a
    /// `TaskFrame` root (reported by the runner, not read from the locked
    /// task body), the scope's local and the host root, and nothing else.
    #[test]
    fn rendezvous_snapshot_has_every_mutators_running_frames_and_host_roots() {
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        const HANDSHAKE: Duration = Duration::from_secs(10);

        let rt = Runtime::new();
        let heap = rt.heap();
        let exe = spin_until_stopped();
        let host_root = rt.scope(|s| {
            let o = s.new_object();
            s.persist(o)
        });
        let stop = Arc::new(AtomicBool::new(false));
        let mut expected = vec![(
            RootCategory::PersistentHandle,
            object_id(host_root.value_in(heap).unwrap()),
        )];
        let (entered_tx, entered_rx) = mpsc::channel();
        let runners: Vec<_> = (0..2)
            .map(|_| {
                let arg = rt.scope(|s| {
                    let o = s.new_object();
                    s.persist(o)
                });
                expected.push((
                    RootCategory::TaskFrame,
                    object_id(arg.value_in(heap).unwrap()),
                ));
                let task = rt.spawn(&exe, &[&arg]).unwrap();
                let mut host = SpinHost {
                    entered: Some(entered_tx.clone()),
                    stop: Arc::clone(&stop),
                    deadline: Instant::now() + HANDSHAKE,
                };
                std::thread::spawn(move || {
                    drop(arg); // only the running frame holds it now
                    match task.run_with(crate::Budget::UNLIMITED, &mut host).unwrap() {
                        RunOutcome::Complete(r) => r,
                        other => panic!("{other:?}"),
                    }
                })
            })
            .collect();
        let (opened_tx, opened_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel::<()>();
        let scope_thread = {
            let rt = rt.clone();
            std::thread::spawn(move || {
                rt.scope(|s| {
                    let local = s.new_object();
                    opened_tx.send(object_id(s.raw(local))).unwrap();
                    s.released(move || finish_rx.recv_timeout(HANDSHAKE).unwrap());
                })
            })
        };
        for _ in 0..2 {
            entered_rx.recv_timeout(HANDSHAKE).unwrap();
        }
        expected.push((
            RootCategory::ScopeLocal,
            opened_rx.recv_timeout(HANDSHAKE).unwrap(),
        ));

        assert_eq!(heap.root_snapshot(), Err(CollectError::MutatorActive));
        let mut snapshot = heap.root_snapshot_at(true).unwrap();
        snapshot.sort_by_key(|&(_, id)| id);
        expected.sort_by_key(|&(_, id)| id);
        assert_eq!(snapshot, expected);

        stop.store(true, Ordering::Release);
        finish_tx.send(()).unwrap();
        scope_thread.join().unwrap();
        for runner in runners {
            let result = object_id(runner.join().unwrap().value_in(heap).unwrap());
            assert!(expected.contains(&(RootCategory::TaskFrame, result)));
            assert!(heap.is_live(result));
        }
    }
}
