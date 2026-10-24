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
//!   operand area and the loop resumes the native with `CallResult`.
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
//! accumulator) stays alive; when the task finishes (completes, fails or is
//! cancelled) its VM state, native frames included, is dropped and those
//! values become garbage unless something else holds them.
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
//! # Errors
//!
//! A failure in a step (a callback that is not a function, an invalid
//! definition id, a failed or cancelled dependency) fails the task at the
//! starting instruction; a failure inside a callback fails it there, with
//! the calling frame in its call chain at the starting instruction. Unwinding
//! a callback or dependency failure *through* a native frame into a
//! JavaScript handler is deferred (clj-quickjs-28): for now every failure is
//! terminal for the task. A cancelled task drops its native frames with the
//! rest of its VM state.
//!
//! # QuickJS
//!
//! QuickJS's `js_call_c_function` (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8) calls a C function on the C stack, which reenters
//! `JS_CallInternal` for callbacks; that recursion is what this protocol
//! replaces. Its `JSAsyncFunctionState` (saved frame, arguments, locals,
//! and resume position) is the inventory of state a pending native must
//! keep, but no promise or job is involved here.

use super::dispatch::{Fault, Flow, callee_code, dependency_id, describe, result_kind};
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
#[derive(Clone, Copy, Debug)]
enum Input {
    Start,
    CallResult(Value),
    Dependency(Value),
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
                [*f, *vec, *acc].trace(visit);
            }
            NativeState::NeedAll { defs, out, .. } => [*defs, *out].trace(visit),
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
        | PushFinally(_) | PopCatch | PopFinally | EndFinally => false,
    }
}

/// The top `N` operands of the running frame, oldest first, not popped.
fn operands<const N: usize>(state: &VmState) -> Result<[Value; N], Fault> {
    let ops = state.operands();
    ops.len()
        .checked_sub(N)
        .and_then(|at| <[Value; N]>::try_from(&ops[at..]).ok())
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
            let len = coll::vector_count(heap, vec)?;
            // Checked once, up front, even for an empty vector.
            callee_code(heap, f)?;
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
            let len = coll::vector_count(heap, defs)?;
            let kind = result_kind(kind).ok_or_else(|| {
                Fault::Unsupported(format!(
                    "need_all takes a result kind as a non-negative integer number, got {}",
                    describe(kind)
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
    let action = frame.state.step(heap, input)?;
    state.truncate_operands(base)?;
    match action {
        Action::Call { callee, args } => {
            let code = callee_code(heap, callee)?;
            let Value::Object(id) = callee else {
                return Err(Fault::Internal("callee is not an object"));
            };
            set_awaiting(state, Awaiting::CallResult)?;
            state.push(callee);
            args.iter().for_each(|&a| state.push(a));
            state.enter(code, id, args.len())?;
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
                    return Ok(Action::Return(*acc));
                }
                let x = coll::nth(heap, *vec, *next)?;
                Ok(Action::Call {
                    callee: *f,
                    args: [*acc, x],
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
                        *out = coll::conj(heap, *out, v)?;
                        *next += 1;
                    }
                    Input::CallResult(_) => {
                        return Err(Fault::Internal("need_all received a call result"));
                    }
                }
                if *next >= *len {
                    return Ok(Action::Return(*out));
                }
                let def = coll::nth(heap, *defs, *next)?;
                let dependency = dependency_id(def).ok_or_else(|| {
                    Fault::Unsupported(format!(
                        "need_all takes definition ids as non-negative integer numbers, got {} at index {next}",
                        describe(def)
                    ))
                })?;
                Ok(Action::Need {
                    dependency,
                    kind: *kind,
                })
            }
        }
    }
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
        let (done, s) = run(&code, &heap, vec, 2);
        assert_eq!(done, None);
        assert!(s.natives.is_empty());
        assert_eq!(s.operands(), [Value::Number(1000.0)], "count already ran");
        assert_eq!(run(&code, &heap, vec, 3).0, Some(Value::Number(1000.0)));
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
        assert_eq!(run(&code, &heap, vec, total - 1).0, None);
        assert_eq!(run(&code, &heap, vec, total).0, Some(Value::Number(6.0)));

        // After the instruction and the first step: inside the first
        // callback, above the native frame.
        let (_, s) = run(&code, &heap, vec, 5);
        assert_eq!((s.natives.len(), s.depth()), (1, 2));
        assert_eq!(running(&s), None, "the callback runs, not the native");
        // After the first callback returned: the native is running again,
        // with the result delivered to its operand area.
        let (_, s) = run(&code, &heap, vec, 9);
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
}
