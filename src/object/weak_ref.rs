//! Task-owned WeakRef. The slot is the sole authority on target liveness;
//! retaining an immutable symbol body does not mark its identity as live.

use crate::heap::ObjectId;
use crate::heap::gc::{ConditionalVisitor, Trace, TraceEdge, WeakSlot};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;

#[derive(Debug)]
pub(crate) struct WeakRefObject {
    object: OrdinaryObject,
    target: WeakSlot,
}

impl WeakRefObject {
    pub(crate) fn new(owner: TaskId, prototype: ObjectId, target: &Value, edge: TraceEdge) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(owner, prototype),
            target: match target {
                Value::Symbol(symbol) => WeakSlot::for_symbol(symbol.clone()),
                _ => WeakSlot::new(edge),
            },
        }
    }

    pub(crate) fn object(&self) -> &OrdinaryObject {
        &self.object
    }

    pub(crate) fn object_mut(&mut self) -> &mut OrdinaryObject {
        &mut self.object
    }

    pub(crate) fn deref(&self) -> Value {
        match self.target.get() {
            Some(TraceEdge::Object(id)) => Value::Object(id),
            Some(TraceEdge::Symbol(_)) => {
                Value::Symbol(self.target.symbol().expect("symbol target").clone())
            }
            None => Value::Undefined,
        }
    }
}

impl Trace for WeakRefObject {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.object.trace(visit);
    }

    fn trace_conditional(&mut self, visit: &mut dyn ConditionalVisitor) {
        self.target.trace_conditional(visit);
    }
}
