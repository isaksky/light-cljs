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
//! * `native`: resumable native operations, the task-owned native
//!   continuations the loop drives step by step (clj-quickjs-12).
//! * `descriptor`: property descriptors, `Object.defineProperty` and
//!   `Object.getOwnPropertyDescriptor` (appended by clj-quickjs-119).
//! * `prototype`: prototype reflection, `Object.getPrototypeOf`,
//!   `Object.setPrototypeOf`, `Object.create` and `__proto__` (appended by
//!   clj-quickjs-121).
//! * `integrity`: extensibility and the frozen integrity level,
//!   `Object.preventExtensions`, `isExtensible`, `freeze` and `isFrozen`
//!   (appended by clj-quickjs-122).
//! * `keys`: own-property key enumeration, `Object.keys` and
//!   `Object.getOwnPropertyNames` (appended by clj-quickjs-123).

pub mod asm;
pub(crate) mod asynchronous;
pub(crate) mod builtin;
pub(crate) mod date;
pub(crate) mod descriptor;
pub(crate) mod dispatch;
pub(crate) mod exception;
pub mod executable;
pub(crate) mod frames;
pub(crate) mod function;
pub(crate) mod generator;
pub mod instr;
pub(crate) mod integrity;
pub mod intrinsics;
pub(crate) mod json;
pub(crate) mod keys;
pub(crate) mod native;
pub(crate) mod promise;
pub(crate) mod property;
pub(crate) mod prototype;

pub use asm::{Assembler, Label};
pub use executable::{
    ConstructorKind, Executable, ExecutableSpec, ValidationError, ValidationErrorKind,
};
pub use frames::VmLimits;
pub use instr::{Opcode, ThrowKind};

pub(crate) mod async_from_sync;
pub(crate) mod async_generator;

pub(crate) mod private;
pub(crate) mod proxy;
