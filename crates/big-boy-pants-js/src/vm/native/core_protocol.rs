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
    receiver: Value,
    through_lazy: bool,
    called: bool,
    /// A raw JavaScript member read (`jsMember`): the member's name. The
    /// resolver's result is the member itself, not a method to call.
    member: Option<Value>,
}

/// A native form is the same object in JavaScript and in the compiler's
/// runtime: a named member read (`property::Deferred::Form`) asks the task's
/// resolver for the member of `form` (the form found on the base's prototype
/// chain, usually the base itself); the call's `this` stays the base. The
/// resolver may suspend (declaring a cold type, demanding a helper) without
/// replaying the read. Without a resolver a form has no members.
pub(crate) fn start_member(
    state: &mut VmState,
    form: Value,
    key: &crate::vm::property::Key,
) -> Result<(), Fault> {
    let (Some(resolver), crate::vm::property::Key::Name(name)) = (state.core_resolver.clone(), key)
    else {
        state.push(Value::Undefined);
        return Ok(());
    };
    push_native(
        state,
        NativeState::CoreProtocol(State {
            op: FormOp::CljCoreResolver,
            operands: vec![],
            resolver,
            receiver: form,
            through_lazy: false,
            called: true,
            member: Some(Value::String(name.clone())),
        }),
    )
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
            | FormOp::CljWithExportedMeta
            | FormOp::CljList
            | FormOp::CljInvoke
            | FormOp::CljIsList
            | FormOp::CljIsSeq
            | FormOp::CljIsSequential
            | FormOp::CljIsColl
            | FormOp::CljIsVector
            | FormOp::CljIsMapEntry
            | FormOp::CljPeek
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
    let mut receiver = args[0].clone();
    // An array sequence cursor is task-owned. Equality must also offer a
    // cursor on the right to the adapter, without reversing its operands.
    if op == FormOp::Equals
        && form::kind(heap, &form::unwrap(heap, receiver.clone())) != Some(FormKind::Struct)
        && form::kind(heap, &form::unwrap(heap, args[1].clone())).is_none()
        && matches!(form::unwrap(heap, args[1].clone()), Value::Object(_))
    {
        receiver = args[1].clone();
    }
    if op == FormOp::CljInvoke {
        if !matches!(
            form::kind(heap, &form::unwrap(heap, args[0].clone())),
            Some(FormKind::Keyword | FormKind::Symbol)
        ) || !matches!(args[3], Value::Number(1.0 | 2.0))
        {
            return Ok(false);
        }
        receiver = args[1].clone();
    }
    // A realized lazy wrapper can expose a user sequence. Keep the original
    // operands and let its task-local adapter perform the sequence operation;
    // unresolved cells still enter the ordinary realization frame first.
    let through_lazy = matches!(
        op,
        FormOp::CljSeq
            | FormOp::CljFirst
            | FormOp::CljRest
            | FormOp::CljNext
            | FormOp::CljCount
            | FormOp::CljList
            | FormOp::CljNth
            | FormOp::CljNthOr
    ) && form::kind(heap, &form::unwrap(heap, receiver.clone()))
        == Some(FormKind::Lazy);
    if through_lazy {
        receiver = match form::protocol_head(
            heap,
            receiver,
            &mut crate::collections::lazy::Visited::default(),
        ) {
            Ok(head) => head,
            // The leaf preserves pending-cell and cycle diagnostics.
            Err(_) => return Ok(false),
        };
    }
    // Predicates never follow lazy values. Builtin markers are leaf checks;
    // other receivers may satisfy a user marker through a host/default
    // extension as well as an immutable or mutable user type.
    let builtin_marker = match op {
        FormOp::CljIsList => Some(crate::collections::clj::is_list(heap, &receiver)),
        FormOp::CljIsSeq => Some(crate::collections::clj::is_seq(heap, &receiver)),
        FormOp::CljIsSequential => Some(crate::collections::clj::is_sequential(heap, &receiver)),
        FormOp::CljIsColl => Some(crate::collections::clj::is_coll(heap, &receiver)),
        FormOp::CljIsVector => Some(crate::collections::clj::is_vector(heap, &receiver)),
        FormOp::CljIsMapEntry => Some(crate::collections::clj::is_map_entry(heap, &receiver)),
        _ => None,
    };
    let receiver_kind = form::kind(heap, &form::unwrap(heap, receiver.clone()));
    // Public peek bypasses nil dispatch. Other receivers can have explicit
    // IStack extensions, including core collections and primitive strings.
    let peek_adapter = op == FormOp::CljPeek
        && !matches!(
            form::unwrap(heap, receiver.clone()),
            Value::Null | Value::Undefined
        );
    let array_adapter = receiver_kind.is_none()
        && matches!(form::unwrap(heap, receiver.clone()), Value::Object(_))
        && matches!(
            op,
            FormOp::CljSeq
                | FormOp::Equals
                | FormOp::Hash
                | FormOp::CljFirst
                | FormOp::CljRest
                | FormOp::CljNext
                | FormOp::CljMeta
                | FormOp::CljWithMeta
                | FormOp::CljWithExportedMeta
                | FormOp::CljCount
                | FormOp::CljList
                | FormOp::CljPeek
        );
    if builtin_marker == Some(true)
        || (builtin_marker.is_none()
            && !array_adapter
            && !peek_adapter
            && receiver_kind != Some(FormKind::Struct))
    {
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
            receiver,
            through_lazy,
            called: false,
            member: None,
        }),
    )?;
    Ok(true)
}

impl State {
    pub(super) fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.resolver.trace(visit);
        self.receiver.trace(visit);
        self.operands.iter().for_each(|value| value.trace(visit));
        if let Some(member) = &self.member {
            member.trace(visit);
        }
    }

    pub(super) fn step(&mut self, input: Input) -> Result<Action, Fault> {
        match input {
            Input::Start => Ok(Action::CallArray {
                callee: self.resolver.clone(),
                this: Value::Undefined,
                args: [
                    Value::String(
                        match self.member {
                            Some(_) => "jsMember",
                            None => self.op.intrinsic(),
                        }
                        .into(),
                    ),
                    self.receiver.clone(),
                    Value::Bool(self.through_lazy),
                    self.member.clone().unwrap_or(Value::Undefined),
                ],
                argc: if self.member.is_some() { 4 } else { 3 },
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
