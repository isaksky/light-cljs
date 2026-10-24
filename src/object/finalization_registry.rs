//! Task-owned finalization cells. Only collector notifications clear targets;
//! callback delivery transfers held values into the owner's ordinary job inbox.
use crate::heap::ObjectId;
use crate::heap::gc::{ConditionalVisitor, Trace, TraceEdge, WeakSlot};
use crate::ids::TaskId;
use crate::job::Job;
use crate::object::OrdinaryObject;
use crate::value::Value;

#[derive(Debug)]
struct Cell {
    id: u64,
    target: WeakSlot,
    held: Value,
    token: Option<WeakSlot>,
}

#[derive(Debug)]
pub(crate) struct FinalizationRegistryObject {
    object: OrdinaryObject,
    callback: Value,
    cells: Vec<Cell>,
    next_id: u64,
}

impl FinalizationRegistryObject {
    pub(crate) fn new(owner: TaskId, prototype: ObjectId, callback: Value) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(owner, prototype),
            callback,
            cells: Vec::new(),
            next_id: 0,
        }
    }
    pub(crate) fn object(&self) -> &OrdinaryObject {
        &self.object
    }
    pub(crate) fn object_mut(&mut self) -> &mut OrdinaryObject {
        &mut self.object
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub(crate) fn register(
        &mut self,
        target: TraceEdge,
        held: Value,
        token: Option<TraceEdge>,
    ) -> Option<()> {
        let id = self.next_id;
        self.next_id = id.checked_add(1)?;
        self.cells.push(Cell {
            id,
            target: WeakSlot::notifying(target, id),
            held,
            token: token.map(WeakSlot::new),
        });
        Some(())
    }
    pub(crate) fn unregister(&mut self, token: TraceEdge) -> bool {
        let before = self.cells.len();
        self.cells
            .retain(|cell| cell.token.as_ref().and_then(WeakSlot::get) != Some(token));
        self.cells.len() != before
    }
    /// Called after the pause under heap access. Repeated/out-of-order
    /// notifications cannot remove a different (or already removed) cell.
    pub(crate) fn cleanup(&mut self, token: u64) -> Option<Job> {
        let index = self
            .cells
            .iter()
            .position(|cell| cell.id == token && cell.target.get().is_none())?;
        let cell = self.cells.remove(index);
        Some(Job {
            callable: self.callback.clone(),
            args: vec![cell.held],
        })
    }
}
impl Trace for FinalizationRegistryObject {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.object.trace(visit);
        self.callback.trace(visit);
        for cell in &self.cells {
            cell.held.trace(visit);
        }
    }
    fn trace_conditional(&mut self, visit: &mut dyn ConditionalVisitor) {
        for cell in &mut self.cells {
            cell.target.trace_conditional(visit);
            if let Some(token) = &mut cell.token {
                token.trace_conditional(visit);
            }
        }
    }
}
