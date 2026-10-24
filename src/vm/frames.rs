//! Task-owned VM storage.
//!
//! Everything a task needs to continue executing lives here, owned by the
//! task (not by a worker thread or a Rust stack frame): the instruction
//! position, the operand stack, the locals, and diagnostic context. A worker
//! only borrows it for the duration of one run (see [`crate::task`]).
//!
//! This slice has a single frame per task; pellet clj-quickjs-4 turns
//! [`Frame`] into an explicit task-owned call-frame stack. Unlike QuickJS's
//! `JSStackFrame` (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8), frames
//! are not `alloca`'d on the native stack and there is no runtime-global
//! current-frame pointer.

use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::{TaskId, WorldId};
use crate::value::Value;
use crate::vm::executable::Code;

/// One activation of an executable.
#[derive(Debug)]
pub(crate) struct Frame {
    /// Index of the next instruction to execute.
    pub(crate) pc: usize,
    pub(crate) locals: Box<[Value]>,
    pub(crate) operands: Vec<Value>,
}

/// Context reported with every runtime failure.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DiagnosticContext {
    pub(crate) task: TaskId,
    pub(crate) world: WorldId,
}

/// The complete VM state of one task.
#[derive(Debug)]
pub(crate) struct VmState {
    pub(crate) frame: Frame,
    pub(crate) diag: DiagnosticContext,
}

impl VmState {
    /// Fresh state at instruction 0. Arguments fill the parameter locals; as in
    /// JavaScript, missing arguments are `undefined` and extra ones are ignored.
    pub(crate) fn new(code: &Code, args: &[Value], diag: DiagnosticContext) -> VmState {
        let mut locals = vec![Value::Undefined; usize::from(code.locals)].into_boxed_slice();
        for (local, arg) in locals
            .iter_mut()
            .zip(args.iter().take(usize::from(code.params)))
        {
            *local = *arg;
        }
        VmState {
            frame: Frame {
                pc: 0,
                locals,
                operands: Vec::with_capacity(code.max_stack),
            },
            diag,
        }
    }
}

impl Trace for VmState {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        let frame = &self.frame;
        frame
            .locals
            .iter()
            .chain(&frame.operands)
            .for_each(|v| v.trace(visit));
    }
}
