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

#[derive(Debug)]
pub(crate) struct Promise {
    pub(crate) prototype: ObjectId,
    result: Option<Settlement>,
    reactions: Vec<Registration>,
    is_handled: bool,
}

/// A fresh resolve/reject pair shares this concurrent first-call guard.
/// It only points to a shareable Promise, never to task-owned callbacks.
#[derive(Debug)]
pub(crate) struct Resolving {
    promise: ObjectId,
    resolved: bool,
}
impl Trace for Resolving {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        visit(self.promise);
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
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        visit(self.prototype);
        if let Some(result) = &self.result {
            result.value.trace(visit);
        }
        for reaction in &self.reactions {
            reaction.callable.trace(visit);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Reaction {
    source: ObjectId,
    derived: ObjectId,
    fulfilled: Value,
    rejected: Value,
}
impl Trace for Reaction {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        visit(self.source);
        visit(self.derived);
        self.fulfilled.trace(visit);
        self.rejected.trace(visit);
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
    alloc(
        heap,
        HeapObject::Promise(Promise {
            prototype,
            result: None,
            reactions: vec![],
            is_handled: false,
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
    let guard = alloc(
        heap,
        HeapObject::PromiseResolving(Resolving {
            promise,
            resolved: false,
        }),
    )?;
    let resolve = function(heap, Builtin::PromiseResolver(guard, false))?;
    let reject = function(heap, Builtin::PromiseResolver(guard, true))?;
    Ok((guard, resolve, reject))
}
fn claim(heap: &Heap, id: ObjectId) -> Result<Option<ObjectId>, Fault> {
    heap.with_object_mut(id, |o| match o {
        HeapObject::PromiseResolving(p) => {
            let first = !p.resolved;
            p.resolved = true;
            Ok(first.then_some(p.promise))
        }
        _ => Err(Fault::Internal("promise changed kind")),
    })
    .ok_or(Fault::Internal("promise reclaimed"))?
}
fn settle(
    heap: &Heap,
    task: TaskId,
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
                    rejection_tracking(id, false);
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
/// Hook for clj-quickjs-249: rejection without handler / handler attached.
fn rejection_tracking(_promise: ObjectId, _handled: bool) {}

#[derive(Debug)]
pub(crate) enum PromiseState {
    RejectTypeError {
        promise: ObjectId,
        message: &'static str,
        called: bool,
    },
    Return(Value),
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
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match self {
            Self::RejectTypeError { promise, .. } => visit(*promise),
            Self::Return(value) => value.trace(visit),
            Self::Static {
                promise,
                resolve,
                value,
                ..
            } => {
                visit(*promise);
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
                visit(*promise);
                visit(*guard);
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
                visit(*promise);
                visit(*guard);
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
                visit(*promise);
                if let Some(guard) = guard {
                    visit(*guard);
                }
                value.trace(visit);
            }
            Self::Resolve { promise, value, .. } => {
                visit(*promise);
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
        matches!(
            self,
            Self::Constructor { .. }
                | Self::Resolve { .. }
                | Self::Reaction { .. }
                | Self::Thenable { .. }
        )
    }
    pub(crate) fn failed(&mut self, value: Value) {
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
            Self::RejectTypeError {
                promise,
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
                settle(heap, task, *promise, true, value)?;
                Ok(Action::Return(Value::Undefined))
            }
            Self::Return(value) => Ok(Action::Return(value.clone())),
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
                if guard.is_none() || claim(heap, guard.unwrap())?.is_some() {
                    settle(heap, task, *promise, true, value.clone())?;
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
                    settle(heap, task, *promise, false, value.clone())?;
                }
                Ok(Action::Return(Value::Undefined))
            }
            Self::Reaction { reaction, called } => {
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
                    *self = Self::RejectTypeError {
                        promise: reaction.derived,
                        message: "promise result belongs to another task",
                        called: false,
                    };
                    return Ok(Action::Continue);
                }
                let handler = if result.rejected {
                    &reaction.rejected
                } else {
                    &reaction.fulfilled
                };
                if dispatch::callee(heap, handler).is_err() {
                    if result.rejected {
                        settle(heap, task, reaction.derived, true, result.value)?;
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
    task: TaskId,
    inbox: &Weak<JobInbox>,
    which: Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
) -> Result<Option<PromiseState>, Fault> {
    let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
    Ok(Some(match which {
        Builtin::PromiseResolve if !construct => {
            let intrinsic = crate::realm::intrinsics(heap)
                .ok_or(Fault::OutOfMemory("realm exhausted"))?
                .global(crate::realm::Global::Promise);
            if *this != intrinsic {
                return Err(Fault::Unsupported(
                    "Promise subclass capabilities are not supported".into(),
                ));
            }
            if receiver(heap, &arg(0)).is_ok() {
                return Ok(Some(PromiseState::Return(arg(0))));
            }
            let promise = new(heap)?;
            let (_, resolve, _) = resolving_functions(heap, promise)?;
            PromiseState::Static {
                promise,
                resolve,
                value: arg(0),
                called: false,
            }
        }
        Builtin::PromiseReject | Builtin::PromiseThen if !construct => {
            PromiseState::Return(leaf(heap, task, inbox, which, this, args)?)
        }
        Builtin::Promise if construct => {
            let executor = arg(0);
            dispatch::callee(heap, &executor)?;
            let promise = new(heap)?;
            let (guard, resolve, reject) = resolving_functions(heap, promise)?;
            PromiseState::Constructor {
                promise,
                guard,
                executor,
                resolve,
                reject,
                called: false,
            }
        }
        Builtin::Promise => return Err(Fault::TypeError("Promise requires new".into())),
        Builtin::PromiseResolver(guard, reject) if !construct => {
            let Some(promise) = claim(heap, guard)? else {
                return Ok(None);
            };
            if reject {
                settle(heap, task, promise, true, arg(0))?;
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

pub(crate) fn leaf(
    heap: &Heap,
    task: TaskId,
    inbox: &Weak<JobInbox>,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
    match which {
        Builtin::PromiseReject => {
            let intrinsic = crate::realm::intrinsics(heap)
                .ok_or(Fault::OutOfMemory("realm exhausted"))?
                .global(crate::realm::Global::Promise);
            if *this != intrinsic {
                return Err(Fault::Unsupported(
                    "Promise subclass capabilities are not supported".into(),
                ));
            }
            let promise = new(heap)?;
            settle(heap, task, promise, true, arg(0))?;
            Ok(Value::Object(promise))
        }
        Builtin::PromiseThen => {
            let source = receiver(heap, this)?;
            let derived = new(heap)?;
            let handler = |i| {
                let value = arg(i);
                if dispatch::callee(heap, &value).is_ok() {
                    value
                } else {
                    Value::Undefined
                }
            };
            let record = alloc(
                heap,
                HeapObject::PromiseReaction(Reaction {
                    source,
                    derived,
                    fulfilled: handler(0),
                    rejected: handler(1),
                }),
            )?;
            let callable = function(heap, Builtin::PromiseReaction(record))?;
            heap.with_object_mut(source, |o| {
                let HeapObject::Promise(p) = o else {
                    unreachable!()
                };
                if p.result.as_ref().is_some_and(|r| r.rejected) && !p.is_handled {
                    rejection_tracking(source, true);
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
            Ok(Value::Object(derived))
        }
        _ => Err(Fault::Internal("invalid promise leaf")),
    }
}
