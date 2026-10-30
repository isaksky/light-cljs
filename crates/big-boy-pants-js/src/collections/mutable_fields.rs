//! Mutable deftype fields. Before publication they may hold task-owned values.
//! Publication proves one version of the fields and promotes the cell under its
//! lock. Afterwards every store proves its new value before taking that lock.
//! No heap access or user code runs with the cell locked. PVector accounts for
//! retained field storage; tracing reads the current vector, not an initial copy.

use std::sync::{Mutex, PoisonError};

use super::{CollectionError, PVector};
use crate::heap::{
    Heap,
    gc::{Trace, TraceEdge},
};
use crate::value::{Value, share};
use crate::vm::dispatch::Fault;

#[derive(Debug)]
struct State {
    fields: PVector,
    version: u64,
    shared: bool,
}

#[derive(Debug)]
pub(crate) struct MutableFields {
    state: Mutex<State>,
    _reservation: crate::heap::budget::ByteReservation,
}

impl MutableFields {
    pub(crate) fn new(heap: &Heap, fields: PVector) -> Result<Self, CollectionError> {
        let reservation = heap
            .reserve_bytes(std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>())
            .ok_or(CollectionError::HeapLimit)?;
        Ok(Self {
            state: Mutex::new(State {
                fields,
                version: 0,
                shared: false,
            }),
            _reservation: reservation,
        })
    }

    pub(crate) fn snapshot(&self) -> (PVector, u64) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        (state.fields.clone(), state.version)
    }

    pub(crate) fn mark_shared(&self, version: u64) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.version != version {
            return false;
        }
        state.shared = true;
        true
    }

    pub(crate) fn is_shared(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .shared
    }

    pub(crate) fn get(&self, index: usize) -> Option<Value> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fields
            .get(index)
    }

    pub(crate) fn set(&self, heap: &Heap, index: usize, value: Value) -> Result<(), Fault> {
        let mut checked = false;
        loop {
            let (fields, version) = self.snapshot();
            if self.is_shared() && !checked {
                share::check(heap, &value).map_err(|e| Fault::TypeError(e.to_string()))?;
                checked = true;
            }
            let updated = fields
                .try_set(heap, index, value.clone())
                .map_err(Fault::from)?;
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.version != version || (state.shared && !checked) {
                continue;
            }
            state.fields = updated;
            state.version = state
                .version
                .checked_add(1)
                .ok_or(CollectionError::HeapLimit)?;
            return Ok(());
        }
    }
}

impl Trace for MutableFields {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.snapshot().0.trace(visit);
    }
}
