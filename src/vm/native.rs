//! Resumable native operations: task-owned native continuations driven by
//! the interpreter loop (`docs/design.md` §§5, 10–11).
//!
//! # Leaf and resumable natives
//!
//! A native operation is Rust code the VM runs on behalf of an instruction.
//! There are two kinds, and an instruction is exactly one of them:
//!
//! * **Leaf natives** cannot call JavaScript and cannot demand a
//!   dependency: the collection, builder and atom intrinsics
//!   ([`crate::vm::intrinsics`]) and `HostEffect` ([`crate::host`]). Each is
//!   an ordinary Rust call made inside its instruction, which runs to
//!   completion in one step of the loop; no state is saved for it and no
//!   continuation object exists. That is the fast path, and it stays one.
//! * **Resumable natives** may call JavaScript callbacks and demand
//!   dependencies, so the task may yield, suspend, be collected around and
//!   migrate to another thread in the middle of one. They are `Reduce`
//!   (`__rt.reduce`) and `NeedAll` (`__rt.needAll`). Their instruction only
//!   validates and pops its operands into a [`NativeFrame`] pushed on the
//!   task's VM state; the loop then drives that frame one *step* at a time.
//!
//! [`is_resumable`] is the exhaustive classification.
//!
//! # The protocol
//!
//! A step receives what the previous action produced and returns the next
//! action. Inputs:
//!
//! * `Start`: the first step, right after the instruction.
//! * `CallResult(v)`: the JavaScript callback the previous step called
//!   returned `v`.
//! * `Dependency(v)`: the dependency the previous step demanded is ready
//!   with the shared value `v`.
//!
//! Actions:
//!
//! * `Call { callee, args }`: invoke a JavaScript function. The trampoline
//!   (the dispatch loop) pushes the callee and arguments in the native's
//!   operand area and enters a new task-owned VM frame for it, exactly as a
//!   `Call` instruction does; the loop then runs the callback's
//!   instructions. Its `Return` delivers the result into the native's
//!   operand area and the loop resumes the native with `CallResult`. A
//!   built-in function callee (`crate::vm::builtin`, clj-quickjs-117) runs
//!   to completion instead and its result is delivered the same way, with
//!   `this` undefined.
//! * `Need { dependency, kind }`: demand result `kind` of `dependency` in
//!   the task's world. The loop leaves with `Exit::Need`, like a `Need`
//!   instruction, and the run guard performs the dependency protocol
//!   (`crate::task`, "Dependency waits in the VM"): a ready result is pushed
//!   into the native's operand area at once; a pending one suspends the
//!   whole task, and the run that resumes it pushes the result first. Either
//!   way the next step gets `Dependency`.
//! * `Return(v)`: the operation is complete. The frame is popped and `v` is
//!   pushed onto the calling frame's operands, as the instruction's result;
//!   the calling frame continues after the instruction.
//!
//! Failures are delivered too (clj-quickjs-28, "Errors" below): when an
//! exception unwinds through a pending native, the native receives
//! `Failure::Callback` (the callback it called raised, and nothing inside the
//! callback caught it), `Failure::Dependency` (the dependency it demanded
//! failed or was cancelled: the producer's failure) or `Failure::Step` (its own
//! step failed), and answers with the action `Unwind`: release the frame
//! and let the exception go on to the handler below it. Neither current
//! native recovers from a failure; any other answer is refused as an
//! internal error, so a native can never resume after its frame was
//! partially unwound.
//!
//! No Rust frame represents a pending native: between steps everything it
//! needs is in its `NativeFrame` (a trampoline, not a callback stack), so a
//! callback that suspends the task, or a budget that runs out between two
//! steps, ends the run like any other instruction boundary, and any thread
//! may continue it.
//!
//! # Native frames
//!
//! A [`NativeFrame`] holds, in task-owned storage:
//!
//! * the operation's arguments (the callback, the collection, a result
//!   kind), its progress index and length, and its intermediate value (the
//!   accumulator, or the results vector so far);
//! * what it waits for (a callback result, a dependency, or nothing) and the
//!   result destination: the value-stack slot `base`, the native's own
//!   operand area just above the calling frame's operands, where a
//!   callback's `Return` or the dependency protocol puts the completion;
//! * its diagnostic context: the index of the instruction that started it
//!   in the calling frame's code, which failures and a suspended task's
//!   [`source_location`](crate::Task::source_location) report (the task and
//!   world are the task's, in `VmState::diag`: a native never changes
//!   world, and its dependencies are read in the task's world).
//!
//! Native frames are stacked beside the call frames: each records the call
//! depth of the frame that started it, and it is *running* when no call
//! frame is above that depth. A callback may itself start a resumable
//! native, so frames nest; at most one is running per call depth, so their
//! number is bounded by the task's call-depth limit.
//!
//! # Rooting, locks and safepoints
//!
//! Native frames are part of the task's VM state, so the collector traces
//! them wherever it traces the task's frames (`RootCategory::TaskFrame`):
//! through the task's root source while it is suspended or yielded, and
//! from the runner at a VM safepoint while it runs. A value held only by a
//! pending native (the collection being folded, its unvisited elements, the
//! accumulator) stays alive; when the native returns, an exception or a
//! cancellation unwinds through it ("Errors" below), or the task finishes
//! (completes, fails or is cancelled), its frame is dropped and those values
//! become garbage unless something else holds them.
//!
//! The loop polls for a requested collection before every step, at which
//! point every value the native holds is in its frame. Within a step,
//! values are read in place: a delivered result stays in the operand area
//! (a root) until the step has stored it in the frame, and anything a step
//! allocates is stored in the frame before the step returns. A step reads
//! one element at a time through [`crate::collections`], which takes and
//! releases the heap's lock inside the call; no heap lock, `Heap::read`
//! closure or Rust reference into `VmState` is held across a step boundary,
//! so none is held across a callback (reentry), stack growth, a collection
//! or a suspension.
//!
//! # Budget
//!
//! Each step counts as one instruction against the run's [`Budget`](crate::Budget),
//! checked between steps like instructions, so a long fold over a vector
//! whose callbacks are cheap still yields. The starting instruction is one
//! more.
//!
//! # Errors (clj-quickjs-28)
//!
//! A failure in a step (an invalid definition id, a failed or cancelled
//! dependency) is reported at the starting instruction; a failure inside a
//! callback is reported there, with the calling frame in its call chain at
//! the starting instruction. Whether it is an exception follows the usual
//! rule (`crate::vm::exception`): a `throw`, a catchable run-time error or a
//! failed or cancelled dependency is; anything else ends the run at once.
//!
//! An exception unwinds through native frames like through call frames
//! ([`VmState::raise`](super::frames::VmState::raise)):
//!
//! * a `try` inside a callback catches what the callback raises, and the
//!   fold continues with the value its `catch` returns;
//! * otherwise, when the innermost handler belongs to the frame that started
//!   a pending native or to one of its callers, the exception passes that
//!   native: the callback frames above it are popped with their operands,
//!   the native receives the failure (`Failure::Callback`, `Failure::Dependency` or
//!   `Failure::Step`, by what it was waiting for), answers `Unwind`, and its
//!   frame is popped ([`unwind`]): the collection, the callback, the
//!   accumulator or results so far, and the operand area are released, so
//!   nothing the native held is a root any more. Nested natives are passed
//!   innermost first. The handler then runs as for any exception: a catch
//!   block sees the exception value (for a dependency, the error object
//!   whose cause is the producer's published value), a finally block runs
//!   with the exception pending;
//! * a failure of the starting instruction itself (a callee that is not a
//!   function, an operand that is not a vector) happens before the native
//!   frame exists and is caught like any failing instruction's.
//!
//! A dependency demanded by `needAll` that fails after the task suspended
//! is delivered by the resuming run at the starting instruction (the
//! pending need is cleared first), so a task that waited inside a native
//! and resumes on another thread or worker unwinds exactly as one that
//! found the failure at once.
//!
//! **Cancellation** ([`crate::Task::cancel`], `crate::task`, "Cancellation
//! and finally blocks") is not an exception and gives a native no step: the
//! observing run leaves the cell the task waits on (so the producer and its
//! other consumers are unaffected), and unwinding to the next finally
//! handler drops every native frame it passes without delivering anything
//! ([`discard`]): no native code runs for a cancelled task, and nothing it
//! held stays a root. A finally block inside a callback runs without
//! popping the native below it; when that block ends, the cancellation goes
//! on to the next handler, dropping the native then. If no finally block is
//! left, terminal cleanup drops the whole VM state, native frames included.
//!
//! # Safepoint contract and debug borrow checks
//!
//! `docs/design.md` §10: "Do not hold Rust references into mutable VM
//! storage across operations that may reenter, collect, grow storage, or
//! suspend." A step and a failure delivery run while holding a **scoped
//! borrow** ([`ScopedBorrow`], taken by the trampoline): a `&mut` into the
//! task's native frame, and possibly heap object reads. The operations that
//! reenter, collect, grow storage or suspend check that the running thread
//! holds no scoped borrow ([`assert_no_borrow`]): entering the dispatch loop
//! or starting a task run (reentry), a VM safepoint (collection), pushing a
//! value, a call frame or a native frame (storage growth), and leaving the
//! loop for a dependency or parking (suspension). A native that does any of
//! them from inside its step, instead of returning the corresponding action
//! to the trampoline, violates the contract: debug builds reject it with a
//! panic naming the operation; release builds compile the checks away (the
//! counter does not exist), so the contract is then upheld by construction
//! only: a step receives `&mut self` and `&Heap`, never the `VmState`.
//! Native code that needs its own scoped borrow takes one with
//! [`scoped_borrow`].
//!
//! # QuickJS
//!
//! QuickJS's `js_call_c_function` (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8) calls a C function on the C stack, which reenters
//! `JS_CallInternal` for callbacks; that recursion is what this protocol
//! replaces. Its `JSAsyncFunctionState` (saved frame, arguments, locals,
//! and resume position) is the inventory of state a pending native must
//! keep, but no promise or job is involved here.

use std::marker::PhantomData;

use super::builtin;
use super::dispatch::{self, Callee, Fault, Flow, dependency_id, describe, result_kind};
use super::frames::VmState;
use super::instr::Instr;
use crate::collections as coll;
use crate::heap::gc::Trace;
use crate::heap::{Heap, ObjectId};
use crate::ids::{DependencyId, ResultKind};
use crate::value::Value;

/// What a native frame is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Awaiting {
    /// Nothing: the next step is the first (`Start`).
    Nothing,
    /// The return value of the callback its last step called.
    CallResult,
    /// The value of the dependency its last step demanded.
    Dependency,
}

/// The completion delivered to a step.
#[derive(Clone, Debug)]
enum Input {
    Start,
    CallResult(Value),
    Dependency(Value),
}

/// A failure delivered to a native frame an exception unwinds through
/// (module docs, "Errors"), named by what the native was waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// The callback its last step called raised an exception that nothing
    /// inside the callback caught (or calling it failed).
    Callback,
    /// The dependency its last step demanded failed or was cancelled: the
    /// exception is the error object for the producer's failure.
    Dependency,
    /// Its own step raised a catchable failure.
    Step,
}

/// What a step asks the trampoline to do next.
#[derive(Debug)]
enum Action {
    /// Call `callee` with these arguments in a new VM frame.
    Call { callee: Value, args: [Value; 2] },
    /// Read a dependency in the task's world, suspending if it is pending.
    Need {
        dependency: DependencyId,
        kind: ResultKind,
    },
    /// Complete with this value.
    Return(Value),
    /// The answer to a [`Failure`]: release the frame and let the exception
    /// unwind on to the handler below it. Never the result of a step.
    Unwind,
}

/// The operation-specific state of a native frame.
#[derive(Debug)]
enum NativeState {
    /// `reduce(f, init, vec)`: `acc = f(acc, x)` for each element `x` of
    /// `vec` in order, starting from `init`.
    Reduce {
        f: Value,
        vec: Value,
        /// The accumulator: `init`, then each callback's result.
        acc: Value,
        /// The element whose callback is in flight, or the next to visit.
        next: usize,
        len: usize,
    },
    /// `needAll(defs, kind)`: the results of needing `kind` of each
    /// definition in the vector `defs`, in order, as a new vector.
    NeedAll {
        defs: Value,
        kind: ResultKind,
        /// The results so far (a persistent vector once started).
        out: Value,
        /// The element whose dependency is demanded, or the next to demand.
        next: usize,
        len: usize,
    },
}

/// A pending resumable native operation. See the module docs.
#[derive(Debug)]
pub(crate) struct NativeFrame {
    /// Call depth of the frame that started it; the native is running when
    /// the task's depth equals this.
    depth: usize,
    /// Value-stack index of the native's operand area: the destination of a
    /// callback's result or a dependency's value.
    base: usize,
    /// Index of the starting instruction in the calling frame's code.
    pc: usize,
    awaiting: Awaiting,
    state: NativeState,
}

impl Trace for NativeFrame {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match &self.state {
            NativeState::Reduce { f, vec, acc, .. } => {
                for v in [f, vec, acc] {
                    v.trace(visit);
                }
            }
            NativeState::NeedAll { defs, out, .. } => {
                defs.trace(visit);
                out.trace(visit);
            }
        }
    }
}

/// True for the instructions that start a resumable native; every other
/// instruction (leaf natives included) runs to completion in one step.
/// Exhaustive, so a new instruction must decide.
pub(crate) fn is_resumable(instr: Instr) -> bool {
    use Instr::*;
    match instr {
        Reduce | NeedAll => true,
        Const(_) | Undefined | GetLocal(_) | SetLocal(_) | Pop | Dup | Add | Sub | Mul | Lt
        | StrictEq | Not | Jump(_) | JumpIfFalse(_) | Return | NewObject | HostEffect(_)
        | NewVector(_) | NewMap(_) | Count | Nth | Get | GetOr | Assoc | Dissoc | Conj
        | PopLast | NewVectorBuilder | NewMapBuilder | BuilderConj | BuilderAssoc | Freeze
        | Div | Rem | Neg | ToNumber | ToNumberPair | Gt | Le | Ge | LooseEq | ThrowError(_)
        | Function(..) | Call(_) | CurrentFunction | GetCapture(_) | NewCell | CellInit
        | CellGet | CellSet | AtomNew | AtomLoad | AtomCas | Need | Throw | PushCatch(_)
        | PushFinally(_) | PopCatch | PopFinally | EndFinally | BitAnd | BitOr | BitXor | Shl
        | Sar | UShr | BitNot | Pow | CallFinally | DropCompletion | Plus | CompareLt
        | CompareGt | CompareLe | CompareGe | Typeof | GetField(_) | PutField(_)
        | DefineField(_) | GetBuiltinGlobal(_) | CallMethod(_) | NewObjectLiteral | ToString
        | GetThis | NewArray | AppendElement | AppendHole | GetElem | PutElem | Dup2
        | ToPropertyKey => false,
    }
}

/// The top `N` operands of the running frame, oldest first, not popped.
fn operands<const N: usize>(state: &VmState) -> Result<[Value; N], Fault> {
    let ops = state.operands();
    ops.len()
        .checked_sub(N)
        .map(|at| std::array::from_fn(|i| ops[at + i].clone()))
        .ok_or(Fault::Internal("operand stack underflow"))
}

/// Execute the starting instruction `instr` at `pc` of the running frame:
/// validate its operands, pop them into a new native frame and push it. The
/// operation's first step runs next. Allocates nothing.
pub(crate) fn start(
    state: &mut VmState,
    heap: &Heap,
    instr: Instr,
    pc: usize,
) -> Result<(), Fault> {
    let (popped, native) = match instr {
        Instr::Reduce => {
            let [f, init, vec] = operands::<3>(state)?;
            let len = coll::vector_count(heap, vec.clone())?;
            // Checked once, up front, even for an empty vector.
            dispatch::callee(heap, &f)?;
            let native = NativeState::Reduce {
                f,
                vec,
                acc: init,
                next: 0,
                len,
            };
            (3, native)
        }
        Instr::NeedAll => {
            let [defs, kind] = operands::<2>(state)?;
            let len = coll::vector_count(heap, defs.clone())?;
            let kind = result_kind(&kind).ok_or_else(|| {
                Fault::Unsupported(format!(
                    "need_all takes a result kind as a non-negative integer number, got {}",
                    describe(&kind)
                ))
            })?;
            let native = NativeState::NeedAll {
                defs,
                kind,
                out: Value::Undefined,
                next: 0,
                len,
            };
            (2, native)
        }
        _ => return Err(Fault::Internal("not a resumable native instruction")),
    };
    for _ in 0..popped {
        state.pop()?;
    }
    assert_no_borrow("starting a native frame (growing native storage)");
    let frame = NativeFrame {
        depth: state.depth(),
        base: state.stack_len(),
        pc,
        awaiting: Awaiting::Nothing,
        state: native,
    };
    state.natives.push(frame);
    Ok(())
}

/// An exception unwinds to a handler of frame index `frame` (0: the entry
/// frame): deliver the failure to every pending native it passes, innermost
/// first (the natives started by `frame` or by a frame above it), and pop
/// each one that answers `Unwind`, releasing what it held (module docs,
/// "Errors"). An `Err` names a native that refused; the natives not yet
/// passed are left as they were.
pub(crate) fn unwind(natives: &mut Vec<NativeFrame>, frame: usize) -> Result<(), &'static str> {
    // A native started by frame index `i` has depth `i + 1`; depths strictly
    // increase along the stack (at most one native runs per call depth).
    while let Some(native) = natives.last_mut().filter(|n| frame < n.depth) {
        let failure = match native.awaiting {
            Awaiting::CallResult => Failure::Callback,
            Awaiting::Dependency => Failure::Dependency,
            Awaiting::Nothing => Failure::Step,
        };
        let action = {
            let _borrow = scoped_borrow();
            native.state.fail(failure)
        };
        match action {
            Action::Unwind => {
                natives.pop();
            }
            Action::Call { .. } | Action::Need { .. } | Action::Return(_) => {
                return Err("a native frame did not unwind from a failure");
            }
        }
    }
    Ok(())
}

/// Cancellation unwinds to a finally handler of frame index `frame`: drop
/// every pending native it passes without running any native code (module
/// docs, "Errors", cancellation).
pub(crate) fn discard(natives: &mut Vec<NativeFrame>, frame: usize) {
    let kept = natives.partition_point(|n| n.depth <= frame);
    natives.truncate(kept);
}

/// The starting instruction of the running native frame, if one is running
/// (no call frame above it).
pub(crate) fn running(state: &VmState) -> Option<usize> {
    state
        .natives
        .last()
        .filter(|n| n.depth == state.depth())
        .map(|n| n.pc)
}

/// Run one step of the running native frame and apply its action (module
/// docs, "The protocol").
pub(crate) fn step(state: &mut VmState, heap: &Heap) -> Result<Flow, Fault> {
    let depth = state.depth();
    let frame = state
        .natives
        .last()
        .filter(|n| n.depth == depth)
        .ok_or(Fault::Internal("no running native frame"))?;
    let (base, awaiting) = (frame.base, frame.awaiting);
    // The completion is read in place: it stays rooted in the operand area
    // until the step has stored it in the frame.
    let delivered = match awaiting {
        Awaiting::Nothing => None,
        Awaiting::CallResult | Awaiting::Dependency => Some(state.stack_value(base)?),
    };
    let expected = base + usize::from(delivered.is_some());
    if state.stack_len() != expected {
        return Err(Fault::Internal("native operand area out of step"));
    }
    let input = match (awaiting, delivered) {
        (Awaiting::CallResult, Some(v)) => Input::CallResult(v),
        (Awaiting::Dependency, Some(v)) => Input::Dependency(v),
        _ => Input::Start,
    };
    let frame = state
        .natives
        .last_mut()
        .ok_or(Fault::Internal("no running native frame"))?;
    frame.awaiting = Awaiting::Nothing;
    let action = {
        let _borrow = scoped_borrow();
        frame.state.step(heap, input)?
    };
    state.truncate_operands(base)?;
    match action {
        Action::Call { callee, args } => {
            let (id, code) = match dispatch::callee(heap, &callee)? {
                Callee::Code(id, code) => (id, code),
                Callee::Builtin(which) => {
                    // Runs to completion (`crate::vm::builtin`); no safepoint
                    // falls between here and the push that roots its result.
                    // The next step receives it like a callback's return.
                    let result =
                        builtin::call(heap, state.diag.task, which, &Value::Undefined, &args)?;
                    set_awaiting(state, Awaiting::CallResult)?;
                    state.push(result);
                    return Ok(Flow::Next);
                }
            };
            set_awaiting(state, Awaiting::CallResult)?;
            state.push(callee);
            let argc = args.len();
            args.into_iter().for_each(|a| state.push(a));
            state.enter(code, id, Value::Undefined, argc)?;
            Ok(Flow::FrameChanged)
        }
        Action::Need { dependency, kind } => {
            set_awaiting(state, Awaiting::Dependency)?;
            Ok(Flow::Need(dependency, kind))
        }
        Action::Return(v) => {
            state.natives.pop();
            state.push(v);
            Ok(Flow::Next)
        }
        Action::Unwind => Err(Fault::Internal("a native step unwound without a failure")),
    }
}

fn set_awaiting(state: &mut VmState, awaiting: Awaiting) -> Result<(), Fault> {
    let frame = state
        .natives
        .last_mut()
        .ok_or(Fault::Internal("no running native frame"))?;
    frame.awaiting = awaiting;
    Ok(())
}

impl NativeState {
    /// One step: consume `input`, return the next action. Anything allocated
    /// is stored in `self` (a root) before returning.
    fn step(&mut self, heap: &Heap, input: Input) -> Result<Action, Fault> {
        match self {
            NativeState::Reduce {
                f,
                vec,
                acc,
                next,
                len,
            } => {
                match input {
                    Input::Start => {}
                    Input::CallResult(v) => {
                        *acc = v;
                        *next += 1;
                    }
                    Input::Dependency(_) => {
                        return Err(Fault::Internal("reduce received a dependency"));
                    }
                }
                if *next >= *len {
                    return Ok(Action::Return(acc.clone()));
                }
                let x = coll::nth(heap, vec.clone(), *next)?;
                Ok(Action::Call {
                    callee: f.clone(),
                    args: [acc.clone(), x],
                })
            }
            NativeState::NeedAll {
                defs,
                kind,
                out,
                next,
                len,
            } => {
                match input {
                    Input::Start => *out = coll::new_vector(heap, &[])?,
                    Input::Dependency(v) => {
                        *out = coll::conj(heap, out.clone(), v)?;
                        *next += 1;
                    }
                    Input::CallResult(_) => {
                        return Err(Fault::Internal("need_all received a call result"));
                    }
                }
                if *next >= *len {
                    return Ok(Action::Return(out.clone()));
                }
                let def = coll::nth(heap, defs.clone(), *next)?;
                let dependency = dependency_id(&def).ok_or_else(|| {
                    Fault::Unsupported(format!(
                        "need_all takes definition ids as non-negative integer numbers, got {} at index {next}",
                        describe(&def)
                    ))
                })?;
                Ok(Action::Need {
                    dependency,
                    kind: *kind,
                })
            }
        }
    }

    /// Receive a failure an exception carries through this native and
    /// answer it (module docs, "Errors"). Neither operation recovers: the
    /// fold's accumulator and `needAll`'s results so far are abandoned with
    /// the frame, and the exception goes on to the caller's handler, which
    /// is what JavaScript's `Array.prototype.reduce` does with a throwing
    /// callback. Allocates nothing and touches no heap object.
    fn fail(&mut self, failure: Failure) -> Action {
        match (self, failure) {
            (
                NativeState::Reduce { .. } | NativeState::NeedAll { .. },
                Failure::Callback | Failure::Dependency | Failure::Step,
            ) => Action::Unwind,
        }
    }
}

#[cfg(debug_assertions)]
thread_local! {
    /// Scoped borrows held by native code on this thread (debug builds).
    static BORROWS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// A scoped borrow held by native code (module docs, "Safepoint contract
/// and debug borrow checks"): while one is alive on a thread, every
/// operation that may reenter, collect, grow storage or suspend panics in
/// debug builds. Bound to its thread; released on drop, also on unwind.
#[must_use = "a scoped borrow is released when dropped"]
pub(crate) struct ScopedBorrow {
    _thread_bound: PhantomData<*const ()>,
}

/// Take a scoped borrow for the current thread (counted in debug builds).
pub(crate) fn scoped_borrow() -> ScopedBorrow {
    #[cfg(debug_assertions)]
    BORROWS.with(|b| b.set(b.get() + 1));
    ScopedBorrow {
        _thread_bound: PhantomData,
    }
}

impl Drop for ScopedBorrow {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        BORROWS.with(|b| b.set(b.get().saturating_sub(1)));
    }
}

/// Reject (debug builds: panic) `operation`, one that may reenter, collect,
/// grow storage or suspend, if the current thread holds a scoped borrow: a
/// violation of the native safepoint contract. Free in release builds.
#[inline]
#[track_caller]
pub(crate) fn assert_no_borrow(operation: &'static str) {
    #[cfg(debug_assertions)]
    {
        let held = BORROWS.with(std::cell::Cell::get);
        assert!(
            held == 0,
            "native safepoint contract violated: {operation} while holding {held} scoped \
             borrow(s) (crate::vm::native, \"Safepoint contract and debug borrow checks\")"
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = operation;
}

/// Early VM tests (hand-assembled bytecode, not JavaScript source support)
/// of the leaf/resumable split; the embedding-level tests, from source, are
/// in `tests/native_fold.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NoHost;
    use crate::ids::{TaskId, WorldId};
    use crate::value::Primitive;
    use crate::vm::dispatch::{Exit, execute};
    use crate::vm::frames::{DiagnosticContext, VmLimits};
    use crate::vm::{Assembler, Executable, Opcode};

    fn vector(heap: &Heap, n: usize) -> Value {
        let items: Vec<Value> = (1..=n).map(|i| Value::Number(i as f64)).collect();
        coll::new_vector(heap, &items).unwrap()
    }

    /// Run `code(vec)` with a budget of `budget` units; the result is `None`
    /// if it yielded.
    fn run(code: &Executable, heap: &Heap, vec: Value, budget: u64) -> (Option<Value>, VmState) {
        let diag = DiagnosticContext {
            task: TaskId::from_raw(1),
            world: WorldId::INITIAL,
        };
        let mut s = VmState::new(&code.0, &[vec], diag, VmLimits::DEFAULT);
        match execute(&mut s, heap, &mut NoHost, Some(budget)) {
            Exit::Return(v) => (Some(v), s),
            Exit::Yield => (None, s),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_reduce_and_need_all_are_resumable() {
        let leaves = [
            Instr::Count,
            Instr::Nth,
            Instr::Conj,
            Instr::Freeze,
            Instr::AtomCas,
            Instr::HostEffect(0),
            Instr::Need,
            Instr::Call(2),
        ];
        assert!(leaves.iter().all(|&i| !is_resumable(i)));
        assert!(is_resumable(Instr::Reduce) && is_resumable(Instr::NeedAll));
    }

    /// A leaf intrinsic is one instruction: one budget unit, no native frame.
    #[test]
    fn a_leaf_native_runs_inside_its_instruction() {
        let heap = Heap::default();
        let code = Assembler::new()
            .get_local(0)
            .op(Opcode::Count)
            .op(Opcode::Return)
            .finish("count", 1, 1)
            .unwrap();
        let vec = vector(&heap, 1000);
        let (done, s) = run(&code, &heap, vec.clone(), 2);
        assert_eq!(done, None);
        assert!(s.natives.is_empty());
        assert_eq!(s.operands(), [Value::Number(1000.0)], "count already ran");
        assert_eq!(
            run(&code, &heap, vec.clone(), 3).0,
            Some(Value::Number(1000.0))
        );
    }

    /// `reduce(add, 0, vec)` costs its instruction, one step per element plus
    /// the final one, and each callback's instructions; between them the
    /// native frame is the task's own state.
    #[test]
    fn a_resumable_native_is_driven_step_by_step() {
        let heap = Heap::default();
        let add = Assembler::new()
            .get_local(0)
            .get_local(1)
            .op(Opcode::Add)
            .op(Opcode::Return)
            .finish("add", 2, 2)
            .unwrap();
        let mut asm = Assembler::new();
        asm.function(&add)
            .constant(Primitive::Number(0.0))
            .get_local(0)
            .op(Opcode::Reduce)
            .op(Opcode::Return);
        let code = asm.finish("sum", 1, 1).unwrap();
        let vec = vector(&heap, 3);
        // 4 instructions to the reduce, 4 steps, 3 callbacks of 4, 1 return.
        let total = 4 + 4 + 3 * 4 + 1;
        assert_eq!(run(&code, &heap, vec.clone(), total - 1).0, None);
        assert_eq!(
            run(&code, &heap, vec.clone(), total).0,
            Some(Value::Number(6.0))
        );

        // After the instruction and the first step: inside the first
        // callback, above the native frame.
        let (_, s) = run(&code, &heap, vec.clone(), 5);
        assert_eq!((s.natives.len(), s.depth()), (1, 2));
        assert_eq!(running(&s), None, "the callback runs, not the native");
        // After the first callback returned: the native is running again,
        // with the result delivered to its operand area.
        let (_, s) = run(&code, &heap, vec.clone(), 9);
        assert_eq!((s.natives.len(), s.depth()), (1, 1));
        assert_eq!(running(&s), Some(3), "started by instruction 3");
        assert_eq!(s.operands(), [Value::Number(1.0)]);
        // The fold's frame holds the vector (and the callback).
        let mut traced = Vec::new();
        s.natives[0].trace(&mut |id| traced.push(id));
        let Value::Object(v) = vec else {
            panic!("a vector is an object")
        };
        assert!(traced.contains(&v), "{traced:?}");
    }

    /// Run `f` and return its panic message (it must panic).
    #[cfg(debug_assertions)]
    fn violation(f: impl FnOnce()) -> String {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
            .expect_err("the operation is rejected");
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    }

    /// The safepoint contract is enforced in debug builds: while native code
    /// holds a scoped borrow, reentering the VM, growing VM storage, a
    /// safepoint and suspending are each rejected with a panic naming the
    /// violation; the borrow is released on unwind, and the trampoline's own
    /// borrows never leak past a step.
    #[cfg(debug_assertions)]
    #[test]
    fn scoped_borrows_reject_reentry_growth_and_suspension_in_debug_builds() {
        let heap = Heap::default();
        let code = Assembler::new()
            .get_local(0)
            .op(Opcode::Return)
            .finish("id", 1, 1)
            .unwrap();
        let vec = vector(&heap, 2);
        let diag = DiagnosticContext {
            task: TaskId::from_raw(1),
            world: WorldId::INITIAL,
        };
        let fresh = || VmState::new(&code.0, std::slice::from_ref(&vec), diag, VmLimits::DEFAULT);
        let expect = |message: String, operation: &str| {
            assert!(
                message.contains("native safepoint contract violated")
                    && message.contains(operation),
                "{message}"
            );
        };

        // Reentry: the dispatch loop, and a call frame.
        let mut s = fresh();
        expect(
            violation(|| {
                let _b = scoped_borrow();
                execute(&mut s, &heap, &mut NoHost, None);
            }),
            "reentering the dispatch loop",
        );
        let mut s = fresh();
        let callee = coll::new_vector(&heap, &[]).unwrap();
        let Value::Object(id) = callee else {
            panic!("a vector is an object")
        };
        s.push(callee);
        expect(
            violation(|| {
                let _b = scoped_borrow();
                let _ = s.enter(std::sync::Arc::clone(&code.0), id, Value::Undefined, 0);
            }),
            "entering a call frame",
        );
        // Storage growth.
        expect(
            violation(|| {
                let _b = scoped_borrow();
                s.push(Value::Null);
            }),
            "pushing a value",
        );
        // A safepoint reached directly through the heap (as a step holding
        // `&Heap` could), whether or not a collection is requested.
        expect(
            violation(|| {
                let _b = scoped_borrow();
                heap.safepoint(&Value::Undefined);
            }),
            "reaching a VM safepoint",
        );
        // Released on unwind: the loop runs normally again.
        assert_no_borrow("after the violations");
        let mut s = fresh();
        assert!(matches!(
            execute(&mut s, &heap, &mut NoHost, None),
            Exit::Return(_)
        ));

        // Suspension and reentry through the embedding API.
        let rt = crate::Runtime::new();
        let exe =
            crate::frontend::compile_function("function f() { return __rt.need(1, 1); }").unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        let mut guard = task.try_begin_run().unwrap();
        let pending = guard.need(DependencyId::new(1), ResultKind::new(1));
        assert!(
            matches!(pending, Ok(crate::Need::Waiting { .. })),
            "{pending:?}"
        );
        expect(
            violation(|| {
                let _b = scoped_borrow();
                guard.park();
            }),
            "parking the task",
        );
        expect(
            violation(|| {
                let _b = scoped_borrow();
                let _ = task.try_begin_run();
            }),
            "starting a task run",
        );

        // A fold's steps and failure deliveries take and release borrows.
        let add = Assembler::new()
            .get_local(0)
            .get_local(1)
            .op(Opcode::Add)
            .op(Opcode::Return)
            .finish("add", 2, 2)
            .unwrap();
        let mut asm = Assembler::new();
        asm.function(&add)
            .constant(Primitive::Number(0.0))
            .get_local(0)
            .op(Opcode::Reduce)
            .op(Opcode::Return);
        let sum = asm.finish("sum", 1, 1).unwrap();
        assert_eq!(
            run(&sum, &heap, vec.clone(), 100).0,
            Some(Value::Number(3.0))
        );
        assert_no_borrow("after a fold");
    }

    /// A failure is delivered to each native it passes, innermost first, and
    /// each answers `Unwind`; natives started below the handler's frame are
    /// kept. Cancellation drops the same natives without delivering anything.
    #[test]
    fn unwinding_delivers_failures_and_pops_only_the_natives_passed() {
        let native = |depth, awaiting| NativeFrame {
            depth,
            base: 0,
            pc: 0,
            awaiting,
            state: NativeState::Reduce {
                f: Value::Undefined,
                vec: Value::Undefined,
                acc: Value::Undefined,
                next: 0,
                len: 0,
            },
        };
        let stack = || {
            vec![
                native(1, Awaiting::CallResult),
                native(3, Awaiting::CallResult),
                native(4, Awaiting::Dependency),
            ]
        };
        let depths = |n: &[NativeFrame]| n.iter().map(|n| n.depth).collect::<Vec<_>>();
        for (frame, kept) in [
            (4, vec![1, 3, 4]),
            (3, vec![1, 3]),
            (1, vec![1]),
            (0, vec![]),
        ] {
            let mut natives = stack();
            unwind(&mut natives, frame).unwrap();
            assert_eq!(depths(&natives), kept, "raise to frame {frame}");
            let mut natives = stack();
            discard(&mut natives, frame);
            assert_eq!(depths(&natives), kept, "cancel to frame {frame}");
        }
        let mut reduce = stack().pop().unwrap().state;
        for failure in [Failure::Callback, Failure::Dependency, Failure::Step] {
            assert!(matches!(reduce.fail(failure), Action::Unwind));
        }
    }
}
