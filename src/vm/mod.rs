//! The bytecode VM.
//!
//! * [`instr`]: the instruction format (opcode registry, encoding, stack
//!   effects) and the documented primitive operations.
//! * [`executable`]: immutable shared code and its load-time validation.
//! * `frames`: task-owned VM storage: the call-frame stack (instruction
//!   positions, operands, locals), resource limits, diagnostic context.
//! * `dispatch`: the interpreter loop over task-owned state.
//! * [`intrinsics`]: the private collection intrinsics and task-owned
//!   transient builder operations, and their documented contract.
//! * [`asm`]: the bytecode assembler, an early VM test tool (not JavaScript
//!   source support).
//! * `function`: function objects, the heap kind `Call` invokes (appended by
//!   clj-quickjs-4).
//! * `exception`: the task-owned handler stack, completion records and
//!   error objects of `throw`/`try` (clj-quickjs-11).

pub mod asm;
pub(crate) mod dispatch;
pub(crate) mod exception;
pub mod executable;
pub(crate) mod frames;
pub(crate) mod function;
pub mod instr;
pub mod intrinsics;

pub use asm::{Assembler, Label};
pub use executable::{Executable, ExecutableSpec, ValidationError, ValidationErrorKind};
pub use frames::VmLimits;
pub use instr::{Opcode, ThrowKind};
