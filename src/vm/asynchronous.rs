//! Async functions use the same movable activation as generators. Only an
//! internal promise reaction retains a detached activation; task diagnostics
//! record names and identities without tracing them.
use super::dispatch::{Fault, Flow};
use super::frames::{DetachedActivation, VmState};
use super::native::{Action, Input};
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::Value;

#[derive(Debug)]
pub(crate) struct AsyncState {
    pub(crate) owner: TaskId,
    pub(crate) activation: Option<DetachedActivation>,
    promise: ObjectId,
    resolve: Value,
    reject: Value,
}
impl Trace for AsyncState {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.activation.iter().for_each(|a| a.trace(visit));
        visit(self.promise.into());
        self.resolve.trace(visit);
        self.reject.trace(visit);
    }
}

#[derive(Debug)]
pub(crate) struct Boundary {
    pub(crate) id: ObjectId,
    pub(crate) failed: Option<Value>,
    settling: bool,
    rejecting: bool,
    initial: bool,
}
impl Trace for Boundary {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        visit(self.id.into());
        self.failed.iter().for_each(|v| v.trace(visit));
    }
}
impl Boundary {
    pub(crate) fn step(&mut self, heap: &Heap, input: Input) -> Result<Action, Fault> {
        let (promise, resolve, reject, suspended) = heap
            .read(self.id, |o| match o {
                HeapObject::AsyncState(a) => (
                    a.promise,
                    a.resolve.clone(),
                    a.reject.clone(),
                    a.activation.is_some(),
                ),
                _ => unreachable!(),
            })
            .ok_or(Fault::Internal("async activation reclaimed"))?;
        if let Some(value) = self.failed.take() {
            // Resolver entry itself can fail (e.g. the native-frame limit).
            // Reject the still-unclaimed capability rather than silently
            // returning a pending promise. A failed reject must propagate.
            if self.rejecting {
                return Err(Fault::Thrown(value));
            }
            self.settling = true;
            self.rejecting = true;
            return Ok(Action::Call {
                callee: reject,
                this: Value::Undefined,
                args: vec![value],
            });
        }
        if !suspended && !self.settling {
            let Input::CallResult(value) = input else {
                return Err(Fault::Internal("async completion missing"));
            };
            self.settling = true;
            return Ok(Action::Call {
                callee: resolve,
                this: Value::Undefined,
                args: vec![value],
            });
        }
        Ok(Action::Return(if self.initial {
            Value::Object(promise)
        } else {
            Value::Undefined
        }))
    }
}

pub(crate) fn start(heap: &Heap, vm: &mut VmState) -> Result<Flow, Fault> {
    let (promise, resolve, reject) = super::promise::capability(heap)?;
    let saved = vm.detach_activation()?;
    let id = heap
        .alloc(HeapObject::AsyncState(Box::new(AsyncState {
            owner: vm.diag.task,
            activation: None,
            promise,
            resolve,
            reject,
        })))
        .ok_or(Fault::OutOfMemory("async activation"))?;
    super::native::async_boundary(
        vm,
        Boundary {
            id,
            failed: None,
            settling: false,
            rejecting: false,
            initial: true,
        },
    )?;
    vm.attach_activation(saved);
    Ok(Flow::FrameChanged)
}

pub(crate) fn await_value(heap: &Heap, vm: &mut VmState) -> Result<Flow, Fault> {
    if vm.activation_cancelling() {
        return Ok(if vm.unwind_cancel(heap) {
            Flow::FrameChanged
        } else {
            Flow::CleanupEnded
        });
    }
    let value = vm.pop()?;
    super::native::await_promise(vm, super::promise::await_state(heap, value)?)?;
    Ok(Flow::FrameChanged)
}

pub(crate) fn suspend(heap: &Heap, vm: &mut VmState, source: ObjectId) -> Result<Flow, Fault> {
    let id = super::native::async_id(vm).ok_or(Fault::Internal("await outside async function"))?;
    // Register while still attached. There is no safepoint until the saved
    // activation is installed below. Even an already fulfilled source queues.
    super::promise::await_then(heap, &vm.jobs, source, id)?;
    let name = vm.top().code.function_name.to_string_lossy();
    let saved = vm.detach_activation()?;
    heap.with_object_mut(id, |o| {
        if let HeapObject::AsyncState(a) = o {
            a.activation = Some(saved);
        }
    })
    .ok_or(Fault::Internal("async activation reclaimed"))?;
    vm.suspended_async.push((id, name));
    vm.push(Value::Undefined);
    Ok(Flow::FrameChanged)
}

pub(crate) fn resume(
    heap: &Heap,
    vm: &mut VmState,
    id: ObjectId,
    rejected: bool,
    value: Value,
) -> Result<Flow, Fault> {
    heap.read(id, |o| match o {
        HeapObject::AsyncState(a) if a.owner == vm.diag.task => a
            .activation
            .as_ref()
            .map_or(Ok(()), |a| vm.check_activation(a)),
        _ => Err(Fault::Internal("foreign async activation")),
    })
    .ok_or(Fault::Internal("async activation reclaimed"))??;
    let saved = heap
        .with_object_mut(id, |o| match o {
            HeapObject::AsyncState(a) => a.activation.take(),
            _ => None,
        })
        .flatten()
        .ok_or(Fault::Internal("async activation not suspended"))?;
    super::native::async_boundary(
        vm,
        Boundary {
            id,
            failed: None,
            settling: false,
            rejecting: false,
            initial: false,
        },
    )?;
    vm.suspended_async.retain(|(other, _)| *other != id);
    vm.attach_activation(saved);
    if rejected {
        return Err(Fault::Thrown(value));
    }
    vm.push(value);
    Ok(Flow::FrameChanged)
}
