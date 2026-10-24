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

use crate::heap::{HandleError, Heap, HeapStats, Rooted, Scope};
use crate::ids::{TaskId, WorldId};
use crate::task::Task;
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

    /// Create a task that will run `executable` with `args` bound to its
    /// parameters. The task is not run; the caller drives it.
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

    /// Heap usage counters.
    pub fn heap_stats(&self) -> HeapStats {
        self.0.heap.stats()
    }
}
