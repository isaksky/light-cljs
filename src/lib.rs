//! clj-quickjs: a Rust-native JavaScript runtime for concurrent compile-time
//! execution. Requirements: `docs/design.md`.
//!
//! # Embedding boundary
//!
//! * [`Runtime`]: shared heap and execution machinery; owns no threads.
//! * [`Executable`]: immutable, validated code shared by all tasks running it.
//! * [`Task`]: task-owned VM state, driven by the embedder via [`Task::run`] or
//!   [`Task::run_with`] (instruction [`Budget`] plus leaf [`Host`] effects)
//!   under a non-blocking execution guard ([`Task::try_begin_run`]).
//! * [`Rooted`] (persistent root) and [`Local`] (scoped access within
//!   [`Runtime::scope`]): the only ways to hold heap values from Rust.
//! * [`RunOutcome`]: `Complete(value)`, `Failed(error)`, `Yielded` (budget used
//!   up; run again to continue), and the reserved `Waiting(key)`.
//!
//! See `docs/architecture.md` for the boundaries and continuation shape.
//!
//! # Module layout
//!
//! | Module | Owns |
//! |--------|------|
//! | `runtime` | [`Runtime`]: spawning tasks, scopes, roots |
//! | [`task`] | [`Task`], execution guard, [`RunOutcome`] |
//! | [`vm`] | instruction format, executable validation, task-owned frames, dispatch, test assembler |
//! | [`heap`] | object storage and identity, roots and scoped handles ([`heap::roots`]), tracing contract ([`heap::gc`]) |
//! | `value` | raw VM values (crate-private) and root-free views |
//! | `ids` | private task/world/dependency identifiers |
//! | `error` | runtime failure and diagnostics |
//! | [`host`] | the leaf host-effect trait ([`Host`]) and its non-reentrancy contract |
//!
//! The module list is append-only per pellet.
//!
//! # Status
//!
//! No JavaScript source frontend exists yet. Executables are built from
//! hand-assembled bytecode ([`Assembler`]), which is an early VM test tool,
//! not JavaScript source support. The heap is a temporary non-reclaiming stub;
//! no garbage collection is implemented.

mod error;
pub mod heap;
pub mod host;
mod ids;
mod runtime;
pub mod task;
mod value;
pub mod vm;

pub use error::{Diagnostic, VmError, VmErrorKind};
pub use heap::{HandleError, HeapStats, Local, Rooted, Scope};
pub use host::{Host, HostCall};
pub use ids::{DependencyId, TaskId, WaitKey, WorldId};
pub use runtime::Runtime;
pub use task::{Budget, RunError, RunGuard, RunOutcome, Task, TaskStatus};
pub use value::{Primitive, ValueView};
pub use vm::{
    Assembler, Executable, ExecutableSpec, Label, Opcode, ValidationError, ValidationErrorKind,
};
