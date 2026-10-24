//! The interpreter loop.
//!
//! [`execute`] runs a task's [`VmState`] against shared, validated [`Code`]
//! until the task returns, fails, or exhausts its execution budget. All state it mutates is the task-owned
//! `VmState`; it keeps no Rust references into the operand stack or locals
//! across instructions, and nothing is held on the Rust stack that a later
//! run would need (`docs/design.md` §§3, 10). The instruction position is
//! advanced before each instruction executes, so `frame.pc` always names the
//! next instruction to run.
//!
//! The loop never panics on its inputs: validation rules out bad indices and
//! stack underflow, and the loop still re-checks them, failing the task with
//! [`VmErrorKind::Internal`] if an invariant is ever broken.
//!
//! **Budget and yield.** The budget counts instructions. It is checked only
//! *between* instructions: when it is exhausted the loop returns
//! [`Exit::Yield`] with `frame.pc` naming the next instruction (possibly a
//! branch target just jumped to) and the operands and locals as that
//! instruction expects them. Every instruction, including `HostEffect`, runs
//! to completion within one run, so a later run continues exactly where this
//! one stopped and never replays an executed instruction or its effect.

use crate::error::{Diagnostic, VmError, VmErrorKind};
use crate::heap::{Heap, HeapObject};
use crate::host::{Host, HostCall, call_leaf};
use crate::value::Value;
use crate::vm::executable::Code;
use crate::vm::frames::VmState;
use crate::vm::instr::{Instr, Opcode};

/// How a run of the interpreter loop ended.
#[derive(Debug)]
pub(crate) enum Exit {
    Return(Value),
    Fail(VmError),
    /// The budget ran out; the task can be run again.
    Yield,
}

/// A failure before diagnostic context is attached.
enum Fault {
    Unsupported(String),
    Internal(&'static str),
    /// The host reported an error from a leaf effect.
    Host(String),
    /// The heap could not allocate.
    OutOfMemory(&'static str),
}

fn pop(operands: &mut Vec<Value>) -> Result<Value, Fault> {
    operands
        .pop()
        .ok_or(Fault::Internal("operand stack underflow"))
}

fn local_slot(locals: &mut [Value], index: u16) -> Result<&mut Value, Fault> {
    locals
        .get_mut(usize::from(index))
        .ok_or(Fault::Internal("local index out of range"))
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

/// Execute one instruction. `Ok(Some(v))` means the task returned `v`.
fn step(
    state: &mut VmState,
    code: &Code,
    heap: &Heap,
    host: &mut dyn Host,
    instr: Instr,
) -> Result<Option<Value>, Fault> {
    let frame = &mut state.frame;
    let ops = &mut frame.operands;
    match instr {
        Instr::Const(i) => {
            let v = code
                .constants
                .get(usize::from(i))
                .copied()
                .ok_or(Fault::Internal("constant index out of range"))?;
            ops.push(v);
        }
        Instr::Undefined => ops.push(Value::Undefined),
        Instr::GetLocal(i) => {
            let v = *local_slot(&mut frame.locals, i)?;
            ops.push(v);
        }
        Instr::SetLocal(i) => {
            let v = pop(ops)?;
            *local_slot(&mut frame.locals, i)? = v;
        }
        Instr::Pop => {
            pop(ops)?;
        }
        Instr::Dup => {
            let v = *ops
                .last()
                .ok_or(Fault::Internal("operand stack underflow"))?;
            ops.push(v);
        }
        Instr::Add | Instr::Sub | Instr::Mul | Instr::Lt => {
            let b = pop(ops)?;
            let a = pop(ops)?;
            let (x, y) = numbers(instr.opcode(), a, b)?;
            ops.push(match instr {
                Instr::Add => Value::Number(x + y),
                Instr::Sub => Value::Number(x - y),
                Instr::Mul => Value::Number(x * y),
                _ => Value::Bool(x < y),
            });
        }
        Instr::StrictEq => {
            let b = pop(ops)?;
            let a = pop(ops)?;
            ops.push(Value::Bool(a.strict_equals(b)));
        }
        Instr::Not => {
            let v = pop(ops)?;
            ops.push(Value::Bool(!v.truthy()));
        }
        Instr::Jump(target) => frame.pc = target as usize,
        Instr::JumpIfFalse(target) => {
            if !pop(ops)?.truthy() {
                frame.pc = target as usize;
            }
        }
        Instr::Return => return pop(ops).map(Some),
        Instr::NewObject => {
            // The new object is pushed in the same instruction; no collection
            // can run in between because this run holds a mutator guard.
            let id = heap
                .alloc(HeapObject::Plain)
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            ops.push(Value::Object(id));
        }
        Instr::HostEffect(op) => {
            let v = pop(ops)?;
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
            // host call (`ops` is not used after this point).
            let result = call_leaf(host, call).map_err(Fault::Host)?;
            state.frame.operands.push(Value::from(result));
        }
    }
    Ok(None)
}

/// Run until the task returns or fails, or until `budget` instructions have
/// executed (`None`: unlimited).
pub(crate) fn execute(
    state: &mut VmState,
    code: &Code,
    heap: &Heap,
    host: &mut dyn Host,
    mut budget: Option<u64>,
) -> Exit {
    loop {
        if let Some(left) = &mut budget {
            if *left == 0 {
                return Exit::Yield;
            }
            *left -= 1;
        }
        let pc = state.frame.pc;
        let instr = code.instrs.get(pc).copied();
        let result = match instr {
            Some(instr) => {
                state.frame.pc = pc + 1;
                step(state, code, heap, host, instr)
            }
            None => Err(Fault::Internal("instruction position out of range")),
        };
        let (kind, message) = match result {
            Ok(None) => continue,
            Ok(Some(value)) => return Exit::Return(value),
            Err(Fault::Unsupported(m)) => (VmErrorKind::UnsupportedOperand, m),
            Err(Fault::Internal(m)) => (VmErrorKind::Internal, m.to_string()),
            Err(Fault::Host(m)) => (VmErrorKind::HostEffect, m),
            Err(Fault::OutOfMemory(m)) => (VmErrorKind::OutOfMemory, m.to_string()),
        };
        return Exit::Fail(VmError {
            kind,
            message,
            diagnostic: Diagnostic {
                executable: code.name.clone(),
                task: state.diag.task,
                world: state.diag.world,
                instruction: pc,
                byte_offset: code.offsets.get(pc).copied(),
                opcode: instr.map(Instr::opcode),
            },
        });
    }
}
