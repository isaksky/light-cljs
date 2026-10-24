//! The interpreter loop.
//!
//! [`execute`] runs a task's [`VmState`] over shared, validated [`Code`]
//! until the task returns, fails, or exhausts its execution budget. All state
//! it mutates is the task-owned `VmState`; it keeps no Rust references into
//! the value stack or the frame stack across instructions, and nothing is
//! held on the Rust stack that a later run would need (`docs/design.md`
//! §§3, 10). The only thing it keeps between instructions is an `Arc` of the
//! running frame's immutable code, refreshed whenever `Call` or `Return`
//! changes the frame. The instruction position is advanced before each
//! instruction executes, so the running frame's `pc` always names the next
//! instruction to run, and a caller's saved `pc` names the instruction after
//! its `Call`.
//!
//! **Calls.** `Call` and `Return` push and pop task-owned frames
//! (`crate::vm::frames`); the loop continues in the new frame instead of
//! recursing, so call depth never consumes Rust stack.
//!
//! The loop never panics on its inputs: validation rules out bad indices and
//! stack underflow, and the loop still re-checks them, failing the task with
//! [`VmErrorKind::Internal`] if an invariant is ever broken.
//!
//! **Budget and yield.** The budget counts instructions (a `Call` or
//! `Return` is one). It is checked only *between* instructions: when it is
//! exhausted the loop returns [`Exit::Yield`] with the running frame's `pc`
//! naming the next instruction (possibly a branch target just jumped to, or
//! the first instruction of a function just called) and every frame's
//! operands and locals as their next instructions expect them. Every
//! instruction, including `HostEffect`, runs to completion within one run, so
//! a later run continues exactly where this one stopped and never replays an
//! executed instruction or its effect.
//!
//! **Safepoints.** At run entry and before each instruction [`polls`] selects
//! (backward branches, `Call`, `Return`, and every instruction that may
//! allocate) the loop calls `Heap::safepoint` with the task's `VmState`: if
//! a collection is requested on another thread, the run reports its frames
//! and parks until the collection has run (`crate::heap::gc`). Polls happen
//! only between instructions, so a value an instruction holds in a Rust
//! local (an allocation not yet pushed, an atom value not yet rooted) never
//! meets a collection, and a straight-line stretch without poll points is
//! bounded by the code length.
//!
//! **Exceptions.** A `Throw`, an `EndFinally` re-raising a saved exception,
//! or a failing instruction whose error is a catchable JavaScript error
//! raises an exception: [`VmState::raise`] unwinds the task-owned frames and
//! handler stack to the innermost handler and the loop continues there, in
//! the same run and budget. Raising never recurses and never keeps a Rust
//! frame alive. A caught exception is a poll point (after unwinding, when
//! every value is in task-owned state). Other failures, and exceptions no
//! handler catches, end the run with [`Exit::Fail`] (`crate::vm::exception`).
//! While a cancelled task runs its finally blocks, the loop leaves with
//! [`Exit::CleanupEnded`] when the last one finishes or when an exception or
//! `return` would override the cancellation (`crate::task`, "Cancellation
//! and finally blocks").
//!
//! **Dependency waits.** `Need` decodes its operands and leaves the loop with
//! [`Exit::Need`] (the instruction position already past it, the remaining
//! budget carried along): the dependency protocol, suspension and the
//! injection of the result belong to the run guard (`crate::task`, "Dependency
//! waits in the VM"), which either pushes the value and calls [`execute`]
//! again or ends the run. No Rust frame of this loop survives a wait.
//!
//! **Resumable natives.** `Reduce` and `NeedAll` push a task-owned native
//! frame (`crate::vm::native`) instead of running to completion. While one is
//! running (no call frame above it) the loop takes its next *step* in place
//! of an instruction: a trampoline. A step that calls a callback enters a VM
//! frame like `Call`, whose `Return` hands the result back to the native; a
//! step that demands a dependency leaves with [`Exit::Need`] exactly like a
//! `Need` instruction. Each step counts against the budget and is a poll
//! point, so a yield, a suspension or a collection may fall between any two
//! steps, and the next run continues with the pending step.

use std::sync::Arc;

use crate::collections::{BuilderError, CollectionError, object_name};
use crate::error::{VmError, VmErrorKind};
use crate::heap::{Heap, HeapObject};
use crate::host::{Host, HostCall, call_leaf};
use crate::ids::{DependencyId, ResultKind, TaskId};
use crate::value::Value;
use crate::vm::exception::{
    Completion, ErrorObject, Exception, HandlerKind, describe_thrown, error_object,
};
use crate::vm::executable::Code;
use crate::vm::frames::{Raised, Returned, VmState};
use crate::vm::function::{BindingCell, FunctionObject};
use crate::vm::instr::{Instr, Opcode, ThrowKind};
use crate::vm::intrinsics;
use crate::vm::native;

/// How a run of the interpreter loop ended.
#[derive(Debug)]
pub(crate) enum Exit {
    Return(Value),
    Fail(VmError),
    /// The budget ran out; the task can be run again.
    Yield,
    /// A `Need` instruction at `pc` asks for result `kind` of `dependency`;
    /// its operands are popped and the running frame's `pc` is past it. Or a
    /// running native frame started by the instruction at `pc` demands it.
    /// The caller pushes the result and continues with `budget`, or suspends.
    Need {
        dependency: DependencyId,
        kind: ResultKind,
        pc: usize,
        budget: Option<u64>,
    },
    /// Cleanup for the task's consumer cancellation is over: its last
    /// finally block finished, or an exception or `return` would have
    /// overridden the cancellation (`crate::task`, "Cancellation and finally
    /// blocks").
    CleanupEnded,
}

/// A failure before diagnostic context is attached.
pub(crate) enum Fault {
    Unsupported(String),
    Internal(&'static str),
    /// The host reported an error from a leaf effect.
    Host(String),
    /// The heap could not allocate.
    OutOfMemory(&'static str),
    /// A collection intrinsic rejected its operands.
    Collection(CollectionError),
    /// A builder operation was rejected by the builder lifecycle.
    Builder(BuilderError),
    /// A `ThrowError` instruction ran.
    Throw(ThrowKind),
    /// A `Call` callee that is not a function object; names what it was.
    NotCallable(&'static str),
    /// A `Call` would exceed the task's `VmLimits`.
    StackOverflow(String),
    /// A binding-cell instruction on another task's cell.
    ForeignBinding {
        owner: TaskId,
        caller: TaskId,
    },
    /// An atom intrinsic rejected its operands; the message names why.
    Atom(crate::AtomErrorKind, String),
    /// A `Throw` instruction raised this value.
    Thrown(Value),
    /// An `EndFinally` re-raises the exception its finally block saved.
    Rethrow(Exception),
}

/// What the loop does after an instruction (or a native step).
pub(crate) enum Flow {
    Next,
    /// `Call` or `Return` changed the running frame.
    FrameChanged,
    /// The entry frame returned: the task completes with this value.
    Complete(Value),
    /// A `Need` popped its operands: the run guard reads the dependency.
    Need(DependencyId, ResultKind),
    /// The cancellation cleanup is over ([`Exit::CleanupEnded`]).
    CleanupEnded,
}

/// Allocate a function object for `code` with its captured values. Known
/// shareable at allocation when every captured value is (its code constants
/// are primitives): see `crate::vm::function`.
fn new_function(heap: &Heap, code: Arc<Code>, captures: Box<[Value]>) -> Result<Value, Fault> {
    let object = HeapObject::Function(FunctionObject { code, captures });
    heap.alloc_persistent(object, None)
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

fn numbers(op: Opcode, a: Value, b: Value) -> Result<(f64, f64), Fault> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok((x, y)),
        _ => Err(Fault::Unsupported(format!(
            "{} supports number operands only, got {} and {}",
            op.name(),
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// JavaScript ToNumber for primitives. Objects need ToPrimitive, which is
/// deferred, so they are rejected explicitly.
fn to_number(v: Value) -> Result<f64, Fault> {
    match v {
        Value::Undefined => Ok(f64::NAN),
        Value::Null => Ok(0.0),
        Value::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
        Value::Number(x) => Ok(x),
        Value::Object(_) => Err(Fault::Unsupported(
            "converting an object to a number (ToPrimitive) is not supported".to_string(),
        )),
    }
}

/// JavaScript `==` (IsLooselyEqual) for the supported value types. Strings
/// do not exist yet; an object compared with a number or boolean would need
/// ToPrimitive, which is deferred, so that case is rejected explicitly.
fn loose_equals(a: Value, b: Value) -> Result<bool, Fault> {
    use Value::*;
    Ok(match (a, b) {
        (Undefined | Null, Undefined | Null) => true,
        (Undefined | Null, _) | (_, Undefined | Null) => false,
        (Object(_), Object(_)) | (Number(_), Number(_)) | (Bool(_), Bool(_)) => a.strict_equals(b),
        (Number(_), Bool(_)) | (Bool(_), Number(_)) => to_number(a)? == to_number(b)?,
        (Object(_), _) | (_, Object(_)) => {
            return Err(Fault::Unsupported(format!(
                "loose_eq between {} and {} needs ToPrimitive, which is not supported",
                a.type_name(),
                b.type_name()
            )));
        }
    })
}

/// Failure message of a `ThrowError`, naming the binding when the code has
/// source text.
fn throw_message(code: &Code, pc: usize, kind: ThrowKind) -> String {
    let name = code.location(pc).and_then(|at| code.source_text(at));
    match (kind, name) {
        (ThrowKind::Unresolvable, Some(n)) => format!("{n} is not defined"),
        (ThrowKind::Unresolvable, None) => "reference to an undeclared name".to_string(),
        (ThrowKind::Uninitialized, Some(n)) => {
            format!("cannot access '{n}' before initialization")
        }
        (ThrowKind::Uninitialized, None) => "binding accessed before initialization".to_string(),
        (ThrowKind::ConstAssignment, Some(n)) => {
            format!("assignment to constant variable '{n}'")
        }
        (ThrowKind::ConstAssignment, None) => "assignment to a constant variable".to_string(),
    }
}

/// The callee of a `Call` (or of a native's callback): the code of a function
/// object, or the name of what was found instead.
pub(crate) fn callee_code(heap: &Heap, callee: Value) -> Result<Arc<Code>, Fault> {
    let Value::Object(id) = callee else {
        return Err(Fault::NotCallable(callee.type_name()));
    };
    heap.read(id, |object| match object {
        HeapObject::Function(f) => Ok(Arc::clone(&f.code)),
        other => Err(Fault::NotCallable(object_name(other))),
    })
    // A frame keeps its operands alive; a stale id is reported, not trusted.
    .unwrap_or(Err(Fault::Internal("callee was reclaimed")))
}

/// The top `n` operands of the running frame, oldest first.
fn top_operands(state: &VmState, n: usize) -> Result<&[Value], Fault> {
    let operands = state.operands();
    operands
        .len()
        .checked_sub(n)
        .map(|at| &operands[at..])
        .ok_or(Fault::Internal("operand stack underflow"))
}

/// Run `f` on the binding of `cell` for the running task: fails if `cell` is
/// not a binding cell, or belongs to another task.
fn with_cell<R>(
    heap: &Heap,
    cell: Value,
    task: TaskId,
    f: impl FnOnce(&mut Option<Value>) -> Result<R, Fault>,
) -> Result<R, Fault> {
    let not_cell =
        |found: &str| Fault::Unsupported(format!("expected a binding cell, got {found}"));
    let Value::Object(id) = cell else {
        return Err(not_cell(cell.type_name()));
    };
    heap.with_object_mut(id, |object| match object {
        HeapObject::Cell(c) if c.owner == task => f(&mut c.value),
        HeapObject::Cell(c) => Err(Fault::ForeignBinding {
            owner: c.owner,
            caller: task,
        }),
        other => Err(not_cell(object_name(other))),
    })
    // A frame keeps its operands alive; a stale id is reported, not trusted.
    .unwrap_or(Err(Fault::Internal("binding cell was reclaimed")))
}

/// The operands of `Need`: a definition id (an integer number in
/// `0..=2^53-1`, exactly representable) and a result kind (an integer in
/// `0..=u32::MAX`). Anything else is rejected, never rounded.
fn need_operands(dependency: Value, kind: Value) -> Result<(DependencyId, ResultKind), Fault> {
    match (dependency_id(dependency), result_kind(kind)) {
        (Some(d), Some(k)) => Ok((d, k)),
        _ => Err(Fault::Unsupported(format!(
            "need takes a definition id and a result kind as non-negative integer numbers, got {} and {}",
            describe(dependency),
            describe(kind)
        ))),
    }
}

/// `v` as an integer number in `0..=max`, never rounded.
fn integer(v: Value, max: f64) -> Option<f64> {
    match v {
        Value::Number(n) if n.fract() == 0.0 && (0.0..=max).contains(&n) => Some(n),
        _ => None,
    }
}

/// A definition id operand: an integer number in `0..=2^53-1`.
pub(crate) fn dependency_id(v: Value) -> Option<DependencyId> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    integer(v, MAX_SAFE_INTEGER).map(|d| DependencyId::new(d as u64))
}

/// A result kind operand: an integer number in `0..=u32::MAX`.
pub(crate) fn result_kind(v: Value) -> Option<ResultKind> {
    integer(v, f64::from(u32::MAX)).map(|k| ResultKind::new(k as u32))
}

/// A value for a diagnostic: numbers as themselves, other values by type.
pub(crate) fn describe(v: Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        other => other.type_name().to_string(),
    }
}

/// Return `v` from the running frame, through its finally blocks.
fn return_flow(state: &mut VmState, v: Value) -> Result<Flow, Fault> {
    Ok(match state.return_value(v)? {
        Returned::Complete(result) => Flow::Complete(result),
        Returned::Left | Returned::Finally => Flow::FrameChanged,
        Returned::Cancelled => Flow::CleanupEnded,
    })
}

/// The error kind and message of a fault that is not an exception value.
fn fault_error(fault: Fault, code: &Code, pc: usize) -> (VmErrorKind, String) {
    match fault {
        Fault::Unsupported(m) => (VmErrorKind::UnsupportedOperand, m),
        Fault::Internal(m) => (VmErrorKind::Internal, m.to_string()),
        Fault::Host(m) => (VmErrorKind::HostEffect, m),
        Fault::OutOfMemory(m) => (VmErrorKind::OutOfMemory, m.to_string()),
        Fault::Collection(e) => (VmErrorKind::Collection(e), e.to_string()),
        Fault::Builder(e) => (VmErrorKind::Builder(e), e.to_string()),
        Fault::NotCallable(what) => (VmErrorKind::TypeError, format!("{what} is not a function")),
        Fault::StackOverflow(m) => (VmErrorKind::StackOverflow, m),
        Fault::Atom(kind, m) => (VmErrorKind::Atom(kind), m),
        Fault::ForeignBinding { owner, caller } => (
            VmErrorKind::ForeignBinding { owner, caller },
            format!(
                "binding is owned by {owner}, not {caller}: a closure over mutable bindings is task-owned"
            ),
        ),
        Fault::Throw(kind) => {
            let error_kind = match kind {
                ThrowKind::Unresolvable | ThrowKind::Uninitialized => VmErrorKind::ReferenceError,
                ThrowKind::ConstAssignment => VmErrorKind::TypeError,
            };
            (error_kind, throw_message(code, pc, kind))
        }
        // Exception values are raised by the caller, never described here.
        Fault::Thrown(_) | Fault::Rethrow(_) => (
            VmErrorKind::Internal,
            "an exception reached the failure path".to_string(),
        ),
    }
}

/// Run-time failures that are JavaScript errors, raised as catchable
/// exceptions (`crate::vm::exception`). Exhaustive, so a new kind must decide.
fn catchable(kind: VmErrorKind) -> bool {
    match kind {
        VmErrorKind::ReferenceError
        | VmErrorKind::TypeError
        | VmErrorKind::StackOverflow
        | VmErrorKind::DependencyFailed(_)
        | VmErrorKind::DependencyCancelled(_) => true,
        VmErrorKind::UnsupportedOperand
        | VmErrorKind::Internal
        | VmErrorKind::HostEffect
        | VmErrorKind::OutOfMemory
        | VmErrorKind::Collection(_)
        | VmErrorKind::Builder(_)
        | VmErrorKind::ForeignBinding { .. }
        | VmErrorKind::Atom(_)
        | VmErrorKind::Thrown => false,
    }
}

/// The error an exception with value `v`, raised by `Throw` at `pc`, reports
/// if nothing catches it: an error object's own error, otherwise `Thrown`.
fn thrown_error(state: &VmState, heap: &Heap, v: Value, pc: usize) -> VmError {
    match error_object(heap, v) {
        Some((original, _)) => original,
        None => {
            let message = format!("uncaught exception: {}", describe_thrown(v));
            state.failure(VmErrorKind::Thrown, message, pc)
        }
    }
}

/// Raise `exception` in task-owned state (`VmState::raise`): `None` when a
/// handler took it and execution continues at the handler's target;
/// otherwise how the run ends: failing with the exception's error, or
/// ending a cancellation cleanup the exception would override.
pub(crate) fn raise(state: &mut VmState, exception: Exception) -> Option<Exit> {
    match state.raise(exception) {
        Raised::Caught => None,
        Raised::Uncaught(exception) => Some(Exit::Fail(*exception.error)),
        Raised::Cancelled => Some(Exit::CleanupEnded),
        Raised::Broken(message) => {
            let at = state.top().pc;
            let error = state.failure(VmErrorKind::Internal, message.to_string(), at);
            Some(Exit::Fail(error))
        }
    }
}

/// Raise the run-time failure `error` of the running frame as a JavaScript
/// exception if it is catchable and some handler could see it: its value is
/// a fresh error object with `cause` (`crate::vm::exception`). Otherwise, or
/// if the heap is exhausted, the run fails with the error; see [`raise`].
///
/// `cause` must stay alive without this call rooting it (a published
/// dependency result is held by its cell); the caller is a registered
/// mutator with no safepoint between here and the object being stored in
/// task-owned state.
pub(crate) fn raise_failure(
    state: &mut VmState,
    heap: &Heap,
    error: VmError,
    cause: Value,
) -> Option<Exit> {
    if !catchable(error.kind) || !state.has_handlers() {
        return Some(Exit::Fail(error));
    }
    let pc = error.diagnostic.instruction;
    // The error object is stored by `raise` (pushed for a catch block or
    // kept in a pending completion) before any poll.
    let object = HeapObject::Native(Box::new(ErrorObject {
        error: error.clone(),
        cause,
    }));
    let Some(id) = heap.alloc(object) else {
        let message = "heap object table exhausted".to_string();
        return Some(Exit::Fail(state.failure(
            VmErrorKind::OutOfMemory,
            message,
            pc,
        )));
    };
    raise(
        state,
        Exception {
            value: Value::Object(id),
            error: Box::new(error),
        },
    )
}

/// Execute one instruction of `code`, the running frame's code.
fn step(
    state: &mut VmState,
    code: &Code,
    heap: &Heap,
    host: &mut dyn Host,
    instr: Instr,
) -> Result<Flow, Fault> {
    if native::is_resumable(instr) {
        // A resumable native: its frame is pushed and its first step runs
        // next (`crate::vm::native`). `pc` was advanced past the instruction.
        let pc = state.top().pc.saturating_sub(1);
        native::start(state, heap, instr, pc)?;
        return Ok(Flow::Next);
    }
    // Everything below, leaf natives included, runs to completion here.
    match instr {
        Instr::Const(i) => {
            let v = code
                .constants
                .get(usize::from(i))
                .copied()
                .ok_or(Fault::Internal("constant index out of range"))?;
            state.push(v);
        }
        Instr::Undefined => state.push(Value::Undefined),
        Instr::GetLocal(i) => {
            let v = *state.local(i)?;
            state.push(v);
        }
        Instr::SetLocal(i) => {
            let v = state.pop()?;
            *state.local(i)? = v;
        }
        Instr::Pop => {
            state.pop()?;
        }
        Instr::Dup => {
            let v = *state
                .operands()
                .last()
                .ok_or(Fault::Internal("operand stack underflow"))?;
            state.push(v);
        }
        Instr::Add
        | Instr::Sub
        | Instr::Mul
        | Instr::Lt
        | Instr::Div
        | Instr::Rem
        | Instr::Gt
        | Instr::Le
        | Instr::Ge => {
            let b = state.pop()?;
            let a = state.pop()?;
            let (x, y) = numbers(instr.opcode(), a, b)?;
            state.push(match instr {
                Instr::Add => Value::Number(x + y),
                Instr::Sub => Value::Number(x - y),
                Instr::Mul => Value::Number(x * y),
                Instr::Div => Value::Number(x / y),
                // Rust's `%` on f64 is C `fmod`: the result has the sign of
                // the dividend, as JavaScript's `%` requires.
                Instr::Rem => Value::Number(x % y),
                Instr::Lt => Value::Bool(x < y),
                Instr::Gt => Value::Bool(x > y),
                Instr::Le => Value::Bool(x <= y),
                _ => Value::Bool(x >= y),
            });
        }
        Instr::StrictEq => {
            let b = state.pop()?;
            let a = state.pop()?;
            state.push(Value::Bool(a.strict_equals(b)));
        }
        Instr::Not => {
            let v = state.pop()?;
            state.push(Value::Bool(!v.truthy()));
        }
        Instr::Neg => match state.pop()? {
            Value::Number(x) => state.push(Value::Number(-x)),
            v => {
                return Err(Fault::Unsupported(format!(
                    "neg supports number operands only, got {}",
                    v.type_name()
                )));
            }
        },
        Instr::ToNumber => {
            let v = state.pop()?;
            state.push(Value::Number(to_number(v)?));
        }
        Instr::ToNumberPair => {
            let b = state.pop()?;
            let a = state.pop()?;
            let x = to_number(a)?;
            let y = to_number(b)?;
            state.push(Value::Number(x));
            state.push(Value::Number(y));
        }
        Instr::LooseEq => {
            let b = state.pop()?;
            let a = state.pop()?;
            state.push(Value::Bool(loose_equals(a, b)?));
        }
        Instr::ThrowError(kind) => return Err(Fault::Throw(kind)),
        Instr::Jump(target) => state.top_mut().pc = target as usize,
        Instr::JumpIfFalse(target) => {
            if !state.pop()?.truthy() {
                state.top_mut().pc = target as usize;
            }
        }
        Instr::Return => {
            let v = state.pop()?;
            return return_flow(state, v);
        }
        Instr::Throw => return Err(Fault::Thrown(state.pop()?)),
        Instr::PushCatch(target) => state.push_handler(HandlerKind::Catch { target })?,
        Instr::PushFinally(target) => state.push_handler(HandlerKind::Finally { target })?,
        Instr::PopCatch => state.pop_catch()?,
        Instr::PopFinally => state.pop_finally()?,
        Instr::EndFinally => match state.end_finally()? {
            Completion::Normal => {}
            Completion::Return(v) => return return_flow(state, v),
            Completion::Throw(exception) => return Err(Fault::Rethrow(exception)),
            // Cancellation unwinding goes on to the next finally block.
            Completion::Cancel if state.unwind_cancel() => return Ok(Flow::FrameChanged),
            Completion::Cancel => return Ok(Flow::CleanupEnded),
        },
        Instr::NewObject => {
            // The new object is pushed in the same instruction; no collection
            // can run in between because this run holds a mutator guard.
            let id = heap
                .alloc(HeapObject::Plain)
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            state.push(Value::Object(id));
        }
        Instr::HostEffect(op) => {
            let v = state.pop()?;
            let arg = v.to_primitive().ok_or_else(|| {
                Fault::Unsupported(format!(
                    "host_effect takes a primitive argument, got {}",
                    v.type_name()
                ))
            })?;
            let call = HostCall {
                task: state.diag.task,
                op,
                arg,
            };
            // No reference into the task's VM storage is live across the
            // host call.
            let result = call_leaf(host, call).map_err(Fault::Host)?;
            state.push(Value::from(result));
        }
        Instr::NewVector(_)
        | Instr::NewMap(_)
        | Instr::Count
        | Instr::Nth
        | Instr::Get
        | Instr::GetOr
        | Instr::Assoc
        | Instr::Dissoc
        | Instr::Conj
        | Instr::PopLast
        | Instr::NewVectorBuilder
        | Instr::NewMapBuilder
        | Instr::BuilderConj
        | Instr::BuilderAssoc
        | Instr::Freeze
        | Instr::AtomNew
        | Instr::AtomLoad
        | Instr::AtomCas => intrinsics::step(state, heap, instr)?,
        Instr::Function(i, n) => {
            let child = code
                .functions
                .get(usize::from(i))
                .cloned()
                .ok_or(Fault::Internal("function index out of range"))?;
            if child.captures != n {
                return Err(Fault::Internal("capture count differs from the child code"));
            }
            // The captured values stay on the operand stack (rooted) until
            // the closure holding them replaces them, in the same instruction
            // and under this run's mutator guard.
            let n = usize::from(n);
            let captures: Box<[Value]> = top_operands(state, n)?.into();
            let f = new_function(heap, child, captures)?;
            state.replace(n, f)?;
        }
        Instr::GetCapture(i) => {
            let captured = state.top().callee.and_then(|id| {
                heap.read(id, |object| match object {
                    HeapObject::Function(f) => f.captures.get(usize::from(i)).copied(),
                    _ => None,
                })
                .flatten()
            });
            // Validation bounds `i` by the code's declared captures, which
            // every closure over the code carries; only code with captures
            // run directly as a task entry (no closure) gets here.
            let v = captured.ok_or(Fault::Internal(
                "captured value read in a frame that is not a closure over this code",
            ))?;
            state.push(v);
        }
        Instr::NewCell => {
            let cell = BindingCell {
                owner: state.diag.task,
                value: None,
            };
            // Pushed in the same instruction, under this run's mutator guard.
            let id = heap
                .alloc(HeapObject::Cell(cell))
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            state.push(Value::Object(id));
        }
        Instr::CellInit | Instr::CellSet => {
            let &[cell, v] = top_operands(state, 2)? else {
                return Err(Fault::Internal("operand stack underflow"));
            };
            with_cell(heap, cell, state.diag.task, |binding| {
                if instr == Instr::CellSet && binding.is_none() {
                    return Err(Fault::Throw(ThrowKind::Uninitialized));
                }
                *binding = Some(v);
                Ok(())
            })?;
            state.pop()?;
            state.pop()?;
        }
        Instr::CellGet => {
            let &[cell] = top_operands(state, 1)? else {
                return Err(Fault::Internal("operand stack underflow"));
            };
            let v = with_cell(heap, cell, state.diag.task, |binding| {
                binding.ok_or(Fault::Throw(ThrowKind::Uninitialized))
            })?;
            state.replace(1, v)?;
        }
        Instr::CurrentFunction => {
            let f = match state.top().callee {
                Some(id) => Value::Object(id),
                None => {
                    // The entry frame has no function object until asked for
                    // one; it is kept in the frame (a root) from now on.
                    let f = new_function(heap, Arc::clone(&state.top().code), Box::new([]))?;
                    if let Value::Object(id) = f {
                        state.top_mut().callee = Some(id);
                    }
                    f
                }
            };
            state.push(f);
        }
        Instr::Reduce | Instr::NeedAll => {
            return Err(Fault::Internal("resumable native on the leaf path"));
        }
        Instr::Need => {
            let kind = state.pop()?;
            let dependency = state.pop()?;
            let (dependency, kind) = need_operands(dependency, kind)?;
            return Ok(Flow::Need(dependency, kind));
        }
        Instr::Call(argc) => {
            let argc = usize::from(argc);
            let operands = state.operands();
            let callee = operands
                .len()
                .checked_sub(argc + 1)
                .map(|i| operands[i])
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let callee_code = callee_code(heap, callee)?;
            let Value::Object(id) = callee else {
                return Err(Fault::Internal("callee is not an object"));
            };
            state.enter(callee_code, id, argc)?;
            return Ok(Flow::FrameChanged);
        }
    }
    Ok(Flow::Next)
}

/// True when the loop polls for a requested collection before running
/// `instr` at `pc` (the VM poll points of `crate::heap::gc`): backward
/// branches (every loop), calls and returns, and every instruction that may
/// allocate. Each is an instruction boundary, where every value the run
/// holds is in its frames. Exhaustive, so a new instruction must decide.
fn polls(instr: Instr, pc: usize) -> bool {
    match instr {
        Instr::Jump(target) | Instr::JumpIfFalse(target) => target as usize <= pc,
        Instr::Call(_) | Instr::Return => true,
        Instr::NewObject
        | Instr::NewVector(_)
        | Instr::NewMap(_)
        | Instr::Assoc
        | Instr::Dissoc
        | Instr::Conj
        | Instr::PopLast
        | Instr::NewVectorBuilder
        | Instr::NewMapBuilder
        | Instr::BuilderConj
        | Instr::BuilderAssoc
        | Instr::Freeze
        | Instr::Function(..)
        | Instr::CurrentFunction
        | Instr::NewCell
        | Instr::AtomNew
        | Instr::AtomLoad
        | Instr::AtomCas => true,
        Instr::Const(_)
        | Instr::Undefined
        | Instr::GetLocal(_)
        | Instr::SetLocal(_)
        | Instr::Pop
        | Instr::Dup
        | Instr::Add
        | Instr::Sub
        | Instr::Mul
        | Instr::Lt
        | Instr::StrictEq
        | Instr::Not
        | Instr::HostEffect(_)
        | Instr::Count
        | Instr::Nth
        | Instr::Get
        | Instr::GetOr
        | Instr::Div
        | Instr::Rem
        | Instr::Neg
        | Instr::ToNumber
        | Instr::ToNumberPair
        | Instr::Gt
        | Instr::Le
        | Instr::Ge
        | Instr::LooseEq
        | Instr::ThrowError(_)
        | Instr::GetCapture(_)
        | Instr::CellInit
        | Instr::CellGet
        | Instr::CellSet
        // A caught exception polls after unwinding (see `execute`).
        | Instr::Throw
        | Instr::PushCatch(_)
        | Instr::PushFinally(_)
        | Instr::PopCatch
        | Instr::PopFinally
        | Instr::EndFinally
        // Allocates nothing; each step of the native it starts polls.
        | Instr::Reduce
        | Instr::NeedAll
        // Leaves the loop; continuing after it re-enters `execute`, which
        // polls at run entry.
        | Instr::Need => false,
    }
}

/// Run until the task returns or fails, or until `budget` instructions have
/// executed (`None`: unlimited).
pub(crate) fn execute(
    state: &mut VmState,
    heap: &Heap,
    host: &mut dyn Host,
    mut budget: Option<u64>,
) -> Exit {
    let mut code = Arc::clone(&state.top().code);
    // Run entry is a poll point.
    heap.safepoint(state);
    loop {
        if let Some(left) = &mut budget {
            if *left == 0 {
                return Exit::Yield;
            }
            *left -= 1;
        }
        // A running native frame (no call frame above it) takes the next
        // step instead of the running frame's next instruction; `pc` is then
        // its starting instruction, which a failure or a need reports.
        let (pc, result) = match native::running(state) {
            Some(pc) => {
                // Every step is a poll point: the native's values are in its
                // frame.
                heap.safepoint(state);
                (pc, native::step(state, heap))
            }
            None => {
                let pc = state.top().pc;
                let instr = code.instrs.get(pc).copied();
                if instr.is_some_and(|i| polls(i, pc)) {
                    heap.safepoint(state);
                }
                let result = match instr {
                    Some(instr) => {
                        state.top_mut().pc = pc + 1;
                        step(state, &code, heap, host, instr)
                    }
                    None => Err(Fault::Internal("instruction position out of range")),
                };
                (pc, result)
            }
        };
        let raised = match result {
            Ok(Flow::Next) => continue,
            Ok(Flow::FrameChanged) => {
                code = Arc::clone(&state.top().code);
                continue;
            }
            Ok(Flow::Complete(value)) => return Exit::Return(value),
            Ok(Flow::CleanupEnded) => return Exit::CleanupEnded,
            Ok(Flow::Need(dependency, kind)) => {
                return Exit::Need {
                    dependency,
                    kind,
                    pc,
                    budget,
                };
            }
            Err(Fault::Thrown(v)) => {
                let error = Box::new(thrown_error(state, heap, v, pc));
                raise(state, Exception { value: v, error })
            }
            Err(Fault::Rethrow(exception)) => raise(state, exception),
            Err(fault) => {
                // A failed instruction leaves the running frame as it was (a
                // failing `Call` pushes no frame), so the failure names it
                // and its callers.
                let (kind, message) = fault_error(fault, &code, pc);
                let error = state.failure(kind, message, pc);
                raise_failure(state, heap, error, Value::Undefined)
            }
        };
        match raised {
            None => {
                code = Arc::clone(&state.top().code);
                // Every value is back in task-owned state: a poll point, so
                // a loop through handlers cannot starve a collection.
                heap.safepoint(state);
            }
            Some(exit) => return exit,
        }
    }
}

/// Early VM tests (hand-assembled bytecode) of the frame stack itself; the
/// embedding-level tests are in `tests/calls.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NoHost;
    use crate::ids::{TaskId, WorldId};
    use crate::value::Primitive;
    use crate::vm::frames::{DiagnosticContext, VmLimits};
    use crate::vm::{Assembler, Executable};

    fn state(code: &Executable, task: u64) -> VmState {
        let diag = DiagnosticContext {
            task: TaskId::from_raw(task),
            world: WorldId::INITIAL,
        };
        VmState::new(&code.0, &[], diag, VmLimits::DEFAULT)
    }

    #[test]
    fn two_tasks_share_function_code_but_not_frames() {
        // function child() { 1; 2; return 3; }  function parent() { return child(); }
        let mut asm = Assembler::new();
        asm.constant(Primitive::Number(1.0)).op(Opcode::Pop);
        asm.constant(Primitive::Number(2.0)).op(Opcode::Pop);
        asm.constant(Primitive::Number(3.0)).op(Opcode::Return);
        let child = asm.finish("child", 0, 0).unwrap();
        let mut asm = Assembler::new();
        asm.function(&child).call(0).op(Opcode::Return);
        let parent = asm.finish("parent", 0, 0).unwrap();
        let heap = Heap::default();
        let (mut a, mut b) = (state(&parent, 1), state(&parent, 2));
        let yielded = |exit| matches!(exit, Exit::Yield);
        assert!(yielded(execute(&mut a, &heap, &mut NoHost, Some(3))));
        assert!(yielded(execute(&mut b, &heap, &mut NoHost, Some(5))));
        let (ca, cb) = (a.frame_code(), b.frame_code());
        assert_eq!((ca.len(), cb.len()), (2, 2), "both are inside child");
        assert!(Arc::ptr_eq(&ca[0], &parent.0) && Arc::ptr_eq(&cb[0], &parent.0));
        assert!(Arc::ptr_eq(&ca[1], &child.0) && Arc::ptr_eq(&cb[1], &child.0));
        // Same code, different saved positions and operands.
        let (pa, oa, _) = a.snapshot();
        let (pb, ob, _) = b.snapshot();
        assert_eq!((pa, oa), (1, vec![Value::Number(1.0)]));
        assert_eq!((pb, ob), (3, vec![Value::Number(2.0)]));
        assert_eq!(a.depth(), 2);
        // Each finishes independently, back in its entry frame.
        for s in [&mut a, &mut b] {
            match execute(s, &heap, &mut NoHost, None) {
                Exit::Return(v) => assert_eq!(v, Value::Number(3.0)),
                other => panic!("{other:?}"),
            }
            assert_eq!(s.depth(), 1);
        }
    }

    #[test]
    fn handler_targets_are_stack_checked_and_round_trip() {
        // A catch target starts one deeper than its `push_catch`.
        let mut asm = Assembler::new();
        let (catch, finally) = (asm.label(), asm.label());
        asm.push_finally(finally).push_catch(catch);
        asm.op(Opcode::Undefined).op(Opcode::Throw);
        asm.bind(catch).op(Opcode::Pop).op(Opcode::PopFinally);
        asm.bind(finally).op(Opcode::EndFinally);
        asm.constant(Primitive::Number(5.0)).op(Opcode::Return);
        let exe = asm.finish("t", 0, 0).unwrap();
        let again = Executable::load(exe.to_spec()).unwrap();
        assert_eq!(again.0.instrs, exe.0.instrs);
        let heap = Heap::default();
        match execute(&mut state(&exe, 1), &heap, &mut NoHost, None) {
            Exit::Return(v) => assert_eq!(v, Value::Number(5.0)),
            other => panic!("{other:?}"),
        }
        // Falling into a catch target at the try block's depth is a mismatch.
        let mut asm = Assembler::new();
        let catch = asm.label();
        asm.push_catch(catch).op(Opcode::PopCatch);
        asm.bind(catch).op(Opcode::Undefined).op(Opcode::Return);
        let e = asm.finish("t", 0, 0).unwrap_err();
        assert!(
            matches!(
                e.kind,
                crate::ValidationErrorKind::StackDepthMismatch { .. }
            ),
            "{e}"
        );
    }

    #[test]
    fn mismatched_handler_instructions_fail_as_internal_errors() {
        // Pops with no matching entry, and a handler whose frame's operands
        // dropped below its height before the throw (hand-assembled code).
        let mut programs = Vec::new();
        programs.push(
            Assembler::new()
                .op(Opcode::PopCatch)
                .op(Opcode::Undefined)
                .op(Opcode::Return)
                .finish("t", 0, 0)
                .unwrap(),
        );
        let mut asm = Assembler::new();
        let finally = asm.label();
        asm.push_finally(finally).op(Opcode::PopCatch);
        asm.bind(finally).op(Opcode::EndFinally);
        asm.op(Opcode::Undefined).op(Opcode::Return);
        programs.push(asm.finish("t", 0, 0).unwrap());
        programs.push(
            Assembler::new()
                .op(Opcode::EndFinally)
                .op(Opcode::Undefined)
                .op(Opcode::Return)
                .finish("t", 0, 0)
                .unwrap(),
        );
        let mut asm = Assembler::new();
        let catch = asm.label();
        asm.op(Opcode::Undefined).push_catch(catch).op(Opcode::Pop);
        asm.op(Opcode::Undefined).op(Opcode::Throw);
        asm.bind(catch).op(Opcode::Return);
        programs.push(asm.finish("t", 0, 0).unwrap());
        let heap = Heap::default();
        for exe in programs {
            match execute(&mut state(&exe, 1), &heap, &mut NoHost, None) {
                Exit::Fail(e) => assert_eq!(e.kind, VmErrorKind::Internal, "{e}"),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn broken_call_invariants_fail_instead_of_panicking() {
        let child = Assembler::new()
            .op(Opcode::Undefined)
            .op(Opcode::Return)
            .finish("child", 0, 0)
            .unwrap();
        // Code that skipped validation: a call with no callee, a function
        // index past the table, and a call with fewer operands than its count.
        let programs: [(Vec<Instr>, Vec<Arc<Code>>); 3] = [
            (vec![Instr::Call(0)], vec![]),
            (vec![Instr::Function(0, 0)], vec![]),
            (
                vec![Instr::Function(0, 0), Instr::Call(2)],
                vec![Arc::clone(&child.0)],
            ),
        ];
        let heap = Heap::default();
        for (instrs, functions) in programs {
            let code = Code {
                name: "corrupt".into(),
                params: 0,
                locals: 0,
                constants: Box::new([]),
                functions: functions.into(),
                captures: 0,
                offsets: (0..instrs.len() as u32).collect(),
                instrs: instrs.clone().into(),
                max_stack: 0,
                debug: None,
            };
            let mut s = state(&Executable(Arc::new(code)), 1);
            match execute(&mut s, &heap, &mut NoHost, None) {
                Exit::Fail(e) => assert_eq!(e.kind, VmErrorKind::Internal, "{instrs:?}"),
                other => panic!("{instrs:?}: {other:?}"),
            }
        }
    }
}
