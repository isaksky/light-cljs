//! Persistent map body: a hash array mapped trie (HAMT) with path copying.
//!
//! Keys are compared with [`key_eq`] (SameValueZero: primitives by value,
//! `NaN` equal to itself, `+0` equal to `-0`, strings by content; heap
//! objects by identity) and hashed with [`key_hash`], a fixed private
//! function of the key's bits, string contents or object identity. A string
//! carries its own immutable contents (`crate::value::JsString`), so neither
//! ever reads another heap object or runs user code,
//! so lookups need no heap lock beyond resolving the map itself.
//!
//! Each trie level consumes 5 hash bits. Keys whose 64-bit hashes are fully
//! equal end in a collision node. Nodes are immutable and shared through
//! `Arc`; an update copies only the path to the changed entry. Iteration
//! order is the trie order: deterministic for the same keys and identities,
//! but otherwise unspecified (not insertion order).

use std::sync::Arc;

#[cfg(test)]
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::value::Value;

const BITS: u32 = 5;
const MASK: u64 = (1 << BITS) - 1;

/// SameValueZero key equality. See the module docs.
pub(crate) fn key_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a.strict_equals(b),
    }
}

/// The stored form of a key: `-0` is stored as `+0` (as JavaScript `Map`
/// does), so a key read back from a map is canonical.
pub(crate) fn canonical_key(key: Value) -> Value {
    match key {
        Value::Number(n) => Value::Number(if n == 0.0 { 0.0 } else { n }),
        other => other,
    }
}

/// Private, deterministic key hash consistent with [`key_eq`].
pub(crate) fn key_hash(key: &Value) -> u64 {
    let raw = match key {
        Value::Undefined => 1,
        Value::Null => 2,
        Value::Bool(false) => 3,
        Value::Bool(true) => 4,
        Value::Number(n) => {
            let n = *n;
            let n = if n == 0.0 {
                0.0
            } else if n.is_nan() {
                f64::NAN
            } else {
                n
            };
            n.to_bits() ^ 0x6e75_6d62_6572_0000
        }
        Value::Object(id) => id.to_bits() ^ 0x6f62_6a65_6374_0000,
        Value::String(s) => s.content_hash() ^ 0x7374_7269_6e67_0000,
        Value::Symbol(s) => s.identity() ^ 0x7379_6d62_6f6c_0000,
        Value::BigInt(n) => {
            // JsBigInt has one canonical form per value. Its derived Hash
            // reads the sign and limb contents (Arc hashes its slice, not
            // its address), in O(limbs) within MAX_BIGINT_BITS, without JS.
            use std::hash::{DefaultHasher, Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            n.hash(&mut hasher);
            hasher.finish() ^ 0x6269_6769_6e74_0000
        }
    };
    // splitmix64 finalizer.
    let mut z = raw;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Same value for the purpose of "nothing changed": numbers by bits (so
/// replacing `0` with `-0` is a change), everything else by `===`.
fn identical(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.to_bits() == y.to_bits(),
        _ => a.strict_equals(b),
    }
}

#[derive(Clone, Debug)]
enum Entry {
    Pair { hash: u64, key: Value, value: Value },
    Child(Arc<Node>),
}

#[derive(Debug)]
enum Node {
    /// One entry per set bit of `bitmap`, in bit order.
    Bitmap { bitmap: u32, entries: Vec<Entry> },
    /// Keys whose full hashes are equal (only below the last hash digit).
    Collision {
        hash: u64,
        pairs: Vec<(Value, Value)>,
    },
}

fn bit_for(hash: u64, shift: u32) -> u32 {
    1 << ((hash >> shift) & MASK)
}

fn position(bitmap: u32, bit: u32) -> usize {
    (bitmap & (bit - 1)).count_ones() as usize
}

/// An immutable map body. Cloning is O(1).
#[derive(Clone, Debug, Default)]
pub(crate) struct PMap {
    len: usize,
    root: Option<Arc<Node>>,
}

enum Assoc {
    /// The key already maps to an identical value.
    Unchanged,
    Replaced(Node),
    Added(Node),
}

enum Dissoc {
    Absent,
    /// The node after removal; `None` when it became empty.
    Removed(Option<Node>),
}

impl PMap {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn get(&self, key: &Value) -> Option<Value> {
        self.get_hashed(key_hash(key), key)
    }

    /// A new version mapping `key` to `value`, or `None` when the key already
    /// maps to an identical value (so the caller can return the original).
    pub(crate) fn assoc(&self, key: Value, value: Value) -> Option<PMap> {
        self.assoc_hashed(key_hash(&key), key, value)
    }

    /// A new version without `key`, or `None` when the key is absent.
    pub(crate) fn dissoc(&self, key: &Value) -> Option<PMap> {
        self.dissoc_hashed(key_hash(key), key)
    }

    fn get_hashed(&self, hash: u64, key: &Value) -> Option<Value> {
        let mut node = self.root.as_deref()?;
        let mut shift = 0;
        loop {
            match node {
                Node::Bitmap { bitmap, entries } => {
                    let bit = bit_for(hash, shift);
                    if bitmap & bit == 0 {
                        return None;
                    }
                    match entries.get(position(*bitmap, bit))? {
                        Entry::Pair { key: k, value, .. } => {
                            return key_eq(k, key).then(|| value.clone());
                        }
                        Entry::Child(child) => {
                            node = child;
                            shift += BITS;
                        }
                    }
                }
                Node::Collision { pairs, .. } => {
                    return pairs
                        .iter()
                        .find(|(k, _)| key_eq(k, key))
                        .map(|p| p.1.clone());
                }
            }
        }
    }

    fn assoc_hashed(&self, hash: u64, key: Value, value: Value) -> Option<PMap> {
        let key = canonical_key(key);
        let Some(root) = &self.root else {
            let root = Node::Bitmap {
                bitmap: bit_for(hash, 0),
                entries: vec![Entry::Pair { hash, key, value }],
            };
            return Some(PMap {
                len: 1,
                root: Some(Arc::new(root)),
            });
        };
        let (root, len) = match assoc_in(root, 0, hash, key, value) {
            Assoc::Unchanged => return None,
            Assoc::Replaced(n) => (n, self.len),
            Assoc::Added(n) => (n, self.len + 1),
        };
        Some(PMap {
            len,
            root: Some(Arc::new(root)),
        })
    }

    fn dissoc_hashed(&self, hash: u64, key: &Value) -> Option<PMap> {
        match dissoc_in(self.root.as_deref()?, 0, hash, key) {
            Dissoc::Absent => None,
            Dissoc::Removed(root) => Some(PMap {
                len: self.len - 1,
                root: root.map(Arc::new),
            }),
        }
    }

    /// Call `f` on every entry, in trie order.
    pub(crate) fn for_each(&self, mut f: impl FnMut(&Value, &Value)) {
        fn walk(node: &Node, f: &mut dyn FnMut(&Value, &Value)) {
            match node {
                Node::Bitmap { entries, .. } => {
                    for entry in entries {
                        match entry {
                            Entry::Pair { key, value, .. } => f(key, value),
                            Entry::Child(child) => walk(child, f),
                        }
                    }
                }
                Node::Collision { pairs, .. } => pairs.iter().for_each(|(k, v)| f(k, v)),
            }
        }
        if let Some(root) = &self.root {
            walk(root, &mut f);
        }
    }

    /// Every entry, in trie order.
    pub(crate) fn to_vec(&self) -> Vec<(Value, Value)> {
        let mut out = Vec::with_capacity(self.len);
        self.for_each(|k, v| out.push((k.clone(), v.clone())));
        out
    }

    /// Addresses of every trie node (tests of structural sharing).
    #[cfg(test)]
    fn node_addresses(&self) -> Vec<usize> {
        fn walk(node: &Arc<Node>, out: &mut Vec<usize>) {
            out.push(Arc::as_ptr(node) as usize);
            if let Node::Bitmap { entries, .. } = &**node {
                for entry in entries {
                    if let Entry::Child(child) = entry {
                        walk(child, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            walk(root, &mut out);
        }
        out
    }
}

/// A node holding exactly the two given pairs, starting at `shift`.
fn merge(shift: u32, a: (u64, Value, Value), b: (u64, Value, Value)) -> Node {
    if shift >= u64::BITS {
        return Node::Collision {
            hash: a.0,
            pairs: vec![(a.1, a.2), (b.1, b.2)],
        };
    }
    let (bit_a, bit_b) = (bit_for(a.0, shift), bit_for(b.0, shift));
    if bit_a == bit_b {
        return Node::Bitmap {
            bitmap: bit_a,
            entries: vec![Entry::Child(Arc::new(merge(shift + BITS, a, b)))],
        };
    }
    let pair = |(hash, key, value)| Entry::Pair { hash, key, value };
    let entries = if bit_a < bit_b {
        vec![pair(a), pair(b)]
    } else {
        vec![pair(b), pair(a)]
    };
    Node::Bitmap {
        bitmap: bit_a | bit_b,
        entries,
    }
}

fn assoc_in(node: &Node, shift: u32, hash: u64, key: Value, value: Value) -> Assoc {
    match node {
        Node::Bitmap { bitmap, entries } => {
            let bit = bit_for(hash, shift);
            let pos = position(*bitmap, bit);
            if bitmap & bit == 0 {
                let mut copy = entries.clone();
                copy.insert(pos, Entry::Pair { hash, key, value });
                return Assoc::Added(Node::Bitmap {
                    bitmap: bitmap | bit,
                    entries: copy,
                });
            }
            let (replacement, added) = match &entries[pos] {
                Entry::Pair {
                    hash: h,
                    key: k,
                    value: v,
                } => {
                    if key_eq(k, &key) {
                        if identical(v, &value) {
                            return Assoc::Unchanged;
                        }
                        let entry = Entry::Pair {
                            hash: *h,
                            key: k.clone(),
                            value,
                        };
                        (entry, false)
                    } else {
                        let child =
                            merge(shift + BITS, (*h, k.clone(), v.clone()), (hash, key, value));
                        (Entry::Child(Arc::new(child)), true)
                    }
                }
                Entry::Child(child) => match assoc_in(child, shift + BITS, hash, key, value) {
                    Assoc::Unchanged => return Assoc::Unchanged,
                    Assoc::Replaced(n) => (Entry::Child(Arc::new(n)), false),
                    Assoc::Added(n) => (Entry::Child(Arc::new(n)), true),
                },
            };
            let mut copy = entries.clone();
            copy[pos] = replacement;
            let node = Node::Bitmap {
                bitmap: *bitmap,
                entries: copy,
            };
            if added {
                Assoc::Added(node)
            } else {
                Assoc::Replaced(node)
            }
        }
        Node::Collision { hash: h, pairs } => {
            // Collision nodes sit below every hash digit, so every key that
            // reaches one has the node's full hash.
            debug_assert_eq!(*h, hash);
            let mut copy = pairs.clone();
            match pairs.iter().position(|(k, _)| key_eq(k, &key)) {
                Some(i) if identical(&pairs[i].1, &value) => Assoc::Unchanged,
                Some(i) => {
                    copy[i].1 = value;
                    Assoc::Replaced(Node::Collision {
                        hash: *h,
                        pairs: copy,
                    })
                }
                None => {
                    copy.push((key, value));
                    Assoc::Added(Node::Collision {
                        hash: *h,
                        pairs: copy,
                    })
                }
            }
        }
    }
}

/// The single pair of a node holding exactly one, so a parent can inline it.
fn single_pair(node: &Node) -> Option<Entry> {
    match node {
        Node::Bitmap { entries, .. } if entries.len() == 1 => match &entries[0] {
            pair @ Entry::Pair { .. } => Some(pair.clone()),
            Entry::Child(_) => None,
        },
        Node::Collision { hash, pairs } if pairs.len() == 1 => Some(Entry::Pair {
            hash: *hash,
            key: pairs[0].0.clone(),
            value: pairs[0].1.clone(),
        }),
        _ => None,
    }
}

fn dissoc_in(node: &Node, shift: u32, hash: u64, key: &Value) -> Dissoc {
    match node {
        Node::Bitmap { bitmap, entries } => {
            let bit = bit_for(hash, shift);
            if bitmap & bit == 0 {
                return Dissoc::Absent;
            }
            let pos = position(*bitmap, bit);
            let replacement = match &entries[pos] {
                Entry::Pair { key: k, .. } if key_eq(k, key) => None,
                Entry::Pair { .. } => return Dissoc::Absent,
                Entry::Child(child) => match dissoc_in(child, shift + BITS, hash, key) {
                    Dissoc::Absent => return Dissoc::Absent,
                    Dissoc::Removed(None) => None,
                    Dissoc::Removed(Some(n)) => {
                        Some(single_pair(&n).unwrap_or_else(|| Entry::Child(Arc::new(n))))
                    }
                },
            };
            let mut copy = entries.clone();
            let bitmap = match replacement {
                Some(entry) => {
                    copy[pos] = entry;
                    *bitmap
                }
                None => {
                    copy.remove(pos);
                    bitmap & !bit
                }
            };
            Dissoc::Removed((!copy.is_empty()).then_some(Node::Bitmap {
                bitmap,
                entries: copy,
            }))
        }
        Node::Collision { hash: h, pairs } => {
            let Some(i) = pairs.iter().position(|(k, _)| key_eq(k, key)) else {
                return Dissoc::Absent;
            };
            let mut copy = pairs.clone();
            copy.remove(i);
            Dissoc::Removed((!copy.is_empty()).then_some(Node::Collision {
                hash: *h,
                pairs: copy,
            }))
        }
    }
}

impl Trace for PMap {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.for_each(|k, v| {
            k.trace(visit);
            v.trace(visit);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn num(n: f64) -> Value {
        Value::Number(n)
    }

    /// Assert `map` holds exactly `model` (an association list).
    fn check(map: &PMap, model: &[(Value, Value)]) {
        assert_eq!(map.len(), model.len());
        for (k, v) in model {
            assert!(
                map.get(k).is_some_and(|got| identical(&got, v)),
                "{k:?} -> {v:?}"
            );
        }
        let entries = map.to_vec();
        assert_eq!(entries.len(), model.len());
        for (k, _) in &entries {
            assert!(model.iter().any(|(mk, _)| key_eq(mk, k)));
        }
    }

    #[test]
    fn key_equality_is_same_value_zero_and_hash_consistent() {
        let pairs = [
            (num(0.0), num(-0.0)),
            (num(f64::NAN), num(-f64::NAN)),
            (num(1.5), num(1.5)),
            (Value::Null, Value::Null),
        ];
        for (a, b) in pairs {
            assert!(key_eq(&a, &b));
            assert_eq!(key_hash(&a), key_hash(&b));
        }
        assert!(!key_eq(&Value::Null, &Value::Undefined));
        assert!(!key_eq(&num(1.0), &Value::Bool(true)));
        let a = Value::Object(ObjectId::from_index(1));
        let b = Value::Object(ObjectId::from_index(2));
        assert!(key_eq(&a, &a) && !key_eq(&a, &b));
        assert_ne!(key_hash(&a), key_hash(&b));
        let s = |t: &str| Value::String(crate::value::JsString::from(t));
        assert!(key_eq(&s("k"), &s("k")) && !key_eq(&s("k"), &s("K")));
        assert_eq!(key_hash(&s("k")), key_hash(&s("k")));
        assert!(!key_eq(&s("1"), &num(1.0)));
        assert_ne!(key_hash(&s("1")), key_hash(&num(1.0)));
    }

    #[test]
    fn negative_zero_keys_are_stored_as_positive_zero() {
        let map = PMap::default().assoc(num(-0.0), Value::Null).unwrap();
        let (k, _) = map.to_vec()[0].clone();
        assert!(matches!(k, Value::Number(z) if z.to_bits() == 0));
        assert!(map.get(&num(0.0)).is_some());
    }

    #[test]
    fn random_operations_match_a_model_and_never_change_old_versions() {
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize
        };
        let mut versions: Vec<(PMap, Vec<(Value, Value)>)> = vec![(PMap::default(), Vec::new())];
        for step in 0..4000 {
            let (map, model) = versions.last().unwrap().clone();
            let key = num((next() % 600) as f64);
            let mut model2 = model.clone();
            let found = model2.iter().position(|(k, _)| key_eq(k, &key));
            let next_map = if next() % 3 == 0 {
                match found {
                    Some(i) => {
                        model2.remove(i);
                        map.dissoc(&key).expect("present key is removed")
                    }
                    None => {
                        assert!(map.dissoc(&key).is_none(), "absent key: unchanged");
                        continue;
                    }
                }
            } else {
                let value = num(step as f64);
                match found {
                    Some(i) => model2[i].1 = value.clone(),
                    None => model2.push((key.clone(), value.clone())),
                }
                let updated = map
                    .assoc(key.clone(), value.clone())
                    .expect("a new value is a change");
                assert!(updated.assoc(key, value).is_none(), "identical: unchanged");
                updated
            };
            check(&next_map, &model2);
            versions.push((next_map, model2));
        }
        for (map, model) in &versions {
            check(map, model);
        }
    }

    #[test]
    fn full_hash_collisions_and_shared_prefixes_are_handled() {
        // Crafted hashes: four keys with one full hash, two sharing all but the
        // top digit, and one unrelated.
        let same = 0xdead_beef_dead_beef_u64;
        let near = same ^ (1 << 62);
        let keys: Vec<(u64, Value)> = vec![
            (same, num(1.0)),
            (same, num(2.0)),
            (same, num(3.0)),
            (same, num(4.0)),
            (near, num(5.0)),
            (7, num(6.0)),
        ];
        let mut map = PMap::default();
        let mut model = Vec::new();
        for (h, k) in &keys {
            map = map.assoc_hashed(*h, k.clone(), k.clone()).unwrap();
            model.push((k.clone(), k.clone()));
            for (mk, mv) in &model {
                let h = keys.iter().find(|(_, x)| key_eq(x, mk)).unwrap().0;
                assert_eq!(map.get_hashed(h, mk), Some(mv.clone()));
            }
        }
        assert_eq!(map.len(), 6);
        assert!(map.assoc_hashed(same, num(2.0), num(2.0)).is_none());
        let replaced = map.assoc_hashed(same, num(2.0), Value::Null).unwrap();
        assert_eq!(replaced.get_hashed(same, &num(2.0)), Some(Value::Null));
        assert_eq!(map.get_hashed(same, &num(2.0)), Some(num(2.0)));
        assert!(map.dissoc_hashed(same, &num(9.0)).is_none());
        for (h, k) in &keys {
            map = map.dissoc_hashed(*h, k).unwrap();
            assert_eq!(map.get_hashed(*h, k), None);
        }
        assert_eq!(map.len(), 0);
        assert!(map.root.is_none());
    }

    #[test]
    fn assoc_copies_only_the_path_to_the_changed_entry() {
        let mut map = PMap::default();
        for i in 0..5000 {
            map = map.assoc(num(i as f64), num(i as f64)).unwrap();
        }
        let updated = map.assoc(num(1234.0), Value::Null).unwrap();
        let old: HashSet<usize> = map.node_addresses().into_iter().collect();
        let fresh = updated
            .node_addresses()
            .into_iter()
            .filter(|a| !old.contains(a))
            .count();
        assert!(fresh <= 4, "only the root-to-entry path is copied: {fresh}");
        assert!(old.len() > 100);
        assert_eq!(map.get(&num(1234.0)), Some(num(1234.0)));
    }
}
