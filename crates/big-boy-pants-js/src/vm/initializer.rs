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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Scope;

    #[test]
    fn slot_body_requires_exact_headroom_and_releases_its_charge() {
        let heap = std::sync::Arc::new(Heap::with_byte_limit(usize::MAX / 2));
        // Warm the slot batch/free list before setting the payload headroom.
        let scope = Scope::new(&heap);
        scope.new_object();
        drop(scope);
        heap.collect().unwrap();
        let baseline = heap.byte_budget().used();
        let charge = HeapObject::charged_with_box::<InitializerSlot>();
        let args = [Value::Number(1.5), Value::Null];
        let owner = TaskId::from_raw(1);

        heap.byte_budget().set_limit(baseline + charge - 1);
        assert!(operation(&heap, owner, FormOp::InitializerSlot, &args).is_err());
        assert_eq!(heap.byte_budget().used(), baseline);
        assert!(heap.byte_budget().take_denial());

        heap.byte_budget().set_limit(baseline + charge);
        let scope = Scope::new(&heap);
        let slot = operation(&heap, owner, FormOp::InitializerSlot, &args)
            .ok()
            .expect("exact payload headroom");
        scope.new_local(slot);
        assert_eq!(heap.byte_budget().used(), baseline + charge);
        heap.collect().unwrap();
        assert_eq!(heap.byte_budget().used(), baseline + charge);
        drop(scope);
        heap.collect().unwrap();
        assert_eq!(heap.byte_budget().used(), baseline);
        eprintln!(
            "slot payload/heap object/total bytes: {}/{}/{}",
            std::mem::size_of::<InitializerSlot>(),
            std::mem::size_of::<HeapObject>(),
            charge
        );
    }
}
