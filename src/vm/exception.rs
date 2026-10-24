//! Exceptions, handlers and completion records (clj-quickjs-11).
//!
//! JavaScript exception control flow lives entirely in task-owned VM state,
//! never on the Rust stack (`docs/design.md` §10). Each task's `VmState` keeps
//! one **handler stack** beside its frames ([`Handler`]); every entry belongs
//! to one frame (its index, the entry frame being 0) and records the operand
//! stack height to restore. An entry is one of:
//!
//! * a **catch handler** (`push_catch`): an exception raised while it is on
//!   the stack unwinds to it: frames above its frame are popped, the operand
//!   stack is cut to its height, the exception value is pushed and execution
//!   continues at its target;
//! * a **finally handler** (`push_finally`): an exception, or a `return` of
//!   its own frame, unwinds to it the same way, but instead of pushing a
//!   value it is replaced by a **pending completion** recording why the
//!   finally block runs, and execution continues at its target (the block);
//! * a **pending completion** ([`Completion`]): the reason a finally block is
//!   running: `Normal` (`pop_finally`, the protected code finished), `Return`
//!   with the saved return value, or `Throw` with the saved [`Exception`].
//!   `end_finally` pops it and resumes that completion: continue after the
//!   block, continue returning (through the next finally handler of the
//!   frame, if any), or re-raise the saved exception.
//!
//! An abrupt completion inside a finally block overrides its pending one, as
//! in JavaScript: unwinding discards every pending completion it passes.
//!
//! The QuickJS analogue (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8:
//! `OP_catch`, `OP_gosub`, `OP_ret`, `OP_throw` and the `exception:` label of
//! `JS_CallInternal`) pushes catch offsets and `gosub` return addresses onto
//! the C interpreter's value stack and unwinds by scanning it. Here handlers
//! are typed entries of task-owned state, the finally block's reason is an
//! explicit record rather than a return address, and frames are popped by the
//! same non-recursive loop as `Return`. No QuickJS code is copied.
//!
//! **Exception values and catchability.** `throw v` raises `v` itself. Some
//! run-time failures are JavaScript errors and are raised as catchable
//! exceptions too: `ReferenceError` (undeclared name, temporal dead zone),
//! `TypeError` (assignment to a constant, calling a non-function) and
//! `StackOverflow` (QuickJS's catchable "stack overflow" InternalError).
//! Their exception value is a fresh, immutable **error object**
//! ([`ErrorObject`], a native heap object, shareable) that carries the
//! failure; it is allocated only when some handler could see it. Every
//! other failure (`UnsupportedOperand`, `Internal`, host, collection,
//! builder, atom and binding-cell contract violations, out of memory, failed
//! or cancelled dependencies) is not a JavaScript exception: it ends the run
//! at once without running catch or finally blocks, as QuickJS's uncatchable
//! errors do. Making dependency failures catchable is clj-quickjs-27.
//!
//! **Diagnostics.** Each raised exception carries the [`VmError`] to report if
//! nothing catches it, built where it was raised: the failing instruction,
//! its source position and the call chain at that moment. A finally block
//! that re-raises a saved exception re-raises it with that original error.
//! An uncaught `throw` of an error object reports the error the object was
//! created for; an uncaught `throw` of any other value reports
//! [`VmErrorKind::Thrown`](crate::VmErrorKind::Thrown) at the `throw`.
//!
//! **Rooting.** Exception values and saved return values of pending
//! completions are traced with the task's frames (`Trace for VmState`), so
//! they stay alive across calls, yields, waits and collections.

use crate::error::VmError;
use crate::heap::gc::Trace;
use crate::heap::{NativePayload, ObjectId};
use crate::value::Value;
use crate::value::share::Shareability;

/// The payload of an error object: the exception value of a catchable
/// run-time error. Immutable and without heap edges.
#[derive(Debug)]
pub(crate) struct ErrorObject {
    /// The failure, as reported if the exception is never caught.
    pub(crate) error: VmError,
}

impl Trace for ErrorObject {
    fn trace(&self, _visit: &mut dyn FnMut(ObjectId)) {}
}

impl NativePayload for ErrorObject {
    fn shareability(&self) -> Shareability {
        Shareability::Persistent
    }
}

/// A raised exception: the JavaScript value and the error reported if no
/// handler catches it.
#[derive(Debug)]
pub(crate) struct Exception {
    pub(crate) value: Value,
    pub(crate) error: Box<VmError>,
}

/// Why a finally block is running.
#[derive(Debug)]
pub(crate) enum Completion {
    /// The protected code (or its catch block) finished normally.
    Normal,
    /// A `return` of the frame, with its value.
    Return(Value),
    /// An exception, re-raised when the block finishes.
    Throw(Exception),
}

/// What a handler-stack entry is. See the module docs.
#[derive(Debug)]
pub(crate) enum HandlerKind {
    Catch { target: u32 },
    Finally { target: u32 },
    Pending(Completion),
}

/// One entry of a task's handler stack.
#[derive(Debug)]
pub(crate) struct Handler {
    /// Index of the frame it belongs to (0: the entry frame).
    pub(crate) frame: usize,
    /// Absolute value-stack height to restore when it is unwound to.
    pub(crate) height: usize,
    pub(crate) kind: HandlerKind,
}

impl Trace for Handler {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match &self.kind {
            HandlerKind::Catch { .. } | HandlerKind::Finally { .. } => {}
            HandlerKind::Pending(Completion::Normal) => {}
            HandlerKind::Pending(Completion::Return(v)) => v.trace(visit),
            HandlerKind::Pending(Completion::Throw(e)) => e.value.trace(visit),
        }
    }
}

/// A thrown value for a diagnostic message, written as JavaScript would.
pub(crate) fn describe_thrown(value: Value) -> String {
    match value {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) if n.is_infinite() => {
            if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
        }
        Value::Number(n) => n.to_string(),
        Value::Object(_) => "an object".to_string(),
    }
}
