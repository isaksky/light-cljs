//! Stable slot storage and the heap's only unsafe boundary.
//!
//! **Slots.** Segments never move or disappear before the heap dies. A slot's
//! atomic state word holds its generation and flags; its body is a boxed
//! [`HeapObject`] behind an `UnsafeCell`. Every rule below is stated for one
//! slot, and every unsafe body access in this module names the rule it uses.
//!
//! 1. **Initialization.** A reservation exclusively owns empty slots removed
//!    from the table's free list (or a never-issued range). Only that
//!    reservation may write their bodies. It publishes the body with a Release
//!    state store and then relinquishes the slot. Every accessor Acquires the
//!    state and touches the body only if it names the expected live
//!    generation. Empty reserved slots are skipped by sweep and remain with
//!    their mutator.
//! 2. **Frozen bodies** (`FROZEN`) never change while the slot is live: any
//!    thread may hold shared references to them without any lock. Only kinds
//!    listed in [`freezable`] are frozen; their types are `Sync` (asserted
//!    below) and have no conditional storage. Persistent collections, form
//!    values and atom handles are frozen when allocated. Functions, built-ins and realm
//!    intrinsics become frozen when they are marked known shareable. The flag
//!    is set while holding the slot's access word (rule 3), so the last
//!    mutation happens-before the Release that sets it, and it is never
//!    cleared while the slot is live. Nobody creates a mutable reference to a
//!    frozen body: [`Directory::write`] gives its closure a private copy.
//! 3. **Mutable bodies** are reached only while holding the slot's access
//!    word (`Slot::locked`), a per-object spin lock. One holder at a time
//!    gets a shared or mutable reference; nothing escapes the closure that
//!    receives it. Memory safety therefore does not depend on task identity
//!    or the scheduler: duplicate task IDs, host threads and foreign tasks
//!    can only contend for the word. The engine's ownership rules (only the
//!    owning task's run changes an owned object, and a task runs on one
//!    worker at a time) make that word uncontended in ordinary execution, and
//!    its cache line stays with the owning worker. Owner checks and foreign
//!    faults remain the closures' job, exactly as before.
//! 4. **Multiple words.** A thread holding an access word never waits for
//!    another one, except [`Directory::mark_shareable_unless`], which takes
//!    the words of distinct slots in ascending index order. The object table
//!    lock may be held while taking access words (GC, statistics); the
//!    reverse never happens. No heap lock and no safepoint is taken while an
//!    access word is held.
//! 5. **Reclamation** holds the stopped-world rendezvous, the object-table
//!    lock and each reclaimed slot's access word: no reservation is
//!    initializing a slot, no table reader and no mutable-body holder can
//!    overlap. A lock-free frozen read is made only by an unparked
//!    registered mutator (which delays the rendezvous), by a thread that
//!    keeps the object reachable from a root (so it is not reclaimed), or
//!    under the table lock. Sweep resets the state word, clearing `FROZEN`,
//!    before the slot can be reused.
//! 6. **Lifetime.** Mutator tokens borrow the heap and remove their
//!    thread-local reservations before the last token drops, so neither TLS
//!    nor a raw object reference can outlive its storage.

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
const FROZEN: u64 = 1 << 37;

/// Kinds whose bodies may be frozen (module docs, rule 2). An ordinary
/// object or array qualifies only as a realm intrinsic or template array;
/// functions and built-ins only once known shareable, when their one
/// mutable edge can no longer be attached.
fn freezable(object: &HeapObject) -> bool {
    match object {
        HeapObject::Vector(_)
        | HeapObject::Map(_)
        | HeapObject::Occurrence(_)
        | HeapObject::Form(_)
        | HeapObject::Atom(_)
        | HeapObject::Function(_)
        | HeapObject::BoundFunction(_)
        | HeapObject::Builtin(_) => true,
        HeapObject::Plain(o) => o.is_intrinsic(),
        HeapObject::Array(a) => a.object().is_intrinsic(),
        _ => false,
    }
}

/// Kinds that never change after allocation, frozen from the start.
fn frozen_at_allocation(object: &HeapObject) -> bool {
    matches!(
        object,
        HeapObject::Vector(_)
            | HeapObject::Map(_)
            | HeapObject::Occurrence(_)
            | HeapObject::Form(_)
            | HeapObject::Atom(_)
    )
}

/// Every freezable kind is shared between threads by reference.
#[allow(dead_code)]
fn freezable_kinds_are_sync() {
    fn sync<T: Sync + ?Sized>() {}
    sync::<crate::collections::PVector>();
    sync::<crate::collections::PMap>();
    sync::<crate::collections::Occurrence>();
    sync::<Arc<crate::collections::form::FormObject>>();
    sync::<Arc<crate::atom::AtomCell>>();
    sync::<crate::vm::function::FunctionObject>();
    sync::<crate::vm::function::BoundFunction>();
    sync::<crate::vm::builtin::BuiltinFunction>();
    sync::<crate::object::OrdinaryObject>();
    sync::<crate::object::array::ArrayObject>();
}

/// A private copy of a frozen body for [`Directory::write`]: callers inspect
/// a frozen object through the mutable API only to reject the change, and
/// whatever they write to the copy is discarded. `None` for intrinsic
/// ordinary objects carrying internal slots, which no frozen object has.
fn stand_in(object: &HeapObject) -> Option<HeapObject> {
    Some(match object {
        HeapObject::Vector(v) => HeapObject::Vector(v.clone()),
        HeapObject::Map(m) => HeapObject::Map(m.clone()),
        HeapObject::Occurrence(o) => HeapObject::Occurrence(o.clone()),
        HeapObject::Form(f) => HeapObject::Form(f.clone()),
        HeapObject::Atom(a) => HeapObject::Atom(a.clone()),
        HeapObject::Function(f) => HeapObject::Function(f.clone()),
        HeapObject::BoundFunction(f) => HeapObject::BoundFunction(f.clone()),
        HeapObject::Builtin(f) => HeapObject::Builtin(crate::vm::builtin::BuiltinFunction {
            which: f.which,
            properties: f.properties.stand_in()?,
        }),
        HeapObject::Plain(o) => HeapObject::Plain(o.stand_in()?),
        HeapObject::Array(a) => HeapObject::Array(Box::new(a.stand_in()?)),
        _ => return None,
    })
}

#[derive(Debug, Default)]
struct Slot {
    // Low 32 bits: generation. Flags and generation are one atomic snapshot.
    state: AtomicU64,
    marked: AtomicBool,
    /// The access word of a mutable body (module docs, rule 3).
    locked: AtomicBool,
    object: UnsafeCell<Option<Box<HeapObject>>>,
    // Same initialization/reclamation exclusion as object (rules 1 and 5).
    reservation: UnsafeCell<super::budget::ByteReservation>,
}

// SAFETY: the only non-atomic field is accessed under the module rules:
// initialization has an exclusive reservation (1), frozen bodies are only
// shared, and their kinds are Sync (2), mutable bodies have one access-word
// holder at a time, which needs only HeapObject: Send (3), and sweep
// additionally excludes all mutators and table readers (5).
unsafe impl Sync for Slot {}

// The same consistency guarantee as a recovered Mutex: initialization has no
// unwinding step between installing the box and publishing its state, and an
// access word is released on unwind like the object-table lock before it.
impl std::panic::RefUnwindSafe for Slot {}

impl Slot {
    /// The state word, if it names `id`'s live generation (Acquire, rule 1).
    fn current(&self, id: ObjectId) -> Option<u64> {
        let state = self.state.load(Ordering::Acquire);
        (state as u32 == id.generation && state & LIVE != 0).then_some(state)
    }

    /// Take the access word (module docs, rule 3).
    #[inline]
    fn lock(&self) -> SlotGuard<'_> {
        if self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            self.lock_contended();
        }
        SlotGuard(self)
    }

    #[cold]
    fn lock_contended(&self) {
        let mut spins = 0;
        loop {
            while self.locked.load(Ordering::Relaxed) {
                if spins < 64 {
                    spins += 1;
                    std::hint::spin_loop();
                } else {
                    // Holders run short closures without heap locks or
                    // safepoints; yielding lets a descheduled one finish.
                    std::thread::yield_now();
                }
            }
            if self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }

    /// SAFETY: the caller observed this slot live through [`Slot::current`]
    /// and holds a right to share its body: the body is frozen (rule 2),
    /// or the caller holds the access word (rule 3).
    unsafe fn body(&self) -> &HeapObject {
        // SAFETY: per the caller's right, nobody holds a mutable reference;
        // Acquire observed the initializing write, and only sweep, excluded
        // by rule 5, empties a live slot.
        unsafe { (*self.object.get()).as_deref().unwrap() }
    }

    /// SAFETY: the caller observed this slot live and not frozen while
    /// holding its access word, which it keeps for the reference's lifetime.
    #[allow(clippy::mut_from_ref)]
    unsafe fn body_mut(&self) -> &mut HeapObject {
        // SAFETY: the access word excludes every other reference to a
        // mutable body; a body that is not frozen has no lock-free readers.
        unsafe { (*self.object.get()).as_deref_mut().unwrap() }
    }
}

/// A held access word, released on drop (including unwinding).
struct SlotGuard<'a>(&'a Slot);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.0.locked.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
struct Segment {
    slots: Box<[Slot]>,
    live: AtomicUsize,
    _reservation: super::budget::ByteReservation,
}

impl Default for Segment {
    fn default() -> Self {
        Self {
            // Build in heap storage: a temporary 256-slot array and its debug
            // build copies would exhaust the VM's supported 64 KiB stacks.
            slots: (0..SEGMENT_SIZE).map(|_| Slot::default()).collect(),
            live: AtomicUsize::new(0),
            _reservation: Default::default(),
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

/// Lock-free object access: header lookup and the body rules in the module
/// docs. Callers are registered mutators, keep the object reachable, or hold
/// the table lock (rule 5).
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

    #[inline]
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
        self.state(id)
            .is_some_and(|state| state & (SHAREABLE | CONCURRENT) != 0)
    }

    /// The live state word of `id`, without touching its body.
    fn state(&self, id: ObjectId) -> Option<u64> {
        self.slot(id)?.current(id)
    }

    pub(super) fn is_live(&self, id: ObjectId) -> bool {
        self.state(id).is_some()
    }

    #[cfg(test)]
    pub(super) fn known(&self, id: ObjectId) -> Option<bool> {
        Some(self.state(id)? & SHAREABLE != 0)
    }

    /// Run `f` on a live body and its known-shareable bit: frozen bodies
    /// without any lock, others under their access word.
    #[inline]
    pub(super) fn read<R>(
        &self,
        id: ObjectId,
        f: impl FnOnce(&HeapObject, bool) -> R,
    ) -> Option<R> {
        let slot = self.slot(id)?;
        let state = slot.current(id)?;
        if state & FROZEN != 0 {
            // SAFETY: observed live and frozen (rule 2).
            return Some(f(unsafe { slot.body() }, state & SHAREABLE != 0));
        }
        let _word = slot.lock();
        let state = slot.current(id)?;
        // SAFETY: observed live while holding the access word (rule 3).
        Some(f(unsafe { slot.body() }, state & SHAREABLE != 0))
    }

    /// Run `f` with mutable access to a live body. A frozen body is never
    /// lent mutably: `f` gets a [`stand_in`] copy, and changes to it are
    /// discarded (`None` if no copy can be made). With `unless_shareable`,
    /// a known-shareable object is refused instead (`Some(None)`); the check
    /// and the change happen under one access word, which is what marking
    /// takes (see [`Directory::mark_shareable_unless`]).
    #[inline]
    pub(super) fn write<R>(
        &self,
        id: ObjectId,
        unless_shareable: bool,
        f: impl FnOnce(&mut HeapObject) -> R,
    ) -> Option<Option<R>> {
        let slot = self.slot(id)?;
        let mut state = slot.current(id)?;
        if state & FROZEN == 0 {
            let _word = slot.lock();
            state = slot.current(id)?;
            if unless_shareable && state & SHAREABLE != 0 {
                return Some(None);
            }
            if state & FROZEN == 0 {
                // SAFETY: observed live and not frozen while holding the
                // access word, which outlives `f` (rule 3).
                return Some(Some(f(unsafe { slot.body_mut() })));
            }
            // Frozen meanwhile; frozen bodies need no word.
        }
        if unless_shareable && state & SHAREABLE != 0 {
            return Some(None);
        }
        // SAFETY: observed live and frozen (rule 2); only shared here.
        let mut copy = stand_in(unsafe { slot.body() })?;
        Some(Some(f(&mut copy)))
    }

    /// Set the known-shareable bit of a live slot whose access word the
    /// caller holds (or whose body is frozen), freezing a freezable body.
    fn mark_locked(slot: &Slot, id: ObjectId) {
        let Some(state) = slot.current(id) else {
            return;
        };
        if state & FROZEN != 0 {
            slot.state.fetch_or(SHAREABLE, Ordering::Release);
            return;
        }
        // SAFETY: observed live while the caller holds the access word.
        let frozen = freezable(unsafe { slot.body() });
        slot.state.fetch_or(
            SHAREABLE | if frozen { FROZEN } else { 0 },
            Ordering::Release,
        );
    }

    /// Mark every live object in `ids` known shareable, one access word at
    /// a time. Stale ids are ignored.
    pub(super) fn mark_shareable(&self, ids: &[ObjectId]) {
        for &id in ids {
            if let Some(slot) = self.slot(id) {
                let _word = slot.lock();
                Self::mark_locked(slot, id);
            }
        }
    }

    /// The publication protocol: holding the access words of every distinct
    /// slot in `ids` (ascending index, rule 4), fail without marking if
    /// `changed` holds for a live object, otherwise mark all of them. A
    /// function's own-property record is attached through
    /// [`Directory::write`] with `unless_shareable`, under that function's
    /// word, so an attach is ordered entirely before the check (which then
    /// sees it) or after the marking (which refuses it).
    pub(super) fn mark_shareable_unless(
        &self,
        ids: &[ObjectId],
        changed: impl Fn(&HeapObject) -> bool,
    ) -> bool {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        ids.dedup();
        let slots: Vec<(ObjectId, &Slot)> = ids
            .iter()
            .filter_map(|&id| Some((id, self.slot(id)?)))
            .collect();
        let mut words = Vec::with_capacity(slots.len());
        for (i, &(_, slot)) in slots.iter().enumerate() {
            // Two generations of one index share a word: take it once.
            if i == 0 || slots[i - 1].0.index != slots[i].0.index {
                words.push(slot.lock());
            }
        }
        for &(id, slot) in &slots {
            if slot.current(id).is_some() {
                // SAFETY: observed live while holding its access word.
                if changed(unsafe { slot.body() }) {
                    return false;
                }
            }
        }
        for &(id, slot) in &slots {
            Self::mark_locked(slot, id);
        }
        drop(words);
        true
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

/// The allocation and collection table, only reachable through
/// `Heap::objects`. Reservations never borrow or mutate this Vec. Its lock
/// orders refills and sweeps; body access follows the module rules.
#[derive(Debug, Default)]
pub(super) struct ObjectTable {
    segments: Vec<Arc<Segment>>,
    free: Vec<u32>,
    high_water: usize,
}

impl ObjectTable {
    fn reserve(
        &mut self,
        directory: &Directory,
        count: usize,
        budget: &Arc<super::budget::ByteBudget>,
    ) -> Option<Reservation> {
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
                // Covers the stable slots, free-list/segment Vec capacity,
                // and conservatively three new directory pages per segment.
                let bytes = size_of::<Segment>()
                    + SEGMENT_SIZE
                        * (size_of::<Slot>()
                            + 2 * size_of::<u32>()
                            + 2 * size_of::<Arc<Segment>>())
                    + 3 * 256 * size_of::<OnceLock<Box<Page>>>();
                let Some(reservation) = budget.reserve(bytes) else {
                    budget.note_denial();
                    return None;
                };
                let segment = Arc::new(Segment {
                    _reservation: reservation,
                    ..Segment::default()
                });
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
        slot.current(id).map(|_| slot)
    }

    /// Mark a live object and trace its strong edges (stopped world).
    pub(super) fn mark(&self, id: ObjectId, children: &mut dyn FnMut(gc::TraceEdge)) -> bool {
        let Some(slot) = self.slot(id) else {
            return false;
        };
        if slot.marked.swap(true, Ordering::Relaxed) {
            return false;
        }
        let state = slot.state.load(Ordering::Acquire);
        if state & FROZEN != 0 {
            // SAFETY: live (checked above under the table lock, which
            // excludes sweep) and frozen (rule 2).
            unsafe { slot.body() }.trace(children);
        } else {
            let _word = slot.lock();
            // SAFETY: live and holding the access word (rule 3).
            unsafe { slot.body() }.trace(children);
        }
        true
    }

    /// Whether a live object has been marked in the collection in progress
    /// (stopped world, between marking and sweep). A stale identity is
    /// unmarked, as [`ObjectTable::mark`] never marks one.
    pub(super) fn is_marked(&self, id: ObjectId) -> bool {
        self.slot(id)
            .is_some_and(|slot| slot.marked.load(Ordering::Relaxed))
    }

    /// Run the collector's conditional pass over a live object. Frozen kinds
    /// have no conditional storage (rule 2), so they are skipped.
    pub(super) fn trace_conditional(&self, id: ObjectId, visit: &mut dyn gc::ConditionalVisitor) {
        let Some(slot) = self.slot(id) else {
            return;
        };
        let _word = slot.lock();
        if slot.state.load(Ordering::Acquire) & FROZEN == 0 {
            // SAFETY: live, not frozen, holding the access word (rule 3).
            unsafe { slot.body_mut() }.trace_conditional(visit);
        }
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

    /// Free every unmarked live slot and return the bodies, still boxed: a
    /// body is not moved out of its box (a `HeapObject` is 304 bytes on
    /// aarch64, and one collection's garbage can be thousands of them, which
    /// moved into one growing `Vec` cost a fresh multi-MiB allocation and
    /// copy per pause), so the pause copies one pointer per object and the
    /// caller frees each box with its contents after the pause.
    pub(super) fn sweep(&mut self) -> Vec<Box<HeapObject>> {
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
                let word = slot.lock();
                // SAFETY: the caller holds the stopped world and the table
                // lock, and this holds the access word (rule 5): no
                // allocator, table reader or body holder can touch this
                // unreachable body, and no mutator can be reading it.
                let object = unsafe { (*slot.object.get()).take().unwrap() };
                garbage.push(object);
                // SAFETY: the same stopped-world/access-word exclusion as
                // the body above, which this slot no longer holds.
                drop(unsafe { std::mem::take(&mut *slot.reservation.get()) });
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
                drop(word);
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

    /// Visit every live body and its known-shareable bit, under the table
    /// lock (which excludes sweep) and each mutable body's access word.
    pub(super) fn for_each(&self, mut visit: impl FnMut(&HeapObject, bool)) {
        for segment in &self.segments {
            for slot in &segment.slots {
                let state = slot.state.load(Ordering::Acquire);
                if state & LIVE == 0 {
                    continue;
                }
                if state & FROZEN != 0 {
                    // SAFETY: live and frozen (rule 2); sweep is excluded.
                    visit(unsafe { slot.body() }, state & SHAREABLE != 0);
                } else {
                    let _word = slot.lock();
                    // A slot stays live while the table lock is held.
                    let state = slot.state.load(Ordering::Acquire);
                    // SAFETY: live and holding the access word (rule 3).
                    visit(unsafe { slot.body() }, state & SHAREABLE != 0);
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
    fn alloc(
        &mut self,
        object: HeapObject,
        shareable: bool,
        reservation: super::budget::ByteReservation,
    ) -> ObjectId {
        self.len -= 1;
        let offset = self.indices[self.len];
        let slot = &self.segment.slots[usize::from(offset)];
        let state = slot.state.load(Ordering::Relaxed);
        debug_assert_eq!(state & (LIVE | RETIRED), 0);
        let concurrent = share::classify(&object) == Shareability::ConcurrentReference;
        let frozen = frozen_at_allocation(&object) || (shareable && freezable(&object));
        // SAFETY: this reservation exclusively owns the empty slot; no other
        // reservation or free-list entry names it (rule 1). Accessors skip
        // empty slots, even under its access word, and the mutator
        // registration (or fallback table lock) excludes GC.
        unsafe {
            *slot.object.get() = Some(Box::new(object));
            *slot.reservation.get() = reservation;
        }
        self.segment.live.fetch_add(1, Ordering::Relaxed);
        slot.state.store(
            state
                | USED
                | LIVE
                | if shareable { SHAREABLE } else { 0 }
                | if concurrent { CONCURRENT } else { 0 }
                | if frozen { FROZEN } else { 0 },
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
    limit_failed: bool,
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
                    limit_failed: false,
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
            if allocators[index].depth == 0 {
                self.take_byte_denial();
            }
            if allocators[index].depth == 0
                && let Some(reservation) = allocators.swap_remove(index).reservation
                && reservation.len > 0
            {
                lock(&self.objects).release(reservation);
            }
        });
    }

    pub(super) fn allocate(
        &self,
        object: HeapObject,
        shareable: bool,
        reservation: super::budget::ByteReservation,
    ) -> Option<ObjectId> {
        let heap = std::ptr::from_ref(self) as usize;
        ALLOCATORS.with_borrow_mut(|allocators| {
            if let Some(local) = allocators.iter_mut().find(|a| a.heap == heap) {
                if local.reservation.as_ref().is_none_or(|r| r.len == 0) {
                    local.reservation = lock(&self.objects).reserve(
                        &self.slot_directory,
                        BATCH_SIZE,
                        &self.byte_budget,
                    );
                }
                Some(
                    local
                        .reservation
                        .as_mut()?
                        .alloc(object, shareable, reservation),
                )
            } else {
                // Internal setup/tests without a mutator keep the original
                // table-lock exclusion. Every public allocation has a token.
                let mut table = lock(&self.objects);
                Some(
                    table
                        .reserve(&self.slot_directory, 1, &self.byte_budget)?
                        .alloc(object, shareable, reservation),
                )
            }
        })
    }

    pub(super) fn note_limit_failure(&self) {
        let heap = std::ptr::from_ref(self) as usize;
        ALLOCATORS.with_borrow_mut(|allocators| {
            if let Some(local) = allocators.iter_mut().find(|a| a.heap == heap) {
                local.limit_failed = true;
            }
        });
    }

    pub(crate) fn take_limit_failure(&self) -> bool {
        let heap = std::ptr::from_ref(self) as usize;
        ALLOCATORS.with_borrow_mut(|allocators| {
            allocators
                .iter_mut()
                .find(|a| a.heap == heap)
                .is_some_and(|local| std::mem::take(&mut local.limit_failed))
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
                Default::default(),
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
        let id = heap.alloc(HeapObject::host_object(&heap)).unwrap();
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
        let next = heap.alloc(HeapObject::host_object(&heap)).unwrap();
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
                heap.alloc(HeapObject::host_object(&heap)).unwrap();
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

    use crate::ids::TaskId;
    use crate::object::OrdinaryObject;
    use crate::vm::dispatch::Fault;
    use crate::vm::property::{self, Key};

    const OWNER: TaskId = TaskId::from_raw(1);
    const OTHER: TaskId = TaskId::from_raw(2);

    fn owned(heap: &Heap, owner: TaskId) -> ObjectId {
        heap.alloc(HeapObject::Plain(OrdinaryObject::new(heap, Some(owner))))
            .unwrap()
    }

    fn name(s: &str) -> crate::value::JsString {
        crate::value::JsString::from(s)
    }

    fn vector(heap: &Heap, items: &[Value]) -> ObjectId {
        use crate::collections::PVector;
        heap.alloc_persistent(HeapObject::Vector(PVector::from_values(items)), Some(items))
            .unwrap()
    }

    fn vector_len(heap: &Heap, id: ObjectId) -> Option<usize> {
        heap.read(id, |o| match o {
            HeapObject::Vector(v) => v.len(),
            other => panic!("not a vector: {other:?}"),
        })
    }

    fn try_put(
        heap: &Heap,
        task: TaskId,
        id: ObjectId,
        key: &str,
        value: f64,
    ) -> Result<(), Fault> {
        let key = Key::from_name(&name(key));
        property::prepare_put(
            heap,
            task,
            &Value::Object(id),
            &key,
            Value::Number(value),
            true,
        )
        .map(|_| ())
    }

    fn put(heap: &Heap, task: TaskId, id: ObjectId, key: &str, value: f64) {
        assert!(
            try_put(heap, task, id, key, value).is_ok(),
            "owner write failed"
        );
    }

    fn value(heap: &Heap, task: TaskId, id: ObjectId, key: &str) -> Option<Value> {
        get(heap, task, id, key).ok()
    }

    fn get(heap: &Heap, task: TaskId, id: ObjectId, key: &str) -> Result<Value, Fault> {
        property::get_field(heap, task, &Value::Object(id), &name(key))
    }

    /// The access paths of a running task, publication and liveness checks
    /// all finish while another thread holds the object-table lock: owned
    /// reads and writes use the object's own access word, frozen bodies no
    /// lock at all (light-cljs-471).
    #[test]
    fn object_access_and_publication_complete_while_the_table_lock_is_held() {
        use std::sync::mpsc;
        use std::time::Duration;

        let heap = Arc::new(Heap::default());
        let (object, inner, outer, native) = {
            let _access = heap.enter_mutator(gc::Access::Scope);
            let object = owned(&heap, OWNER);
            put(&heap, OWNER, object, "x", 1.0);
            let inner = vector(&heap, &[Value::Number(1.0)]);
            // Not known yet: publication must walk and mark it.
            let outer = heap
                .alloc_persistent(
                    HeapObject::Vector(crate::collections::PVector::from_values(&[Value::Object(
                        inner,
                    )])),
                    Some(&[Value::Bool(true), Value::Object(object)]),
                )
                .unwrap();
            let native = heap
                .alloc(HeapObject::Native(Box::new(Persistent)))
                .unwrap();
            (object, inner, outer, native)
        };
        assert_eq!(heap.known_shareable(outer), Some(false));
        let held = heap.hold_objects();
        let (tx, rx) = mpsc::channel();
        let worker = {
            let heap = heap.clone();
            std::thread::spawn(move || {
                let _access = heap.enter_mutator(gc::Access::Scope);
                put(&heap, OWNER, object, "x", 2.0);
                put(&heap, OWNER, object, "y", 3.0);
                let results = [
                    value(&heap, OWNER, object, "x") == Some(Value::Number(2.0)),
                    value(&heap, OWNER, object, "y") == Some(Value::Number(3.0)),
                    vector_len(&heap, inner) == Some(1),
                    heap.is_live(object) && heap.is_live(outer),
                    heap.read_shareable(native, |_, known| !known) == Some(true),
                    crate::value::share::check(&heap, &Value::Object(outer)).is_ok(),
                    heap.known_shareable(outer) == Some(true),
                    heap.mark_shareable_unless(&[native], |_| false),
                    heap.known_shareable(native) == Some(true),
                ];
                tx.send(results).unwrap();
            })
        };
        let results = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("object access waited for the object-table lock");
        drop(held);
        worker.join().unwrap();
        assert!(results.iter().all(|&ok| ok), "{results:?}");
    }

    #[derive(Debug)]
    struct Persistent;

    impl gc::Trace for Persistent {
        fn trace(&self, _: &mut dyn FnMut(gc::TraceEdge)) {}
    }

    impl super::super::NativePayload for Persistent {
        fn shareability(&self) -> Shareability {
            Shareability::Persistent
        }
    }

    /// Unlocked access keeps the owner check and the generation check:
    /// another task still faults on an owned object, and a reclaimed id
    /// never reaches its slot's new occupant, frozen or mutable. A reused
    /// slot does not inherit the frozen state: its new mutable body changes.
    #[test]
    fn unlocked_access_keeps_owner_faults_and_generation_checks() {
        let heap = Heap::default();
        let object = owned(&heap, OWNER);
        put(&heap, OWNER, object, "x", 1.0);
        assert!(matches!(
            get(&heap, OTHER, object, "x"),
            Err(Fault::ForeignObject {
                owner: Some(OWNER),
                caller: OTHER
            })
        ));
        assert!(matches!(
            try_put(&heap, OTHER, object, "x", 2.0),
            Err(Fault::ForeignObject {
                owner: Some(OWNER),
                caller: OTHER
            })
        ));
        assert_eq!(value(&heap, OWNER, object, "x"), Some(Value::Number(1.0)));

        let frozen = vector(&heap, &[]);
        assert_eq!(heap.collect().unwrap().reclaimed, 2);
        // Free-list order hands the frozen vector's slot out first.
        let reused = owned(&heap, OWNER);
        let other = owned(&heap, OWNER);
        assert_eq!(reused.index, frozen.index);
        assert_eq!(other.index, object.index);
        for stale in [frozen, object] {
            assert!(!heap.is_live(stale));
            assert_eq!(heap.known_shareable(stale), None);
            assert_eq!(heap.read(stale, |_| ()), None);
            assert_eq!(heap.with_object_mut(stale, |_| ()), None);
            assert_eq!(heap.with_unshared_object_mut(stale, |_| ()), None);
        }
        put(&heap, OWNER, reused, "z", 4.0);
        assert_eq!(value(&heap, OWNER, reused, "z"), Some(Value::Number(4.0)));
        assert_eq!(value(&heap, OWNER, other, "x"), Some(Value::Undefined));
    }

    /// A frozen body is never lent mutably: the mutable API inspects a
    /// private copy (enough to reject a change), the shared original is
    /// untouched, and a known-shareable object is refused outright.
    #[test]
    fn frozen_bodies_are_lent_mutably_only_as_discarded_copies() {
        use crate::collections::PVector;
        let heap = Heap::default();
        let id = vector(&heap, &[Value::Number(1.0)]);
        let original = heap.address(id).unwrap();
        let copy = heap
            .with_object_mut(id, |o| {
                let address = std::ptr::from_ref(&*o) as usize;
                *o = HeapObject::Vector(PVector::from_values(&[]));
                address
            })
            .unwrap();
        assert_ne!(copy, original);
        assert_eq!(heap.address(id), Some(original));
        assert_eq!(vector_len(&heap, id), Some(1));
        assert_eq!(heap.with_unshared_object_mut(id, |_| ()), Some(None));

        // A persistent native is known but not frozen: it stays mutable
        // under its access word for the mutable API, and is refused by the
        // shareability-checked one.
        let native = heap
            .alloc(HeapObject::Native(Box::new(Persistent)))
            .unwrap();
        heap.mark_shareable(&[native]);
        let address = heap.address(native).unwrap();
        assert_eq!(
            heap.with_object_mut(native, |o| std::ptr::from_ref(&*o) as usize),
            Some(address)
        );
        assert_eq!(heap.with_unshared_object_mut(native, |_| ()), Some(None));
    }

    /// Workers mutate their own objects without the table lock and read a
    /// shared frozen vector while the main thread collects between barrier
    /// phases: owned values stay exact, unrooted ones are reclaimed, and
    /// frozen reads continue across collections.
    #[test]
    fn owned_and_frozen_access_interleave_with_collections() {
        use super::super::Scope;
        use std::sync::Barrier;

        const WORKERS: usize = 4;
        const ROUNDS: usize = 6;
        const OBJECTS: usize = 40;
        let heap = Arc::new(Heap::default());
        let shared_id = vector(&heap, &[Value::Number(7.0)]);
        let _shared = Rooted::new(&heap, Value::Object(shared_id));
        let phase = Arc::new(Barrier::new(WORKERS + 1));
        let mut workers = Vec::new();
        for worker in 0..WORKERS {
            let heap = heap.clone();
            let phase = phase.clone();
            workers.push(std::thread::spawn(move || {
                let _thread = heap.register_thread();
                let task = TaskId::from_raw(10 + worker as u64);
                for round in 0..ROUNDS {
                    let scope = Scope::new(&heap);
                    let mut ids = Vec::new();
                    for i in 0..OBJECTS {
                        let id = owned(&heap, task);
                        let value = (worker * 1000 + round * 100 + i) as f64;
                        put(&heap, task, id, "v", value);
                        put(&heap, task, id, "v", value + 0.5);
                        assert_eq!(vector_len(&heap, shared_id), Some(1));
                        if i % 2 == 0 {
                            scope.new_local(Value::Object(id));
                        }
                        ids.push((id, value + 0.5));
                    }
                    scope.released(|| {
                        phase.wait();
                        phase.wait();
                    });
                    for (i, &(id, expected)) in ids.iter().enumerate() {
                        if i % 2 == 0 {
                            assert_eq!(value(&heap, task, id, "v"), Some(Value::Number(expected)));
                            assert!(matches!(
                                get(&heap, TaskId::from_raw(99), id, "v"),
                                Err(Fault::ForeignObject { .. })
                            ));
                        } else {
                            assert!(!heap.is_live(id));
                            assert_eq!(heap.read(id, |_| ()), None);
                        }
                    }
                    assert_eq!(vector_len(&heap, shared_id), Some(1));
                }
            }));
        }
        for round in 0..ROUNDS {
            phase.wait();
            let stats = heap.collect().unwrap();
            // This round's unrooted half, plus the previous round's rooted
            // half once its scope closed.
            let rounds = if round == 0 { 1 } else { 2 };
            assert_eq!(stats.reclaimed, rounds * WORKERS * OBJECTS / 2);
            phase.wait();
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(vector_len(&heap, shared_id), Some(1));
    }

    use super::super::Rooted;
}
