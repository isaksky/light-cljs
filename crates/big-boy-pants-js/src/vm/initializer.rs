//! Private compiler initializer storage. The public roots and their getters
//! remain ordinary objects; only their unreachable bookkeeping bags use this
//! fixed payload instead of a property dictionary. Unit, name and result keep
//! full JavaScript identities, including mutable results and thrown values.
//!
//! Native payloads use the existing per-object access word, tracing and boxed
//! byte charge. Every read/write checks the owning task, so a saved task may
//! migrate workers but the slot cannot be shared with another task. No user
//! code or heap access runs while a slot's access word is held.

use std::any::Any;

use super::dispatch::Fault;
use super::instr::FormOp;
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject, NativePayload};
use crate::ids::TaskId;
use crate::value::Value;
use crate::value::share::Shareability;

#[derive(Debug)]
struct InitializerSlot {
    owner: TaskId,
    unit: Value,
    name: Value,
    value: Value,
    state: u8,
    defined: bool,
}

impl Trace for InitializerSlot {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.unit.trace(visit);
        self.name.trace(visit);
        self.value.trace(visit);
    }
}

impl NativePayload for InitializerSlot {
    fn shareability(&self) -> Shareability {
        Shareability::TaskOwned
    }
}

fn invalid() -> Fault {
    Fault::TypeError("invalid initializer slot or field".into())
}

pub(crate) fn operation(
    heap: &Heap,
    caller: TaskId,
    op: FormOp,
    args: &[Value],
) -> Result<Value, Fault> {
    if op == FormOp::InitializerSlot {
        let slot = InitializerSlot {
            owner: caller,
            unit: args[0].clone(),
            name: args[1].clone(),
            value: Value::Null,
            state: 0,
            defined: false,
        };
        return heap
            .alloc(HeapObject::Native(Box::new(slot)))
            .map(Value::Object)
            .ok_or(Fault::OutOfMemory("heap object table exhausted"));
    }
    let Value::Object(id) = args[0] else {
        return Err(invalid());
    };
    heap.with_object_mut(id, |object| {
        let HeapObject::Native(payload) = object else {
            return Err(invalid());
        };
        let slot = (&mut **payload as &mut dyn Any)
            .downcast_mut::<InitializerSlot>()
            .ok_or_else(invalid)?;
        if slot.owner != caller {
            return Err(Fault::ForeignObject {
                owner: Some(slot.owner),
                caller,
            });
        }
        if op == FormOp::InitializerSlotGet {
            return Ok(match args[1] {
                Value::Number(0.0) => slot.unit.clone(),
                Value::Number(1.0) => slot.name.clone(),
                Value::Number(2.0) => Value::Number(f64::from(slot.state)),
                Value::Number(3.0) => slot.value.clone(),
                Value::Number(4.0) => Value::Bool(slot.defined),
                _ => return Err(invalid()),
            });
        }
        match (&args[1], &args[2]) {
            (Value::Number(2.0), Value::Number(state)) if [0.0, 1.0, 2.0, 3.0].contains(state) => {
                slot.state = *state as u8;
            }
            (Value::Number(3.0), value) => slot.value = value.clone(),
            (Value::Number(4.0), Value::Bool(defined)) => slot.defined = *defined,
            _ => return Err(invalid()),
        }
        Ok(args[2].clone())
    })
    .ok_or_else(invalid)?
}
