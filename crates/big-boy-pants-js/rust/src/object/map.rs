//! Task-owned JavaScript Map storage (clj-quickjs-169).
//!
//! Entries have monotonic logical insertion positions, independent of storage.
//! Deletion and clear reclaim entries eagerly; a live cursor finds the first
//! remaining position at or after its next position, including later appends.
//! Hashing and equality are private, never calling JavaScript.

use std::collections::{BTreeMap, HashMap};

use crate::collections::map::{canonical_key, key_eq, key_hash};
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;

#[derive(Debug)]
pub(crate) struct MapObject {
    object: OrdinaryObject,
    entries: BTreeMap<u64, (Value, Value)>,
    // Hash collisions are resolved by SameValueZero, never hash alone.
    index: HashMap<u64, Vec<u64>>,
    next_position: u64,
    reservation: crate::heap::budget::ByteReservation,
}

impl MapObject {
    pub(crate) fn template_copy(&self, owner: TaskId) -> Result<Self, &'static str> {
        let reservation = self.reservation.duplicate().ok_or("template: heap byte limit exceeded")?;
        Ok(Self { object: self.object.template_copy(owner)?, entries: self.entries.clone(),
            index: self.index.clone(), next_position: self.next_position, reservation })
    }
    pub(crate) fn template_remap(&mut self, ids: &crate::template::IdMap) {
        self.object.template_remap(ids);
        self.index.clear();
        for (&position, (key, value)) in &mut self.entries {
            crate::template::remap_value(key, ids);
            crate::template::remap_value(value, ids);
            self.index.entry(key_hash(key)).or_default().push(position);
        }
    }
    pub(crate) fn new(heap: &crate::heap::Heap, owner: TaskId, prototype: ObjectId) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(heap, owner, prototype),
            entries: BTreeMap::new(),
            index: HashMap::new(),
            next_position: 0,
            reservation: Default::default(),
        }
    }

    pub(crate) fn object(&self) -> &OrdinaryObject {
        &self.object
    }

    pub(crate) fn object_mut(&mut self) -> &mut OrdinaryObject {
        &mut self.object
    }

    pub(crate) fn size(&self) -> usize {
        self.entries.len()
    }

    /// Return the next live entry and the cursor to save before calling JS.
    /// Positions are never reused, even after clear, so independent suspended
    /// cursors need neither registration nor remapping when storage is freed.
    pub(crate) fn entry(&self, position: u64) -> Option<(u64, &(Value, Value))> {
        self.entries
            .range(position..)
            .next()
            .map(|(&position, entry)| (position + 1, entry))
    }

    fn find(&self, key: &Value) -> Option<u64> {
        self.index
            .get(&key_hash(key))?
            .iter()
            .copied()
            .find(|&i| self.entries.get(&i).is_some_and(|(k, _)| key_eq(k, key)))
    }

    pub(crate) fn get(&self, key: &Value) -> Option<&Value> {
        self.entries.get(&self.find(key)?).map(|(_, v)| v)
    }

    pub(crate) fn set(&mut self, key: Value, value: Value) -> Result<(), &'static str> {
        if let Some(i) = self.find(&key) {
            self.entries.get_mut(&i).expect("indexed live entry").1 = value;
        } else {
            // Reserve u64::MAX as an end cursor; never wrap into an older
            // iteration. Exhaustion leaves both stores unchanged.
            let next = self
                .next_position
                .checked_add(1)
                .ok_or("Map insertion positions exhausted")?;
            let Some(additional) = self.object.budget.reserve(2048) else {
                self.object.budget.note_denial();
                return Err("heap byte limit exceeded");
            };
            self.reservation.merge(additional);
            let key = canonical_key(key);
            self.index
                .entry(key_hash(&key))
                .or_default()
                .push(self.next_position);
            self.entries.insert(self.next_position, (key, value));
            self.next_position = next;
        }
        Ok(())
    }

    pub(crate) fn delete(&mut self, key: &Value) -> bool {
        let Some(i) = self.find(key) else {
            return false;
        };
        let hash = key_hash(key);
        let bucket = self.index.get_mut(&hash).expect("indexed live entry");
        bucket.retain(|&position| position != i);
        if bucket.is_empty() {
            self.index.remove(&hash);
        } else if bucket.capacity() > bucket.len().saturating_mul(4) {
            bucket.shrink_to_fit();
        }
        self.entries.remove(&i);
        // Bound retained hash capacity as well as tree storage. Shrinking
        // geometrically keeps the rebuild cost amortized over deletions.
        if self.index.capacity() > self.index.len().saturating_mul(4) {
            self.index.shrink_to_fit();
        }
        self.reservation.refund(2048);
        true
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.index = HashMap::new();
        self.reservation = Default::default();
    }
}

impl Trace for MapObject {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.object.trace(visit);
        for (key, value) in self.entries.values() {
            key.trace(visit);
            value.trace(visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Heap;

    #[test]
    fn churn_compacts_without_live_cursors() {
        let heap = Heap::default();
        let prototype = heap
            .alloc(crate::heap::HeapObject::host_object(&heap))
            .unwrap();
        for clear in [false, true] {
            let mut map = MapObject::new(&heap, TaskId::from_raw(1), prototype);
            for i in 0..10_000 {
                map.set(Value::Number(0.0), Value::Number(i as f64))
                    .unwrap();
                if clear {
                    map.clear();
                } else {
                    assert!(map.delete(&Value::Number(0.0)));
                }
                assert_eq!(map.size(), 0);
                assert!(map.entries.is_empty());
                assert_eq!(map.index.capacity(), 0);
            }
        }
    }

    #[test]
    fn deletion_reclaims_storage_with_live_cursors() {
        let heap = Heap::default();
        let prototype = heap
            .alloc(crate::heap::HeapObject::host_object(&heap))
            .unwrap();
        let mut map = MapObject::new(&heap, TaskId::from_raw(1), prototype);
        for i in 0..1000 {
            map.set(Value::Number(i as f64), Value::Null).unwrap();
        }
        let (early, _) = map.entry(0).unwrap();
        let (late, _) = map.entry(899).unwrap();
        for i in 0..999 {
            assert!(map.delete(&Value::Number(i as f64)));
        }
        assert_eq!(map.entries.len(), 1);
        assert!(map.index.capacity() <= 4);
        assert_eq!(map.entry(early), map.entry(late));
        assert_eq!(map.entry(early).unwrap().1.0, Value::Number(999.0));
        map.clear();
        assert_eq!(map.entries.len(), 0);
        map.set(Value::Number(3.0), Value::Null).unwrap();
        assert_eq!(map.entry(early).unwrap().1.0, Value::Number(3.0));
        assert_eq!(map.entry(early), map.entry(late));
    }

    #[test]
    fn collision_bucket_capacity_shrinks_after_deletion() {
        let heap = Heap::default();
        let prototype = heap
            .alloc(crate::heap::HeapObject::host_object(&heap))
            .unwrap();
        let mut map = MapObject::new(&heap, TaskId::from_raw(1), prototype);
        let first = Value::Null;
        let survivor = Value::Number(f64::from_bits(0x6e75_6d62_6572_0002));
        let hash = key_hash(&first);
        assert_eq!(hash, key_hash(&survivor));
        map.set(first.clone(), Value::Bool(false)).unwrap();
        map.set(survivor.clone(), Value::Bool(true)).unwrap();
        let (cursor, _) = map.entry(0).unwrap();
        // Model historical collision-bucket allocation independently of
        // the number of convenient distinct keys with the same full hash.
        let bucket = map.index.get_mut(&hash).unwrap();
        bucket.reserve(128);
        assert!(bucket.capacity() >= 128);
        assert!(map.delete(&first));
        let bucket = &map.index[&hash];
        assert_eq!(bucket.len(), 1);
        assert!(bucket.capacity() <= 4);
        assert_eq!(map.get(&survivor), Some(&Value::Bool(true)));
        assert!(map.get(&first).is_none());
        let (next, entry) = map.entry(cursor).unwrap();
        assert_eq!(entry, &(survivor, Value::Bool(true)));
        assert!(map.entry(next).is_none());
    }

    #[test]
    fn insertion_position_exhaustion_never_wraps_or_partially_inserts() {
        let heap = Heap::default();
        let prototype = heap
            .alloc(crate::heap::HeapObject::host_object(&heap))
            .unwrap();
        let mut map = MapObject::new(&heap, TaskId::from_raw(1), prototype);
        map.next_position = u64::MAX - 1;
        map.set(Value::Null, Value::Bool(false)).unwrap();
        let (cursor, _) = map.entry(0).unwrap();
        assert_eq!(cursor, u64::MAX);
        assert!(map.set(Value::Undefined, Value::Null).is_err());
        assert_eq!(map.size(), 1);
        assert_eq!(map.index.len(), 1);
        map.set(Value::Null, Value::Bool(true)).unwrap();
        assert_eq!(map.get(&Value::Null), Some(&Value::Bool(true)));
        assert!(map.entry(cursor).is_none());
        map.clear();
        assert!(map.set(Value::Null, Value::Null).is_err());
        assert!(map.entry(0).is_none());
        assert!(map.index.is_empty());
    }

    #[test]
    fn insertion_positions_survive_updates_deletes_and_clear() {
        let heap = Heap::default();
        let prototype = heap
            .alloc(crate::heap::HeapObject::host_object(&heap))
            .unwrap();
        let mut map = MapObject::new(&heap, TaskId::from_raw(1), prototype);
        let n = Value::Number;
        map.set(n(-0.0), n(1.0)).unwrap();
        map.set(n(2.0), n(2.0)).unwrap();
        map.set(n(0.0), n(3.0)).unwrap();
        assert_eq!(map.entries.len(), 2);
        assert!(matches!(&map.entries[&0], (Value::Number(k), _) if k.to_bits() == 0));
        assert!(map.delete(&n(0.0)));
        map.set(n(-0.0), n(4.0)).unwrap();
        assert!(!map.entries.contains_key(&0));
        assert_eq!(map.find(&n(0.0)), Some(2));
        map.clear();
        map.set(n(2.0), n(5.0)).unwrap();
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.find(&n(2.0)), Some(3));
        assert_eq!(map.size(), 1);
    }

    #[test]
    fn colliding_keys_remain_independent() {
        let heap = Heap::default();
        let prototype = heap
            .alloc(crate::heap::HeapObject::host_object(&heap))
            .unwrap();
        let mut map = MapObject::new(&heap, TaskId::from_raw(1), prototype);
        // Both raw hashes are 2 before the shared finalizer. These are
        // distinct JS keys: a null and a finite nonzero number.
        let a = Value::Null;
        let b = Value::Number(f64::from_bits(0x6e75_6d62_6572_0002));
        assert_eq!(key_hash(&a), key_hash(&b));
        assert!(!key_eq(&a, &b));
        map.set(a.clone(), Value::Bool(false)).unwrap();
        map.set(b.clone(), Value::Bool(true)).unwrap();
        assert_eq!(map.size(), 2);
        assert_eq!(map.get(&a), Some(&Value::Bool(false)));
        assert_eq!(map.get(&b), Some(&Value::Bool(true)));
        map.set(a.clone(), Value::Number(1.0)).unwrap();
        map.set(b.clone(), Value::Number(2.0)).unwrap();
        assert_eq!(map.size(), 2);
        assert_eq!(map.get(&a), Some(&Value::Number(1.0)));
        assert_eq!(map.get(&b), Some(&Value::Number(2.0)));
        assert!(map.delete(&a));
        assert!(!map.delete(&a));
        assert_eq!(map.size(), 1);
        assert!(map.get(&a).is_none());
        assert_eq!(map.get(&b), Some(&Value::Number(2.0)));
        map.set(a.clone(), Value::Number(3.0)).unwrap();
        assert_eq!(map.size(), 2);
        assert_eq!(map.find(&a), Some(2));
        assert!(map.delete(&b));
        assert_eq!(map.size(), 1);
        assert!(map.get(&b).is_none());
        assert_eq!(map.get(&a), Some(&Value::Number(3.0)));
        assert!(map.delete(&a));
        assert_eq!(map.size(), 0);
        assert!(map.index.is_empty());
        map.set(a, Value::Undefined).unwrap();
        map.set(b, Value::Undefined).unwrap();
        map.clear();
        assert_eq!(map.size(), 0);
        assert!(map.index.is_empty());
        assert!(map.entries.is_empty());
    }
}
