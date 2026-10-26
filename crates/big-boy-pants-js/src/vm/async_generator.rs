//! Async generators retain their FIFO requests and detached activations in
//! the owning task's generator object. Await reactions are owner-bound jobs;
//! no worker or Rust stack is retained while a promise is pending.
use std::collections::VecDeque;

use super::dispatch::{Fault, Flow};
use super::frames::VmState;
use super::generator::{self, ResumeKind, State};
use super::native::{Action, Input};
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::value::Value;

#[derive(Debug)]
struct Request {
    kind: ResumeKind,
    value: Value,
    promise: ObjectId,
    resolve: Value,
    reject: Value,
}
impl Trace for Request {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.value.trace(visit);
        visit(self.promise.into());
        self.resolve.trace(visit);
        self.reject.trace(visit);
    }
}

#[derive(Debug, Default)]
pub(crate) struct Data {
    queue: VecDeque<Request>,
    queue_reservation: crate::heap::budget::ByteReservation,
    busy: bool,
}
impl Trace for Data {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.queue.iter().for_each(|r| r.trace(visit));
    }
}

pub(crate) fn prototype(heap: &Heap) -> Result<ObjectId, Fault> {
    crate::realm::intrinsics(heap)
        .map(|r| r.async_generator_prototype)
        .ok_or(Fault::OutOfMemory("async generator prototype"))
}

#[derive(Debug)]
enum Stage {
    Pump,
    Running,
    Settling,
    Suspended,
    Invalid,
}

#[derive(Debug)]
pub(crate) struct Boundary {
    pub(crate) id: Option<ObjectId>,
    pub(crate) failed: Option<Value>,
    response: Value,
    request: Option<Request>,
    stage: Stage,
    rejecting: bool,
}
impl Trace for Boundary {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.id.iter().for_each(|id| visit((*id).into()));
        self.failed.iter().for_each(|v| v.trace(visit));
        self.response.trace(visit);
        self.request.iter().for_each(|r| r.trace(visit));
    }
}
impl Boundary {
    pub(crate) fn catches(&self) -> bool {
        !self.rejecting
    }
    pub(crate) fn catches_step(&self) -> bool {
        matches!(self.stage, Stage::Invalid)
    }
    fn new(id: ObjectId, response: Value) -> Self {
        Self {
            id: Some(id),
            response,
            failed: None,
            request: None,
            stage: Stage::Pump,
            rejecting: false,
        }
    }
    fn settle(
        &mut self,
        heap: &Heap,
        task: TaskId,
        value: Value,
        rejected: bool,
        done: bool,
    ) -> Result<Action, Fault> {
        if self.request.is_none() {
            self.request = heap
                .with_object_mut(self.id.unwrap(), |o| match o {
                    HeapObject::Generator(g) => g.asynchronous.as_mut().unwrap().queue.pop_front(),
                    _ => None,
                })
                .flatten();
        }
        let request = self
            .request
            .as_ref()
            .ok_or(Fault::Internal("async generator request missing"))?;
        self.stage = Stage::Settling;
        self.rejecting = rejected;
        Ok(Action::Call {
            callee: if rejected {
                request.reject.clone()
            } else {
                request.resolve.clone()
            },
            this: Value::Undefined,
            args: vec![if rejected {
                value
            } else {
                generator::result(heap, task, value, done)?
            }],
        })
    }
    pub(crate) fn suspended(&mut self) {
        self.stage = Stage::Suspended;
    }
    pub(crate) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if let Some(error) = self.failed.take() {
            if self.rejecting {
                return Err(Fault::Thrown(error));
            }
            if let Some(id) = self.id {
                generator::complete(heap, id);
            }
            return self.settle(heap, task, error, true, true);
        }
        match self.stage {
            Stage::Invalid => {
                return Err(Fault::TypeError(
                    "async generator method requires an async generator receiver".into(),
                ));
            }
            Stage::Suspended => return Ok(Action::Return(self.response.clone())),
            Stage::Settling => {
                self.request = None;
                self.rejecting = false;
                if self.id.is_none() {
                    return Ok(Action::Return(self.response.clone()));
                }
                self.stage = Stage::Pump;
            }
            Stage::Running => {
                let id = self.id.unwrap();
                let suspended = heap.read(id, |o| matches!(o, HeapObject::Generator(g) if matches!(g.state, State::Awaiting(_)))).unwrap_or(false);
                if suspended {
                    return Ok(Action::Return(self.response.clone()));
                }
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("async generator completion missing"));
                };
                let yielded = heap.read(id, |o| matches!(o, HeapObject::Generator(g) if matches!(g.state, State::SuspendedYield(_) | State::SuspendedYieldStar(_)))).unwrap_or(false);
                if !yielded {
                    generator::complete(heap, id);
                }
                return self.settle(heap, task, value, false, !yielded);
            }
            Stage::Pump => {}
        }
        let id = self.id.unwrap();
        let request = heap
            .with_object_mut(id, |o| match o {
                HeapObject::Generator(g) => {
                    let a = g.asynchronous.as_mut().unwrap();
                    if a.queue.is_empty() {
                        a.busy = false;
                    }
                    a.queue.front().map(|r| {
                        (
                            r.kind,
                            r.value.clone(),
                            matches!(g.state, State::Completed),
                            matches!(g.state, State::SuspendedStart(_)),
                        )
                    })
                }
                _ => None,
            })
            .flatten();
        let Some((kind, value, mut completed, start)) = request else {
            return Ok(Action::Return(self.response.clone()));
        };
        if start && kind != ResumeKind::Next {
            generator::complete(heap, id);
            completed = true;
        }
        if completed {
            return match kind {
                ResumeKind::Next => self.settle(heap, task, Value::Undefined, false, true),
                ResumeKind::Throw => self.settle(heap, task, value, true, true),
                ResumeKind::Return => {
                    heap.with_object_mut(id, |o| {
                        if let HeapObject::Generator(g) = o {
                            g.state = State::AwaitingReturn;
                        }
                    });
                    self.stage = Stage::Suspended;
                    Ok(Action::AwaitGeneratorReturn { value })
                }
            };
        }
        self.stage = Stage::Running;
        Ok(Action::ResumeGenerator { id, value, kind })
    }
}

pub(crate) fn enqueue(
    heap: &Heap,
    vm: &mut VmState,
    this: &Value,
    value: Value,
    kind: ResumeKind,
    popped: usize,
) -> Result<(), Fault> {
    let (promise, resolve, reject) = super::promise::capability(heap)?;
    let request = Request {
        kind,
        value,
        promise,
        resolve,
        reject,
    };
    let id = if let Value::Object(id) = this {
        heap.read(*id, |o| match o {
            HeapObject::Generator(g) if g.owner == vm.diag.task && g.asynchronous.is_some() => {
                Some(*id)
            }
            _ => None,
        })
        .flatten()
    } else {
        None
    };
    vm.truncate_operands(vm.stack_len() - popped)?;
    let Some(id) = id else {
        return super::native::async_generator_boundary(
            vm,
            Boundary {
                id: None,
                failed: None,
                response: Value::Object(promise),
                request: Some(request),
                stage: Stage::Invalid,
                rejecting: false,
            },
        );
    };
    let start = heap
        .read(id, |o| match o {
            HeapObject::Generator(g) => !g.asynchronous.as_ref().unwrap().busy,
            _ => unreachable!(),
        })
        .ok_or(Fault::Internal("async generator reclaimed"))?;
    // Admit geometric queue capacity before installing a native boundary.
    let capacity = heap
        .read(id, |o| match o {
            HeapObject::Generator(g) => {
                let queue = &g.asynchronous.as_ref().unwrap().queue;
                if queue.len() == queue.capacity() {
                    queue.capacity().max(4)
                } else {
                    0
                }
            }
            _ => unreachable!(),
        })
        .ok_or(Fault::Internal("async generator reclaimed"))?;
    let charge = heap
        .reserve_bytes(
            capacity
                .checked_mul(std::mem::size_of::<Request>())
                .ok_or(Fault::HeapLimit)?,
        )
        .ok_or(Fault::HeapLimit)?;
    // Reserve the native boundary and catch handler before claiming the
    // queue, so a limit cannot leave it marked busy without a driver.
    if start {
        super::native::async_generator_boundary(vm, Boundary::new(id, Value::Object(promise)))?;
    }
    heap.with_object_mut(id, |o| match o {
        HeapObject::Generator(g) => {
            let a = g.asynchronous.as_mut().unwrap();
            a.queue.push_back(request);
            a.queue_reservation.merge(charge);
            a.busy = true;
        }
        _ => unreachable!(),
    })
    .ok_or(Fault::Internal("async generator reclaimed"))?;
    if !start {
        vm.push(Value::Object(promise));
    }
    Ok(())
}

pub(crate) fn suspend(
    heap: &Heap,
    vm: &mut VmState,
    source: ObjectId,
    id: ObjectId,
) -> Result<Flow, Fault> {
    let awaiting_return = heap
        .read(
            id,
            |o| matches!(o, HeapObject::Generator(g) if matches!(g.state, State::AwaitingReturn)),
        )
        .unwrap_or(false);
    // Admit the saved activation before registering the reaction that
    // resumes it (see `asynchronous::suspend`).
    let reservation = if awaiting_return {
        None
    } else {
        Some(vm.reserve_activation(heap)?)
    };
    super::promise::await_then(heap, &vm.jobs, source, id)?;
    if let Some(reservation) = reservation {
        let activation = vm.detach_reserved_activation(reservation);
        heap.with_object_mut(id, |o| {
            if let HeapObject::Generator(g) = o {
                g.state = State::Awaiting(activation);
            }
        });
    } else {
        super::native::suspend_generator_return(vm)?;
    }
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
    let awaiting_return = heap
        .read(id, |o| match o {
            HeapObject::Generator(g) if g.owner == vm.diag.task => match &g.state {
                State::Awaiting(_) => Ok(false),
                State::AwaitingReturn => Ok(true),
                _ => Err(Fault::Internal("async generator not awaiting")),
            },
            _ => Err(Fault::Internal("foreign async generator")),
        })
        .ok_or(Fault::Internal("async generator reclaimed"))??;
    let mut boundary = Boundary::new(id, Value::Undefined);
    boundary.stage = Stage::Running;
    if awaiting_return {
        generator::complete(heap, id);
        boundary.failed = rejected.then_some(value.clone());
        super::native::async_generator_boundary(vm, boundary)?;
        super::native::generator_awaiting(vm)?;
        vm.push(value);
        return Ok(Flow::FrameChanged);
    }
    // Register the rejection boundary before checking the saved frame's
    // limits. An attachment failure must reject and drain its requests.
    super::native::async_generator_boundary(vm, boundary)?;
    super::native::generator_awaiting(vm)?;
    heap.read(id, |o| match o {
        HeapObject::Generator(g) => match &g.state {
            State::Awaiting(a) => vm.check_activation(a),
            _ => Err(Fault::Internal("async generator not awaiting")),
        },
        _ => Err(Fault::Internal("async generator reclaimed")),
    })
    .ok_or(Fault::Internal("async generator reclaimed"))??;
    let old = heap
        .with_object_mut(id, |o| match o {
            HeapObject::Generator(g) => std::mem::replace(&mut g.state, State::Executing),
            _ => unreachable!(),
        })
        .ok_or(Fault::Internal("async generator reclaimed"))?;
    match old {
        State::Awaiting(a) => {
            vm.attach_activation(a);
            if rejected {
                return Err(Fault::Thrown(value));
            }
            vm.push(value);
        }
        _ => unreachable!(),
    }
    Ok(Flow::FrameChanged)
}
