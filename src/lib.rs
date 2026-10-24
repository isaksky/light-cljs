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
//!   [`Runtime::scope`]): the only ways to keep heap values alive from Rust.
//!   A [`WeakHandle`] names a value without keeping it alive.
//! * [`Runtime::collect`]: deterministic stop-the-world mark/sweep of the
//!   shared heap ([`heap::gc`]).
//! * [`Runtime::publish`]: the publication check for handing a value to
//!   another task; rejects values reaching task-owned mutation with a
//!   [`PublishError`] naming the path (`value::share`).
//! * [`RunOutcome`]: `Complete(value)`, `Failed(error)`, `Yielded` (budget used
//!   up; run again to continue), and the reserved `Waiting(key)`.
//! * Dependency cells ([`dep`]): [`RunGuard::need`] reads or subscribes to the
//!   single-assignment cell of a [`DependencyKey`] (world, definition, result
//!   kind); [`Runtime::complete`] publishes its outcome and returns the wake
//!   handles to enqueue.
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
//! | `value` | raw VM values (crate-private), root-free views, and publication rules (`value::share`) |
//! | `ids` | opaque task, world, definition, result-kind, dependency-key and producer identifiers |
//! | `error` | runtime failure and diagnostics |
//! | [`host`] | the leaf host-effect trait ([`Host`]) and its non-reentrancy contract |
//! | [`collections`] | persistent vectors and maps, syntax occurrences, and the native collection contract |
//! | [`frontend`] | JavaScript subset lexer, parser and AST with positioned diagnostics (`docs/grammar.md`) |
//! | [`dep`] | dependency keys, single-assignment cells, read-or-subscribe, per-task dependency log |
//!
//! The module list is append-only per pellet.
//!
//! # Status
//!
//! JavaScript source is parsed ([`frontend`]) but not yet compiled.
//! Executables are built from hand-assembled bytecode ([`Assembler`]), which
//! is an early VM test tool, not JavaScript source support. Garbage collection is a precise nonmoving
//! mark/sweep that runs only when no task run or foreign scope is active
//! (cooperative safepoints are deferred; see [`heap::gc`]).

pub mod collections;
pub mod dep;
mod error;
pub mod frontend;
pub mod heap;
pub mod host;
mod ids;
mod runtime;
pub mod task;
mod value;
pub mod vm;

pub use collections::{BuilderError, CollectionError, PersistentKind, Provenance};
pub use dep::{
    CellStatus, ClaimError, CompleteError, Completion, DependencyLog, Need, NeedError,
    ProducerRequest,
};
pub use error::{Diagnostic, VmError, VmErrorKind};
pub use heap::{
    CollectError, CollectStats, HandleError, HeapStats, Local, Rooted, Scope, WeakHandle,
};
pub use host::{Host, HostCall};
pub use ids::{DependencyId, DependencyKey, ProducerId, ResultKind, TaskId, WorldId};
pub use runtime::Runtime;
pub use task::{Budget, RunError, RunGuard, RunOutcome, Task, TaskStatus};
pub use value::share::{KeyName, PathStep, PublishError, ValuePath};
pub use value::{Primitive, ValueView};
pub use vm::{
    Assembler, Executable, ExecutableSpec, Label, Opcode, ValidationError, ValidationErrorKind,
};
