//! Task-owned generator objects. Suspended execution lives in a movable VM
//! activation; `next` is a native boundary around ordinary attached frames.
use super::dispatch::Fault;
use super::frames::{DetachedActivation, VmState};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::Value;

#[derive(Debug)]
pub(crate) enum State {
    SuspendedStart(DetachedActivation),
    SuspendedYield(DetachedActivation),
    Executing,
    Completed,
}

#[derive(Debug)]
pub(crate) struct Generator {
    pub(crate) owner: TaskId,
    pub(crate) object: OrdinaryObject,
    pub(crate) state: State,
}

impl Trace for Generator {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.object.trace(visit);
        if let State::SuspendedStart(a) | State::SuspendedYield(a) = &self.state {
            a.trace(visit);
        }
    }
}

pub(crate) fn prototype(heap: &Heap) -> Result<ObjectId, Fault> {
    crate::realm::intrinsics(heap)
        .map(|r| r.generator_prototype)
        .ok_or(Fault::OutOfMemory("generator prototype"))
}

/// Called after normal argument binding, before the body executes.
pub(crate) fn create(heap: &Heap, vm: &mut VmState) -> Result<(), Fault> {
    let function = match vm.top().callee {
        Some(id) => id,
        None => {
            let f = super::function::FunctionObject::new(
                vm.top().code.clone(),
                Box::new([]),
                vm.diag.task,
            );
            let id = heap
                .alloc(HeapObject::Function(f))
                .ok_or(Fault::OutOfMemory("generator function"))?;
            vm.top_mut().callee = Some(id);
            id
        }
    };
    let key = super::property::Key::Name("prototype".into());
    let parent = match super::function::read(heap, vm.diag.task, function, &key)? {
        super::function::FunctionRead::Found(Value::Object(id)) => id,
        _ => prototype(heap)?,
    };
    let activation = vm.detach_activation()?;
    let id = heap
        .alloc(HeapObject::Generator(Box::new(Generator {
            owner: vm.diag.task,
            object: OrdinaryObject::with_prototype(vm.diag.task, parent),
            state: State::SuspendedStart(activation),
        })))
        .ok_or(Fault::OutOfMemory("generator object"))?;
    vm.push(Value::Object(id));
    Ok(())
}

pub(crate) fn receiver(heap: &Heap, task: TaskId, value: &Value) -> Result<ObjectId, Fault> {
    let Value::Object(id) = value else {
        return Err(Fault::TypeError(
            "generator next requires a generator receiver".into(),
        ));
    };
    heap.read(*id, |o| match o {
        HeapObject::Generator(g) if g.owner != task => Err(Fault::ForeignBinding {
            owner: g.owner,
            caller: task,
        }),
        HeapObject::Generator(g) if matches!(g.state, State::Executing) => {
            Err(Fault::TypeError("cannot invoke a running generator".into()))
        }
        HeapObject::Generator(_) => Ok(*id),
        _ => Err(Fault::TypeError(
            "generator next requires a generator receiver".into(),
        )),
    })
    .unwrap_or(Err(Fault::Internal("generator reclaimed")))
}

pub(crate) fn resume(
    heap: &Heap,
    vm: &mut VmState,
    id: ObjectId,
    value: Value,
) -> Result<bool, Fault> {
    heap.read(id, |o| match o {
        HeapObject::Generator(g) => match &g.state {
            State::SuspendedStart(a) | State::SuspendedYield(a) => vm.check_activation(a),
            _ => Ok(()),
        },
        _ => Err(Fault::Internal("generator missing")),
    })
    .ok_or(Fault::Internal("generator reclaimed"))??;
    let old = heap
        .with_object_mut(id, |o| match o {
            HeapObject::Generator(g) => std::mem::replace(&mut g.state, State::Executing),
            _ => unreachable!(),
        })
        .ok_or(Fault::Internal("generator reclaimed"))?;
    match old {
        State::SuspendedStart(a) => vm.attach_activation(a),
        State::SuspendedYield(a) => {
            vm.attach_activation(a);
            vm.push(value);
        }
        State::Completed => {
            complete(heap, id);
            return Ok(false);
        }
        State::Executing => {
            return Err(Fault::TypeError("cannot invoke a running generator".into()));
        }
    }
    Ok(true)
}

pub(crate) fn suspend(heap: &Heap, vm: &mut VmState) -> Result<(), Fault> {
    let id =
        super::native::generator_boundary(vm).ok_or(Fault::Internal("yield outside generator"))?;
    let value = vm.pop()?;
    let activation = vm.detach_activation()?;
    heap.with_object_mut(id, |o| {
        if let HeapObject::Generator(g) = o {
            g.state = State::SuspendedYield(activation);
        }
    })
    .ok_or(Fault::Internal("generator reclaimed"))?;
    vm.push(value);
    Ok(())
}

pub(crate) fn complete(heap: &Heap, id: ObjectId) {
    heap.with_object_mut(id, |o| {
        if let HeapObject::Generator(g) = o {
            g.state = State::Completed;
        }
    });
}

pub(crate) fn result(heap: &Heap, task: TaskId, value: Value, done: bool) -> Result<Value, Fault> {
    let parent = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("realm"))?
        .object_prototype;
    let mut o = OrdinaryObject::with_prototype(task, parent);
    for (name, value) in [("value", value), ("done", Value::Bool(done))] {
        let key = heap
            .keys()
            .intern(&name.into())
            .ok_or(Fault::OutOfMemory("iterator result key"))?;
        o.define_with(key, value, PropertyFlags::DATA);
    }
    heap.alloc(HeapObject::Plain(o))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("iterator result"))
}
