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
//!   with the saved return value, `Throw` with the saved [`Exception`], or
//!   `Jump` with a resume point (`call_finally`, below). `end_finally` pops
//!   it and resumes that completion: continue after the block, continue
//!   returning (through the next finally handler of the frame, if any),
//!   re-raise the saved exception, or continue at the resume point.
//!
//! An abrupt completion inside a finally block overrides its pending one, as
//! in JavaScript: unwinding discards every pending completion it passes.
//!
//! **`break` and `continue` through finally blocks** (clj-quickjs-79). The
//! compiler lowers a jump that leaves `try` statements to one instruction
//! per construct left, innermost first (`crate::frontend` lowering,
//! `Lowerer::jump`), then the `jump` to the loop: `pop_catch` for a `try`
//! block with a catch handler; `call_finally` for a `try` block or catch
//! body with a finally handler, which turns that handler into a pending
//! `Jump` completion whose resume point is the next instruction and runs the
//! finally block, whose `end_finally` comes back there to leave the next
//! construct; and `drop_completion` for a finally block itself, which pops
//! its pending completion unresumed (the jump overrides a pending return,
//! exception or jump). Like QuickJS's `emit_break` (`quickjs.c`, revision
//! 7d1c13c5d8f8), which emits a `gosub` to each finally block on the way
//! and drops the return addresses of finally blocks it leaves. A finally
//! block that completes abruptly discards the pending jump like any other
//! pending completion. The resume point lives only in the handler stack, so
//! a task may yield, wait, migrate or be collected inside a finally block
//! that a jump runs and continue the jump afterwards. A `drop_completion`
//! that would discard a pending cancellation ends the cleanup instead, as a
//! `return` does (`crate::task`, "Cancellation and finally blocks").
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
//! `TypeError` (assignment to a constant, calling a non-function),
//! `RangeError` (invalid array length, clj-quickjs-154) and
//! `StackOverflow` (QuickJS's catchable "stack overflow" InternalError).
//! Their exception value is a fresh, immutable **error object**
//! ([`ErrorObject`], a native heap object, shareable) that carries the
//! failure; it is allocated only when some handler could see it. A `need`
//! whose dependency failed or was cancelled raises one too (see
//! "Dependency failures" below). Every other failure (`UnsupportedOperand`,
//! `Internal`, host, collection, builder, atom and binding-cell contract
//! violations, out of memory) is not a JavaScript exception: it ends the run
//! at once without running catch or finally blocks, as QuickJS's uncatchable
//! errors do.
//!
//! **Dependency failures** (clj-quickjs-27, `docs/design.md` §§7, 10). A
//! `need` that reads a cell whose outcome is `Failed(e)` or `Cancelled`
//! raises, at that `need`, an error object for
//! [`VmErrorKind::DependencyFailed`](crate::VmErrorKind::DependencyFailed)
//! or [`DependencyCancelled`](crate::VmErrorKind::DependencyCancelled): its
//! error names the key (world, definition, result kind), the `need`'s
//! instruction and source position and the call chain at that moment, and
//! its **cause** is the producer's published error value `e` itself (by
//! identity; `undefined` for a cancelled cell). This happens whether the
//! cell was already complete or completed while the task waited: a resumed
//! `need` raises exactly where it suspended. It is an ordinary catchable
//! exception from there on: catch blocks see the error object, finally
//! blocks run, a rethrow keeps the original error, and if nothing catches it
//! the run fails with that error. The published value stays in the cell; the
//! embedder reads the failure and cause of a caught error object with
//! [`Rooted::error`](crate::Rooted::error) and
//! [`Rooted::error_cause`](crate::Rooted::error_cause).
//!
//! **Waiting is not unwinding.** A `need` that finds its cell pending
//! suspends the task; it raises nothing, and no catch or finally block runs
//! because of it. The handler stack, pending completions (with their saved
//! return values and exceptions), frames, operands and locals stay in the
//! task while it waits, may be collected around and resumed on any thread,
//! and execution continues at the `need`, whether it was in a protected
//! block, a catch block, a finally block (whose pending completion then
//! resumes as usual) or a function called from any of them. Consumer
//! cancellation is not an exception either: see `crate::task`, "Cancellation
//! and finally blocks".
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
use crate::heap::{Heap, HeapObject, NativePayload, ObjectId};
use crate::value::share::Shareability;
use crate::value::{JsString, Value};

/// The payload of an error object: the exception value of a catchable
/// run-time error. Immutable; its only heap edge is its cause, so it is
/// shareable exactly when its cause is.
#[derive(Debug)]
pub(crate) struct ErrorObject {
    /// Bounded immutable snapshot captured before exception unwinding.
    pub(crate) stack: JsString,
    /// The failure, as reported if the exception is never caught.
    pub(crate) error: VmError,
    /// What caused it: for a failed dependency, the error value its producer
    /// published (the cell's own value, by identity); otherwise `undefined`.
    pub(crate) cause: Value,
}

impl Trace for ErrorObject {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.cause.trace(visit);
    }
}

/// The failure and cause of `value` if it is an error object.
pub(crate) fn error_object(heap: &Heap, value: &Value) -> Option<(VmError, Value)> {
    let Value::Object(id) = *value else {
        return None;
    };
    heap.read(id, |object| match object {
        HeapObject::Native(payload) => {
            let any: &dyn std::any::Any = &**payload;
            any.downcast_ref::<ErrorObject>()
                .map(|e| (e.error.clone(), e.cause.clone()))
        }
        _ => None,
    })
    .flatten()
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
    /// The task's consumer cancellation (`crate::task`, "Cancellation and
    /// finally blocks"): when the block finishes, cancellation unwinding
    /// continues to the next finally handler. Nothing may override it: an
    /// exception or `return` that would discard it ends the cleanup.
    Cancel,
    /// A `break` or `continue` leaving the protected code (`CallFinally`):
    /// when the block finishes, execution continues at this instruction
    /// index of the frame, the instruction after the `CallFinally`, with the
    /// operand stack at the handler's height.
    Jump(u32),
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
            HandlerKind::Pending(Completion::Normal | Completion::Cancel | Completion::Jump(_)) => {
            }
            HandlerKind::Pending(Completion::Return(v)) => v.trace(visit),
            HandlerKind::Pending(Completion::Throw(e)) => e.value.trace(visit),
        }
    }
}

/// A thrown value for a diagnostic message, written as JavaScript would.
pub(crate) fn describe_thrown(value: &Value) -> String {
    match *value {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) if n.is_infinite() => {
            if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
        }
        Value::Number(n) => n.to_string(),
        Value::Object(_) => "an object".to_string(),
        // A thrown string is reported by its text, as JavaScript ToString
        // shows it (a lone surrogate as U+FFFD).
        Value::String(ref s) => s.to_string_lossy(),
    }
}
