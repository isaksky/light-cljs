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
//! * [`Runtime::collect`] (never waits) and [`Runtime::request_collection`]
//!   (waits for the safepoint rendezvous): deterministic stop-the-world
//!   mark/sweep of the shared heap ([`heap::gc`]). [`Scope`]s are the no-GC
//!   access scope for host code; [`Scope::released`] is a safepoint.
//! * [`Runtime::publish`]: the publication check for handing a value to
//!   another task; rejects values reaching task-owned mutation with a
//!   [`PublishError`] naming the path (`value::share`).
//! * [`RunOutcome`]: `Complete(value)`, `Failed(error)`, `Yielded` (budget used
//!   up, or a park found its wake already delivered: run again),
//!   `Waiting(key)` (parked by [`RunGuard::park`] until a wake), and
//!   `Cancelled` (the run observed [`Task::cancel`] and cleaned up).
//!   `Complete` carries the script body's value after task-owned engine
//!   jobs drain. Jobs share the run budget and may yield or wait; a wait
//!   blocks the whole task. An uncaught job error fails the task and drops
//!   later jobs; cancellation drops pending jobs. Enqueue is crate-internal.
//! * [`module::ModuleLoader`]: pre-run static graph loading through a separate
//!   [`module::ModuleHost`], with world-scoped immutable record caching. It
//!   accepts cycles and carries JSON source text, without executing bodies.
//! * Dependency cells ([`dep`]): [`RunGuard::need`] reads or subscribes to the
//!   single-assignment cell of a [`DependencyKey`] (world, definition, result
//!   kind); [`Runtime::complete`] publishes its outcome, delivers each waiter's
//!   [`WakeHandle`] to its task's state machine with no lock held, and
//!   returns the [`Wakeups`]: the tasks to enqueue. [`Runtime::cancel_cell`]
//!   lets the embedder settle a cell `Cancelled`; [`Task::cancel`] cancels a
//!   consumer without touching its cell ([`Cancel`]).
//! * Read-only measurement snapshots: [`Runtime::heap_stats`],
//!   [`Runtime::heap_usage`] ([`HeapUsage`], objects by kind and shallow
//!   table bytes) and [`Task::stats`] ([`TaskStats`], saved VM state);
//!   `docs/measurement.md` and `examples/bench` use them.
//!
//! See `docs/architecture.md` for the boundaries and continuation shape.
//!
//! # Module layout
//!
//! | Module | Owns |
//! |--------|------|
//! | `runtime` | [`Runtime`]: spawning tasks, scopes, roots |
//! | [`task`] | [`Task`], execution claim and state machine (park, wake), [`RunOutcome`] |
//! | [`vm`] | instruction format, executable validation, task-owned frames, dispatch, test assembler |
//! | [`heap`] | object storage and identity, roots and scoped handles ([`heap::roots`]), tracing contract ([`heap::gc`]) |
//! | `value` | raw VM values (crate-private), root-free views, string values (`value::string`, [`JsString`]), and publication rules (`value::share`) |
//! | `ids` | opaque task, world, definition, result-kind, dependency-key, producer and wake-handle identifiers |
//! | `error` | runtime failure and diagnostics |
//! | [`host`] | the leaf host-effect trait ([`Host`]) and its non-reentrancy contract; [`host::sim`], the simulated compiler host (worlds, result kinds, producers) |
//! | [`collections`] | persistent vectors and maps, syntax occurrences, and the native collection contract |
//! | [`frontend`] | JavaScript subset lexer, parser, AST and compiler to bytecode, with positioned diagnostics (`docs/grammar.md`) |
//! | [`dep`] | dependency keys, single-assignment cells, read-or-subscribe, per-task dependency log |
//! | [`atom`] | atomic references: single-writer publication, identity compare-and-set, load-to-root acquisition |
//! | [`exec`] | optional reference executor: worker threads, ready queue, placement hook, events, bounded shutdown and cancellation |
//! | [`module`] | pre-run host resolution/fetch, world-scoped record cache and static module graphs |
//!
//! The module list is append-only per pellet.
//!
//! # Status
//!
//! JavaScript source in the documented subset is parsed and compiled to
//! validated bytecode ([`frontend::compile_script`]): straight-line code,
//! conditionals, loops, locals, functions, calls, closures and the
//! compiler-reserved intrinsic binding `__rt`, and string values
//! ([`JsString`]: immutable UTF-16 code units, compared by content); property
//! access cannot be compiled yet. Executables can also be built from hand-assembled bytecode
//! ([`Assembler`]), which is an early VM test tool, not JavaScript source
//! support; at that level the VM runs nested calls and recursion in explicit
//! task-owned frames, bounded by [`VmLimits`] instead of the Rust stack.
//! Garbage collection is a precise nonmoving mark/sweep run at a safepoint
//! rendezvous: [`Runtime::request_collection`] waits for running tasks to
//! reach VM poll points and for scopes to close or be released
//! ([`Scope::released`]); see [`heap::gc`].
//! An optional reference executor ([`exec`]) drives tasks on worker threads
//! with a ready queue; the runtime itself owns no threads.

pub mod atom;
pub mod collections;
pub mod dep;
mod error;
pub mod exec;
pub mod frontend;
pub mod heap;
pub mod host;
mod ids;
mod object;
mod realm;
mod runtime;
pub mod task;
mod value;
pub mod vm;
// Internal enqueue APIs have no production producer until the promise and
// FinalizationRegistry pellets land; this pellet exercises them in lib tests.
#[cfg_attr(not(test), allow(dead_code))]
mod job;

pub mod regexp;

pub use atom::{AtomError, AtomErrorKind, CasOutcome, Publisher};
pub use collections::{BuilderError, CollectionError, PersistentKind, Provenance};
pub use dep::{
    CellStatus, ClaimError, CompleteError, Completion, DemandSite, DependencyGraph, DependencyLog,
    Need, NeedError, ProducerRequest, WaitCycle, WaitStep, Wakeups,
};
pub use error::{CallSite, Diagnostic, SourceLocation, VmError, VmErrorKind};
pub use exec::{Executor, ExecutorConfig, Placement, WorkerId};
pub use heap::{
    CollectError, CollectStats, CollectionObserver, CollectionReport, HandleError, HeapStats,
    HeapUsage, Local, MutatorState, ObjectCounts, Rooted, Scope, ThreadRegistration, WeakHandle,
};
pub use host::{Host, HostCall};
pub use ids::{DependencyId, DependencyKey, ProducerId, ResultKind, TaskId, WakeHandle, WorldId};
pub use runtime::Runtime;
pub use task::{
    Budget, Cancel, RunError, RunGuard, RunOutcome, Task, TaskStats, TaskStatus,
    UnhandledRejection, UnhandledRejections, Wake,
};
pub use value::share::{KeyName, PathStep, PublishError, ValuePath};
pub use value::{BigIntError, JsBigInt, JsString, JsSymbol, MAX_BIGINT_BITS, Primitive, ValueView};
pub use vm::{
    Assembler, ConstructorKind, Executable, ExecutableSpec, Label, Opcode, ThrowKind,
    ValidationError, ValidationErrorKind, VmLimits,
};

pub mod intl;

mod number;

pub mod module;
