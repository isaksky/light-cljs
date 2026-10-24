//! Runtime failure reported by [`RunOutcome::Failed`](crate::RunOutcome::Failed).
//!
//! JavaScript exception values (`throw`, `try/catch`) are deferred to pellet
//! clj-quickjs-11; until then a failure is this Rust-side error carrying the
//! diagnostic context saved in the task's VM state.

use std::fmt;
use std::sync::Arc;

use crate::ids::{TaskId, WorldId};
use crate::vm::Opcode;

/// Category of a runtime failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmErrorKind {
    /// An instruction received operand types this slice does not support
    /// (for example `add` on an object). Rejected explicitly instead of
    /// applying JavaScript coercions differently.
    UnsupportedOperand,
    /// A VM invariant was violated at run time. Validation makes this
    /// unreachable; it exists so a bug fails the task instead of panicking.
    Internal,
    /// A leaf host effect reported an error, or the run had no host
    /// ([`Task::run`](crate::Task::run) supplies none).
    HostEffect,
    /// The heap could not allocate an object (its object table is full).
    OutOfMemory,
}

/// Where a failure happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Name of the executable.
    pub executable: Arc<str>,
    pub task: TaskId,
    pub world: WorldId,
    /// Index of the failing instruction.
    pub instruction: usize,
    /// Byte offset and opcode of the failing instruction (`None` only for an
    /// internal error where the position itself was invalid).
    pub byte_offset: Option<u32>,
    pub opcode: Option<Opcode>,
}

/// A runtime failure with its diagnostic context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmError {
    pub(crate) kind: VmErrorKind,
    pub(crate) message: String,
    pub(crate) diagnostic: Diagnostic,
}

impl VmError {
    pub fn kind(&self) -> VmErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn diagnostic(&self) -> &Diagnostic {
        &self.diagnostic
    }
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = &self.diagnostic;
        write!(
            f,
            "{:?}: {} (in {} at instruction {} [{} @ byte {}], {}, {})",
            self.kind,
            self.message,
            d.executable,
            d.instruction,
            d.opcode.map_or("?", Opcode::name),
            d.byte_offset.map_or("?".to_string(), |o| o.to_string()),
            d.task,
            d.world
        )
    }
}

impl std::error::Error for VmError {}
