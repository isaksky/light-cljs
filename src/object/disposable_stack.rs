//! Task-owned explicit resource stack. Registration and ownership transfer
//! happen under the heap metadata lock; callbacks run only in VM frames.
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;
use crate::vm::dispatch::{self, Fault};

#[derive(Debug)]
pub(crate) enum Hint {
    Sync,
    Adopt,
    Defer,
}

#[derive(Debug)]
pub(crate) struct Resource {
    pub(crate) value: Value,
    pub(crate) method: Value,
    pub(crate) hint: Hint,
}
impl Trace for Resource {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.value.trace(visit);
        self.method.trace(visit);
    }
}

#[derive(Debug)]
pub(crate) struct DisposableStack {
    object: OrdinaryObject,
    pub(crate) disposed: bool,
    pub(crate) resources: Vec<Resource>,
}
impl DisposableStack {
    pub(crate) fn new(owner: TaskId, prototype: ObjectId) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(owner, prototype),
            disposed: false,
            resources: vec![],
        }
    }
    pub(crate) fn object(&self) -> &OrdinaryObject {
        &self.object
    }
    pub(crate) fn object_mut(&mut self) -> &mut OrdinaryObject {
        &mut self.object
    }
}
impl Trace for DisposableStack {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.object.trace(visit);
        for resource in &self.resources {
            resource.trace(visit);
        }
    }
}

pub(crate) fn access<T>(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    live: bool,
    f: impl FnOnce(&mut DisposableStack) -> T,
) -> Result<T, Fault> {
    let invalid =
        || Fault::TypeError("DisposableStack method requires a DisposableStack receiver".into());
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    heap.with_object_mut(*id, |object| {
        let HeapObject::DisposableStack(stack) = object else {
            return Err(invalid());
        };
        if stack.object.owner() != Some(task) {
            return Err(Fault::ForeignObject {
                owner: stack.object.owner(),
                caller: task,
            });
        }
        if live && stack.disposed {
            return Err(Fault::ReferenceError(
                "DisposableStack has been disposed".into(),
            ));
        }
        Ok(f(stack))
    })
    .unwrap_or(Err(Fault::Internal("DisposableStack was reclaimed")))
}

pub(crate) fn callable(heap: &Heap, method: &Value) -> Result<(), Fault> {
    match dispatch::callee(heap, method) {
        Err(Fault::NotCallable(_)) => {
            Err(Fault::TypeError("disposal callback is not callable".into()))
        }
        result => result.map(|_| ()),
    }
}
