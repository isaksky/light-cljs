//! Checked byte admission for runtime-owned backing storage.
//!
//! Reserve before allocating, keep the reservation with the storage owner,
//! and drop it only after that storage is released. An unsuccessful allocation
//! drops its temporary reservation. Reservations do not trace heap edges and
//! may outlive the heap (immutable payloads can have host owners).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

thread_local! {
    // Leaf helpers with boolean/Option results cannot carry a Fault. The VM
    // consumes this at the same instruction/native-step boundary.
    static DENIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

static NEXT_BUDGET: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug, Default)]
pub(crate) struct ByteBudget {
    id: usize,
    limit: Option<AtomicUsize>,
    used: AtomicUsize,
}

impl ByteBudget {
    pub(crate) fn is_limited(&self) -> bool {
        self.limit.is_some()
    }
    pub(crate) fn note_denial(&self) {
        DENIED.set(self.id);
    }
    pub(crate) fn take_denial(&self) -> bool {
        DENIED.with(|denied| {
            if self.id != 0 && denied.get() == self.id {
                denied.set(0);
                true
            } else {
                false
            }
        })
    }

    pub(crate) fn denied(&self) -> bool {
        self.id != 0 && DENIED.get() == self.id
    }
    pub(crate) fn new(limit: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            id: NEXT_BUDGET.fetch_add(1, Ordering::Relaxed),
            limit: limit.map(AtomicUsize::new),
            used: AtomicUsize::new(0),
        })
    }

    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn set_limit(&self, bytes: usize) {
        self.limit.as_ref().unwrap().store(bytes, Ordering::Relaxed);
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<ByteReservation> {
        // Unconfigured runtimes do not pay for an atomic counter or an Arc
        // clone on each payload allocation.
        let Some(limit) = &self.limit else {
            return Some(ByteReservation::default());
        };
        let limit = limit.load(Ordering::Relaxed);
        let mut used = self.used.load(Ordering::Relaxed);
        loop {
            let next = used.checked_add(bytes).filter(|&next| next <= limit)?;
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(actual) => used = actual,
            }
        }
        Some(ByteReservation {
            budget: Some(self.clone()),
            bytes,
        })
    }
}

/// Unique ownership of an admitted number of bytes. Moving is free; cloning
/// is deliberately unsupported because a second storage owner must reserve
/// independently. Shared payloads put one reservation inside their Arc body.
#[derive(Debug, Default)]
pub(crate) struct ByteReservation {
    budget: Option<Arc<ByteBudget>>,
    bytes: usize,
}

impl ByteReservation {
    pub(crate) fn refund(&mut self, bytes: usize) {
        if let Some(budget) = &self.budget {
            self.bytes = self.bytes.checked_sub(bytes).expect("owned byte refund");
            budget.used.fetch_sub(bytes, Ordering::Relaxed);
        }
    }
    pub(crate) fn is_charged(&self) -> bool {
        self.budget.is_some()
    }

    /// Commit another reservation for the same owner. The caller reserves
    /// before growth, and merges only after allocation succeeds.
    pub(crate) fn merge(&mut self, mut other: Self) {
        match (&self.budget, &other.budget) {
            (None, _) => std::mem::swap(self, &mut other),
            (Some(a), Some(b)) => {
                assert!(Arc::ptr_eq(a, b), "storage changed byte budgets");
                self.bytes = self
                    .bytes
                    .checked_add(other.bytes)
                    .expect("admitted byte sum");
                other.bytes = 0;
            }
            (Some(_), None) => {}
        }
    }
}

impl Drop for ByteReservation {
    fn drop(&mut self) {
        if let Some(budget) = &self.budget {
            let before = budget.used.fetch_sub(self.bytes, Ordering::Relaxed);
            debug_assert!(before >= self.bytes, "byte reservation refunded twice");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_reservation_refunds_and_rejects_overflow() {
        let budget = ByteBudget::new(Some(usize::MAX));
        let reservation = budget.reserve(usize::MAX).unwrap();
        assert!(budget.reserve(1).is_none());
        assert_eq!(budget.used(), usize::MAX);
        drop(reservation);
        assert_eq!(budget.used(), 0);
        assert!(budget.reserve(1).is_some());
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn eight_workers_share_one_atomic_admission_limit() {
        let budget = ByteBudget::new(Some(1024));
        let ready = std::sync::Barrier::new(8);
        let admitted = AtomicUsize::new(0);
        std::thread::scope(|threads| {
            for _ in 0..8 {
                threads.spawn(|| {
                    let mut held = Vec::new();
                    ready.wait();
                    while let Some(reservation) = budget.reserve(1) {
                        held.push(reservation);
                        admitted.fetch_add(1, Ordering::Relaxed);
                    }
                    ready.wait();
                    assert_eq!(budget.used(), 1024);
                    ready.wait();
                });
            }
        });
        assert_eq!(admitted.load(Ordering::Relaxed), 1024);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn native_collections_fail_catchably_and_release_their_reservations() {
        use crate::frontend::compile_function;
        use crate::{RunOutcome, Runtime, ValueView};
        for expression in [
            "__rt.vector(1,2,3)",
            "__rt.formVector(1,2,3)",
            "__rt.formList(1,2,3)",
            "__rt.map(1,2)",
            "__rt.formMap(1,2)",
        ] {
            let runtime = Runtime::with_byte_limit(16384);
            let code = compile_function(&format!("function f() {{ let retained = []; try {{ for (;;) retained.push({expression}); }} catch(e) {{ return e === 'heap byte limit exceeded'; }} }}")).unwrap();
            let task = runtime.spawn(&code, &[]).unwrap();
            let outcome = task.run().unwrap();
            let RunOutcome::Complete(result) = outcome else {
                panic!("{expression}: {outcome:?}");
            };
            assert_eq!(result.view(), ValueView::Bool(true), "{expression}");
            drop(task);
            runtime.collect().unwrap();
            let code =
                compile_function(&format!("function f() {{ return {expression}; }}")).unwrap();
            let task = runtime.spawn(&code, &[]).unwrap();
            assert!(
                matches!(task.run().unwrap(), RunOutcome::Complete(_)),
                "{expression}"
            );
        }
    }

    #[test]
    fn javascript_array_growth_denial_survives_yield_and_cancellation() {
        use crate::frontend::compile_function;
        use crate::{Budget, RunOutcome, Runtime, ValueView};
        let runtime = Runtime::with_byte_limit(1024);
        let code = compile_function("function f() { let a = []; try { for (;;) a.push(1); } catch(e) { return e === 'heap byte limit exceeded' && a.length > 0 && a[0] === 1; } }").unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        loop {
            match task
                .run_with(Budget::instructions(1), &mut crate::host::NoHost)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                RunOutcome::Complete(result) => {
                    assert_eq!(result.view(), ValueView::Bool(true));
                    break;
                }
                outcome => panic!("{outcome:?}"),
            }
        }
        drop(task);
        runtime.collect().unwrap();
        let code = compile_function("function f() { let a = [1,2,3]; while(true) {} }").unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        assert!(matches!(
            task.run_with(Budget::instructions(100), &mut crate::host::NoHost)
                .unwrap(),
            RunOutcome::Yielded
        ));
        let _ = task.cancel();
        assert!(matches!(task.run().unwrap(), RunOutcome::Cancelled));
        drop(task);
        runtime.collect().unwrap();
        let code =
            compile_function("function f() { return new ArrayBuffer(1024).byteLength; }").unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        let outcome = task.run().unwrap();
        assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
    }
}
