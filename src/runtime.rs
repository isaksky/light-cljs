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

use crate::heap::{CollectError, CollectStats, HandleError, Heap, HeapStats, Rooted, Scope};
use crate::ids::{TaskId, WorldId};
use crate::task::Task;
use crate::value::share::{self, PublishError};
use crate::value::{Primitive, Value};
use crate::vm::Executable;
use crate::vm::frames::{DiagnosticContext, VmState};

#[derive(Debug)]
struct RuntimeShared {
    heap: Arc<Heap>,
    next_task: AtomicU64,
    world: WorldId,
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
    pub fn new() -> Runtime {
        Runtime(Arc::new(RuntimeShared {
            heap: Arc::new(Heap::default()),
            next_task: AtomicU64::new(1),
            world: WorldId::INITIAL,
        }))
    }

    /// The world new tasks run in. Only the initial world exists in this slice.
    pub fn world(&self) -> WorldId {
        self.0.world
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
        let args = args
            .iter()
            .map(|a| a.value_in(&self.0.heap))
            .collect::<Result<Vec<Value>, _>>()?;
        // Relaxed: the counter only needs uniqueness, not ordering with other memory.
        let id = TaskId::from_raw(self.0.next_task.fetch_add(1, Ordering::Relaxed));
        let diag = DiagnosticContext {
            task: id,
            world: self.0.world,
        };
        let vm = VmState::new(&executable.0, &args, diag);
        Ok(Task::new(
            id,
            self.0.world,
            executable.clone(),
            Arc::clone(&self.0.heap),
            vm,
        ))
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
