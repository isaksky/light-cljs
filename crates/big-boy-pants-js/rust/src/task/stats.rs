//! Read-only snapshot of a task's retained VM state, for measurement
//! (clj-quickjs-15, `docs/design.md` §15).
//!
//! [`TaskStats`] describes the task-owned state a suspended (or not yet
//! finished) task keeps between runs: its saved call frames, its value
//! stack, and its pending resumable native continuations. Its byte figure
//! is a shallow lower-bound estimate from lengths and type sizes; the
//! measurement harness (`examples/bench`) measures actual allocation with a
//! counting allocator. Heap objects the task created (builders, binding
//! cells, functions) are shared-heap objects counted by
//! [`HeapUsage`](crate::HeapUsage), not here.

use std::mem::size_of;

use super::{Task, TaskBody, TaskInner};
use crate::value::Value;
use crate::vm::frames::{Frame, VmState};
use crate::vm::native::NativeFrame;

/// Shape and estimated size of one task's saved VM state ([`Task::stats`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskStats {
    /// Saved call frames, the entry frame included.
    pub frames: usize,
    /// Values on the task's value stack: the locals and operands of every
    /// frame.
    pub stack_values: usize,
    /// Resumable native operations in progress (`__rt.reduce`,
    /// `__rt.needAll`).
    pub native_frames: usize,
    /// True while the VM is suspended at a `__rt.need` whose result it has
    /// not received yet.
    pub pending_need: bool,
    /// Shallow estimate, in bytes, of the task's own storage: the shared task
    /// record and boxed VM state plus `frames`, `stack_values` and
    /// `native_frames` at their type sizes. A lower bound: it counts lengths,
    /// not vector capacities, and excludes the dependency log, producer
    /// requests, handler stack, the waiter entry in a dependency cell, the
    /// root-source registration, executable code (shared, counted once
    /// wherever it is measured) and heap objects.
    pub shallow_bytes: usize,
}

impl Task {
    /// A snapshot of the task's saved VM state, or `None` while a run holds
    /// it or once the task has finished. Like
    /// [`Task::source_location`], it holds the task body for an instant: a
    /// claim made at that instant fails with
    /// [`RunError::AlreadyRunning`](crate::RunError::AlreadyRunning) and
    /// leaves the task runnable. Changes nothing.
    pub fn stats(&self) -> Option<TaskStats> {
        match &*self.0.body.try_lock().ok()? {
            TaskBody::Ready(vm) => Some(stats_of(vm)),
            TaskBody::Finished => None,
        }
    }
}

fn stats_of(vm: &VmState) -> TaskStats {
    let frames = vm.depth();
    let stack_values = vm.stack_len();
    let native_frames = vm.natives.len();
    TaskStats {
        frames,
        stack_values,
        native_frames,
        pending_need: vm.pending_need.is_some(),
        // The entry frame is inside `VmState`; called frames are in a vector.
        shallow_bytes: size_of::<TaskInner>()
            + size_of::<VmState>()
            + (frames - 1) * size_of::<Frame>()
            + stack_values * size_of::<Value>()
            + native_frames * size_of::<NativeFrame>(),
    }
}
