//! The runtime: the embedding entry point.
//!
//! A [`Runtime`] owns the shared heap and task bookkeeping. It is a
//! cheap-to-clone, `Send + Sync` handle. It owns no threads: the embedder
//! spawns [`Task`]s and drives them by calling [`Task::run`] on threads of its
//! choosing (`docs/design.md` §3). There is no global interpreter lock; tasks
//! of one runtime run in parallel and share only the heap's short metadata
//! locks.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::dep::{
    self, CellStatus, ClaimError, CompleteError, Completion, DependencyTable, ProducerRequest,
    Wakeups,
};
use crate::heap::{CollectError, CollectStats, HandleError, Heap, HeapStats, Rooted, Scope};
use crate::ids::{DependencyKey, ProducerId, TaskId, WorldId};
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
        let heap = Arc::new(Heap::default());
        Runtime(Arc::new(RuntimeShared {
            deps: Arc::new(DependencyTable::new(Arc::clone(&heap))),
            heap,
            next_task: AtomicU64::new(1),
            world: WorldId::INITIAL,
            next_world: AtomicU64::new(1),
            limits,
        }))
    }

    /// The initial world: where [`Runtime::spawn`] runs tasks.
    pub fn world(&self) -> WorldId {
        self.0.world
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
        share::check(&self.0.heap, value)
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
        // Relaxed: the counter only needs uniqueness, not ordering with other memory.
        let id = TaskId::from_raw(self.0.next_task.fetch_add(1, Ordering::Relaxed));
        let diag = DiagnosticContext { task: id, world };
        let vm = VmState::new(&executable.0, &args, diag, self.0.limits);
        Ok(Task::new(
            id,
            world,
            executable.clone(),
            Arc::clone(&self.0.heap),
            Arc::clone(&self.0.deps),
            vm,
        ))
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

    /// A snapshot of `key`'s cell: absent, pending (producer and waiters),
    /// or its terminal outcome with values rooted for the caller.
    pub fn cell_status(&self, key: DependencyKey) -> CellStatus {
        self.0.deps.status(key)
    }

    /// The shared heap (crate tests).
    #[cfg(test)]
    pub(crate) fn heap(&self) -> &Arc<Heap> {
        &self.0.heap
    }

    /// Heap usage counters.
    pub fn heap_stats(&self) -> HeapStats {
        self.0.heap.stats()
    }

    /// Collect now: a deterministic, stop-the-world mark/sweep of the shared
    /// heap. Everything not reachable from a persistent root, an open scope
    /// on this thread, or a live task's frames and executable constants is
    /// reclaimed, cycles included; reachable objects keep their identity
    /// (nothing moves).
    ///
    /// Fails without collecting, and without waiting, with
    /// [`CollectError::MutatorActive`] while any [`RunGuard`](crate::RunGuard)
    /// exists (including inside a host effect) or a scope is open on another
    /// thread. See [`crate::heap::gc`].
    pub fn collect(&self) -> Result<CollectStats, CollectError> {
        self.0.heap.collect()
    }
}
