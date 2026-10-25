//! Stable slot storage and the heap's only unsafe boundary.
//!
//! A reservation exclusively owns empty slots removed from the table's free
//! list (or a never-issued range). Only that reservation may initialize them.
//! It publishes the body with a Release state store and then relinquishes the
//! slot. Readers Acquire the state before touching a body, and all body access
//! after publication holds `Heap::objects`. Reclamation holds that lock AND the
//! stop-the-world rendezvous: no reservation can be initializing a slot then.
//! Empty reserved slots are skipped by sweep and remain with their mutator.
//! Segments never move or disappear before the heap dies. Mutator tokens borrow
//! the heap and remove their thread-local reservations before the last token
//! drops, so neither TLS nor a raw object reference can outlive its storage.

use std::cell::{RefCell, UnsafeCell};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use super::{Heap, HeapObject, ObjectId, gc, lock};
use crate::value::Value;
use crate::value::share::{self, Shareability};

const SEGMENT_SIZE: usize = 256;
const BATCH_SIZE: usize = 64;
const LIVE: u64 = 1 << 32;
const SHAREABLE: u64 = 1 << 33;
const CONCURRENT: u64 = 1 << 34;
const USED: u64 = 1 << 35;
const RETIRED: u64 = 1 << 36;

#[derive(Debug, Default)]
struct Slot {
    // Low 32 bits: generation. Flags and generation are one atomic snapshot.
    state: AtomicU64,
    marked: AtomicBool,
    object: UnsafeCell<Option<Box<HeapObject>>>,
}

// SAFETY: the only non-atomic field is accessed under the protocol above.
// Initialization has an exclusive reservation, published body accesses have
// the object-table lock, and sweep additionally excludes all mutators.
// HeapObject is Send (native payloads need not be Sync): shared body references
// never escape the table guard and never overlap a mutable body reference.
unsafe impl Sync for Slot {}

// The same consistency guarantee as the object-table Mutex: initialization
// has no unwinding step between installing the box and publishing its state,
// and all later body mutations recover through the table lock.
impl std::panic::RefUnwindSafe for Slot {}

#[derive(Debug)]
struct Segment {
    slots: Box<[Slot]>,
    live: AtomicUsize,
}

impl Default for Segment {
    fn default() -> Self {
        Self {
            // Build in heap storage: a temporary 256-slot array and its debug
            // build copies would exhaust the VM's supported 64 KiB stacks.
            slots: (0..SEGMENT_SIZE).map(|_| Slot::default()).collect(),
            live: AtomicUsize::new(0),
        }
    }
}

// Three lazy radix levels cover all 24 segment-index bits (32 slot-index bits).
// OnceLock publishes stable, owned storage: even header lookup needs no raw
// pointer and no table lock. Empty heaps allocate no directory pages.
type Row = [OnceLock<Arc<Segment>>];
type Page = [OnceLock<Box<Row>>];
type Top = [OnceLock<Box<Page>>];

fn directory_level<T>() -> Box<[OnceLock<T>]> {
    (0..256).map(|_| OnceLock::new()).collect()
}

#[derive(Debug, Default)]
pub(super) struct Directory(OnceLock<Box<Top>>);

impl Directory {
    fn install(&self, index: usize, segment: Arc<Segment>) {
        let top = self.0.get_or_init(directory_level);
        let page = top[index >> 16].get_or_init(directory_level);
        let row = page[(index >> 8) & 255].get_or_init(directory_level);
        assert!(
            row[index & 255].set(segment).is_ok(),
            "segment installed twice"
        );
    }

    fn slot(&self, id: ObjectId) -> Option<&Slot> {
        let index = id.index as usize / SEGMENT_SIZE;
        let page = self.0.get()?[index >> 16].get()?;
        let row = page[(index >> 8) & 255].get()?;
        Some(&row[index & 255].get()?.slots[id.index as usize % SEGMENT_SIZE])
    }

    pub(super) fn known_shareable(&self, value: &Value) -> bool {
        let Value::Object(id) = *value else {
            return true;
        };
        self.slot(id).is_some_and(|slot| {
            let state = slot.state.load(Ordering::Acquire);
            state as u32 == id.generation
                && state & LIVE != 0
                && state & (SHAREABLE | CONCURRENT) != 0
        })
    }

    fn bytes(&self) -> usize {
        let Some(top) = self.0.get() else { return 0 };
        size_of::<OnceLock<Box<Page>>>() * 256
            + top
                .iter()
                .filter_map(OnceLock::get)
                .map(|page| {
                    size_of::<OnceLock<Box<Row>>>() * 256
                        + page.iter().filter(|row| row.get().is_some()).count()
                            * size_of::<OnceLock<Arc<Segment>>>()
                            * 256
                })
                .sum::<usize>()
    }
}

/// Safe body access requires borrowing this table, which is only exposed
/// through `Heap::objects`. Reservations never borrow or mutate this Vec.
#[derive(Debug, Default)]
pub(super) struct ObjectTable {
    segments: Vec<Arc<Segment>>,
    free: Vec<u32>,
    high_water: usize,
}

impl ObjectTable {
    fn reserve(&mut self, directory: &Directory, count: usize) -> Option<Reservation> {
        let mut indices = [0; BATCH_SIZE];
        let mut len = 0;
        let segment_index;
        if let Some(&last) = self.free.last() {
            segment_index = last as usize / SEGMENT_SIZE;
            while len < count
                && self
                    .free
                    .last()
                    .is_some_and(|&index| index as usize / SEGMENT_SIZE == segment_index)
            {
                indices[len] = (self.free.pop().unwrap() as usize % SEGMENT_SIZE) as u16;
                len += 1;
            }
            // Allocate in free-list order, including immediate reuse after GC.
            indices[..len].reverse();
        } else {
            let first = u32::try_from(self.high_water).ok()? as usize;
            segment_index = first / SEGMENT_SIZE;
            if segment_index == self.segments.len() {
                let segment = Arc::new(Segment::default());
                directory.install(segment_index, segment.clone());
                self.segments.push(segment);
            }
            len = count.min(SEGMENT_SIZE - first % SEGMENT_SIZE);
            for (offset, index) in indices[..len].iter_mut().rev().enumerate() {
                *index = (first % SEGMENT_SIZE + offset) as u16;
            }
            self.high_water += len;
        }
        Some(Reservation {
            segment: self.segments[segment_index].clone(),
            base: (segment_index * SEGMENT_SIZE) as u32,
            indices,
            len,
        })
    }

    fn release(&mut self, reservation: Reservation) {
        self.free.extend(
            reservation.indices[..reservation.len]
                .iter()
                .map(|&offset| reservation.base + u32::from(offset)),
        );
    }

    fn slot(&self, id: ObjectId) -> Option<&Slot> {
        let slot = self
            .segments
            .get(id.index as usize / SEGMENT_SIZE)?
            .slots
            .get(id.index as usize % SEGMENT_SIZE)?;
        let state = slot.state.load(Ordering::Acquire);
        (state as u32 == id.generation && state & LIVE != 0).then_some(slot)
    }

    pub(super) fn get(&self, id: ObjectId) -> Option<&HeapObject> {
        let slot = self.slot(id)?;
        // SAFETY: Acquire observed a published live body; self's table guard
        // excludes mutation and sweep. Allocation never writes a live slot.
        unsafe { (&*slot.object.get()).as_deref() }
    }

    pub(super) fn get_mut(&mut self, id: ObjectId) -> Option<&mut HeapObject> {
        let slot = self.slot(id)?;
        // SAFETY: exclusive table borrow excludes every other body accessor;
        // Acquire observed publication, and only sweep can empty a live slot.
        unsafe { (&mut *slot.object.get()).as_deref_mut() }
    }

    pub(super) fn known(&self, id: ObjectId) -> Option<bool> {
        Some(self.slot(id)?.state.load(Ordering::Acquire) & SHAREABLE != 0)
    }

    pub(super) fn mark_shareable(&mut self, id: ObjectId) {
        if let Some(slot) = self.slot(id) {
            slot.state.fetch_or(SHAREABLE, Ordering::Release);
        }
    }

    pub(super) fn is_live(&self, id: ObjectId) -> bool {
        self.slot(id).is_some()
    }

    pub(super) fn mark(&mut self, id: ObjectId, children: &mut dyn FnMut(gc::TraceEdge)) -> bool {
        let Some(slot) = self.slot(id) else {
            return false;
        };
        if slot.marked.swap(true, Ordering::Relaxed) {
            return false;
        }
        self.get(id).unwrap().trace(children);
        true
    }

    pub(super) fn take_atom_retirements(&self) -> Vec<Value> {
        let mut out = Vec::new();
        self.for_each(|object, _| {
            if let HeapObject::Atom(cell) = object {
                out.extend(cell.take_retired());
            }
        });
        out
    }

    pub(super) fn sweep(&mut self) -> Vec<HeapObject> {
        let mut garbage = Vec::new();
        for (segment_index, segment) in self.segments.iter().enumerate() {
            let mut reclaimed = 0;
            for (offset, slot) in segment.slots.iter().enumerate() {
                if slot.marked.swap(false, Ordering::Relaxed) {
                    continue;
                }
                let state = slot.state.load(Ordering::Acquire);
                if state & LIVE == 0 {
                    continue;
                }
                // SAFETY: caller holds the stopped world and table lock: no
                // allocator, reader, or writer can touch this live body.
                let object = unsafe { (&mut *slot.object.get()).take().unwrap() };
                garbage.push(*object);
                reclaimed += 1;
                let generation = state as u32;
                let next = if let Some(next) = generation.checked_add(1) {
                    self.free
                        .push((segment_index * SEGMENT_SIZE + offset) as u32);
                    USED | u64::from(next)
                } else {
                    USED | RETIRED | u64::from(generation)
                };
                slot.state.store(next, Ordering::Release);
            }
            segment.live.fetch_sub(reclaimed, Ordering::Relaxed);
        }
        garbage
    }

    pub(super) fn live(&self) -> usize {
        self.segments
            .iter()
            .map(|s| s.live.load(Ordering::Relaxed))
            .sum()
    }

    pub(super) fn for_each(&self, mut visit: impl FnMut(&HeapObject, bool)) {
        for segment in &self.segments {
            for slot in &segment.slots {
                let state = slot.state.load(Ordering::Acquire);
                if state & LIVE != 0 {
                    // SAFETY: table guard excludes body mutation and sweep;
                    // Acquire observed initialization of this live slot.
                    let object = unsafe { (&*slot.object.get()).as_deref().unwrap() };
                    visit(object, state & SHAREABLE != 0);
                }
            }
        }
    }

    pub(super) fn usage(&self, directory: &Directory) -> (usize, usize, usize) {
        let (mut used, mut free) = (0, 0);
        for segment in &self.segments {
            for slot in &segment.slots {
                let state = slot.state.load(Ordering::Acquire);
                used += usize::from(state & USED != 0);
                free += usize::from(state & USED != 0 && state & (LIVE | RETIRED) == 0);
            }
        }
        (
            used,
            free,
            self.segments.len() * (size_of::<Segment>() + SEGMENT_SIZE * size_of::<Slot>())
                + self.segments.capacity() * size_of::<Arc<Segment>>()
                + self.free.capacity() * size_of::<u32>()
                + directory.bytes(),
        )
    }
}

struct Reservation {
    segment: Arc<Segment>,
    base: u32,
    indices: [u16; BATCH_SIZE],
    len: usize,
}

impl Reservation {
    fn alloc(&mut self, object: HeapObject, shareable: bool) -> ObjectId {
        self.len -= 1;
        let offset = self.indices[self.len];
        let slot = &self.segment.slots[usize::from(offset)];
        let state = slot.state.load(Ordering::Relaxed);
        debug_assert_eq!(state & (LIVE | RETIRED), 0);
        let concurrent = share::classify(&object) == Shareability::ConcurrentReference;
        // SAFETY: this reservation exclusively owns the empty slot; no other
        // reservation or free-list entry names it. Readers skip empty slots,
        // and the mutator registration (or fallback table lock) excludes GC.
        unsafe {
            *slot.object.get() = Some(Box::new(object));
        }
        self.segment.live.fetch_add(1, Ordering::Relaxed);
        slot.state.store(
            state
                | USED
                | LIVE
                | if shareable { SHAREABLE } else { 0 }
                | if concurrent { CONCURRENT } else { 0 },
            Ordering::Release,
        );
        ObjectId {
            index: self.base + u32::from(offset),
            generation: state as u32,
        }
    }
}

struct LocalAllocator {
    heap: usize,
    depth: usize,
    reservation: Option<Reservation>,
}

thread_local! {
    static ALLOCATORS: RefCell<Vec<LocalAllocator>> = const { RefCell::new(Vec::new()) };
}

impl Heap {
    pub(super) fn enter_allocator(&self) {
        let heap = std::ptr::from_ref(self) as usize;
        ALLOCATORS.with_borrow_mut(|allocators| {
            if let Some(local) = allocators.iter_mut().find(|a| a.heap == heap) {
                local.depth += 1;
            } else {
                allocators.push(LocalAllocator {
                    heap,
                    depth: 1,
                    reservation: None,
                });
            }
        });
    }

    pub(super) fn leave_allocator(&self) {
        let heap = std::ptr::from_ref(self) as usize;
        ALLOCATORS.with_borrow_mut(|allocators| {
            let index = allocators
                .iter()
                .position(|a| a.heap == heap)
                .expect("registered allocator");
            allocators[index].depth -= 1;
            if allocators[index].depth == 0
                && let Some(reservation) = allocators.swap_remove(index).reservation
                && reservation.len > 0
            {
                lock(&self.objects).release(reservation);
            }
        });
    }

    pub(super) fn allocate(&self, object: HeapObject, shareable: bool) -> Option<ObjectId> {
        let heap = std::ptr::from_ref(self) as usize;
        ALLOCATORS.with_borrow_mut(|allocators| {
            if let Some(local) = allocators.iter_mut().find(|a| a.heap == heap) {
                if local.reservation.as_ref().is_none_or(|r| r.len == 0) {
                    local.reservation =
                        lock(&self.objects).reserve(&self.slot_directory, BATCH_SIZE);
                }
                Some(local.reservation.as_mut()?.alloc(object, shareable))
            } else {
                // Internal setup/tests without a mutator keep the original
                // table-lock exclusion. Every public allocation has a token.
                let mut table = lock(&self.objects);
                Some(
                    table
                        .reserve(&self.slot_directory, 1)?
                        .alloc(object, shareable),
                )
            }
        })
    }
}

use gc::Trace;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_covers_all_slot_index_bits_without_dense_allocation() {
        let directory = Directory::default();
        for index in [0, 255, 256, 65535, 65536, 0xffffff] {
            let segment = Arc::new(Segment::default());
            let mut reservation = Reservation {
                segment: segment.clone(),
                base: (index * SEGMENT_SIZE) as u32,
                indices: [255; BATCH_SIZE],
                len: 1,
            };
            let id = reservation.alloc(
                HeapObject::Vector(crate::collections::PVector::from_values(&[])),
                true,
            );
            directory.install(index, segment);
            assert_eq!(id.index, (index * SEGMENT_SIZE + 255) as u32);
            assert!(directory.known_shareable(&Value::Object(id)));
            assert!(!directory.known_shareable(&Value::Object(ObjectId {
                generation: 1,
                ..id
            })));
        }
        // Just the touched radix pages, even for the final u32 slot index.
        assert!(directory.bytes() < 100_000);
    }

    #[test]
    fn slots_whose_generation_would_wrap_are_retired() {
        let heap = Heap::default();
        let id = heap.alloc(HeapObject::host_object()).unwrap();
        lock(&heap.objects)
            .slot(id)
            .unwrap()
            .state
            .store(LIVE | USED | u64::from(u32::MAX), Ordering::Release);
        let worn = ObjectId {
            index: id.index,
            generation: u32::MAX,
        };
        assert!(heap.is_live(worn));
        assert_eq!(heap.collect().unwrap().reclaimed, 1);
        let next = heap.alloc(HeapObject::host_object()).unwrap();
        assert_ne!(next.index, id.index, "a retired slot is never reused");
        assert!(!heap.is_live(worn));
        assert!(lock(&heap.objects).free.is_empty());
    }
    #[test]
    fn reserved_allocations_and_shareability_checks_do_not_take_the_table_lock() {
        use crate::collections::PVector;
        use std::sync::mpsc;
        use std::time::Duration;

        let heap = Arc::new(Heap::default());
        let _access = heap.enter_mutator(gc::Access::Scope);
        let known = heap
            .alloc_persistent(HeapObject::Vector(PVector::from_values(&[])), Some(&[]))
            .unwrap();
        let parts = [Value::Object(known)];
        let (held_tx, held_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let holder = {
            let heap = heap.clone();
            std::thread::spawn(move || {
                let _held = heap.hold_objects();
                held_tx.send(()).unwrap();
                done_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("reserved allocation blocked on the table lock");
            })
        };
        held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        for i in 1..BATCH_SIZE {
            if i % 2 == 0 {
                heap.alloc(HeapObject::host_object()).unwrap();
            } else {
                let id = heap
                    .alloc_persistent(
                        HeapObject::Vector(PVector::from_values(&parts)),
                        Some(&parts),
                    )
                    .unwrap();
                assert!(heap.slot_directory.known_shareable(&Value::Object(id)));
            }
        }
        done_tx.send(()).unwrap();
        holder.join().unwrap();
    }

    #[derive(Debug)]
    struct Stamp(usize);

    impl gc::Trace for Stamp {
        fn trace(&self, _: &mut dyn FnMut(gc::TraceEdge)) {}
    }

    impl super::super::NativePayload for Stamp {
        fn shareability(&self) -> Shareability {
            Shareability::TaskOwned
        }
    }

    fn stamp(heap: &Heap, id: ObjectId) -> Option<usize> {
        heap.read(id, |object| {
            let HeapObject::Native(payload) = object else {
                panic!("wrong object kind")
            };
            let any: &dyn std::any::Any = payload.as_ref();
            any.downcast_ref::<Stamp>().expect("wrong native object").0
        })
    }

    #[test]
    fn concurrent_allocations_keep_unique_ids_and_correct_objects_across_collections() {
        use super::super::Scope;
        use std::collections::HashSet;
        use std::sync::Barrier;

        const WORKERS: usize = 4;
        const ROUNDS: usize = 8;
        const COUNT: usize = SEGMENT_SIZE * 3 + 17;
        let heap = Arc::new(Heap::default());
        let phase = Arc::new(Barrier::new(WORKERS + 1));
        let mut workers = Vec::new();
        for worker in 0..WORKERS {
            let heap = heap.clone();
            let phase = phase.clone();
            workers.push(std::thread::spawn(move || {
                let _thread = heap.register_thread();
                let mut all_ids = Vec::new();
                for round in 0..ROUNDS {
                    let scope = Scope::new(&heap);
                    let mut allocated = Vec::new();
                    for offset in 0..COUNT {
                        let value = (worker * ROUNDS + round) * COUNT + offset;
                        let id = heap
                            .alloc(HeapObject::Native(Box::new(Stamp(value))))
                            .unwrap();
                        assert_eq!(stamp(&heap, id), Some(value));
                        if offset % 2 == 0 {
                            scope.new_local(Value::Object(id));
                        }
                        allocated.push((id, value));
                    }
                    // Keep each mutator's partly used reservation across GC.
                    // Only even stamps are roots. The collector must skip the
                    // reservation's empty slots and never issue them elsewhere.
                    scope.released(|| {
                        phase.wait();
                        phase.wait();
                    });
                    for (offset, &(id, value)) in allocated.iter().enumerate() {
                        assert_eq!(stamp(&heap, id), (offset % 2 == 0).then_some(value));
                    }
                    drop(scope); // Return unused slots, including on thread exit.
                    phase.wait();
                    phase.wait();
                    assert!(allocated.iter().all(|&(id, _)| !heap.is_live(id)));
                    all_ids.extend(allocated.into_iter().map(|(id, _)| id));
                }
                all_ids
            }));
        }
        for _ in 0..ROUNDS {
            phase.wait();
            let first = heap.collect().unwrap();
            assert_eq!(first.reclaimed, WORKERS * (COUNT / 2));
            assert_eq!(first.live, WORKERS * COUNT.div_ceil(2));
            phase.wait();
            phase.wait();
            let second = heap.collect().unwrap();
            assert_eq!(second.reclaimed, WORKERS * COUNT.div_ceil(2));
            assert_eq!(second.live, 0);
            phase.wait();
        }
        let mut ids = HashSet::new();
        let mut reused = false;
        for worker in workers {
            for id in worker.join().unwrap() {
                assert!(ids.insert(id), "the same identity was issued twice: {id:?}");
                reused |= id.generation > 0;
                assert_eq!(stamp(&heap, id), None, "stale id resolved after reuse");
            }
        }
        assert!(reused, "the stress must exercise recycled slots");
        assert_eq!(ids.len(), WORKERS * ROUNDS * COUNT);
        assert_eq!(heap.stats().objects, 0);
    }

    #[test]
    fn nested_mutators_return_reservations_on_unwind_and_do_not_mix_heaps() {
        let first = Heap::default();
        let second = Heap::default();
        let _first = first.enter_mutator(gc::Access::Scope);
        let result = std::panic::catch_unwind(|| {
            let _nested = first.enter_mutator(gc::Access::Op);
            let _second = second.enter_mutator(gc::Access::Scope);
            first.alloc(HeapObject::Native(Box::new(Stamp(1)))).unwrap();
            second
                .alloc(HeapObject::Native(Box::new(Stamp(2))))
                .unwrap();
            panic!("unwind nested allocation scopes");
        });
        assert!(result.is_err());
        let id = first.alloc(HeapObject::Native(Box::new(Stamp(3)))).unwrap();
        assert_eq!(stamp(&first, id), Some(3));
        assert_eq!(second.collect().unwrap().reclaimed, 1);
        ALLOCATORS.with_borrow(|locals| {
            assert_eq!(locals.len(), 1);
            assert_eq!(locals[0].heap, std::ptr::from_ref(&first) as usize);
            assert_eq!(locals[0].depth, 1);
        });
        drop(_first);
        ALLOCATORS.with_borrow(|locals| assert!(locals.is_empty()));
    }
}
