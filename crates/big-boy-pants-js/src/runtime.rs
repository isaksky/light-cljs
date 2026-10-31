//! The runtime: the embedding entry point.
//!
//! A [`Runtime`] owns the shared heap and task bookkeeping. It is a
//! cheap-to-clone, `Send + Sync` handle. It owns no threads: the embedder
//! spawns [`Task`]s and drives them by calling [`Task::run`] on threads of its
//! choosing (`docs/design.md` §3). There is no global interpreter lock; tasks
//! of one runtime run in parallel and share only the heap's short metadata
//! locks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use crate::dep::{
    self, CellStatus, ClaimError, CompleteError, Completion, DependencyTable, ProducerRequest,
    Wakeups,
};
use crate::heap::{
    CollectError, CollectStats, CollectionObserver, HandleError, Heap, HeapStats, HeapUsage,
    Rooted, Scope, ThreadRegistration,
};
use crate::ids::{DependencyKey, ProducerId, ResultKind, TaskId, WorldId};
use crate::task::Task;
use crate::value::share::{self, PublishError};
use crate::value::{Primitive, Value};
use crate::vm::Executable;
use crate::vm::frames::{DiagnosticContext, VmLimits, VmState};

#[derive(Debug)]
struct RuntimeShared {
    heap: Arc<Heap>,
    next_task: AtomicU64,
    world: WorldId,
    next_world: AtomicU64,
    deps: Arc<DependencyTable>,
    /// Resource policy given to every task spawned here.
    limits: VmLimits,
}

/// Shared runtime: heap plus execution machinery. See the module docs.
#[derive(Clone, Debug)]
pub struct Runtime(Arc<RuntimeShared>);

/// Why [`Runtime::try_with_heap_policy`] could not start a runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeStartError {
    /// The realm's intrinsics and one task global do not fit the policy's
    /// `hard_limit_bytes` or `hard_limit_objects`.
    HeapLimit,
    /// The object table or the process allocator was exhausted.
    OutOfMemory,
}

impl std::fmt::Display for RuntimeStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::HeapLimit => "the heap limit cannot hold the runtime's realm",
            Self::OutOfMemory => "heap exhausted while building the runtime's realm",
        })
    }
}

impl std::error::Error for RuntimeStartError {}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    /// A runtime whose tasks get the default [`VmLimits`].
    pub fn new() -> Runtime {
        Runtime::with_limits(VmLimits::DEFAULT)
    }

    /// A runtime whose tasks run under `limits`: a call beyond them fails the
    /// task with [`VmErrorKind::StackOverflow`](crate::VmErrorKind::StackOverflow).
    pub fn with_limits(limits: VmLimits) -> Runtime {
        Self::with_heap_policy(limits, crate::HeapPolicy::default())
    }

    /// Set the runtime-wide collection policy alongside per-task VM limits.
    ///
    /// The realm is built lazily, by the first task or scope that needs it.
    /// Under a hard limit too small for it, that first use fails with
    /// [`VmErrorKind::HeapLimit`](crate::VmErrorKind::HeapLimit) (a task) or
    /// `None` (a checked scope operation). Use
    /// [`Runtime::try_with_heap_policy`] to find this out at startup instead.
    pub fn with_heap_policy(limits: VmLimits, policy: crate::HeapPolicy) -> Runtime {
        let heap = Arc::new(Heap::with_policy(policy));
        Self::with_heap(limits, heap)
    }

    /// [`Runtime::with_heap_policy`], building the realm's shared intrinsics
    /// and probing one task's global object before returning, so a hard
    /// limit that cannot hold them fails here rather than in the first task.
    /// Never panics or aborts on a denial; a runtime that fails to start is
    /// dropped with everything it allocated.
    ///
    /// On success the probe global has been collected again:
    /// [`HeapStats::managed_bytes`](crate::HeapStats::managed_bytes) then
    /// reports the fixed footprint of the realm under a byte limit.
    pub fn try_with_heap_policy(
        limits: VmLimits,
        policy: crate::HeapPolicy,
    ) -> Result<Runtime, RuntimeStartError> {
        let runtime = Self::with_heap_policy(limits, policy);
        let heap = &runtime.0.heap;
        {
            let _scope = Scope::new(heap);
            let started = crate::realm::intrinsics(heap).is_some()
                && crate::realm::new_global_object(heap, TaskId::from_raw(0)).is_some();
            // Any admission failure recorded while building is a failed
            // start, even one a builder did not report as `None`.
            // Classified while this thread's allocator registration (which
            // records the denial) is still open.
            let denied = heap.take_byte_denial() || heap.take_limit_failure();
            if !started || denied {
                return Err(if denied {
                    RuntimeStartError::HeapLimit
                } else {
                    RuntimeStartError::OutOfMemory
                });
            }
        }
        runtime
            .collect()
            .map_err(|_| RuntimeStartError::OutOfMemory)?;
        Ok(runtime)
    }

    fn with_heap(limits: VmLimits, heap: Arc<Heap>) -> Runtime {
        Runtime(Arc::new(RuntimeShared {
            deps: Arc::new(DependencyTable::new(Arc::clone(&heap))),
            heap,
            next_task: AtomicU64::new(1),
            world: WorldId::INITIAL,
            next_world: AtomicU64::new(1),
            limits,
        }))
    }

    #[cfg(test)]
    pub(crate) fn with_byte_limit(bytes: usize) -> Runtime {
        Self::with_heap(
            VmLimits::DEFAULT,
            Arc::new(Heap::with_test_byte_headroom(bytes, true)),
        )
    }

    /// The initial world: where [`Runtime::spawn`] runs tasks.
    pub fn world(&self) -> WorldId {
        self.0.world
    }

    /// Process-unique identity of this runtime (task ids are only unique
    /// within one runtime).
    pub(crate) fn runtime_id(&self) -> u64 {
        self.0.deps.runtime
    }

    /// Allocate a new world/version identity, distinct from every other world
    /// of this runtime. Dependency cells of different worlds are distinct, so
    /// a result published in one never satisfies a need in another.
    pub fn new_world(&self) -> WorldId {
        // Relaxed: the counter only needs uniqueness, not ordering.
        WorldId::from_raw(self.0.next_world.fetch_add(1, Ordering::Relaxed))
    }

    /// Open a scope for host access to heap values. [`Local`](crate::Local)
    /// handles created in it cannot escape `f`; see [`crate::heap::roots`].
    ///
    /// # Panics
    ///
    /// When called from inside a leaf host effect, which may not access or
    /// allocate heap values (see [`crate::host`]).
    pub fn scope<R>(&self, f: impl for<'s> FnOnce(&Scope<'s>) -> R) -> R {
        assert!(
            !crate::host::inside_effect(),
            "a leaf host effect may not open a heap scope"
        );
        let scope = Scope::new(&self.0.heap);
        f(&scope)
    }

    /// Create a persistent root holding a primitive.
    pub fn root(&self, value: Primitive) -> Rooted {
        Rooted::new(&self.0.heap, Value::from(value))
    }

    /// Import and persist a primitive after checked backing-store admission.
    /// Returns `None` when admission or allocation fails.
    pub fn try_root(&self, value: Primitive) -> Option<Rooted> {
        self.scope(|scope| scope.try_primitive(value).map(|local| scope.persist(local)))
    }

    /// Check that `value` may be handed to another task: the embedding's
    /// publication entry point (`docs/design.md` §4).
    ///
    /// Succeeds when `value` is a primitive or reaches only persistent
    /// objects (vectors, maps, occurrences) and explicit concurrent
    /// references; nothing is copied, so a task given the value afterwards
    /// sees the same identities. Fails with [`PublishError::NotShareable`],
    /// naming the path to the first task-owned object found (a transient
    /// builder in any state, or a plain object), or
    /// [`PublishError::Handle`] for a handle of another runtime. Cycles and
    /// deep graphs are walked iteratively; a value known shareable (built
    /// from shareable parts, or published before) is accepted in O(1). See
    /// `value::share` for the categories and the cache.
    pub fn publish(&self, value: &Rooted) -> Result<(), PublishError> {
        let value = value.value_in(&self.0.heap).map_err(PublishError::Handle)?;
        share::check(&self.0.heap, &value)
    }

    /// Create a task that will run `executable` with `args` bound to its
    /// parameters. The task is not run; the caller drives it.
    ///
    /// Arguments are not publication-checked here: the embedder decides what
    /// each new task receives, and checks values it hands from one task to
    /// another with [`Runtime::publish`] first. (Task-owned builders stay
    /// protected regardless: only their owning task's run can use them.)
    pub fn spawn(&self, executable: &Executable, args: &[&Rooted]) -> Result<Task, HandleError> {
        self.spawn_in(self.0.world, executable, args)
    }

    /// Lower each record, then spawn a loaded module graph in its
    /// world. Graph traversal and export resolution consume the run budget.
    /// Module cells and
    /// closures are instantiated independently in this task; no host fetch
    /// occurs during its runs. Linking errors fail the task with SyntaxError
    /// before any module body executes. Completion is undefined.
    pub fn spawn_module(&self, graph: &crate::module::ModuleGraph) -> Task {
        let linker = crate::module::Linker::new(graph);
        self.spawn_values(
            graph.world(),
            &graph.entry_code(),
            &[],
            Some(Box::new(linker)),
        )
    }

    /// [`Runtime::spawn`] in world `world`: the task's needs read the
    /// dependency cells of that world (`docs/design.md` §7). `world` should
    /// come from [`Runtime::world`] or [`Runtime::new_world`] of this runtime.
    pub fn spawn_in(
        &self,
        world: WorldId,
        executable: &Executable,
        args: &[&Rooted],
    ) -> Result<Task, HandleError> {
        let args = args
            .iter()
            .map(|a| a.value_in(&self.0.heap))
            .collect::<Result<Vec<Value>, _>>()?;
        Ok(self.spawn_values(world, executable, &args, None))
    }

    fn spawn_values(
        &self,
        world: WorldId,
        executable: &Executable,
        args: &[Value],
        module_link: Option<Box<crate::module::Linker>>,
    ) -> Task {
        // Relaxed: the counter only needs uniqueness, not ordering with other memory.
        let id = TaskId::from_raw(self.0.next_task.fetch_add(1, Ordering::Relaxed));
        let diag = DiagnosticContext { task: id, world };
        let mut vm = VmState::new(&executable.0, args, diag, self.0.limits);
        vm.module_link = module_link;
        Task::new(
            id,
            world,
            executable.clone(),
            Arc::clone(&self.0.heap),
            Arc::clone(&self.0.deps),
            vm,
        )
    }

    /// Claim the single producer of `key`'s cell without subscribing to it,
    /// creating the cell if needed: for a host that produces a result before
    /// (or regardless of whether) any task needs it. Fails if the cell is
    /// already claimed (by a claim or by the first need) or complete. See
    /// [`crate::dep`].
    pub fn claim(&self, key: DependencyKey) -> Result<ProducerRequest, ClaimError> {
        self.0.deps.claim(key)
    }

    /// Complete `key`'s cell with its one terminal outcome, as its claimed
    /// `producer`, and return the wake handles of every task waiting on it,
    /// in subscription order. The caller enqueues them; nothing is run
    /// inline (`docs/design.md` §8).
    /// [`Completion::WeakReady`] instead transfers a transient payload
    /// without retaining it in the cell: its producer must root it until
    /// consumption, and reads after reclamation observe cancellation.
    ///
    /// A `Ready` value or `Failed` error must pass the publication check
    /// ([`Runtime::publish`]); it is not copied, so every reader sees the
    /// same identities, and it stays alive while the cell exists even after
    /// the caller drops its handle. Rejected, with the cell left pending and
    /// its waiters kept: a value reaching task-owned mutation
    /// ([`CompleteError::NotShareable`]), a `producer` that is not the cell's
    /// claimed producer, including one claimed for another world or result
    /// kind ([`CompleteError::NotProducer`]), a handle of another runtime, or
    /// a cell that is already complete ([`CompleteError::AlreadyComplete`]:
    /// outcomes are never replaced or reset).
    pub fn complete(
        &self,
        key: DependencyKey,
        producer: ProducerId,
        completion: Completion<'_>,
    ) -> Result<Wakeups, CompleteError> {
        dep::complete(&self.0.deps, key, producer, completion)
    }

    /// Cancel `key`'s cell on the embedder's authority: install the terminal
    /// outcome `Cancelled` whoever its producer is (claimed or not; the
    /// cell is created if absent, so later needs read it too), and return
    /// the wakes of every waiting task, delivered exactly as by
    /// [`Runtime::complete`]. Their next needs read
    /// [`Need::Cancelled`](crate::Need::Cancelled); the tasks themselves are
    /// not cancelled. Terminal: a later completion by the producer, claim,
    /// or second cancellation is rejected (`AlreadyComplete`), the only
    /// error. A claimed producer can also cancel through `complete`; tasks
    /// cannot cancel cells. See [`crate::dep`], "Cancellation".
    pub fn cancel_cell(&self, key: DependencyKey) -> Result<Wakeups, CompleteError> {
        dep::cancel_cell(&self.0.deps, key)
    }

    /// A snapshot of `key`'s cell: absent, pending (producer and waiters),
    /// or its terminal outcome with values rooted for the caller.
    pub fn cell_status(&self, key: DependencyKey) -> CellStatus {
        self.0.deps.status(key)
    }

    /// The tasks that settlements of lazy sequences woke since the last
    /// call (light-cljs-479, `crate::collections::lazy`): readers parked on
    /// a sequence another task realized, now runnable. A realization runs
    /// inside the realizing task's run, which cannot return them, so they
    /// wait here for the embedder to enqueue, each exactly once.
    pub fn take_lazy_wakeups(&self) -> Vec<crate::Task> {
        self.0.deps.lazy.take_woken()
    }

    /// The serials of the shared lazy cells `task` is realizing now,
    /// outermost first; a cell's serial is its realization key's
    /// dependency ([`Self::lazy_realizer`]). An embedder attributes the
    /// task's effects to these realizations, which a reader on another task
    /// may wait for.
    pub fn realizing(&self, task: crate::TaskId) -> Vec<u64> {
        self.0.heap.realizing.of(task)
    }

    /// As [`Self::realizing`], for an effect the embedder attributes to
    /// those realizations: from then on, a task that reads one of their
    /// settled values without waiting for it is noted
    /// ([`Self::take_lazy_consumed`]), so that the embedder can count the
    /// effect for that reader as for one that waited.
    pub fn attribute_realizing(&self, task: crate::TaskId) -> Vec<u64> {
        self.0.heap.realizing.attribute(task)
    }

    /// The serials of the cells [`Self::attribute_realizing`] returned
    /// whose realized values `task`'s current or latest run on this thread
    /// read, each once, since the last call. Call it on that thread, in a
    /// host effect of the run or after the run returns, before the thread
    /// runs another task: a run of another task, including one in another
    /// runtime, drops what is left.
    pub fn take_lazy_consumed(&self, task: crate::TaskId) -> Vec<u64> {
        crate::collections::lazy::take_consumed(self.runtime_id(), task)
    }

    /// The task realizing the lazy attempt that readers of `key` currently
    /// wait on. An embedder can lend a reader's scheduling priority to that
    /// task. This snapshot does not claim ownership or keep the attempt alive;
    /// settlement or cancellation removes it along with the pending wait.
    pub fn lazy_realizer(&self, key: DependencyKey) -> Option<crate::TaskId> {
        (key.kind() == crate::collections::lazy::LAZY_REALIZATION)
            .then(|| self.0.deps.lazy.realizer(key.dependency().get()))
            .flatten()
    }

    /// Before a task claims a shared lazy sequence or delay that nobody has
    /// realized or is realizing, it asks host effect `effect` (argument 0):
    /// 0 lets it claim the cell, and any other answer is a dependency of
    /// kind `kind` it reads first, suspending while it is pending. The
    /// embedder so decides which task runs a published thunk, whose result
    /// can depend on the task's own state (light-cljs-665). Set once; later
    /// calls are ignored. A task whose host rejects `effect` fails the
    /// realization with that error.
    pub fn set_realization_gate(&self, effect: u16, kind: ResultKind) {
        self.0.heap.realizing.set_gate(effect, kind);
    }

    /// The shared heap (crate-internal native operations and tests).
    pub(crate) fn heap(&self) -> &Arc<Heap> {
        &self.0.heap
    }

    /// Heap usage counters.
    pub fn heap_stats(&self) -> HeapStats {
        self.0.heap.stats()
    }

    /// Collector test hook: the `lastIndex` entries that tasks hold in
    /// quoted global or sticky regex values, as the heap registers them for
    /// release. This diagnostic is never exposed to JavaScript.
    #[doc(hidden)]
    pub fn regex_task_registrations(&self) -> usize {
        self.0.heap.regex_task_registrations()
    }

    /// Whether a [`HeapPolicy::hard_limit_bytes`](crate::HeapPolicy::hard_limit_bytes)
    /// denial happened and no collection since has left an eighth of the
    /// limit free. Every run slice then begins with a collection; an embedder
    /// about to allocate in a host scope outside any run (importing values,
    /// building a request) can do the same with
    /// [`Runtime::request_collection`], since the garbage a task released
    /// after catching its denial is freed only by a later collection
    /// (`docs/architecture.md`, "Byte accounting"). One relaxed atomic load.
    pub fn byte_starved(&self) -> bool {
        self.0.heap.byte_starved()
    }

    /// A read-only snapshot of the object table: live objects by kind and
    /// the shallow bytes the table owns ([`HeapUsage`], whose docs say what
    /// is excluded). Walks every slot under the object-table lock: for tests and
    /// measurement, not hot paths.
    pub fn heap_usage(&self) -> HeapUsage {
        self.0.heap.usage()
    }

    /// Collect now: a deterministic, stop-the-world mark/sweep of the shared
    /// heap. Everything not reachable from a persistent root, an open scope
    /// on this thread, or a live task's frames and executable constants is
    /// reclaimed, cycles included; reachable objects keep their identity
    /// (nothing moves).
    ///
    /// Fails without collecting, and without waiting, with
    /// [`CollectError::MutatorActive`] while this thread holds a
    /// [`RunGuard`](crate::RunGuard) (including inside a host effect), or
    /// another thread is in heap access (a run guard, an unreleased scope, an
    /// atom operation) and not stopped at a safepoint. To wait for running
    /// mutators instead, use [`Runtime::request_collection`]. See
    /// [`crate::heap::gc`].
    pub fn collect(&self) -> Result<CollectStats, CollectError> {
        self.0.heap.collect()
    }

    /// Request a collection and block until the safepoint rendezvous
    /// completes and the collection has run: the deterministic collection
    /// hook for tests and executors.
    ///
    /// Waits until every other thread is outside heap access or stopped at a
    /// safepoint: a running task polls at its VM poll points (run entry,
    /// backward branches, calls, returns, allocation) and parks with its
    /// frames reported as roots; a thread with a scope open delays the
    /// collection until it closes the scope or releases it
    /// ([`Scope::released`], [`Scope::safepoint`]). Threads that never enter
    /// heap access do not delay it. While the request is pending, threads
    /// entering heap access wait for it. Concurrent requests each collect.
    ///
    /// Fails at once with [`CollectError::MutatorActive`], without
    /// requesting, if this thread holds a run guard (including inside a host
    /// effect). Scopes open on this thread are fine: their locals are roots.
    /// It deadlocks only if a thread in heap access waits for this one
    /// without reaching a safepoint (for example a host effect blocking on
    /// it); see [`crate::heap::gc`].
    pub fn request_collection(&self) -> Result<CollectStats, CollectError> {
        self.0.heap.request_collection()
    }

    /// Register the calling thread as a collection participant until the
    /// returned registration drops: every [`CollectionReport`](crate::CollectionReport) names it and
    /// says how it was stopped. Executor workers hold one for their lifetime.
    /// It is not heap access and never delays a collection by itself: a
    /// registered thread holding no run guard, scope or atom operation is
    /// idle and stopped (an idle worker blocked on its ready queue never
    /// holds up a request). Never waits.
    pub fn register_thread(&self) -> ThreadRegistration<'_> {
        self.0.heap.register_thread()
    }

    /// Give every later collection's [`CollectionReport`](crate::CollectionReport) to `observer`
    /// while it is alive (see [`CollectionObserver::collected`] for when and
    /// where it runs). Executors use it to record collections with their
    /// own events.
    pub fn observe_collections(&self, observer: Weak<dyn CollectionObserver>) {
        self.0.heap.observe_collections(observer);
    }

    /// True while a collection request is pending ([`Runtime::request_collection`]):
    /// threads in heap access should reach a safepoint. A snapshot; one atomic load.
    pub fn collection_requested(&self) -> bool {
        self.0.heap.collection_requested()
    }
}
