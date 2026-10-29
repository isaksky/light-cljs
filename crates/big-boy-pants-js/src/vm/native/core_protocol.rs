//! Core-protocol calls on native structs. Resolution belongs to the task's
//! declaration frontier; shared forms, map keys and dependency tables never
//! retain or execute it. Both resolution and the selected entry may suspend.
//!
//! The same frame answers a structural comparison of a map with a
//! `sorted-map-by` map met inside a form instruction (light-cljs-561): the
//! runtime's `=` for the sorted map runs on the task VM (its comparator may
//! demand a helper and park), and the instruction executes again with the
//! answer and every earlier one ([`start_sorted_equality`]).
use super::{Action, Fault, Input, NativeState, VmState, push_native};
use crate::collections::form::{self, FormKind, SortedAnswers, SortedEquality, SortedRequest};
use crate::heap::{
    Heap,
    gc::{Trace, TraceEdge},
};
use crate::ids::TaskId;
use crate::value::Value;
use crate::vm::exception::Exception;
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
    /// Set for a sorted-map-by comparison: the instruction to run again.
    retry: Option<Box<Retry>>,
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
            retry: None,
        }),
    )
}

#[derive(Debug)]
struct Retry {
    instr: Instr,
    operands: Vec<Value>,
    /// The instruction's struct protocol already resolved: execute its leaf
    /// directly again, as a realization does.
    core_resolved: bool,
    answers: SortedAnswers,
    /// Met inside a set comparison: an `Error` the runtime's `=` throws is
    /// caught and answers that the set comparison is unequal, as the legacy
    /// set `equiv` does; anything else thrown propagates.
    in_set: bool,
    /// What the runtime's `=` threw (the native catch delivers it).
    failed: Option<Exception>,
}

/// Ask the task runtime's `=` whether map `left` equals `sorted-map-by` map
/// `right` for `instr`, whose `n` operands move into the new frame;
/// `answers` are the comparisons it was already given.
pub(crate) fn start_sorted_equality(
    state: &mut VmState,
    instr: Instr,
    n: usize,
    request: SortedRequest,
    answers: SortedAnswers,
) -> Result<(), Fault> {
    let SortedRequest {
        left,
        right,
        in_set,
    } = request;
    let resolver = state.core_resolver.clone().ok_or(Fault::Internal(
        "sorted-map-by comparison without a runtime",
    ))?;
    let at = state
        .operands()
        .len()
        .checked_sub(n)
        .ok_or(Fault::Internal("operand stack underflow"))?;
    let operands = state.operands()[at..].to_vec();
    for _ in 0..n {
        state.pop()?;
    }
    push_native(
        state,
        NativeState::CoreProtocol(State {
            op: FormOp::Equals,
            operands: vec![left, right.clone()],
            resolver,
            receiver: right,
            through_lazy: false,
            called: false,
            member: None,
            retry: Some(Box::new(Retry {
                instr,
                operands,
                core_resolved: false,
                answers,
                in_set,
                failed: None,
            })),
        }),
    )
}

/// Mark a sorted-map-by comparison frame that a core operation's leaf
/// started after its resolution (see [`Retry::core_resolved`]).
pub(super) fn mark_core_resolved(state: &mut State) {
    if let Some(retry) = &mut state.retry {
        retry.core_resolved = true;
    }
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
    // A sorted-map-by map orders its keys with a comparator only the task's
    // runtime calls: its lookups, updates and `=` go to the resolver too.
    let sorted_by = |v: &Value| {
        form::form_object(heap, &form::unwrap(heap, v.clone()))
            .is_some_and(|f| f.comparator().is_some())
    };
    // A sorted-map-by map called as a function: a lookup in that map.
    let invoked_map = op == FormOp::CljInvoke
        && sorted_by(&args[0])
        && matches!(args[3], Value::Number(1.0 | 2.0));
    if op == FormOp::CljInvoke && !invoked_map {
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
    // `=` offers a sorted-map-by right operand to the resolver unless a
    // struct's -equiv on the left takes precedence. Operand order is kept.
    let receiver_sorted = matches!(
        op,
        FormOp::CljGet
            | FormOp::CljGet2
            | FormOp::CljContains
            | FormOp::CljAssoc
            | FormOp::CljDissoc
            | FormOp::CljConj
            | FormOp::CljInvoke
            | FormOp::Equals
    ) && receiver_kind == Some(FormKind::Map)
        && sorted_by(&receiver);
    let right_sorted = !receiver_sorted
        && op == FormOp::Equals
        && receiver_kind != Some(FormKind::Struct)
        && sorted_by(&args[1]);
    if right_sorted {
        receiver = args[1].clone();
    }
    let sorted_receiver = receiver_sorted || right_sorted;
    if builtin_marker == Some(true)
        || (builtin_marker.is_none()
            && !array_adapter
            && !peek_adapter
            && !sorted_receiver
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
        if !invoked_map {
            operands.swap(0, 1);
        }
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
            retry: None,
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
        if let Some(retry) = &self.retry {
            retry.operands.iter().for_each(|value| value.trace(visit));
            retry.answers.trace(visit);
            if let Some(e) = &retry.failed {
                e.value.trace(visit);
            }
        }
    }

    /// Whether a native catch must be installed around the runtime's `=`
    /// (only for a comparison met inside a set comparison).
    pub(super) fn catches(&self) -> bool {
        self.called && self.retry.as_ref().is_some_and(|retry| retry.in_set)
    }

    /// The runtime's `=` threw (the native catch delivers it).
    pub(super) fn failed(&mut self, exception: Exception) {
        if let Some(retry) = &mut self.retry {
            retry.failed = Some(exception);
        }
    }

    /// Record `equal` as the next answer, charged before the buffer grows,
    /// and execute the instruction again.
    fn answer(&mut self, heap: &Heap, equal: Option<bool>) -> Result<Action, Fault> {
        let mut retry = self
            .retry
            .take()
            .ok_or(Fault::Internal("sorted-map-by answer without a retry"))?;
        let answers = &mut retry.answers;
        answers.charge.admit(
            heap,
            answers.items.len(),
            std::mem::size_of::<SortedEquality>(),
        )?;
        answers.items.try_reserve(1).map_err(|_| Fault::HeapLimit)?;
        answers.items.push(SortedEquality {
            left: self.operands[0].clone(),
            right: self.operands[1].clone(),
            equal,
        });
        Ok(Action::Retry {
            instr: retry.instr,
            operands: retry.operands,
            answers: retry.answers,
            fallback: retry.core_resolved,
        })
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if let Some(exception) = self.retry.as_mut().and_then(|retry| retry.failed.take()) {
            // Legacy `catch (error) { if (error instanceof Error) return false; throw error; }`.
            let prototype = crate::realm::intrinsics(heap)
                .map(|intrinsics| Value::Object(intrinsics.error_prototype));
            let error = match prototype {
                Some(prototype) => crate::vm::prototype::instance_of_prototype(
                    heap,
                    task,
                    &exception.value,
                    &prototype,
                )
                .unwrap_or(false),
                None => false,
            };
            if !error {
                return Err(Fault::Rethrow(exception));
            }
            return self.answer(heap, None);
        }
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
            Input::CallResult(value) if self.called => {
                if self.retry.is_none() {
                    return Ok(Action::Return(value));
                }
                let equal = !matches!(value, Value::Null | Value::Undefined | Value::Bool(false));
                self.answer(heap, Some(equal))
            }
            // The runtime must answer a sorted-map-by comparison: executing
            // the instruction again without an answer would not end.
            Input::CallResult(Value::Null | Value::Undefined) if self.retry.is_some() => {
                Err(Fault::Error(
                    "unsupported operation: native nested equality with a sorted-map-by map".into(),
                ))
            }
            Input::CallResult(Value::Null | Value::Undefined) => Ok(Action::FormFallback {
                instr: Instr::FormOp(self.op),
                operands: std::mem::take(&mut self.operands),
            }),
            Input::CallResult(method) => {
                self.called = true;
                // The array-call fast path installs no native catch.
                if self.catches() {
                    return Ok(Action::Call {
                        callee: method,
                        this: self.operands[0].clone(),
                        args: self.operands.clone(),
                    });
                }
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
