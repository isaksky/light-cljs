//! Task-owned JavaScript Map storage (clj-quickjs-169).
//!
//! Entries keep insertion positions for the lifetime of the Map. Deletion
//! and clear empty slots, releasing their edges; reinsertion appends. No
//! compaction occurs, so live iteration can retain a slot cursor.
//! Hashing and equality are private, never calling JavaScript.

use std::collections::HashMap;

use crate::collections::map::{canonical_key, key_eq, key_hash};
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;

#[derive(Debug)]
pub(crate) struct MapObject {
    object: OrdinaryObject,
    entries: Vec<Option<(Value, Value)>>,
    // Hash collisions are resolved by SameValueZero, never hash alone.
    index: HashMap<u64, Vec<usize>>,
    size: usize,
}

impl MapObject {
    pub(crate) fn new(owner: TaskId, prototype: ObjectId) -> Self {
        Self {
            object: OrdinaryObject::with_prototype(owner, prototype),
            entries: Vec::new(),
            index: HashMap::new(),
            size: 0,
        }
    }

    pub(crate) fn object(&self) -> &OrdinaryObject {
        &self.object
    }

    pub(crate) fn object_mut(&mut self) -> &mut OrdinaryObject {
        &mut self.object
    }

    pub(crate) fn size(&self) -> usize {
        self.size
    }

    /// None is the current end; Some(None) is a tombstone. Callers re-read
    /// on every step so appends and deletions during callbacks stay visible.
    pub(crate) fn entry(&self, index: u64) -> Option<&Option<(Value, Value)>> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.entries.get(i))
    }

    fn find(&self, key: &Value) -> Option<usize> {
        self.index.get(&key_hash(key))?.iter().copied().find(|&i| {
            self.entries[i]
                .as_ref()
                .is_some_and(|(k, _)| key_eq(k, key))
        })
    }

    pub(crate) fn get(&self, key: &Value) -> Option<&Value> {
        self.entries[self.find(key)?].as_ref().map(|(_, v)| v)
    }

    pub(crate) fn set(&mut self, key: Value, value: Value) {
        if let Some(i) = self.find(&key) {
            self.entries[i].as_mut().expect("indexed live entry").1 = value;
        } else {
            let key = canonical_key(key);
            self.index
                .entry(key_hash(&key))
                .or_default()
                .push(self.entries.len());
            self.entries.push(Some((key, value)));
            self.size += 1;
        }
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
        }
        self.entries[i] = None;
        self.size -= 1;
        true
    }

    pub(crate) fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = None;
        }
        self.index.clear();
        self.size = 0;
    }
}

impl Trace for MapObject {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.object.trace(visit);
        for (key, value) in self.entries.iter().flatten() {
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
    fn insertion_positions_survive_updates_deletes_and_clear() {
        let heap = Heap::default();
        let prototype = heap.alloc(crate::heap::HeapObject::host_object()).unwrap();
        let mut map = MapObject::new(TaskId::from_raw(1), prototype);
        let n = Value::Number;
        map.set(n(-0.0), n(1.0));
        map.set(n(2.0), n(2.0));
        map.set(n(0.0), n(3.0));
        assert_eq!(map.entries.len(), 2);
        assert!(matches!(&map.entries[0], Some((Value::Number(k), _)) if k.to_bits() == 0));
        assert!(map.delete(&n(0.0)));
        map.set(n(-0.0), n(4.0));
        assert!(map.entries[0].is_none());
        assert_eq!(map.find(&n(0.0)), Some(2));
        map.clear();
        map.set(n(2.0), n(5.0));
        assert!(map.entries[..3].iter().all(Option::is_none));
        assert_eq!(map.find(&n(2.0)), Some(3));
        assert_eq!(map.size(), 1);
    }

    #[test]
    fn colliding_keys_remain_independent() {
        let heap = Heap::default();
        let prototype = heap.alloc(crate::heap::HeapObject::host_object()).unwrap();
        let mut map = MapObject::new(TaskId::from_raw(1), prototype);
        // Both raw hashes are 2 before the shared finalizer. These are
        // distinct JS keys: a null and a finite nonzero number.
        let a = Value::Null;
        let b = Value::Number(f64::from_bits(0x6e75_6d62_6572_0002));
        assert_eq!(key_hash(&a), key_hash(&b));
        assert!(!key_eq(&a, &b));
        map.set(a.clone(), Value::Bool(false));
        map.set(b.clone(), Value::Bool(true));
        assert_eq!(map.size(), 2);
        assert_eq!(map.get(&a), Some(&Value::Bool(false)));
        assert_eq!(map.get(&b), Some(&Value::Bool(true)));
        map.set(a.clone(), Value::Number(1.0));
        map.set(b.clone(), Value::Number(2.0));
        assert_eq!(map.size(), 2);
        assert_eq!(map.get(&a), Some(&Value::Number(1.0)));
        assert_eq!(map.get(&b), Some(&Value::Number(2.0)));
        assert!(map.delete(&a));
        assert!(!map.delete(&a));
        assert_eq!(map.size(), 1);
        assert!(map.get(&a).is_none());
        assert_eq!(map.get(&b), Some(&Value::Number(2.0)));
        map.set(a.clone(), Value::Number(3.0));
        assert_eq!(map.size(), 2);
        assert_eq!(map.find(&a), Some(2));
        assert!(map.delete(&b));
        assert_eq!(map.size(), 1);
        assert!(map.get(&b).is_none());
        assert_eq!(map.get(&a), Some(&Value::Number(3.0)));
        assert!(map.delete(&a));
        assert_eq!(map.size(), 0);
        assert!(map.index.is_empty());
        map.set(a, Value::Undefined);
        map.set(b, Value::Undefined);
        map.clear();
        assert_eq!(map.size(), 0);
        assert!(map.index.is_empty());
        assert!(map.entries.iter().all(Option::is_none));
    }
}
