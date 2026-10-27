//! Core-protocol calls on native structs. Resolution belongs to the task's
//! declaration frontier; shared forms, map keys and dependency tables never
//! retain or execute it. Both resolution and the selected entry may suspend.
use super::{Action, Fault, Input, NativeState, VmState, push_native};
use crate::collections::form::{self, FormKind};
use crate::heap::{
    Heap,
    gc::{Trace, TraceEdge},
};
use crate::value::Value;
use crate::vm::instr::{FormOp, Instr};

#[derive(Debug)]
pub(super) struct State {
    op: FormOp,
    operands: Vec<Value>,
    resolver: Value,
    called: bool,
}

pub(crate) fn start(state: &mut VmState, heap: &Heap, mut op: FormOp) -> Result<bool, Fault> {
    if !matches!(
        op,
        FormOp::Equals
            | FormOp::Hash
            | FormOp::CljCount
            | FormOp::CljSeq
            | FormOp::CljFirst
            | FormOp::CljRest
            | FormOp::CljNext
            | FormOp::CljNth
            | FormOp::CljNthOr
            | FormOp::CljGet
            | FormOp::CljGet2
            | FormOp::CljContains
            | FormOp::CljAssoc
            | FormOp::CljDissoc
            | FormOp::CljConj
            | FormOp::CljDisj
            | FormOp::CljMeta
            | FormOp::CljWithMeta
            | FormOp::CljList
            | FormOp::CljInvoke
    ) {
        return Ok(false);
    }
    let Some(resolver) = state.core_resolver.clone() else {
        return Ok(false);
    };
    let n = op.arity();
    let start = state
        .operands()
        .len()
        .checked_sub(n)
        .ok_or(Fault::Internal("core protocol operand underflow"))?;
    let args = &state.operands()[start..];
    let mut receiver = &args[0];
    if op == FormOp::CljInvoke {
        if !matches!(
            form::kind(heap, &form::unwrap(heap, args[0].clone())),
            Some(FormKind::Keyword | FormKind::Symbol)
        ) || !matches!(args[3], Value::Number(1.0 | 2.0))
        {
            return Ok(false);
        }
        receiver = &args[1];
    }
    if form::kind(heap, &form::unwrap(heap, receiver.clone())) != Some(FormKind::Struct) {
        return Ok(false);
    }
    // Language equality keeps its identity shortcut, even when -equiv rejects
    // its own receiver. Internal structural comparison never enters this path.
    if op == FormOp::Equals
        && form::unwrap(heap, args[0].clone()) == form::unwrap(heap, args[1].clone())
    {
        return Ok(false);
    }
    let mut operands = args.to_vec();
    if op == FormOp::CljInvoke {
        operands.swap(0, 1);
        op = if operands[3] == Value::Number(1.0) {
            FormOp::CljGet2
        } else {
            FormOp::CljGet
        };
        operands.truncate(op.arity());
    }
    for _ in 0..n {
        state.pop()?;
    }
    push_native(
        state,
        NativeState::CoreProtocol(State {
            op,
            operands,
            resolver,
            called: false,
        }),
    )?;
    Ok(true)
}

impl State {
    pub(super) fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.resolver.trace(visit);
        self.operands.iter().for_each(|value| value.trace(visit));
    }

    pub(super) fn step(&mut self, input: Input) -> Result<Action, Fault> {
        match input {
            Input::Start => Ok(Action::CallArray {
                callee: self.resolver.clone(),
                this: Value::Undefined,
                args: [
                    Value::String(self.op.intrinsic().into()),
                    self.operands[0].clone(),
                    Value::Undefined,
                    Value::Undefined,
                ],
                argc: 2,
            }),
            Input::CallResult(value) if self.called => Ok(Action::Return(value)),
            Input::CallResult(Value::Null | Value::Undefined) => Ok(Action::FormFallback {
                instr: Instr::FormOp(self.op),
                operands: std::mem::take(&mut self.operands),
            }),
            Input::CallResult(method) => {
                self.called = true;
                let mut args = [
                    Value::Undefined,
                    Value::Undefined,
                    Value::Undefined,
                    Value::Undefined,
                ];
                args[..self.operands.len()].clone_from_slice(&self.operands);
                Ok(Action::CallArray {
                    callee: method,
                    this: self.operands[0].clone(),
                    args,
                    argc: self.operands.len(),
                })
            }
            _ => Err(Fault::Internal("unexpected core protocol input")),
        }
    }
}
