//! Atoms: native atomic references to shareable values, with single-writer
//! publication and multi-writer identity compare-and-set (`docs/design.md`
//! §§4–6, 11).
//!
//! # Three distinct contracts
//!
//! | Operation | Who may write | Repeats | Comparison |
//! |-----------|---------------|---------|------------|
//! | Single-writer publication ([`Publisher::publish`]) | the one [`Publisher`] of a single-writer atom | any number of stores, each replacing the last | none (unconditional store) |
//! | Multi-writer compare-and-set ([`Runtime::atom_compare_and_set`], VM `atom_cas`) | anyone holding the atom | any number | identity (below) |
//! | Dependency completion ([`Runtime::complete`](crate::Runtime::complete)) | the cell's claimed producer | exactly once, never replaced | none (`crate::dep`) |
//!
//! An atom is created in one of two modes and keeps it. A **shared** atom
//! ([`Runtime::new_atom`], VM `atom_new`) changes only by compare-and-set. A
//! **single-writer** atom ([`Runtime::new_single_writer_atom`]) changes only
//! through its [`Publisher`], a capability that is not `Clone` and publishes
//! through `&mut self`, so its stores are totally ordered by construction;
//! compare-and-set on it is rejected with [`AtomError::SingleWriter`] (from
//! Rust and from the VM alike), so no CAS ever races the single writer.
//! Both modes are read the same way. Unlike a dependency cell, an atom always
//! has a value, can be replaced any number of times, has no producer claim,
//! no waiters and no wakeups; there is no add-watch.
//!
//! # Values, handles and publication
//!
//! An atom is a heap object (`HeapObject::Atom`) whose handle is an ordinary
//! value: its stable `ObjectId` (slot plus generation). Embedders hold it as a
//! [`Rooted`] or [`Local`]; tasks hold it in frames and may store it in
//! persistent collections. Atoms are explicit concurrent references
//! (`value::share`): shareable without being immutable, and the publication
//! walk does not descend into them. In exchange, **every operation that
//! installs a value** (creation, CAS, publication) first runs the
//! publication check on it and rejects task-owned mutation (plain objects,
//! builders, values reaching them) with [`AtomError::NotShareable`], naming
//! the path; the atom is left unchanged. Values are never copied: a reader
//! gets the identical object the writer installed.
//!
//! # CAS comparison: identity, never user equality
//!
//! `compare_and_set(atom, expected, new)` succeeds when the atom's current
//! value is **the same value** as `expected` by SameValue (JavaScript
//! `Object.is`): heap objects by identity (`ObjectId`), never by structure
//! (two equal vectors that are distinct objects do not match); numbers by
//! value with `NaN` matching `NaN` and `+0` not matching `-0`; `undefined`,
//! `null` and booleans by value. No user hashing, equality, getter or
//! callback runs (QuickJS's `js_strict_eq` is JavaScript `===`, which differs
//! on `NaN` and `±0`; it was a reference only).
//!
//! **Failure contract.** A failed CAS changes nothing and reports the value
//! it found (`CasOutcome::Conflict { current }` from Rust; from the VM, the
//! witness under a `false` flag). The runtime never retries with user code:
//! there is no `swap`-with-callback in this slice. A caller computes its new
//! value (including any effect or expensive work) once, outside the CAS, and
//! decides itself whether to recompute after a conflict. The only internal
//! retry is in the store itself: when the cell changed between reading and
//! swapping but still holds a value that is SameValue to `expected`, the
//! store is attempted again; nothing outside this module runs in that loop.
//!
//! **ABA.** CAS compares values, so an atom changed `A → B → A` matches an
//! expected `A` (as Clojure's `compare-and-set!`). It cannot match a
//! *different* object that merely reuses `A`'s storage: an `ObjectId` carries
//! its slot generation and is never reissued, and `expected` is a live
//! rooted value anyway (a [`Rooted`], or an operand in the task's frame).
//! A stale atom handle fails the same way (a reclaimed atom cannot be
//! named; a [`WeakHandle`](crate::WeakHandle) to it no longer upgrades).
//!
//! # Representation and memory ordering
//!
//! An atom's cell (`AtomCell`, independently owned `Arc` storage referenced
//! by the heap object, like collection bodies) holds an `AtomicPtr` to an
//! immutable boxed value. Each operation first takes two short heap metadata
//! locks: the `gc` lock to register as a mutator (below) and the object lock
//! to clone the cell's `Arc`, as every heap object access does. The value
//! itself is then read, compared and replaced with Rust atomics only:
//!
//! * a write allocates and fills its box, then installs it with
//!   `compare_exchange(_, _, AcqRel, Acquire)` (CAS) or `swap(_, AcqRel)`
//!   (single-writer publication). The `Release` half publishes the box and
//!   everything the writer did before it, including building the value's
//!   graph;
//! * a read loads the pointer with `Acquire`, so it sees a fully initialized
//!   box and every object of the graph as initialized by the writer;
//!   persistent objects never change afterwards.
//!
//! One pointer per atom gives a single modification order per atom; no
//! `SeqCst` ordering across atoms is promised. There is no volatile access,
//! no per-object mutex, and no global atom: each atom synchronizes alone.
//!
//! # Reclamation of replaced boxes (the only unsafe code)
//!
//! A replaced box may still be read by an operation that loaded its pointer
//! just before the replacement, so it is retired, not freed. Every atom
//! operation runs inside a **mutator registration** (`Heap::enter_mutator`,
//! nested inside a run's registration for VM intrinsics), and a collection
//! runs only while no mutator is registered. So a completed collection is a
//! grace period: a box retired at collection epoch `e` (collections
//! completed when its writer registered) is unreachable by every operation
//! registered after the next collection, and is freed by the next write to
//! that atom that observes an epoch greater than `e`, or when the cell is
//! dropped. A pointer read during an operation cannot be freed and reused
//! within that operation, so the CAS pointer comparison has no ABA either.
//! Memory held by retired boxes is bounded by one box per write between
//! collections. The collector traces the current value only.
//!
//! # Load-to-root
//!
//! Reading a value is only safe if it is rooted before a collection can see
//! it unreferenced (another writer may replace it at any moment). Every load
//! roots its result **inside** the operation's mutator registration, so no
//! collection can run between reading the pointer and rooting the value:
//!
//! * [`Runtime::atom_load`] returns a new [`Rooted`]: the loaded value stays
//!   valid, with its whole graph, until that handle is dropped, however
//!   often the atom is replaced and collected meanwhile; then it is
//!   reclaimable like any other value;
//! * [`Scope::atom_load`] returns a [`Local`] held until the scope ends;
//! * the VM `atom_load` pushes the value on the task's operand stack, a root
//!   for as long as the task holds it, including across a yield.
//!
//! A CAS conflict roots its `current` value the same way.

use std::fmt;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::collections::object_name;
use crate::heap::gc::Trace;
use crate::heap::{HandleError, Heap, HeapObject, Local, ObjectId, Rooted, Scope};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::value::share::{self, PublishError};

/// SameValue (`Object.is`) on raw values: the CAS comparison. See the
/// module docs.
pub(crate) fn same_value(a: Value, b: Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan())
        }
        (Value::Object(x), Value::Object(y)) => x == y,
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => false,
    }
}

/// A replaced box awaiting a grace period.
struct Retired {
    /// Collection epoch of the writer that replaced it.
    epoch: u64,
    ptr: *mut Value,
}

// SAFETY: `ptr` is a `Box<Value>` allocation that has been unlinked from its
// atom and is owned by the retire list; it is only dereferenced by readers
// (shared, read-only, `Value: Sync`) and freed by whoever holds the list's
// lock after the grace period. Moving that ownership between threads is safe.
unsafe impl Send for Retired {}

/// An atom's storage. See the module docs for the representation, the
/// ordering and the reclamation protocol.
pub(crate) struct AtomCell {
    /// Always a valid `Box<Value>` allocation owned by the cell.
    current: AtomicPtr<Value>,
    /// Single-writer mode: changed only by its [`Publisher`].
    single_writer: bool,
    /// Replaced boxes not yet known unreachable. Taken only by writers of
    /// this atom (after their store) and by `Drop`; never by readers or the
    /// collector.
    retired: Mutex<Retirement>,
}

/// The retire list of one atom.
#[derive(Default)]
struct Retirement {
    boxes: Vec<Retired>,
    /// The epoch of the last scan: every box retired at an earlier epoch has
    /// been freed. A write scans only when its epoch is newer, so writes
    /// within one epoch only push, and each box is examined once, by the
    /// first write after the collection that makes it free.
    scanned: u64,
    /// Boxes examined by scans (tests: the work is linear in writes).
    #[cfg(test)]
    examined: usize,
}

impl fmt::Debug for AtomCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AtomCell")
            .field("single_writer", &self.single_writer)
            .finish_non_exhaustive()
    }
}

impl AtomCell {
    fn new(initial: Value, single_writer: bool) -> AtomCell {
        AtomCell {
            current: AtomicPtr::new(Box::into_raw(Box::new(initial))),
            single_writer,
            retired: Mutex::new(Retirement::default()),
        }
    }

    /// Read the value behind `ptr`, a pointer loaded from `current` during
    /// the caller's mutator registration (module docs).
    fn read(ptr: *mut Value) -> Value {
        // SAFETY: `ptr` was loaded from `current`, which only ever holds
        // live `Box<Value>` allocations. A box is freed only after it was
        // unlinked and a collection completed since; collections cannot run
        // while the caller is registered as a mutator (every caller of the
        // cell's methods is: see `Access`), and the caller loaded `ptr`
        // during that registration, so the box is still allocated. Boxes are
        // never written after installation, so a shared read is race-free;
        // the `Acquire` load that produced `ptr` synchronizes with the
        // `Release` store that installed it.
        unsafe { *ptr }
    }

    /// The current value. Caller: registered mutator (`Access`).
    fn load(&self) -> Value {
        Self::read(self.current.load(Ordering::Acquire))
    }

    /// Install `new` if the current value is SameValue to `expected`:
    /// `Ok(old)` on success, `Err(current)` on conflict (nothing changed).
    /// Caller: registered mutator at `epoch`.
    fn compare_and_set(&self, epoch: u64, expected: Value, new: Value) -> Result<Value, Value> {
        let fresh = Box::into_raw(Box::new(new));
        let mut seen = self.current.load(Ordering::Acquire);
        loop {
            let current = Self::read(seen);
            if !same_value(current, expected) {
                // SAFETY: `fresh` came from `Box::into_raw` above and was
                // never installed, so this is its only owner.
                drop(unsafe { Box::from_raw(fresh) });
                return Err(current);
            }
            match self
                .current
                .compare_exchange(seen, fresh, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(old) => {
                    self.retire(epoch, old);
                    return Ok(current);
                }
                // Another writer installed a box in between: compare again
                // against what it installed. No caller code reruns.
                Err(now) => seen = now,
            }
        }
    }

    /// Unconditionally install `new` (single-writer publication). Caller:
    /// registered mutator at `epoch`.
    fn store(&self, epoch: u64, new: Value) {
        let fresh = Box::into_raw(Box::new(new));
        let old = self.current.swap(fresh, Ordering::AcqRel);
        self.retire(epoch, old);
    }

    /// Retire a box this writer just unlinked, and free every box retired
    /// before the last completed collection. Scans only when a collection
    /// has completed since the last scan, so the work is linear in writes.
    fn retire(&self, epoch: u64, old: *mut Value) {
        let mut retired = self.retired.lock().unwrap_or_else(PoisonError::into_inner);
        let retired = &mut *retired;
        if epoch > retired.scanned {
            retired.scanned = epoch;
            #[cfg(test)]
            {
                retired.examined += retired.boxes.len();
            }
            retired.boxes.retain(|r| {
                if r.epoch < epoch {
                    // SAFETY: `r.ptr` was unlinked at epoch `r.epoch`, and a
                    // collection has completed since (`epoch` is this
                    // writer's registration epoch). That collection ran with
                    // no mutator registered, so every operation that could
                    // have loaded `r.ptr` (registered before it was
                    // unlinked) has finished, and later ones cannot load it.
                    // The list owned it.
                    drop(unsafe { Box::from_raw(r.ptr) });
                    false
                } else {
                    true
                }
            });
        }
        retired.boxes.push(Retired { epoch, ptr: old });
    }

    /// Retired boxes still held (tests).
    #[cfg(test)]
    fn retired_len(&self) -> usize {
        self.retired.lock().unwrap().boxes.len()
    }

    /// Boxes examined by retire-list scans so far (tests).
    #[cfg(test)]
    fn examined(&self) -> usize {
        self.retired.lock().unwrap().examined
    }
}

impl Trace for AtomCell {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        // The collector runs with no mutator registered, so no writer can
        // replace or retire the current box during the trace.
        Self::read(self.current.load(Ordering::Acquire)).trace(visit);
    }
}

impl Drop for AtomCell {
    fn drop(&mut self) {
        let current = *self.current.get_mut();
        // SAFETY: `&mut self` means no operation holds this cell (each holds
        // an `Arc` clone while it runs), so nothing can still read the
        // current or any retired box; the cell owns all of them.
        drop(unsafe { Box::from_raw(current) });
        let retired = self
            .retired
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        for r in retired.boxes.drain(..) {
            // SAFETY: as above; each retired box is owned by the list.
            drop(unsafe { Box::from_raw(r.ptr) });
        }
    }
}

/// Why an atom operation was rejected. The atom is unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum AtomError {
    /// A handle belongs to a different runtime.
    Handle(HandleError),
    /// The value used as an atom is not one; names what it is.
    NotAnAtom { found: &'static str },
    /// The value to install reaches task-owned mutation (see
    /// [`Runtime::publish`]).
    NotShareable(PublishError),
    /// Compare-and-set on a single-writer atom: only its [`Publisher`] may
    /// change it.
    SingleWriter,
    /// The heap could not allocate the atom (its object table is full).
    OutOfMemory,
}

impl AtomError {
    /// The error's category, as reported by a failed VM atom intrinsic.
    pub fn kind(&self) -> AtomErrorKind {
        match self {
            AtomError::Handle(_) => AtomErrorKind::ForeignRuntime,
            AtomError::NotAnAtom { found } => AtomErrorKind::NotAnAtom { found },
            AtomError::NotShareable(_) => AtomErrorKind::NotShareable,
            AtomError::SingleWriter => AtomErrorKind::SingleWriter,
            AtomError::OutOfMemory => AtomErrorKind::OutOfMemory,
        }
    }
}

impl fmt::Display for AtomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomError::Handle(e) => write!(f, "atom operation: {e}"),
            AtomError::NotAnAtom { found } => write!(f, "expected an atom, got {found}"),
            AtomError::NotShareable(e) => write!(f, "atom operation: {e}"),
            AtomError::SingleWriter => f.write_str(
                "compare-and-set on a single-writer atom: only its publisher may change it",
            ),
            AtomError::OutOfMemory => f.write_str("heap object table exhausted"),
        }
    }
}

impl std::error::Error for AtomError {}

/// Category of an [`AtomError`], carried by
/// [`VmErrorKind::Atom`](crate::VmErrorKind::Atom); the error message names
/// the details (for example the path to a task-owned object).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomErrorKind {
    /// See [`AtomError::Handle`].
    ForeignRuntime,
    /// See [`AtomError::NotAnAtom`].
    NotAnAtom { found: &'static str },
    /// See [`AtomError::NotShareable`].
    NotShareable,
    /// See [`AtomError::SingleWriter`].
    SingleWriter,
    /// See [`AtomError::OutOfMemory`].
    OutOfMemory,
}

/// Result of [`Runtime::atom_compare_and_set`].
#[derive(Debug)]
pub enum CasOutcome {
    /// The atom held `expected`; it now holds `new`.
    Swapped,
    /// The atom held another value, rooted here for the caller; nothing
    /// changed and nothing was retried.
    Conflict { current: Rooted },
}

/// One atom operation's access to a cell: the mutator registration that
/// makes reading the cell's boxes safe, and the cell itself.
struct Access<'h> {
    cell: Arc<AtomCell>,
    epoch: u64,
    _mutator: crate::heap::gc::MutatorToken<'h>,
}

/// Register as a mutator, then find the atom cell named by `atom`.
fn access(heap: &Heap, atom: Value) -> Result<Access<'_>, AtomError> {
    let mutator = heap.enter_mutator();
    let Value::Object(id) = atom else {
        return Err(AtomError::NotAnAtom {
            found: atom.type_name(),
        });
    };
    let cell = heap
        .read(id, |o| match o {
            HeapObject::Atom(cell) => Ok(Arc::clone(cell)),
            other => Err(object_name(other)),
        })
        // Callers hold `atom` alive; a stale one is reported, not trusted.
        .unwrap_or(Err("reclaimed object"))
        .map_err(|found| AtomError::NotAnAtom { found })?;
    Ok(Access {
        cell,
        epoch: mutator.epoch(),
        _mutator: mutator,
    })
}

/// Create an atom holding `initial` (checked shareable) and pass it to
/// `keep` inside the creating mutator registration: `keep` roots it, or the
/// caller is itself a registered mutator (a VM run) that will.
pub(crate) fn new_atom<R>(
    heap: &Heap,
    initial: Value,
    single_writer: bool,
    keep: impl FnOnce(Value) -> R,
) -> Result<R, AtomError> {
    let _mutator = heap.enter_mutator();
    share::check(heap, initial).map_err(AtomError::NotShareable)?;
    let cell = Arc::new(AtomCell::new(initial, single_writer));
    let id = heap
        .alloc(HeapObject::Atom(cell))
        .ok_or(AtomError::OutOfMemory)?;
    Ok(keep(Value::Object(id)))
}

/// Load `atom`'s current value and pass it to `keep` (which roots it, as in
/// [`new_atom`]) inside the loading mutator registration (load-to-root).
pub(crate) fn load<R>(
    heap: &Heap,
    atom: Value,
    keep: impl FnOnce(Value) -> R,
) -> Result<R, AtomError> {
    let access = access(heap, atom)?;
    Ok(keep(access.cell.load()))
}

/// Multi-writer compare-and-set: `Ok(Ok(()))` when swapped, `Ok(Err(keep(current)))`
/// on conflict, with `current` passed to `keep` (as in [`load`]).
pub(crate) fn compare_and_set<R>(
    heap: &Heap,
    atom: Value,
    expected: Value,
    new: Value,
    keep: impl FnOnce(Value) -> R,
) -> Result<Result<(), R>, AtomError> {
    let access = access(heap, atom)?;
    if access.cell.single_writer {
        return Err(AtomError::SingleWriter);
    }
    share::check(heap, new).map_err(AtomError::NotShareable)?;
    match access.cell.compare_and_set(access.epoch, expected, new) {
        Ok(_) => Ok(Ok(())),
        Err(current) => Ok(Err(keep(current))),
    }
}

/// The capability to publish to one single-writer atom. Exactly one exists
/// per such atom; it is not `Clone`. See the module docs.
#[derive(Debug)]
pub struct Publisher {
    runtime: Runtime,
    atom: Rooted,
}

impl Publisher {
    /// The atom this publisher writes: share it with readers.
    pub fn atom(&self) -> &Rooted {
        &self.atom
    }

    /// Replace the atom's value with `value` (an unconditional release
    /// store). `value` must pass the publication check; on any error the
    /// atom is unchanged.
    pub fn publish(&mut self, value: &Rooted) -> Result<(), AtomError> {
        let heap = self.runtime.heap();
        let value = value.value_in(heap).map_err(AtomError::Handle)?;
        let atom = self.atom.value_in(heap).map_err(AtomError::Handle)?;
        let access = access(heap, atom)?;
        share::check(heap, value).map_err(AtomError::NotShareable)?;
        access.cell.store(access.epoch, value);
        Ok(())
    }
}

/// Panic like [`Runtime::scope`]: a leaf host effect may not touch the heap.
fn assert_not_in_effect() {
    assert!(
        !crate::host::inside_effect(),
        "a leaf host effect may not access atoms"
    );
}

impl Runtime {
    /// Create a shared (multi-writer) atom holding `initial`, which must pass
    /// the publication check. It changes only by
    /// [`Runtime::atom_compare_and_set`]. See [`crate::atom`].
    ///
    /// # Panics
    ///
    /// This and the other atom methods of `Runtime` panic when called from
    /// inside a leaf host effect, which may not access heap values (as
    /// [`Runtime::scope`]).
    pub fn new_atom(&self, initial: &Rooted) -> Result<Rooted, AtomError> {
        assert_not_in_effect();
        let heap = self.heap();
        let initial = initial.value_in(heap).map_err(AtomError::Handle)?;
        new_atom(heap, initial, false, |atom| Rooted::new(heap, atom))
    }

    /// Create a single-writer atom holding `initial` and return its one
    /// [`Publisher`]; readers get the atom from [`Publisher::atom`].
    pub fn new_single_writer_atom(&self, initial: &Rooted) -> Result<Publisher, AtomError> {
        assert_not_in_effect();
        let heap = self.heap();
        let initial = initial.value_in(heap).map_err(AtomError::Handle)?;
        let atom = new_atom(heap, initial, true, |atom| Rooted::new(heap, atom))?;
        Ok(Publisher {
            runtime: self.clone(),
            atom,
        })
    }

    /// Load `atom`'s current value into a new persistent root (load-to-root):
    /// it stays valid until the returned handle is dropped, whatever later
    /// replaces the atom's contents.
    pub fn atom_load(&self, atom: &Rooted) -> Result<Rooted, AtomError> {
        assert_not_in_effect();
        let heap = self.heap();
        let atom = atom.value_in(heap).map_err(AtomError::Handle)?;
        load(heap, atom, |v| Rooted::new(heap, v))
    }

    /// Install `new` in a shared atom if its current value is the same value
    /// as `expected` (identity for objects, SameValue for primitives; never
    /// structural or user equality). `new` must pass the publication check.
    /// A conflict changes nothing and returns the current value rooted; the
    /// caller decides whether to recompute and retry. See [`crate::atom`].
    pub fn atom_compare_and_set(
        &self,
        atom: &Rooted,
        expected: &Rooted,
        new: &Rooted,
    ) -> Result<CasOutcome, AtomError> {
        assert_not_in_effect();
        let heap = self.heap();
        let [atom, expected, new] =
            [atom, expected, new].map(|r| r.value_in(heap).map_err(AtomError::Handle));
        let outcome = compare_and_set(heap, atom?, expected?, new?, |v| Rooted::new(heap, v))?;
        Ok(match outcome {
            Ok(()) => CasOutcome::Swapped,
            Err(current) => CasOutcome::Conflict { current },
        })
    }
}

impl<'s> Scope<'s> {
    /// Load `atom`'s current value as a local of this scope: valid until the
    /// scope ends, whatever later replaces the atom's contents.
    pub fn atom_load(&self, atom: Local<'s>) -> Result<Local<'s>, AtomError> {
        let atom = self.local_value(atom);
        load(self.heap(), atom, |v| self.new_local(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::ObjectId;

    fn cell_of(heap: &Arc<Heap>, atom: &Rooted) -> Arc<AtomCell> {
        let Value::Object(id) = atom.value_in(heap).unwrap() else {
            panic!("not an object")
        };
        heap.read(id, |o| match o {
            HeapObject::Atom(c) => Arc::clone(c),
            _ => panic!("not an atom"),
        })
        .unwrap()
    }

    #[test]
    fn same_value_is_identity_and_object_is() {
        let n = Value::Number;
        assert!(same_value(n(f64::NAN), n(-f64::NAN)));
        assert!(!same_value(n(0.0), n(-0.0)));
        assert!(same_value(n(1.5), n(1.5)));
        assert!(!same_value(n(1.0), Value::Bool(true)));
        assert!(!same_value(Value::Undefined, Value::Null));
        let a = ObjectId::from_index(3);
        assert!(same_value(Value::Object(a), Value::Object(a)));
        assert!(!same_value(
            Value::Object(a),
            Value::Object(ObjectId::from_index(4))
        ));
    }

    #[test]
    fn a_stale_object_id_never_matches_the_slot_s_new_occupant() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let old = heap.alloc(HeapObject::Plain).unwrap();
        rt.collect().unwrap();
        let new = heap.alloc(HeapObject::Plain).unwrap();
        assert_eq!(new.to_bits() >> 32, old.to_bits() >> 32, "slot reused");
        assert!(!same_value(Value::Object(old), Value::Object(new)));
    }

    #[test]
    fn replaced_boxes_are_freed_only_after_a_collection() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let n = |x| rt.root(crate::Primitive::Number(x));
        let atom = rt.new_atom(&n(0.0)).unwrap();
        let cell = cell_of(heap, &atom);
        for i in 0..3 {
            let swapped = rt.atom_compare_and_set(&atom, &n(f64::from(i)), &n(f64::from(i + 1)));
            assert!(matches!(swapped, Ok(CasOutcome::Swapped)));
        }
        assert_eq!(cell.retired_len(), 3, "no grace period has passed");
        rt.collect().unwrap();
        assert!(matches!(
            rt.atom_compare_and_set(&atom, &n(3.0), &n(4.0)),
            Ok(CasOutcome::Swapped)
        ));
        assert_eq!(
            cell.retired_len(),
            1,
            "older boxes freed after the collection"
        );
        // A failed CAS retires nothing.
        assert!(matches!(
            rt.atom_compare_and_set(&atom, &n(3.0), &n(5.0)),
            Ok(CasOutcome::Conflict { .. })
        ));
        assert_eq!(cell.retired_len(), 1);
    }

    #[test]
    fn retirement_work_is_linear_within_an_epoch() {
        let rt = Runtime::new();
        let n = |x| rt.root(crate::Primitive::Number(x));
        let mut publisher = rt.new_single_writer_atom(&n(0.0)).unwrap();
        let cell = cell_of(rt.heap(), publisher.atom());
        for i in 0..1000 {
            publisher.publish(&n(f64::from(i))).unwrap();
        }
        assert_eq!(cell.examined(), 0, "writes within one epoch only push");
        assert_eq!(cell.retired_len(), 1000);
        rt.collect().unwrap();
        for i in 0..1000 {
            publisher.publish(&n(f64::from(i))).unwrap();
        }
        assert_eq!(cell.examined(), 1000, "each retired box examined once");
        assert_eq!(
            cell.retired_len(),
            1000,
            "the first scan freed the old epoch's boxes"
        );
    }

    /// A small persistent graph (a vector holding a vector).
    fn graph(rt: &Runtime) -> Rooted {
        rt.scope(|s| {
            let one = s.primitive(crate::Primitive::Number(1.0));
            let inner = s.new_vector(&[one]).unwrap();
            let v = s.new_vector(&[inner]).unwrap();
            s.persist(v)
        })
    }

    #[test]
    fn load_to_root_excludes_collection_until_the_value_is_rooted() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let initial = graph(&rt);
        let mut publisher = rt.new_single_writer_atom(&initial).unwrap();
        let weak = initial.downgrade();
        drop(initial);
        let atom = publisher.atom().value_in(heap).unwrap();
        let replacement = graph(&rt);
        let held = load(heap, atom, |v| {
            // Between reading the pointer and rooting the value, another
            // worker replaces it and tries to collect.
            std::thread::scope(|t| {
                t.spawn(|| {
                    publisher.publish(&replacement).unwrap();
                    assert_eq!(
                        rt.collect().unwrap_err(),
                        crate::CollectError::MutatorActive
                    );
                })
                .join()
                .unwrap()
            });
            Rooted::new(heap, v)
        })
        .unwrap();
        rt.collect().unwrap();
        assert!(held.same_object(&weak.upgrade().unwrap()), "rooted in time");
        drop(held);
        rt.collect().unwrap();
        assert!(!weak.is_live(), "reclaimable once released");
    }

    #[test]
    fn a_cas_conflict_roots_its_current_value_before_collection_can_run() {
        let rt = Runtime::new();
        let heap = rt.heap();
        let initial = graph(&rt);
        let atom = rt.new_atom(&initial).unwrap();
        let weak = initial.downgrade();
        drop(initial);
        let atom = atom.value_in(heap).unwrap();
        let (expected, new, replacement) = (graph(&rt), graph(&rt), graph(&rt));
        let raw = |r: &Rooted| r.value_in(heap).unwrap();
        let outcome = compare_and_set(heap, atom, raw(&expected), raw(&new), |current| {
            std::thread::scope(|t| {
                t.spawn(|| {
                    // The unrooted `current` is still the atom's value:
                    // replace it, then try to collect.
                    let swapped = compare_and_set(heap, atom, current, raw(&replacement), |_| ());
                    assert!(matches!(swapped, Ok(Ok(()))));
                    assert_eq!(
                        rt.collect().unwrap_err(),
                        crate::CollectError::MutatorActive
                    );
                })
                .join()
                .unwrap()
            });
            Rooted::new(heap, current)
        })
        .unwrap();
        let Err(current) = outcome else {
            panic!("expected a conflict")
        };
        rt.collect().unwrap();
        assert!(current.same_object(&weak.upgrade().unwrap()));
        drop(current);
        rt.collect().unwrap();
        assert!(!weak.is_live());
    }
}
