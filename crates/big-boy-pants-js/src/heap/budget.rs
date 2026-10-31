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
    peak: AtomicUsize,
    last_live: AtomicUsize,
    pressure: std::sync::atomic::AtomicBool,
    starved: std::sync::atomic::AtomicBool,
    volume: AtomicUsize,
    collected_volume: AtomicUsize,
    next_collection: AtomicUsize,
}

impl ByteBudget {
    /// Highest admitted total since the budget was created.
    pub(crate) fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }

    pub(crate) fn last_live(&self) -> usize {
        self.last_live.load(Ordering::Relaxed)
    }

    pub(crate) fn limit(&self) -> Option<usize> {
        self.limit
            .as_ref()
            .map(|limit| limit.load(Ordering::Relaxed))
    }

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
            peak: AtomicUsize::new(0),
            last_live: AtomicUsize::new(0),
            pressure: std::sync::atomic::AtomicBool::new(false),
            starved: std::sync::atomic::AtomicBool::new(false),
            volume: AtomicUsize::new(0),
            collected_volume: AtomicUsize::new(0),
            next_collection: AtomicUsize::new(limit.map_or(usize::MAX, |bytes| (bytes / 4).max(1))),
        })
    }

    pub(crate) fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn set_limit(&self, bytes: usize) {
        self.limit.as_ref().unwrap().store(bytes, Ordering::Relaxed);
        self.collection_finished(self.volume());
    }

    pub(crate) fn volume(&self) -> usize {
        self.volume.load(Ordering::Relaxed)
    }

    pub(crate) fn collection_due(&self) -> bool {
        self.is_limited()
            && (self.pressure.load(Ordering::Relaxed)
                || self
                    .volume()
                    .saturating_sub(self.collected_volume.load(Ordering::Relaxed))
                    >= self.next_collection.load(Ordering::Relaxed))
    }

    /// A denial happened and no collection since has freed an eighth of the
    /// limit: garbage released after the last pause may be all that stands
    /// between the next run slice and its allocations.
    pub(crate) fn starved(&self) -> bool {
        self.starved.load(Ordering::Relaxed)
    }

    /// The pause's volume snapshot, taken with every mutator stopped. A
    /// denial before it is answered by this collection; one after it
    /// requests the next.
    pub(crate) fn collection_started(&self) -> usize {
        self.pressure.store(false, Ordering::Relaxed);
        self.volume()
    }

    pub(crate) fn collection_finished(&self, volume: usize) {
        let Some(limit) = &self.limit else {
            return;
        };
        // The snapshot was taken with mutators stopped. Preserve allocations made
        // while reclaimed payloads were dropped after the pause. Concurrent GC
        // completions may arrive in either order, so the watermark only advances.
        self.collected_volume.fetch_max(volume, Ordering::Relaxed);
        let live = self.used();
        self.last_live.store(live, Ordering::Relaxed);
        // A denial starves the heap until a collection leaves an eighth of
        // the limit free; until then each run slice starts with one.
        if limit.load(Ordering::Relaxed).saturating_sub(live) >= limit.load(Ordering::Relaxed) / 8 {
            self.starved.store(false, Ordering::Relaxed);
        }
        // The next collection is due once half the remaining headroom has
        // been admitted. Not a fraction of the live bytes: one operation
        // can admit as much as the live set (a large buffer or string), and
        // a live-proportional trigger then collected after every few such
        // operations, each marking everything live (light-cljs-495).
        // Ordinary object churn is still collected in proportion to the
        // live objects by the allocation-count trigger (`HeapPolicy`).
        let growth = (limit.load(Ordering::Relaxed).saturating_sub(live) / 2).max(1);
        self.next_collection.store(growth, Ordering::Relaxed);
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<ByteReservation> {
        let reservation = self.reserve_credit(bytes)?;
        reservation.allocated();
        Some(reservation)
    }

    /// Try admitting storage while the caller still has allocation fallbacks.
    /// A failed probe leaves collection pressure and starvation unchanged;
    /// use [`Self::reserve`] once failure means the allocation is denied.
    pub(crate) fn try_reserve(self: &Arc<Self>, bytes: usize) -> Option<ByteReservation> {
        let reservation = self.try_reserve_credit(bytes)?;
        reservation.allocated();
        Some(reservation)
    }

    /// [`Self::reserve`] for a credit: bytes held against the limit for
    /// storage that may be allocated later, not allocated now. A credit is
    /// not allocation volume, so it does not bring the next collection
    /// closer until [`ByteReservation::allocated`] says its storage exists.
    /// A rope's flattened copy is one: a loop of appends charges a credit
    /// of the whole string at every step and releases the last one, and
    /// counting them paced a collection of the whole heap every half
    /// headroom of credits (light-cljs-635).
    pub(crate) fn reserve_credit(self: &Arc<Self>, bytes: usize) -> Option<ByteReservation> {
        self.try_reserve_credit(bytes).or_else(|| {
            // A denial may be caused by garbage: request a collection at
            // the next safe boundary, so a handler or the next task can
            // proceed after it. There is no last-resort collection at the
            // allocation itself, which cannot park.
            self.pressure.store(true, Ordering::Relaxed);
            self.starved.store(true, Ordering::Relaxed);
            None
        })
    }

    fn try_reserve_credit(self: &Arc<Self>, bytes: usize) -> Option<ByteReservation> {
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
                Ok(_) => {
                    self.peak.fetch_max(next, Ordering::Relaxed);
                    break;
                }
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
    pub(crate) fn belongs_to(&self, budget: &Arc<ByteBudget>) -> bool {
        self.budget
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(owner, budget))
    }
    /// Move `bytes` of this charge to a new owner, or `None` when it holds
    /// fewer (or is unaccounted). The total charge is unchanged.
    pub(crate) fn split(&mut self, bytes: usize) -> Option<Self> {
        let budget = self.budget.clone()?;
        self.bytes = self.bytes.checked_sub(bytes)?;
        Some(Self {
            budget: Some(budget),
            bytes,
        })
    }
    pub(crate) fn refund(&mut self, bytes: usize) {
        if let Some(budget) = &self.budget {
            self.bytes = self.bytes.checked_sub(bytes).expect("owned byte refund");
            budget.used.fetch_sub(bytes, Ordering::Relaxed);
        }
    }
    pub(crate) fn shrink_to(&mut self, bytes: usize) {
        if self.is_charged() && self.bytes > bytes {
            self.refund(self.bytes - bytes);
        }
    }
    /// Count this reservation's bytes as allocation volume, which paces
    /// collections ([`ByteBudget::collection_due`]): its storage now exists.
    /// [`ByteBudget::reserve`] does this at once; a credit
    /// ([`ByteBudget::reserve_credit`]) when its storage is made.
    pub(crate) fn allocated(&self) {
        if let Some(budget) = &self.budget {
            budget.volume.fetch_add(self.bytes, Ordering::Relaxed);
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

/// Reserve `count * element_bytes` with checked arithmetic, as a resource
/// fault. For snapshots whose size is known before they are built.
pub(crate) fn reserve_elements(
    heap: &super::Heap,
    count: usize,
    element_bytes: usize,
) -> Result<ByteReservation, crate::vm::dispatch::Fault> {
    count
        .checked_mul(element_bytes)
        .and_then(|bytes| heap.reserve_bytes(bytes))
        .ok_or(crate::vm::dispatch::Fault::HeapLimit)
}

/// The charge of a buffer that grows one element at a time while retained
/// by a resumable operation. [`GrowthCharge::admit`] runs before each push
/// and reserves geometrically, like `Vec` growth, so the charge covers the
/// buffer's real capacity and costs one atomic per doubling.
#[derive(Debug, Default)]
pub(crate) struct GrowthCharge {
    reservation: ByteReservation,
    capacity: usize,
}

impl GrowthCharge {
    /// Admit room for element `len` (zero-based) of `element_bytes` each.
    pub(crate) fn admit(
        &mut self,
        heap: &super::Heap,
        len: usize,
        element_bytes: usize,
    ) -> Result<(), crate::vm::dispatch::Fault> {
        if len < self.capacity {
            return Ok(());
        }
        let capacity = len
            .saturating_add(1)
            .max(self.capacity.saturating_mul(2))
            .max(8);
        let more = reserve_elements(heap, capacity - self.capacity, element_bytes)?;
        self.reservation.merge(more);
        self.capacity = capacity;
        Ok(())
    }

    /// The charge, for a holder that takes over the finished buffer.
    pub(crate) fn into_reservation(self) -> ByteReservation {
        self.reservation
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
    fn failed_reservation_probes_preserve_collection_flags() {
        // Exercise both a limit denial and checked-add overflow.
        for limit in [1000, usize::MAX] {
            let budget = ByteBudget::new(Some(limit));
            let held = budget.try_reserve(limit).unwrap();
            assert_eq!(budget.used(), limit);
            assert_eq!(budget.peak(), limit);
            assert_eq!(budget.volume(), limit);
            budget.collection_finished(budget.collection_started());
            assert!(!budget.starved());
            assert!(!budget.collection_due());

            assert!(budget.try_reserve(1).is_none());
            assert!(!budget.starved());
            assert!(!budget.collection_due());
            assert_eq!(budget.used(), limit);
            assert_eq!(budget.peak(), limit);
            assert_eq!(budget.volume(), limit);

            // Exhausting all alternatives must still request collection.
            assert!(budget.reserve(1).is_none());
            assert!(budget.starved());
            assert!(budget.collection_due());
            assert!(budget.try_reserve(1).is_none());
            assert!(budget.starved());
            assert!(budget.collection_due());
            drop(held);
            assert_eq!(budget.used(), 0);
        }
    }

    #[test]
    fn split_moves_charge_between_owners() {
        let budget = ByteBudget::new(Some(1000));
        let mut whole = budget.reserve(600).unwrap();
        let part = whole.split(100).unwrap();
        assert_eq!(budget.used(), 600);
        assert!(whole.split(501).is_none());
        assert!(ByteReservation::default().split(1).is_none());
        drop(whole);
        assert_eq!(budget.used(), 100);
        drop(part);
        assert_eq!(budget.used(), 0);
    }

    /// Join and String.raw hand their parts vector to a `StringBuild`,
    /// whose charge must stay held until the builder (and the vector) drops.
    #[test]
    fn join_and_raw_keep_part_charges_while_building() {
        let rt = crate::Runtime::try_with_heap_policy(
            Default::default(),
            crate::HeapPolicy {
                hard_limit_bytes: Some(64 << 20),
                ..Default::default()
            },
        )
        .unwrap();
        let heap = rt.heap();
        let before = heap.byte_budget().used();
        let parts = vec![(crate::value::JsString::from("ab"), 0, 2); 3];
        let charge = heap.reserve_bytes(4096).unwrap();
        let build = crate::value::string::StringBuild::with_length_accounted(heap, parts, 6)
            .ok()
            .expect("builder admitted")
            .with_parts_reservation(charge);
        assert!(heap.byte_budget().used() >= before + 4096);
        drop(build);
        assert_eq!(heap.byte_budget().used(), before);
    }

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
        let runtime = Runtime::with_byte_limit(
            1024 + crate::heap::HeapObject::charged_with_box::<
                crate::object::array_buffer::ArrayBuffer,
            >(),
        );
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

    #[test]
    fn large_string_and_bigint_denial_is_catchable_after_resumption() {
        use crate::frontend::compile_function;
        use crate::{Budget, RunOutcome, Runtime, ValueView};
        for expression in [
            "'x'.repeat(1000000)",
            "1n << 1000000n",
            "Array.prototype.join.call({length: 100000})",
            "String.raw({raw: {length: 100000}})",
            "JSON.stringify(new Array(100000))",
            "new Intl.NumberFormat().format(\"1e9999\")",
            "new Intl.NumberFormat().formatToParts(\"1e9999\")",
            "BigInt('9'.repeat(40000))",
            "('x'.repeat(20000)).normalize('NFKD')",
        ] {
            let runtime = Runtime::with_byte_limit(256 * 1024);
            let code = compile_function(&format!("function f() {{ try {{ return {expression}; }} catch(e) {{ return e === 'heap byte limit exceeded'; }} }}")).unwrap();
            let task = runtime.spawn(&code, &[]).unwrap();
            loop {
                match task
                    .run_with(Budget::instructions(7), &mut crate::host::NoHost)
                    .unwrap()
                {
                    RunOutcome::Yielded => {}
                    RunOutcome::Complete(value) => {
                        assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                        break;
                    }
                    other => panic!("{expression}: {other:?}"),
                }
            }
            drop(task);
            runtime.collect().unwrap();
            let small = compile_function("function f() { return 'ok'.repeat(100); }").unwrap();
            let task = runtime.spawn(&small, &[]).unwrap();
            assert!(
                matches!(task.run().unwrap(), RunOutcome::Complete(_)),
                "{expression}"
            );
        }
    }

    #[test]
    fn eight_workers_share_backing_store_admission_and_reclaim_payloads() {
        use crate::heap::{Heap, HeapObject, Scope};
        use crate::ids::TaskId;
        use crate::value::Value;
        let heap = Arc::new(Heap::with_byte_limit(usize::MAX / 2));
        let ready = std::sync::Barrier::new(9);
        let admitted = AtomicUsize::new(0);
        std::thread::scope(|threads| {
            for worker in 0..8 {
                let heap = &heap;
                let ready = &ready;
                let admitted = &admitted;
                threads.spawn(move || {
                    let scope = Scope::new(heap);
                    // Reserve each worker's slot batch before fixing the payload headroom.
                    let _ = scope.new_object();
                    ready.wait();
                    ready.wait();
                    let result = crate::object::array_buffer::constructor(
                        heap,
                        TaskId::from_raw(worker + 1),
                        &Value::Number(65536.0),
                    );
                    if result.is_ok() {
                        admitted.fetch_add(1, Ordering::Relaxed);
                    }
                    ready.wait();
                    ready.wait();
                    drop(result);
                    drop(scope);
                });
            }
            ready.wait();
            let occupied = heap.byte_budget().used();
            let allowance = 4
                * (65536
                    + HeapObject::charged_with_box::<crate::object::array_buffer::ArrayBuffer>());
            heap.byte_budget().set_limit(occupied + allowance);
            ready.wait();
            ready.wait();
            assert_eq!(admitted.load(Ordering::Relaxed), 4);
            assert_eq!(heap.byte_budget().used(), occupied + allowance);
            ready.wait();
        });
        let occupied = heap.byte_budget().used();
        heap.collect().unwrap();
        assert!(heap.byte_budget().used() <= occupied - 4 * 65536);
    }

    #[test]
    fn byte_pressure_collects_without_object_threshold_or_explicit_collection() {
        use crate::frontend::compile_function;
        use crate::{RunOutcome, Runtime, ValueView};
        let runtime = Runtime::with_byte_limit(2 * 1024 * 1024);
        let code = compile_function("function f() { let total = 0; for (let i = 0; i < 300; ++i) { const b = new ArrayBuffer(32768); total += b.byteLength; } return total; }").unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(result) = task.run().unwrap() else {
            panic!("automatic byte collection failed");
        };
        assert_eq!(result.view(), ValueView::Number(300.0 * 32768.0));
        let stats = runtime.heap_stats();
        assert!(stats.collections > 1);
        assert!(stats.last_live_objects < 16384);
    }

    /// 500 garbage 640 KiB buffers admit 312 MiB against a 64 MiB limit
    /// while very little is live. Collecting at half the live bytes would
    /// do so every operation or two (hundreds of collections, each marking
    /// the whole realm); half the remaining headroom takes about a dozen.
    #[test]
    fn byte_collections_are_due_after_half_the_remaining_headroom() {
        use crate::frontend::compile_function;
        use crate::{RunOutcome, Runtime, ValueView};
        let runtime = Runtime::with_byte_limit(64 << 20);
        let code = compile_function(
            "function f() { let n = 0; for (let i = 0; i < 500; ++i) n += new ArrayBuffer(655360).byteLength > 0; return n; }",
        )
        .unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(result) = task.run().unwrap() else {
            panic!("allocating the buffers failed");
        };
        assert_eq!(result.view(), ValueView::Number(500.0));
        let stats = runtime.heap_stats();
        assert!(stats.admitted_bytes_total > 4 * (64 << 20), "{stats:?}");
        assert!((4..=24).contains(&stats.collections), "{stats:?}");
    }

    /// A form value's shared body is charged with its slot, beyond the
    /// trie node a plain persistent vector of the same element also has.
    #[test]
    fn form_collection_bodies_are_charged_with_their_slots() {
        use crate::frontend::compile_function;
        use crate::{RunOutcome, Runtime};
        let retained = |constructor: &str| {
            let runtime = Runtime::with_byte_limit(64 << 20);
            let code = compile_function(&format!(
                "function f() {{ let kept = []; for (let i = 0; i < 1000; ++i) kept.push({constructor}(i)); return kept; }}"
            ))
            .unwrap();
            let before = runtime.heap().byte_budget().used();
            let task = runtime.spawn(&code, &[]).unwrap();
            let RunOutcome::Complete(kept) = task.run().unwrap() else {
                panic!("{constructor} failed");
            };
            runtime.collect().unwrap();
            let used = runtime.heap().byte_budget().used() - before;
            drop(kept);
            used
        };
        let forms = retained("__rt.formVector");
        let vectors = retained("__rt.vector");
        let reduced = retained("__rt.cljReduced");
        let body = std::mem::size_of::<crate::collections::form::FormObject>();
        assert!(forms >= vectors + 1000 * body, "{forms} vs {vectors}");
        assert!(reduced >= 1000 * body, "reduced marker bodies: {reduced}");
    }

    #[test]
    fn checked_imports_keep_body_charge_until_last_alias_drops() {
        use crate::Primitive;
        use crate::heap::{Heap, Scope};
        let heap = Arc::new(Heap::with_byte_limit(4096));
        let baseline = heap.byte_budget().used();
        let text = {
            let scope = Scope::new(&heap);
            let local = scope.try_string_units(&[120; 1000]).unwrap();
            let text = scope.string(local).unwrap();
            let occupied = heap.byte_budget().used();
            let alias = scope
                .try_primitive(Primitive::String(text.clone()))
                .unwrap();
            assert_eq!(scope.string(alias).unwrap(), text);
            assert_eq!(heap.byte_budget().used(), occupied);
            assert!(scope.try_string_units(&[120; 2000]).is_none());
            assert_eq!(heap.byte_budget().used(), occupied);
            text
        };
        assert!(heap.byte_budget().used() > baseline);
        drop(text);
        assert_eq!(heap.byte_budget().used(), baseline);
    }

    #[test]
    fn atom_denial_preserves_current_value_and_gc_refunds_retired_versions() {
        use crate::{AtomErrorKind, CasOutcome, Primitive, Runtime, ValueView};
        let runtime = Runtime::with_byte_limit(4096);
        let zero = runtime.root(Primitive::Number(0.0));
        let one = runtime.root(Primitive::Number(1.0));
        let atom = runtime.new_atom(&zero).unwrap();
        let budget = runtime.heap().byte_budget();
        budget.set_limit(budget.used() + 512);
        let mut expected = zero.clone();
        let mut new = one.clone();
        let mut successful = 0;
        loop {
            match runtime.atom_compare_and_set(&atom, &expected, &new) {
                Ok(CasOutcome::Swapped) => {
                    std::mem::swap(&mut expected, &mut new);
                    successful += 1;
                }
                Err(error) => {
                    assert_eq!(error.kind(), AtomErrorKind::HeapLimit);
                    break;
                }
                other => panic!("unexpected CAS {other:?}"),
            }
        }
        assert_eq!(successful, 2);
        assert_eq!(runtime.atom_load(&atom).unwrap().view(), expected.view());
        runtime.collect().unwrap();
        assert!(matches!(
            runtime
                .atom_compare_and_set(&atom, &expected, &new)
                .unwrap(),
            CasOutcome::Swapped
        ));
        assert!(matches!(
            runtime.atom_load(&atom).unwrap().view(),
            ValueView::Number(_)
        ));
    }
}
