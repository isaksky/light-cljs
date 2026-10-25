//! Shareable single-assignment promises. State changes use the heap's short
//! metadata lock; reaction callbacks stay opaque, bound to their registering
//! task's inbox. Publication never runs JavaScript or wakes dependency waits.
use std::sync::Weak;

use super::builtin::{Builtin, BuiltinFunction};
use super::dispatch::{self, Fault};
use super::native::{Action, Input};
use super::property::{self, Key, Read};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::job::{Job, JobInbox};
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{JsString, Value, share};

pub(crate) mod standard;

#[derive(Debug)]
pub(crate) struct Promise {
    pub(crate) prototype: ObjectId,
    pub(crate) owner: Option<TaskId>,
    result: Option<Settlement>,
    reactions: Vec<Registration>,
    is_handled: bool,
    // The rejecting task owns the bounded diagnostic. This weak receipt
    // permits handling on a different task without retaining the owner.
    rejection: Option<(Weak<JobInbox>, bool)>,
}

/// A fresh resolve/reject pair shares this concurrent first-call guard.
/// It only points to a shareable Promise, never to task-owned callbacks.
#[derive(Debug)]
pub(crate) struct Resolving {
    promise: ObjectId,
    resolved: bool,
    pub(crate) owner: Option<TaskId>,
}
impl Trace for Resolving {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        visit(self.promise.into());
    }
}

/// An unpublishable result stays in an opaque owner-bound envelope, just as
/// a mutable reaction closure does. Foreign tasks cannot extract that value.
#[derive(Clone, Debug)]
struct Settlement {
    rejected: bool,
    value: Value,
    owner: Option<TaskId>,
}
#[derive(Debug)]
struct Registration {
    inbox: Weak<JobInbox>,
    callable: Value,
}
impl Registration {
    fn enqueue(&self) {
        if let Some(inbox) = self.inbox.upgrade() {
            // Zero arguments: the reaction obtains its result through the
            // promise's checked read, never through unchecked publication.
            let _ = inbox.push(Job {
                callable: self.callable.clone(),
                args: vec![],
            });
        }
    }
}
impl Trace for Promise {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        visit(self.prototype.into());
        if let Some(result) = &self.result {
            result.value.trace(visit);
        }
        for reaction in &self.reactions {
            reaction.callable.trace(visit);
        }
    }
}

fn check_owner(owner: Option<TaskId>, task: TaskId) -> Result<(), Fault> {
    if owner.is_some_and(|owner| owner != task) {
        return Err(Fault::ForeignObject {
            owner,
            caller: task,
        });
    }
    Ok(())
}
impl Promise {
    pub(crate) fn check_owner(&self, task: TaskId) -> Result<(), Fault> {
        check_owner(self.owner, task)
    }
}

pub(crate) fn constructor(
    heap: &Heap,
    task: TaskId,
    args: &[Value],
    prototype: ObjectId,
) -> Result<PromiseState, Fault> {
    let executor = args.first().cloned().unwrap_or(Value::Undefined);
    dispatch::callee(heap, &executor)?;
    let intrinsic = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm exhausted"))?
        .promise_prototype;
    let owner = (prototype != intrinsic).then_some(task);
    let promise = new_with_prototype(heap, prototype, owner)?;
    let (guard, resolve, reject) = resolving_functions(heap, promise)?;
    Ok(PromiseState::Constructor {
        promise,
        guard,
        executor,
        resolve,
        reject,
        called: false,
    })
}

#[derive(Clone, Debug)]
pub(crate) struct Reaction {
    source: ObjectId,
    derived: ObjectId,
    fulfilled: Value,
    rejected: Value,
    // An engine resolve/reject pair forwards to another Promise. If the
    // source result is inaccessible, reject that target locally without
    // exposing the foreign value. The forwarding reaction returns undefined.
    forwarding: Option<ObjectId>,
    async_state: Option<ObjectId>,
    capability: Option<standard::Capability>,
}
impl Trace for Reaction {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        visit(self.source.into());
        visit(self.derived.into());
        self.fulfilled.trace(visit);
        self.rejected.trace(visit);
        if let Some(guard) = self.forwarding {
            visit((guard).into());
        }
        if let Some(id) = self.async_state {
            visit(id.into());
        }
        if let Some(cap) = &self.capability {
            cap.trace(visit);
        }
    }
}
fn alloc(heap: &Heap, object: HeapObject) -> Result<ObjectId, Fault> {
    heap.alloc(object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}
fn new(heap: &Heap) -> Result<ObjectId, Fault> {
    let prototype = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm exhausted"))?
        .promise_prototype;
    new_with_prototype(heap, prototype, None)
}
fn new_with_prototype(
    heap: &Heap,
    prototype: ObjectId,
    owner: Option<TaskId>,
) -> Result<ObjectId, Fault> {
    alloc(
        heap,
        HeapObject::Promise(Promise {
            prototype,
            owner,
            result: None,
            reactions: vec![],
            is_handled: false,
            rejection: None,
        }),
    )
}
fn receiver(heap: &Heap, value: &Value) -> Result<ObjectId, Fault> {
    if let Value::Object(id) = value
        && heap
            .read(*id, |o| matches!(o, HeapObject::Promise(_)))
            .unwrap_or(false)
    {
        return Ok(*id);
    }
    Err(Fault::TypeError("Promise method requires a promise".into()))
}
fn function(heap: &Heap, which: Builtin) -> Result<Value, Fault> {
    let prototype = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm exhausted"))?
        .function_prototype;
    let mut properties = OrdinaryObject::intrinsic(Some(prototype));
    for (name, value) in [
        ("name", Value::String(JsString::from(which.name()))),
        ("length", Value::Number(f64::from(which.length()))),
    ] {
        let key = heap
            .keys()
            .intern(&JsString::from(name))
            .ok_or(Fault::OutOfMemory("key table exhausted"))?;
        properties.define_with(key, value, PropertyFlags::FUNCTION_INFO);
    }
    Ok(Value::Object(alloc(
        heap,
        HeapObject::Builtin(BuiltinFunction { which, properties }),
    )?))
}
fn resolving_functions(heap: &Heap, promise: ObjectId) -> Result<(ObjectId, Value, Value), Fault> {
    let owner = heap
        .read(promise, |o| match o {
            HeapObject::Promise(p) => p.owner,
            _ => None,
        })
        .ok_or(Fault::Internal("promise reclaimed"))?;
    let guard = alloc(
        heap,
        HeapObject::PromiseResolving(Resolving {
            promise,
            resolved: false,
            owner,
        }),
    )?;
    let resolve = function(heap, Builtin::PromiseResolver(guard, false))?;
    let reject = function(heap, Builtin::PromiseResolver(guard, true))?;
    Ok((guard, resolve, reject))
}
fn claim(heap: &Heap, task: TaskId, id: ObjectId) -> Result<Option<ObjectId>, Fault> {
    claim_with_capacity(heap, task, id, true)
}

fn claim_with_capacity(
    heap: &Heap,
    task: TaskId,
    id: ObjectId,
    capacity: bool,
) -> Result<Option<ObjectId>, Fault> {
    heap.with_object_mut(id, |o| match o {
        HeapObject::PromiseResolving(p) => {
            check_owner(p.owner, task)?;
            if p.resolved {
                return Ok(None);
            }
            // Check before the first-call guard is consumed. Native entry
            // cannot change between here and push_native on this task.
            if !capacity {
                return Err(Fault::StackOverflow(
                    "maximum native call depth exceeded".into(),
                ));
            }
            p.resolved = true;
            Ok(Some(p.promise))
        }
        _ => Err(Fault::Internal("promise changed kind")),
    })
    .ok_or(Fault::Internal("promise reclaimed"))?
}
fn forwarding_guard(heap: &Heap, fulfilled: &Value, rejected: &Value) -> Option<ObjectId> {
    let resolver = |value: &Value| {
        let Value::Object(id) = value else {
            return None;
        };
        heap.read(*id, |o| match o {
            HeapObject::Builtin(b) => match b.which {
                Builtin::PromiseResolver(guard, reject) => Some((guard, reject)),
                _ => None,
            },
            _ => None,
        })
        .flatten()
    };
    match (resolver(fulfilled), resolver(rejected)) {
        (Some((a, false)), Some((b, true))) if a == b => Some(a),
        _ => None,
    }
}
fn settle(
    heap: &Heap,
    task: TaskId,
    inbox: &Weak<JobInbox>,
    id: ObjectId,
    rejected: bool,
    value: Value,
) -> Result<(), Fault> {
    // Validate outside the object lock. Owner-bound storage permits normal
    // same-task object/error results without granting foreign access to them.
    let owner = share::check(heap, &value).err().map(|_| task);
    heap.with_object_mut(id, |o| match o {
        HeapObject::Promise(p) => {
            if p.result.is_none() {
                p.result = Some(Settlement {
                    rejected,
                    value,
                    owner,
                });
                if rejected && !p.is_handled {
                    p.rejection = inbox.upgrade().and_then(|queue| {
                        queue
                            .reject(id, p.result.as_ref().unwrap().value.clone())
                            .map(|stored| (inbox.clone(), stored))
                    });
                }
                // Keep the enqueue sequence within the settlement/registration
                // linearization point. Inbox push is a leaf metadata operation.
                for r in p.reactions.drain(..) {
                    r.enqueue();
                }
            }
            Ok(())
        }
        _ => Err(Fault::Internal("promise changed kind")),
    })
    .ok_or(Fault::Internal("promise reclaimed"))?
}
#[derive(Debug)]
pub(crate) enum PromiseState {
    Standard(Box<standard::State>),
    AsyncRejectTypeError {
        id: ObjectId,
        called: bool,
    },
    Await {
        promise: ObjectId,
        resolve: Value,
        value: Value,
        called: bool,
    },
    RejectTypeError {
        promise: ObjectId,
        forwarding: Option<ObjectId>,
        message: &'static str,
        called: bool,
    },
    IntrinsicResolve {
        value: Value,
        awaiting: bool,
        reading: bool,
    },
    Static {
        promise: ObjectId,
        resolve: Value,
        value: Value,
        called: bool,
    },
    Constructor {
        promise: ObjectId,
        guard: ObjectId,
        executor: Value,
        resolve: Value,
        reject: Value,
        called: bool,
    },
    Thenable {
        promise: ObjectId,
        guard: ObjectId,
        thenable: Value,
        then: Value,
        resolve: Value,
        reject: Value,
        called: bool,
    },
    Resolve {
        promise: ObjectId,
        value: Value,
        stage: u8,
    },
    Reaction {
        reaction: Reaction,
        called: bool,
    },
    Catch {
        this: Value,
        handler: Value,
        stage: u8,
    },
    Failed {
        promise: ObjectId,
        guard: Option<ObjectId>,
        value: Value,
        constructor: bool,
    },
}
impl Trace for PromiseState {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Standard(state) => state.trace(visit),
            Self::AsyncRejectTypeError { id, .. } => visit((*id).into()),
            Self::Await {
                promise,
                resolve,
                value,
                ..
            } => {
                visit((*promise).into());
                resolve.trace(visit);
                value.trace(visit);
            }
            Self::RejectTypeError {
                promise,
                forwarding,
                ..
            } => {
                visit((*promise).into());
                if let Some(guard) = forwarding {
                    visit((*guard).into());
                }
            }
            Self::IntrinsicResolve { value, .. } => value.trace(visit),
            Self::Static {
                promise,
                resolve,
                value,
                ..
            } => {
                visit((*promise).into());
                resolve.trace(visit);
                value.trace(visit);
            }
            Self::Constructor {
                promise,
                guard,
                executor,
                resolve,
                reject,
                ..
            } => {
                visit((*promise).into());
                visit((*guard).into());
                executor.trace(visit);
                resolve.trace(visit);
                reject.trace(visit);
            }
            Self::Thenable {
                promise,
                guard,
                thenable,
                then,
                resolve,
                reject,
                ..
            } => {
                visit((*promise).into());
                visit((*guard).into());
                thenable.trace(visit);
                then.trace(visit);
                resolve.trace(visit);
                reject.trace(visit);
            }
            Self::Failed {
                promise,
                guard,
                value,
                ..
            } => {
                visit((*promise).into());
                if let Some(guard) = guard {
                    visit((*guard).into());
                }
                value.trace(visit);
            }
            Self::Resolve { promise, value, .. } => {
                visit((*promise).into());
                value.trace(visit);
            }
            Self::Reaction { reaction, .. } => reaction.trace(visit),
            Self::Catch { this, handler, .. } => {
                this.trace(visit);
                handler.trace(visit);
            }
        }
    }
}
impl PromiseState {
    pub(crate) fn catches(&self) -> bool {
        if let Self::Standard(state) = self {
            return state.catches();
        }
        matches!(
            self,
            Self::Constructor { .. }
                | Self::Resolve { .. }
                | Self::Reaction { .. }
                | Self::Thenable { .. }
        )
    }
    pub(crate) fn failed(&mut self, value: Value) {
        if let Self::Standard(state) = self {
            state.failed(value);
            return;
        }
        let (promise, guard, constructor) = match self {
            Self::Constructor { promise, guard, .. } => (*promise, Some(*guard), true),
            Self::Thenable { promise, guard, .. } => (*promise, Some(*guard), false),
            Self::Resolve { promise, .. } => (*promise, None, false),
            Self::Reaction { reaction, .. } => (reaction.derived, None, false),
            _ => unreachable!("non-catching promise native"),
        };
        *self = Self::Failed {
            promise,
            guard,
            value,
            constructor,
        };
    }
    pub(crate) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        inbox: &Weak<JobInbox>,
        input: Input,
    ) -> Result<Action, Fault> {
        match self {
            Self::Standard(state) => state.step(heap, task, inbox, input),
            Self::IntrinsicResolve {
                value,
                awaiting,
                reading,
            } => {
                if !*reading {
                    *reading = true;
                    return Ok(Action::Instruction {
                        instr: super::instr::Instr::GetElem,
                        operands: vec![value.clone(), Value::String("constructor".into())],
                    });
                }
                let Input::CallResult(constructor) = input else {
                    return Err(Fault::Internal("PromiseResolve constructor missing"));
                };
                let intrinsic = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("realm exhausted"))?
                    .global(crate::realm::Global::Promise);
                if crate::atom::same_value(&constructor, &intrinsic) {
                    return if *awaiting {
                        Ok(Action::SuspendAsync(receiver(heap, value)?))
                    } else {
                        Ok(Action::Return(value.clone()))
                    };
                }
                *self = resolve_fresh(heap, value.clone(), *awaiting)?;
                Ok(Action::Continue)
            }
            Self::AsyncRejectTypeError { id, called } => {
                if !*called {
                    *called = true;
                    let callee = crate::realm::intrinsics(heap)
                        .ok_or(Fault::OutOfMemory("realm exhausted"))?
                        .global(crate::realm::Global::TypeError);
                    return Ok(Action::Call {
                        callee,
                        this: Value::Undefined,
                        args: vec![Value::String(
                            "promise result belongs to another task".into(),
                        )],
                    });
                }
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("missing async TypeError"));
                };
                Ok(Action::ResumeAsync {
                    id: *id,
                    rejected: true,
                    value,
                })
            }
            Self::Await {
                promise,
                resolve,
                value,
                called,
            } => {
                if !*called {
                    *called = true;
                    return Ok(Action::Call {
                        callee: resolve.clone(),
                        this: Value::Undefined,
                        args: vec![value.clone()],
                    });
                }
                Ok(Action::SuspendAsync(*promise))
            }
            Self::RejectTypeError {
                promise,
                forwarding,
                message,
                called,
            } => {
                if !*called {
                    *called = true;
                    let callee = crate::realm::intrinsics(heap)
                        .ok_or(Fault::OutOfMemory("realm exhausted"))?
                        .global(crate::realm::Global::TypeError);
                    return Ok(Action::Call {
                        callee,
                        this: Value::Undefined,
                        args: vec![Value::String(JsString::from(*message))],
                    });
                }
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("missing TypeError result"));
                };
                if let Some(guard) = forwarding {
                    if let Some(target) = claim(heap, task, *guard)? {
                        settle(heap, task, inbox, target, true, value)?;
                    }
                    // A resolver forwards the rejection and returns normally.
                    // Adoption discards this reaction's derived promise; it
                    // must not become a second, uncatchable rejection.
                    settle(heap, task, inbox, *promise, false, Value::Undefined)?;
                } else {
                    settle(heap, task, inbox, *promise, true, value)?;
                }
                Ok(Action::Return(Value::Undefined))
            }
            Self::Static {
                promise,
                resolve,
                value,
                called,
            } => {
                if !*called {
                    *called = true;
                    return Ok(Action::Call {
                        callee: resolve.clone(),
                        this: Value::Undefined,
                        args: vec![value.clone()],
                    });
                }
                Ok(Action::Return(Value::Object(*promise)))
            }
            Self::Constructor {
                promise,
                executor,
                resolve,
                reject,
                called,
                ..
            } => {
                if !*called {
                    *called = true;
                    return Ok(Action::Call {
                        callee: executor.clone(),
                        this: Value::Undefined,
                        args: vec![resolve.clone(), reject.clone()],
                    });
                }
                Ok(Action::Return(Value::Object(*promise)))
            }
            Self::Thenable {
                thenable,
                then,
                resolve,
                reject,
                called,
                ..
            } => {
                if !*called {
                    *called = true;
                    return Ok(Action::Call {
                        callee: then.clone(),
                        this: thenable.clone(),
                        args: vec![resolve.clone(), reject.clone()],
                    });
                }
                Ok(Action::Return(Value::Undefined))
            }
            Self::Failed {
                promise,
                guard,
                value,
                constructor,
            } => {
                if guard.is_none() || claim(heap, task, guard.unwrap())?.is_some() {
                    settle(heap, task, inbox, *promise, true, value.clone())?;
                }
                Ok(Action::Return(if *constructor {
                    Value::Object(*promise)
                } else {
                    Value::Undefined
                }))
            }
            Self::Resolve {
                promise,
                value,
                stage,
            } => {
                if *value == Value::Object(*promise) {
                    *self = Self::RejectTypeError {
                        promise: *promise,
                        forwarding: None,
                        message: "promise cannot resolve itself",
                        called: false,
                    };
                    return Ok(Action::Continue);
                }
                let then = if *stage == 0 && matches!(value, Value::Object(_)) {
                    *stage = 1;
                    match property::read(heap, task, value, &Key::Name(JsString::from("then")))? {
                        Some(Read::Getter(callee)) => {
                            return Ok(Action::Call {
                                callee,
                                this: value.clone(),
                                args: vec![],
                            });
                        }
                        Some(Read::Value(then)) => then,
                        None => Value::Undefined,
                    }
                } else if let Input::CallResult(then) = input {
                    then
                } else {
                    Value::Undefined
                };
                if dispatch::callee(heap, &then).is_ok() {
                    let callable = function(heap, Builtin::PromiseResolveThenable(*promise))?;
                    inbox
                        .upgrade()
                        .ok_or(Fault::Internal("promise task inbox gone"))?
                        .push(Job {
                            callable,
                            args: vec![value.clone(), then],
                        })
                        .map_err(|_| Fault::Internal("promise task inbox closed"))?;
                } else {
                    settle(heap, task, inbox, *promise, false, value.clone())?;
                }
                Ok(Action::Return(Value::Undefined))
            }
            Self::Reaction { reaction, called } => {
                if let Some(cap) = reaction.capability.clone() {
                    let result = heap
                        .read(reaction.source, |o| match o {
                            HeapObject::Promise(p) => p.result.clone(),
                            _ => None,
                        })
                        .flatten()
                        .ok_or(Fault::Internal("reaction source is pending"))?;
                    let handler = if result.rejected {
                        reaction.rejected.clone()
                    } else {
                        reaction.fulfilled.clone()
                    };
                    *self = Self::Standard(Box::new(standard::State::reaction(
                        cap,
                        handler,
                        result.value,
                        result.rejected,
                        result.owner.is_some_and(|owner| owner != task),
                        reaction.forwarding,
                        standard::foreign_rejection_handler(
                            heap,
                            &reaction.fulfilled,
                            &reaction.rejected,
                        ),
                    )));
                    return Ok(Action::Continue);
                }
                if *called {
                    let Input::CallResult(value) = input else {
                        return Err(Fault::Internal("reaction missing result"));
                    };
                    *self = Self::Resolve {
                        promise: reaction.derived,
                        value,
                        stage: 0,
                    };
                    return Ok(Action::Continue);
                }
                let result = heap
                    .read(reaction.source, |o| match o {
                        HeapObject::Promise(p) => p.result.clone(),
                        _ => None,
                    })
                    .flatten()
                    .ok_or(Fault::Internal("reaction source is pending"))?;
                if result.owner.is_some_and(|owner| owner != task) {
                    if let Some(id) = reaction.async_state {
                        *self = Self::AsyncRejectTypeError { id, called: false };
                        return Ok(Action::Continue);
                    }
                    *self = Self::RejectTypeError {
                        promise: reaction.derived,
                        forwarding: reaction.forwarding,
                        message: "promise result belongs to another task",
                        called: false,
                    };
                    return Ok(Action::Continue);
                }
                if let Some(id) = reaction.async_state {
                    return Ok(Action::ResumeAsync {
                        id,
                        rejected: result.rejected,
                        value: result.value,
                    });
                }
                let handler = if result.rejected {
                    &reaction.rejected
                } else {
                    &reaction.fulfilled
                };
                if dispatch::callee(heap, handler).is_err() {
                    if result.rejected {
                        settle(heap, task, inbox, reaction.derived, true, result.value)?;
                        return Ok(Action::Return(Value::Undefined));
                    }
                    // Identity is a normal handler completion: resolve the
                    // derived promise, including its observable `then` lookup.
                    *self = Self::Resolve {
                        promise: reaction.derived,
                        value: result.value,
                        stage: 0,
                    };
                    return Ok(Action::Continue);
                }
                *called = true;
                Ok(Action::Call {
                    callee: handler.clone(),
                    this: Value::Undefined,
                    args: vec![result.value],
                })
            }
            Self::Catch {
                this,
                handler,
                stage,
            } => {
                if *stage == 2 {
                    let Input::CallResult(value) = input else {
                        return Err(Fault::Internal("catch missing result"));
                    };
                    return Ok(Action::Return(value));
                }
                let callee = if *stage == 1 {
                    let Input::CallResult(value) = input else {
                        return Err(Fault::Internal("catch missing then getter result"));
                    };
                    value
                } else {
                    match property::read(heap, task, this, &Key::Name(JsString::from("then")))? {
                        Some(Read::Value(value)) => value,
                        Some(Read::Getter(callee)) => {
                            *stage = 1;
                            return Ok(Action::Call {
                                callee,
                                this: this.clone(),
                                args: vec![],
                            });
                        }
                        None => Value::Undefined,
                    }
                };
                *stage = 2;
                Ok(Action::Call {
                    callee,
                    this: this.clone(),
                    args: vec![Value::Undefined, handler.clone()],
                })
            }
        }
    }
}

pub(crate) fn start(
    heap: &Heap,
    vm: &super::frames::VmState,
    which: Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
) -> Result<Option<PromiseState>, Fault> {
    let task = vm.diag.task;
    let inbox = &vm.jobs;
    let native_capacity = vm.natives.len() < vm.forward_limits(vm.stack_len()).1;
    let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
    Ok(Some(match which {
        Builtin::PromiseResolve
        | Builtin::PromiseReject
        | Builtin::PromiseThen
        | Builtin::PromiseAll
        | Builtin::PromiseRace
        | Builtin::PromiseAllSettled
        | Builtin::PromiseAny
        | Builtin::PromiseWithResolvers
        | Builtin::PromiseFinally
        | Builtin::PromiseCallback(_, _)
            if !construct =>
        {
            PromiseState::Standard(Box::new(standard::State::start(
                heap,
                task,
                which,
                this.clone(),
                args,
                vm.forward_limits(vm.stack_len()).1,
            )?))
        }
        Builtin::Promise if construct => {
            let prototype = crate::realm::intrinsics(heap)
                .ok_or(Fault::OutOfMemory("realm exhausted"))?
                .promise_prototype;
            constructor(heap, task, args, prototype)?
        }
        Builtin::Promise => return Err(Fault::TypeError("Promise requires new".into())),
        Builtin::PromiseResolver(guard, reject) if !construct => {
            let Some(promise) = claim_with_capacity(heap, task, guard, reject || native_capacity)?
            else {
                return Ok(None);
            };
            if reject {
                settle(heap, task, inbox, promise, true, arg(0))?;
                return Ok(None);
            }
            PromiseState::Resolve {
                promise,
                value: arg(0),
                stage: 0,
            }
        }
        Builtin::PromiseResolveThenable(promise) if !construct => {
            let (guard, resolve, reject) = resolving_functions(heap, promise)?;
            PromiseState::Thenable {
                promise,
                guard,
                thenable: arg(0),
                then: arg(1),
                resolve,
                reject,
                called: false,
            }
        }
        Builtin::PromiseReaction(id) if !construct => {
            let reaction = heap
                .read(id, |o| match o {
                    HeapObject::PromiseReaction(r) => Some(r.clone()),
                    _ => None,
                })
                .flatten()
                .ok_or(Fault::Internal("reaction reclaimed"))?;
            PromiseState::Reaction {
                reaction,
                called: false,
            }
        }
        Builtin::PromiseCatch if !construct => PromiseState::Catch {
            this: this.clone(),
            handler: arg(0),
            stage: 0,
        },
        _ => return Err(Fault::Internal("invalid promise native")),
    }))
}

pub(crate) fn capability(heap: &Heap) -> Result<(ObjectId, Value, Value), Fault> {
    let promise = new(heap)?;
    let (_, resolve, reject) = resolving_functions(heap, promise)?;
    Ok((promise, resolve, reject))
}

/// PromiseResolve with the intrinsic constructor. Identity requires an
/// observable constructor match; subclass instances otherwise use thenables.
pub(crate) fn await_state(heap: &Heap, task: TaskId, value: Value) -> Result<PromiseState, Fault> {
    intrinsic_resolve(heap, task, value, true)
}

fn intrinsic_resolve(
    heap: &Heap,
    task: TaskId,
    value: Value,
    awaiting: bool,
) -> Result<PromiseState, Fault> {
    if let Ok(promise) = receiver(heap, &value) {
        heap.read(promise, |o| match o {
            HeapObject::Promise(p) => p.check_owner(task),
            _ => Err(Fault::Internal("promise changed kind")),
        })
        .ok_or(Fault::Internal("promise reclaimed"))??;
        return Ok(PromiseState::IntrinsicResolve {
            value,
            awaiting,
            reading: false,
        });
    }
    resolve_fresh(heap, value, awaiting)
}

fn resolve_fresh(heap: &Heap, value: Value, awaiting: bool) -> Result<PromiseState, Fault> {
    let (promise, resolve, _) = capability(heap)?;
    Ok(if awaiting {
        PromiseState::Await {
            promise,
            resolve,
            value,
            called: false,
        }
    } else {
        PromiseState::Static {
            promise,
            resolve,
            value,
            called: false,
        }
    })
}

/// Internal PerformPromiseThen: queue a reaction bound to the activation's
/// owning task, without JavaScript property access or a derived capability.
pub(crate) fn await_then(
    heap: &Heap,
    inbox: &Weak<JobInbox>,
    source: ObjectId,
    state: ObjectId,
) -> Result<(), Fault> {
    let record = alloc(
        heap,
        HeapObject::PromiseReaction(Reaction {
            source,
            derived: source,
            fulfilled: Value::Undefined,
            rejected: Value::Undefined,
            forwarding: None,
            async_state: Some(state),
            capability: None,
        }),
    )?;
    let callable = function(heap, Builtin::PromiseReaction(record))?;
    register(heap, inbox, source, callable)
}

fn register(
    heap: &Heap,
    inbox: &Weak<JobInbox>,
    source: ObjectId,
    callable: Value,
) -> Result<(), Fault> {
    heap.with_object_mut(source, |o| {
        let HeapObject::Promise(p) = o else {
            unreachable!()
        };
        if p.result.as_ref().is_some_and(|r| r.rejected)
            && !p.is_handled
            && let Some((owner, stored)) = p.rejection.take()
            && let Some(owner) = owner.upgrade()
        {
            owner.handle_rejection(source, stored);
        }
        p.is_handled = true;
        let registration = Registration {
            inbox: inbox.clone(),
            callable,
        };
        if p.result.is_some() {
            registration.enqueue();
        } else {
            p.reactions.push(registration);
        }
    })
    .ok_or(Fault::Internal("promise reclaimed"))?;
    Ok(())
}
