//! Task-owned WeakRef. The slot is the sole authority on target liveness;
//! retaining an immutable symbol body does not mark its identity as live.

use crate::heap::ObjectId;
use crate::heap::gc::{ConditionalVisitor, Trace, TraceEdge, WeakSlot};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::{Value, symbol::JsSymbol};

#[derive(Debug)]
pub(crate) struct WeakRefObject {
    object: OrdinaryObject,
    target: WeakSlot,
    // Symbols live outside the object table. Keep their immutable metadata
    // for deref, but never trace it as a strong edge. Collector clearing only
    // mutates the slot and runs no payload destructor or notification callback.
    symbol: Option<JsSymbol>,
}

impl WeakRefObject {
    pub(crate) fn new(owner: TaskId, prototype: ObjectId, target: &Value, edge: TraceEdge) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(owner, prototype),
            target: WeakSlot::new(edge),
            symbol: match target {
                Value::Symbol(symbol) => Some(symbol.clone()),
                _ => None,
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
                Value::Symbol(self.symbol.clone().expect("symbol target"))
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
