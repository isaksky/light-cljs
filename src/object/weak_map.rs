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

#[cfg(test)]
mod set_tests {
    use super::*;
    use crate::heap::HeapObject;
    use crate::{RunOutcome, Runtime};

    /// Synthetic fixture, not light-cljs output. A live WeakSet keeps its
    /// ordinary properties, but never its object or unregistered symbol keys.
    #[test]
    fn weak_set_keys_are_weak_and_properties_strong() {
        let rt = Runtime::new();
        let code = crate::frontend::compile_script("weak-set-gc", "let k = {}, y = Symbol(), s = new WeakSet([k,y]), p = {}; s.own = p; __rt.vector(s,k,y,p)").unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(result) = task.run().unwrap() else {
            panic!("fixture failed");
        };
        let (set, key, symbol, property) = rt.scope(|scope| {
            let result = scope.local(&result).unwrap();
            (
                scope.persist(scope.nth(result, 0).unwrap()),
                scope.persist(scope.nth(result, 1).unwrap()),
                scope.persist(scope.nth(result, 2).unwrap()),
                scope.downgrade(scope.nth(result, 3).unwrap()),
            )
        });
        let Value::Object(id) = set.value_in(rt.heap()).unwrap() else {
            unreachable!();
        };
        let count = || {
            rt.heap()
                .read(id, |o| match o {
                    HeapObject::WeakSet(s) => s.len(),
                    _ => unreachable!(),
                })
                .unwrap()
        };
        drop((task, result, code));
        rt.collect().unwrap();
        assert_eq!(count(), 2);
        let weak_key = key.downgrade();
        drop((key, symbol));
        rt.collect().unwrap();
        assert_eq!(count(), 0);
        assert!(!weak_key.is_live());
        assert!(property.is_live());
        drop(set);
        rt.collect().unwrap();
        assert!(!property.is_live());
    }
}
