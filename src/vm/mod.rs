//! The bytecode VM.
//!
//! * [`instr`]: the instruction format (opcode registry, encoding, stack
//!   effects) and the documented primitive operations.
//! * [`executable`]: immutable shared code and its load-time validation.
//! * `frames`: task-owned VM storage (instruction position, operands,
//!   locals, diagnostic context).
//! * `dispatch`: the interpreter loop over task-owned state.
//! * [`intrinsics`]: the private collection intrinsics and task-owned
//!   transient builder operations, and their documented contract.
//! * [`asm`]: the bytecode assembler, an early VM test tool (not JavaScript
//!   source support).

pub mod asm;
pub(crate) mod dispatch;
pub mod executable;
pub(crate) mod frames;
pub mod instr;
pub mod intrinsics;

pub use asm::{Assembler, Label};
pub use executable::{Executable, ExecutableSpec, ValidationError, ValidationErrorKind};
pub use instr::{Opcode, ThrowKind};
