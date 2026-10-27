//! Runtime failure reported by [`RunOutcome::Failed`](crate::RunOutcome::Failed).
//!
//! A failure is this Rust-side error carrying the diagnostic context saved in
//! the task's VM state. JavaScript exceptions (`throw`, `try`/`catch`/
//! `finally`, clj-quickjs-11) become a failure only when no handler catches
//! them; the failure then reports where the exception was raised. Which
//! kinds are catchable is documented in `crate::vm::exception`.

use std::fmt;
use std::sync::Arc;

use crate::collections::{BuilderError, CollectionError};
use crate::frontend::Span;
use crate::ids::{DependencyKey, TaskId, WorldId};
use crate::vm::Opcode;

/// Category of a runtime failure.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// A runtime-wide [`HeapPolicy`](crate::HeapPolicy) admission limit was
    /// reached: `hard_limit_objects` (catchable as the primitive string
    /// `heap object limit exceeded`) or `hard_limit_bytes` (`heap byte limit
    /// exceeded`). Neither needs another heap object to be raised. Accounted
    /// bytes approximate, and do not bound, process RSS.
    HeapLimit,
    /// A collection intrinsic rejected its operands by the collection
    /// contract: wrong operand kind, invalid or out-of-bounds index, or a
    /// missing key (see [`crate::vm::intrinsics`]).
    Collection(CollectionError),
    /// A builder operation was rejected by the builder lifecycle: another
    /// task's builder, a frozen builder, or one whose owner has finished.
    Builder(BuilderError),
    /// A JavaScript ReferenceError raised by compiled source: a reference to
    /// an undeclared name, or use of a `let`/`const` binding before its
    /// declaration ran (temporal dead zone). Catchable (clj-quickjs-11).
    ReferenceError,
    /// A JavaScript TypeError: assignment to a `const` binding, a `Call`
    /// whose callee is not a function object (`… is not a function`), or a
    /// property read or write that JavaScript rejects (a property of
    /// `undefined` or `null`, a write to a read-only property or to a value
    /// with no JavaScript properties). Catchable (clj-quickjs-11).
    TypeError,
    /// A plain JavaScript `Error` (no subclass) raised by an intrinsic that
    /// replaces a JavaScript function throwing `new Error(message)`: the
    /// `keyword`/`symbol` operand checks of the ClojureScript core
    /// intrinsics (light-cljs-477). Catchable.
    Error,
    /// A `Call` would exceed the task's [`VmLimits`](crate::VmLimits) (call
    /// depth or operand/local storage). The task fails instead of growing
    /// further; no Rust stack is involved, so deep recursion can never
    /// overflow the worker's stack. QuickJS reports this as an InternalError
    /// "stack overflow" (`JS_ThrowStackOverflow`); like QuickJS's, it is
    /// catchable (clj-quickjs-11).
    StackOverflow,
    /// A binding-cell instruction (`CellInit`, `CellGet`, `CellSet`) ran in a
    /// task other than the one that created the cell: the mutable bindings a
    /// closure captures are task-owned (`crate::vm::function`), so a closure
    /// over them can be called only by its own task.
    ForeignBinding {
        /// The task that created the cell.
        owner: TaskId,
        /// The task that tried to use it.
        caller: TaskId,
    },
    /// An atom intrinsic rejected its operands: not an atom, a value to
    /// install that reaches task-owned mutation, or compare-and-set on a
    /// single-writer atom (see [`crate::atom`]).
    Atom(crate::AtomErrorKind),
    /// A `Need` (`__rt.need`) read a dependency whose producer failed. The
    /// diagnostic names the `Need` instruction (and its source position), so
    /// language exception handling (clj-quickjs-11) can later raise it
    /// there; the published error value stays in the cell
    /// ([`Runtime::cell_status`](crate::Runtime::cell_status)).
    DependencyFailed(DependencyKey),
    /// A `Need` read a dependency whose cell was cancelled (by its producer
    /// or the embedder). Not the cancellation of the needing task itself,
    /// which ends the run [`RunOutcome::Cancelled`](crate::RunOutcome::Cancelled).
    DependencyCancelled(DependencyKey),
    /// A JavaScript `throw` whose value no handler caught (and that is not
    /// an error object, which reports the error it was created for). The
    /// diagnostic names the `throw` that raised it last; the message
    /// describes the value (`uncaught exception: 42`). See
    /// `crate::vm::exception`.
    Thrown,
    /// A property instruction (`GetField`, `PutField`) on an ordinary object
    /// that the running task does not own (clj-quickjs-114): an ordinary
    /// object is task-owned mutable state, so only its owning task reads or
    /// changes its properties. An object the embedder created has no owner
    /// (`owner: None`): every task may read it and none may change it.
    ForeignObject {
        /// The task that created the object; `None` for the embedder.
        owner: Option<TaskId>,
        /// The task that tried to use it.
        caller: TaskId,
    },
    /// An invalid array length or another out-of-range built-in argument.
    /// Catchable as an instance of the realm's RangeError constructor.
    RangeError,
    /// A global declaration clashes with a restricted global property.
    SyntaxError,
    /// A string result exceeds `2^30 - 1` UTF-16 code units. Catchable as
    /// InternalError with message "string too long" (clj-quickjs-231).
    StringTooLong,
}

/// One active caller in a failure's call chain: the `Call` instruction a
/// frame was executing when the failure happened in a frame it called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    /// Name of the caller's executable.
    pub executable: Arc<str>,
    /// Index of the caller's `Call` instruction.
    pub instruction: usize,
    pub byte_offset: Option<u32>,
    /// Source position of the call (its `(` token), for code compiled from
    /// JavaScript.
    pub source: Option<SourceLocation>,
}

/// Where a failure happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Name of the executable of the failing frame (the innermost one).
    pub executable: Arc<str>,
    pub task: TaskId,
    pub world: WorldId,
    /// Index of the failing instruction.
    pub instruction: usize,
    /// Byte offset and opcode of the failing instruction (`None` only for an
    /// internal error where the position itself was invalid).
    pub byte_offset: Option<u32>,
    pub opcode: Option<Opcode>,
    /// Source position of the failing instruction, for code compiled from
    /// JavaScript source (`None` for hand-assembled bytecode).
    pub source: Option<SourceLocation>,
    /// The failing frame's callers, innermost first: the last entry is the
    /// task's entry frame. Empty when the failure happened in the entry
    /// frame. All frames of a task share its task and world.
    pub call_chain: Vec<CallSite>,
}

/// A position in JavaScript source attached to a compiled instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    /// Byte range of the source the instruction is attributed to: the
    /// token of an operator or call (the operator, the call's `(`), the
    /// identifier of a reference, otherwise the construct it was lowered
    /// from (`docs/grammar.md`, "Errors").
    pub span: Span,
    /// 1-based line of `span.start`.
    pub line: u32,
    /// 1-based column of `span.start`, in Unicode scalar values.
    pub column: u32,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
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
        self.kind.clone()
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
        )?;
        if let Some(at) = d.source {
            write!(f, " at source {at}")?;
        }
        for site in d.call_chain.iter().take(SHOWN_CALLERS) {
            write!(
                f,
                "\n  called from {} at instruction {}",
                site.executable, site.instruction
            )?;
            if let Some(at) = site.source {
                write!(f, " (source {at})")?;
            }
        }
        match d.call_chain.len().checked_sub(SHOWN_CALLERS) {
            Some(more) if more > 0 => write!(f, "\n  … {more} more callers"),
            _ => Ok(()),
        }
    }
}

/// Callers printed by `Display` (the full chain stays in [`Diagnostic`]).
const SHOWN_CALLERS: usize = 16;

impl std::error::Error for VmError {}
