//! Task-owned JavaScript WeakMap storage (clj-quickjs-173).
//! Keys are private object/symbol identities; only the collector decides
//! liveness. No key bodies or strong value edges are retained by tracing.

use crate::heap::ObjectId;
use crate::heap::gc::{ConditionalVisitor, EphemeronTable, Trace, TraceEdge};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;

#[derive(Debug)]
pub(crate) struct WeakMapObject {
    object: OrdinaryObject,
    entries: EphemeronTable,
}

pub(crate) fn weak_key(value: &Value) -> Option<TraceEdge> {
    match value {
        Value::Object(id) => Some((*id).into()),
        Value::Symbol(symbol) if symbol.can_be_held_weakly() => {
            Some(TraceEdge::Symbol(symbol.identity()))
        }
        _ => None,
    }
}

impl WeakMapObject {
    pub(crate) fn new(owner: TaskId, prototype: ObjectId) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(owner, prototype),
            entries: EphemeronTable::default(),
        }
    }

    pub(crate) fn object(&self) -> &OrdinaryObject {
        &self.object
    }
    pub(crate) fn object_mut(&mut self) -> &mut OrdinaryObject {
        &mut self.object
    }
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn get(&self, key: TraceEdge) -> Option<&Value> {
        self.entries.get(key)
    }
    pub(crate) fn set(&mut self, key: TraceEdge, value: Value) {
        self.entries.insert(key, value);
    }
    pub(crate) fn delete(&mut self, key: TraceEdge) -> bool {
        self.entries.remove(key).is_some()
    }
}

impl Trace for WeakMapObject {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.object.trace(visit);
    }
    fn trace_conditional(&mut self, visit: &mut dyn ConditionalVisitor) {
        self.entries.trace_conditional(visit);
    }
}
