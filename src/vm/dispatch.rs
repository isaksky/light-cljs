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

use std::sync::Arc;

use crate::collections::{BuilderError, CollectionError, object_name};
use crate::error::{VmError, VmErrorKind};
use crate::heap::{Heap, HeapObject};
use crate::host::{Host, HostCall, call_leaf};
use crate::value::Value;
use crate::vm::executable::Code;
use crate::vm::frames::VmState;
use crate::vm::function::FunctionObject;
use crate::vm::instr::{Instr, Opcode, ThrowKind};
use crate::vm::intrinsics;

/// How a run of the interpreter loop ended.
#[derive(Debug)]
pub(crate) enum Exit {
    Return(Value),
    Fail(VmError),
    /// The budget ran out; the task can be run again.
    Yield,
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
}

/// What the loop does after an instruction.
enum Flow {
    Next,
    /// `Call` or `Return` changed the running frame.
    FrameChanged,
    /// The entry frame returned: the task completes with this value.
    Complete(Value),
}

/// Allocate a function object for `code`. Known shareable when its code
/// constants are (always, in this slice): see `crate::vm::function`.
fn new_function(heap: &Heap, code: Arc<Code>) -> Result<Value, Fault> {
    heap.alloc_persistent(HeapObject::Function(FunctionObject { code }), None)
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

/// The callee of a `Call`: the code of a function object, or the name of
/// what was found instead.
fn callee_code(heap: &Heap, callee: Value) -> Result<Arc<Code>, Fault> {
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

/// Execute one instruction of `code`, the running frame's code.
fn step(
    state: &mut VmState,
    code: &Code,
    heap: &Heap,
    host: &mut dyn Host,
    instr: Instr,
) -> Result<Flow, Fault> {
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
            return Ok(match state.leave(v) {
                Some(result) => Flow::Complete(result),
                None => Flow::FrameChanged,
            });
        }
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
        | Instr::Freeze => intrinsics::step(state, heap, instr)?,
        Instr::Function(i) => {
            let child = code
                .functions
                .get(usize::from(i))
                .cloned()
                .ok_or(Fault::Internal("function index out of range"))?;
            // Pushed in the same instruction, under this run's mutator guard.
            let f = new_function(heap, child)?;
            state.push(f);
        }
        Instr::CurrentFunction => {
            let f = match state.top().callee {
                Some(id) => Value::Object(id),
                None => {
                    // The entry frame has no function object until asked for
                    // one; it is kept in the frame (a root) from now on.
                    let f = new_function(heap, Arc::clone(&state.top().code))?;
                    if let Value::Object(id) = f {
                        state.top_mut().callee = Some(id);
                    }
                    f
                }
            };
            state.push(f);
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

/// Run until the task returns or fails, or until `budget` instructions have
/// executed (`None`: unlimited).
pub(crate) fn execute(
    state: &mut VmState,
    heap: &Heap,
    host: &mut dyn Host,
    mut budget: Option<u64>,
) -> Exit {
    let mut code = Arc::clone(&state.top().code);
    loop {
        if let Some(left) = &mut budget {
            if *left == 0 {
                return Exit::Yield;
            }
            *left -= 1;
        }
        let pc = state.top().pc;
        let instr = code.instrs.get(pc).copied();
        let result = match instr {
            Some(instr) => {
                state.top_mut().pc = pc + 1;
                step(state, &code, heap, host, instr)
            }
            None => Err(Fault::Internal("instruction position out of range")),
        };
        let (kind, message) = match result {
            Ok(Flow::Next) => continue,
            Ok(Flow::FrameChanged) => {
                code = Arc::clone(&state.top().code);
                continue;
            }
            Ok(Flow::Complete(value)) => return Exit::Return(value),
            Err(Fault::Unsupported(m)) => (VmErrorKind::UnsupportedOperand, m),
            Err(Fault::Internal(m)) => (VmErrorKind::Internal, m.to_string()),
            Err(Fault::Host(m)) => (VmErrorKind::HostEffect, m),
            Err(Fault::OutOfMemory(m)) => (VmErrorKind::OutOfMemory, m.to_string()),
            Err(Fault::Collection(e)) => (VmErrorKind::Collection(e), e.to_string()),
            Err(Fault::Builder(e)) => (VmErrorKind::Builder(e), e.to_string()),
            Err(Fault::NotCallable(what)) => {
                (VmErrorKind::TypeError, format!("{what} is not a function"))
            }
            Err(Fault::StackOverflow(m)) => (VmErrorKind::StackOverflow, m),
            Err(Fault::Throw(kind)) => {
                let error_kind = match kind {
                    ThrowKind::Unresolvable | ThrowKind::Uninitialized => {
                        VmErrorKind::ReferenceError
                    }
                    ThrowKind::ConstAssignment => VmErrorKind::TypeError,
                };
                (error_kind, throw_message(&code, pc, kind))
            }
        };
        // A failed instruction leaves the running frame as it was (a failing
        // `Call` pushes no frame), so the failure names it and its callers.
        return Exit::Fail(state.failure(kind, message, pc));
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
            (vec![Instr::Function(0)], vec![]),
            (
                vec![Instr::Function(0), Instr::Call(2)],
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
