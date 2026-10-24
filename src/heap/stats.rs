//! Read-only heap usage snapshot for measurement (clj-quickjs-15,
//! `docs/design.md` §15): what the object table holds, by kind, and the
//! shallow bytes the table itself owns.
//!
//! [`HeapUsage`] is a diagnostic, not an allocator measurement. Its byte
//! fields cover only storage the object table owns: the slot and free-list
//! vectors (by capacity) and one boxed [`HeapObject`] per live object. They
//! exclude storage the table points to but does not own inline: persistent
//! collection trie bodies (immutable `Arc` nodes shared between versions),
//! atom cells, native payload boxes, function captures, ordinary objects'
//! property vectors and key indexes, array object boxes and their element
//! storage, JavaScript Map boxes and their entry/index storage, the property-key table, and everything
//! outside the heap (executable code, task VM state, dependency cells, the
//! root table). The measurement harness (`examples/bench`) measures those
//! with a counting allocator instead; see `docs/measurement.md`.
//! ArrayBuffer backing-store capacity is reported separately in
//! `array_buffer_bytes`; its object box/property storage remains excluded
//! from the shallow fields, as with arrays.
//!
//! Taking a snapshot walks every slot under the object lock: O(slots), for
//! tests and measurement, not for hot paths. It changes nothing.

use std::mem::size_of;

use super::{Heap, HeapObject, Slot, lock};

/// Live heap objects by kind (one field per object kind; append-only like
/// the kind registry).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectCounts {
    /// Ordinary JavaScript objects (object literals, `Scope::new_object`).
    pub plain: usize,
    /// Objects with a Rust native payload.
    pub native: usize,
    /// Persistent vectors.
    pub vector: usize,
    /// Persistent maps.
    pub map: usize,
    /// Syntax occurrences.
    pub occurrence: usize,
    /// Task-owned transient builders (open or frozen-and-closed).
    pub builder: usize,
    /// Function objects.
    pub function: usize,
    /// Binding cells of captured mutable bindings.
    pub cell: usize,
    /// Atoms.
    pub atom: usize,
    /// Built-in functions of the realm (`Object`, `Object.is`, ...).
    pub builtin: usize,
    /// Array objects (array literals).
    pub array: usize,
    /// Bound function objects.
    pub bound_function: usize,
    /// Task-owned JavaScript Map objects.
    pub js_map: usize,

    /// Task-owned ArrayBuffer objects.
    pub array_buffer: usize,
    /// Task-owned typed-array views (backing bytes counted on their buffer).
    pub typed_array: usize,
}

impl ObjectCounts {
    /// All live objects.
    pub fn total(&self) -> usize {
        self.plain
            + self.native
            + self.vector
            + self.map
            + self.occurrence
            + self.builder
            + self.function
            + self.cell
            + self.atom
            + self.builtin
            + self.array
            + self.bound_function
            + self.js_map
            + self.array_buffer
            + self.typed_array
    }

    fn count(&mut self, object: &HeapObject) {
        let n = match object {
            HeapObject::Plain(_) => &mut self.plain,
            HeapObject::Native(_) => &mut self.native,
            HeapObject::Vector(_) => &mut self.vector,
            HeapObject::Map(_) => &mut self.map,
            HeapObject::Occurrence(_) => &mut self.occurrence,
            HeapObject::Builder(_) => &mut self.builder,
            HeapObject::Function(_) => &mut self.function,
            HeapObject::BoundFunction(_) => &mut self.bound_function,
            HeapObject::Cell(_) => &mut self.cell,
            HeapObject::Atom(_) => &mut self.atom,
            HeapObject::Builtin(_) => &mut self.builtin,
            HeapObject::Array(_) => &mut self.array,
            HeapObject::JsMap(_) => &mut self.js_map,

            HeapObject::ArrayBuffer(_) => &mut self.array_buffer,
            HeapObject::TypedArray(_) => &mut self.typed_array,
        };
        *n += 1;
    }
}

/// A snapshot of the object table ([`Runtime::heap_usage`](crate::Runtime::heap_usage)).
/// See the module docs for what the byte fields include and exclude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapUsage {
    /// Live objects by kind; `objects.total()` equals
    /// [`HeapStats::objects`](crate::HeapStats::objects) taken at the same
    /// moment.
    pub objects: ObjectCounts,
    /// Live objects whose known-shareable bit is set (persistent objects
    /// built from shareable parts, or proven shareable by a publication
    /// check).
    pub known_shareable: usize,
    /// Slots in the object table: live, free for reuse, or retired.
    pub slots: usize,
    /// Free slots waiting for reuse.
    pub free_slots: usize,
    /// Bytes reserved by the slot and free-list vectors (capacity times
    /// element size).
    pub table_bytes: usize,
    /// Bytes of the live objects' boxed storage: live objects times the size
    /// of one heap object. Shallow: excludes the out-of-table bodies listed
    /// in the module docs.
    pub object_bytes: usize,
    /// Reserved ArrayBuffer backing-store bytes, including capacity beyond length.
    pub array_buffer_bytes: usize,
}

impl HeapUsage {
    /// `table_bytes + object_bytes`.
    pub fn shallow_bytes(&self) -> usize {
        self.table_bytes + self.object_bytes
    }
}

impl Heap {
    /// A [`HeapUsage`] snapshot, taken under the object lock alone (so it
    /// obeys the lock order from any thread that may take that lock).
    pub(crate) fn usage(&self) -> HeapUsage {
        let table = lock(&self.objects);
        let mut objects = ObjectCounts::default();
        let mut known_shareable = 0;
        let mut array_buffer_bytes = 0;
        for slot in &table.slots {
            if let Some(object) = slot.object.as_deref() {
                objects.count(object);
                known_shareable += usize::from(slot.shareable);
                if let HeapObject::ArrayBuffer(buffer) = object {
                    array_buffer_bytes += buffer.bytes.capacity();
                }
            }
        }
        debug_assert_eq!(objects.total(), table.live);
        HeapUsage {
            objects,
            known_shareable,
            slots: table.slots.len(),
            free_slots: table.free.len(),
            table_bytes: table.slots.capacity() * size_of::<Slot>()
                + table.free.capacity() * size_of::<u32>(),
            object_bytes: table.live * size_of::<HeapObject>(),
            array_buffer_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::PVector;

    #[test]
    fn usage_counts_live_objects_by_kind_and_free_slots() {
        let heap = Heap::default();
        let empty = heap.usage();
        assert_eq!(empty.objects.total(), 0);
        assert_eq!(empty.slots, 0);
        assert_eq!(empty.object_bytes, 0);

        heap.alloc(HeapObject::host_object()).unwrap();
        heap.alloc_persistent(HeapObject::Vector(PVector::from_values(&[])), Some(&[]))
            .unwrap();
        let two = heap.usage();
        assert_eq!(two.objects.plain, 1);
        assert_eq!(two.objects.vector, 1);
        assert_eq!(two.objects.total(), heap.stats().objects);
        assert_eq!(
            two.known_shareable, 1,
            "the empty vector is known shareable"
        );
        assert_eq!(two.slots, 2);
        assert_eq!(two.free_slots, 0);
        assert_eq!(two.object_bytes, 2 * size_of::<HeapObject>());
        assert!(two.table_bytes >= 2 * size_of::<Slot>());
        assert_eq!(two.shallow_bytes(), two.table_bytes + two.object_bytes);

        // Nothing is rooted: a collection frees both slots for reuse.
        assert_eq!(heap.collect().unwrap().reclaimed, 2);
        let swept = heap.usage();
        assert_eq!(swept.objects, ObjectCounts::default());
        assert_eq!((swept.slots, swept.free_slots), (2, 2));
        assert_eq!(swept.object_bytes, 0);
    }
}
