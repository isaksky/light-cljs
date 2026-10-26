//! Task-owned finalization cells. Only collector notifications clear targets;
//! callback delivery transfers held values into the owner's ordinary job inbox.
use crate::heap::ObjectId;
use crate::heap::gc::{ConditionalVisitor, Trace, TraceEdge, WeakSlot};
use crate::ids::TaskId;
use crate::job::Job;
use crate::object::OrdinaryObject;
use crate::value::Value;
use std::collections::BTreeMap;
use std::sync::Weak;

#[derive(Debug)]
struct Cell {
    target: WeakSlot,
    held: Value,
    token: Option<WeakSlot>,
}

#[derive(Debug)]
pub(crate) struct FinalizationRegistryObject {
    object: OrdinaryObject,
    callback: Value,
    cells: BTreeMap<u64, Cell>,
    next_id: u64,
    inbox: Weak<crate::job::JobInbox>,
    reservation: crate::heap::budget::ByteReservation,
}

impl FinalizationRegistryObject {
    pub(crate) fn new(
        heap: &crate::heap::Heap,
        owner: TaskId,
        prototype: ObjectId,
        callback: Value,
        inbox: Weak<crate::job::JobInbox>,
    ) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(heap, owner, prototype),
            callback,
            cells: BTreeMap::new(),
            next_id: 0,
            inbox,
            reservation: Default::default(),
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
        let next = id.checked_add(1)?;
        let Some(additional) = self.object.budget.reserve(2048) else {
            self.object.budget.note_denial();
            return None;
        };
        self.reservation.merge(additional);
        self.next_id = next;
        self.cells.insert(
            id,
            Cell {
                target: WeakSlot::notifying(target, id),
                held,
                token: token.map(WeakSlot::new),
            },
        );
        Some(())
    }
    pub(crate) fn unregister(&mut self, token: TraceEdge) -> bool {
        let before = self.cells.len();
        self.cells
            .retain(|_, cell| cell.token.as_ref().and_then(WeakSlot::get) != Some(token));
        self.reservation.refund((before - self.cells.len()) * 2048);
        self.cells.len() != before
    }
    /// Called after the pause under heap access. Repeated/out-of-order
    /// notifications cannot remove a different (or already removed) cell.
    fn cleanup(&mut self, token: u64) -> Option<Job> {
        if self.cells.get(&token)?.target.get().is_some() {
            return None;
        }
        let cell = self.cells.remove(&token)?;
        self.reservation.refund(2048);
        Some(Job {
            callable: self.callback.clone(),
            args: vec![cell.held],
            reservation: Default::default(),
        })
    }

    /// Heap-lifetime notification consumption, even after the owner handle
    /// disappears. The weak inbox is solely a job-admission capability.
    pub(crate) fn deliver(&mut self, owner: ObjectId, token: u64) {
        let job = self.cleanup(token);
        if let Some(inbox) = self.inbox.upgrade() {
            if let Some(job) = job {
                let _ = inbox.push(job); // Finished/cancelled owners drop cleanup.
            }
            inbox.retain_registry(owner, !self.is_empty());
        }
    }
}
impl Trace for FinalizationRegistryObject {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.object.trace(visit);
        self.callback.trace(visit);
        for cell in self.cells.values() {
            cell.held.trace(visit);
        }
    }
    fn trace_conditional(&mut self, visit: &mut dyn ConditionalVisitor) {
        for cell in self.cells.values_mut() {
            cell.target.trace_conditional(visit);
            if let Some(token) = &mut cell.token {
                token.trace_conditional(visit);
            }
        }
    }
}
