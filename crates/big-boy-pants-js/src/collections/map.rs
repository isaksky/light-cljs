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
//! The trie operations themselves take the key hash and equality as
//! parameters ([`PMap::get_with`] and friends): the native form maps of
//! `crate::collections::form` reuse this storage with ClojureScript value
//! equality and hashing. The SameValueZero API above is unchanged and is
//! the only one internal registries use.
//!
//! Each trie level consumes 5 hash bits. Keys whose 64-bit hashes are fully
//! equal end in a collision node. Nodes are immutable and shared through
//! `Arc`; an update copies only the path to the changed entry, and under a
//! byte limit is charged for exactly those nodes ([`NodeCharge`]). Iteration
//! order is the trie order: deterministic for the same keys and identities,
//! but otherwise unspecified (not insertion order).

use std::sync::Arc;

use super::{CollectionError, NodeCharge};
#[cfg(test)]
use crate::heap::ObjectId;
use crate::heap::budget::ByteReservation;
use crate::heap::gc::Trace;
use crate::value::Value;

const BITS: u32 = 5;
const MASK: u64 = (1 << BITS) - 1;

/// A key equality `eq(stored, probe)` for the generic trie operations.
pub(crate) type KeyEq<'a> = &'a dyn Fn(&Value, &Value) -> bool;

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

/// Incremental immutable-trie traversal. At most fourteen nodes cover all
/// 64 hash bits and a collision leaf; advancing never snapshots the map.
#[derive(Debug)]
pub(crate) struct Cursor {
    path: [Option<(Arc<Node>, usize)>; 14],
    depth: usize,
}

impl Cursor {
    pub(crate) fn new(map: &PMap) -> Self {
        let mut path = std::array::from_fn(|_| None);
        if let Some(root) = &map.root {
            path[0] = Some((root.clone(), 0));
        }
        Self {
            path,
            depth: usize::from(map.root.is_some()),
        }
    }

    pub(crate) fn next(&mut self) -> Option<(Value, Value)> {
        while self.depth > 0 {
            let (node, index) = self.path[self.depth - 1].as_mut().unwrap();
            let at = *index;
            *index += 1;
            match &node.kind {
                Kind::Bitmap { entries, .. } => match entries.get(at) {
                    Some(Entry::Pair { key, value, .. }) => {
                        return Some((key.clone(), value.clone()));
                    }
                    Some(Entry::Child(child)) => {
                        let child = child.clone();
                        self.path[self.depth] = Some((child, 0));
                        self.depth += 1;
                        continue;
                    }
                    None => {}
                },
                Kind::Collision { pairs, .. } => {
                    if let Some(pair) = pairs.get(at) {
                        return Some(pair.clone());
                    }
                }
            }
            self.depth -= 1;
            self.path[self.depth] = None;
        }
        None
    }
}

/// A trie node and the byte charge for its own allocation
/// ([`NodeCharge`]): shared by every version that reaches it, so the charge
/// is released only with the last of them.
#[derive(Debug)]
struct Node {
    kind: Kind,
    _charge: ByteReservation,
}

#[derive(Debug)]
enum Kind {
    /// One entry per set bit of `bitmap`, in bit order.
    Bitmap { bitmap: u32, entries: Vec<Entry> },
    /// Keys whose full hashes are equal (only below the last hash digit).
    Collision {
        hash: u64,
        pairs: Vec<(Value, Value)>,
    },
}

impl Node {
    /// A bitmap node of exactly `len` entries written by `fill`, charged
    /// before its buffer is allocated.
    fn bitmap(
        charge: NodeCharge<'_>,
        bitmap: u32,
        len: usize,
        fill: impl FnOnce(&mut Vec<Entry>),
    ) -> Result<Node, CollectionError> {
        let _charge = charge.reserve::<Node, Entry>(len)?;
        let mut entries = Vec::with_capacity(len);
        fill(&mut entries);
        debug_assert_eq!(entries.len(), len);
        Ok(Node {
            kind: Kind::Bitmap { bitmap, entries },
            _charge,
        })
    }

    /// A collision node of exactly `len` pairs written by `fill`.
    fn collision(
        charge: NodeCharge<'_>,
        hash: u64,
        len: usize,
        fill: impl FnOnce(&mut Vec<(Value, Value)>),
    ) -> Result<Node, CollectionError> {
        let _charge = charge.reserve::<Node, (Value, Value)>(len)?;
        let mut pairs = Vec::with_capacity(len);
        fill(&mut pairs);
        debug_assert_eq!(pairs.len(), len);
        Ok(Node {
            kind: Kind::Collision { hash, pairs },
            _charge,
        })
    }
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
    /// The node became empty.
    Empty,
    /// One pair remains, which the parent holds inline instead of a node.
    Single {
        hash: u64,
        key: Value,
        value: Value,
    },
    /// The node after removal.
    Node(Node),
}

/// The result of an update whose nodes are uncharged, which cannot fail.
#[cfg(test)]
fn uncharged<T>(result: Result<T, CollectionError>) -> T {
    result.expect("uncharged nodes reserve nothing")
}

impl PMap {
    pub(crate) fn try_assoc(
        &self,
        heap: &crate::heap::Heap,
        key: Value,
        value: Value,
    ) -> Result<Option<PMap>, CollectionError> {
        self.try_assoc_with(heap, key_hash(&key), canonical_key(key), value, &key_eq)
    }

    /// A new version mapping `key` to `value` (stored as given) under a
    /// caller-supplied hash and key equality (see [`PMap::get_with`]), or
    /// `None` when the key already maps to an identical value (so the
    /// caller can return the original). Under a byte limit the new version
    /// is charged for the nodes it allocates: the copied root-to-entry path.
    pub(crate) fn try_assoc_with(
        &self,
        heap: &crate::heap::Heap,
        hash: u64,
        key: Value,
        value: Value,
        eq: KeyEq<'_>,
    ) -> Result<Option<PMap>, CollectionError> {
        self.assoc_charged(NodeCharge::of(heap), hash, key, value, eq)
    }

    pub(crate) fn try_dissoc(
        &self,
        heap: &crate::heap::Heap,
        key: &Value,
    ) -> Result<Option<PMap>, CollectionError> {
        self.try_dissoc_with(heap, key_hash(key), key, &key_eq)
    }

    /// A new version without `key`, or `None` when the key is absent;
    /// charged like [`PMap::try_assoc_with`].
    pub(crate) fn try_dissoc_with(
        &self,
        heap: &crate::heap::Heap,
        hash: u64,
        key: &Value,
        eq: KeyEq<'_>,
    ) -> Result<Option<PMap>, CollectionError> {
        self.dissoc_charged(NodeCharge::of(heap), hash, key, eq)
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn get(&self, key: &Value) -> Option<Value> {
        self.get_hashed(key_hash(key), key)
    }

    /// A new version mapping `key` to `value`, or `None` when the key already
    /// maps to an identical value (so the caller can return the original).
    #[cfg(test)]
    pub(crate) fn assoc(&self, key: Value, value: Value) -> Option<PMap> {
        self.assoc_hashed(key_hash(&key), key, value)
    }

    /// A new version without `key`, or `None` when the key is absent.
    #[cfg(test)]
    pub(crate) fn dissoc(&self, key: &Value) -> Option<PMap> {
        self.dissoc_hashed(key_hash(key), key)
    }

    fn get_hashed(&self, hash: u64, key: &Value) -> Option<Value> {
        self.get_with(hash, key, &key_eq)
    }

    /// Lookup with a caller-supplied key equality: `eq(stored, probe)`
    /// must be consistent with `hash` (equal keys have equal hashes). The
    /// structural form maps of `crate::collections::form` use this with
    /// their own equality; the SameValueZero API above passes [`key_eq`].
    pub(crate) fn get_with(&self, hash: u64, key: &Value, eq: KeyEq<'_>) -> Option<Value> {
        let mut node = self.root.as_deref()?;
        let mut shift = 0;
        loop {
            match &node.kind {
                Kind::Bitmap { bitmap, entries } => {
                    let bit = bit_for(hash, shift);
                    if bitmap & bit == 0 {
                        return None;
                    }
                    match entries.get(position(*bitmap, bit))? {
                        Entry::Pair {
                            hash: h,
                            key: k,
                            value,
                        } => {
                            return (*h == hash && eq(k, key)).then(|| value.clone());
                        }
                        Entry::Child(child) => {
                            node = child;
                            shift += BITS;
                        }
                    }
                }
                Kind::Collision { pairs, .. } => {
                    return pairs.iter().find(|(k, _)| eq(k, key)).map(|p| p.1.clone());
                }
            }
        }
    }

    #[cfg(test)]
    fn assoc_hashed(&self, hash: u64, key: Value, value: Value) -> Option<PMap> {
        uncharged(self.assoc_charged(NodeCharge::NONE, hash, canonical_key(key), value, &key_eq))
    }

    /// [`PMap::try_assoc_with`] with new nodes charged by `charge`.
    fn assoc_charged(
        &self,
        charge: NodeCharge<'_>,
        hash: u64,
        key: Value,
        value: Value,
        eq: KeyEq<'_>,
    ) -> Result<Option<PMap>, CollectionError> {
        let Some(root) = &self.root else {
            let root = Node::bitmap(charge, bit_for(hash, 0), 1, |entries| {
                entries.push(Entry::Pair { hash, key, value })
            })?;
            return Ok(Some(PMap {
                len: 1,
                root: Some(Arc::new(root)),
            }));
        };
        let (root, len) = match assoc_in(charge, root, 0, hash, key, value, eq)? {
            Assoc::Unchanged => return Ok(None),
            Assoc::Replaced(n) => (n, self.len),
            Assoc::Added(n) => (n, self.len + 1),
        };
        Ok(Some(PMap {
            len,
            root: Some(Arc::new(root)),
        }))
    }

    #[cfg(test)]
    fn dissoc_hashed(&self, hash: u64, key: &Value) -> Option<PMap> {
        uncharged(self.dissoc_charged(NodeCharge::NONE, hash, key, &key_eq))
    }

    /// [`PMap::try_dissoc_with`] with new nodes charged by `charge`.
    fn dissoc_charged(
        &self,
        charge: NodeCharge<'_>,
        hash: u64,
        key: &Value,
        eq: KeyEq<'_>,
    ) -> Result<Option<PMap>, CollectionError> {
        let Some(root) = self.root.as_deref() else {
            return Ok(None);
        };
        let root = match dissoc_in(charge, root, 0, hash, key, eq)? {
            Dissoc::Absent => return Ok(None),
            Dissoc::Empty => None,
            // The root keeps a one-pair node.
            Dissoc::Single { hash, key, value } => {
                Some(Node::bitmap(charge, bit_for(hash, 0), 1, |entries| {
                    entries.push(Entry::Pair { hash, key, value })
                })?)
            }
            Dissoc::Node(n) => Some(n),
        };
        Ok(Some(PMap {
            len: self.len - 1,
            root: root.map(Arc::new),
        }))
    }

    /// Call `f` on every entry, in trie order.
    pub(crate) fn for_each(&self, mut f: impl FnMut(&Value, &Value)) {
        fn walk(node: &Node, f: &mut dyn FnMut(&Value, &Value)) {
            match &node.kind {
                Kind::Bitmap { entries, .. } => {
                    for entry in entries {
                        match entry {
                            Entry::Pair { key, value, .. } => f(key, value),
                            Entry::Child(child) => walk(child, f),
                        }
                    }
                }
                Kind::Collision { pairs, .. } => pairs.iter().for_each(|(k, v)| f(k, v)),
            }
        }
        if let Some(root) = &self.root {
            walk(root, &mut f);
        }
    }

    /// Call `f` on every entry with its stored hash, in trie order.
    pub(crate) fn for_each_hashed(&self, mut f: impl FnMut(u64, &Value, &Value)) {
        fn walk(node: &Node, f: &mut dyn FnMut(u64, &Value, &Value)) {
            match &node.kind {
                Kind::Bitmap { entries, .. } => {
                    for entry in entries {
                        match entry {
                            Entry::Pair { hash, key, value } => f(*hash, key, value),
                            Entry::Child(child) => walk(child, f),
                        }
                    }
                }
                Kind::Collision { hash, pairs } => pairs.iter().for_each(|(k, v)| f(*hash, k, v)),
            }
        }
        if let Some(root) = &self.root {
            walk(root, &mut f);
        }
    }

    /// The entries whose stored hash is `hash`, in trie order (one, unless
    /// distinct keys share the full hash).
    pub(crate) fn entries_with_hash(&self, hash: u64) -> Vec<(Value, Value)> {
        let Some(mut node) = self.root.as_deref() else {
            return Vec::new();
        };
        let mut shift = 0;
        loop {
            match &node.kind {
                Kind::Bitmap { bitmap, entries } => {
                    let bit = bit_for(hash, shift);
                    if bitmap & bit == 0 {
                        return Vec::new();
                    }
                    match &entries[position(*bitmap, bit)] {
                        Entry::Pair {
                            hash: h,
                            key,
                            value,
                        } if *h == hash => {
                            return vec![(key.clone(), value.clone())];
                        }
                        Entry::Pair { .. } => return Vec::new(),
                        Entry::Child(child) => {
                            node = child;
                            shift += BITS;
                        }
                    }
                }
                Kind::Collision { hash: h, pairs } => {
                    return if *h == hash {
                        pairs.clone()
                    } else {
                        Vec::new()
                    };
                }
            }
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
            if let Kind::Bitmap { entries, .. } = &node.kind {
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
fn merge(
    charge: NodeCharge<'_>,
    shift: u32,
    a: (u64, Value, Value),
    b: (u64, Value, Value),
) -> Result<Node, CollectionError> {
    if shift >= u64::BITS {
        return Node::collision(charge, a.0, 2, |pairs| {
            pairs.push((a.1, a.2));
            pairs.push((b.1, b.2));
        });
    }
    let (bit_a, bit_b) = (bit_for(a.0, shift), bit_for(b.0, shift));
    if bit_a == bit_b {
        let child = Arc::new(merge(charge, shift + BITS, a, b)?);
        return Node::bitmap(charge, bit_a, 1, |entries| {
            entries.push(Entry::Child(child))
        });
    }
    let pair = |(hash, key, value)| Entry::Pair { hash, key, value };
    Node::bitmap(charge, bit_a | bit_b, 2, |entries| {
        let (first, second) = if bit_a < bit_b { (a, b) } else { (b, a) };
        entries.push(pair(first));
        entries.push(pair(second));
    })
}

fn assoc_in(
    charge: NodeCharge<'_>,
    node: &Node,
    shift: u32,
    hash: u64,
    key: Value,
    value: Value,
    eq: KeyEq<'_>,
) -> Result<Assoc, CollectionError> {
    match &node.kind {
        Kind::Bitmap { bitmap, entries } => {
            let bit = bit_for(hash, shift);
            let pos = position(*bitmap, bit);
            if bitmap & bit == 0 {
                let node = Node::bitmap(charge, bitmap | bit, entries.len() + 1, |copy| {
                    copy.extend_from_slice(&entries[..pos]);
                    copy.push(Entry::Pair { hash, key, value });
                    copy.extend_from_slice(&entries[pos..]);
                })?;
                return Ok(Assoc::Added(node));
            }
            let (replacement, added) = match &entries[pos] {
                Entry::Pair {
                    hash: h,
                    key: k,
                    value: v,
                } => {
                    if *h == hash && eq(k, &key) {
                        if identical(v, &value) {
                            return Ok(Assoc::Unchanged);
                        }
                        let entry = Entry::Pair {
                            hash: *h,
                            key: k.clone(),
                            value,
                        };
                        (entry, false)
                    } else {
                        let child = merge(
                            charge,
                            shift + BITS,
                            (*h, k.clone(), v.clone()),
                            (hash, key, value),
                        )?;
                        (Entry::Child(Arc::new(child)), true)
                    }
                }
                Entry::Child(child) => {
                    match assoc_in(charge, child, shift + BITS, hash, key, value, eq)? {
                        Assoc::Unchanged => return Ok(Assoc::Unchanged),
                        Assoc::Replaced(n) => (Entry::Child(Arc::new(n)), false),
                        Assoc::Added(n) => (Entry::Child(Arc::new(n)), true),
                    }
                }
            };
            let node = Node::bitmap(charge, *bitmap, entries.len(), |copy| {
                copy.extend_from_slice(&entries[..pos]);
                copy.push(replacement);
                copy.extend_from_slice(&entries[pos + 1..]);
            })?;
            Ok(if added {
                Assoc::Added(node)
            } else {
                Assoc::Replaced(node)
            })
        }
        Kind::Collision { hash: h, pairs } => {
            // Collision nodes sit below every hash digit, so every key that
            // reaches one has the node's full hash.
            debug_assert_eq!(*h, hash);
            match pairs.iter().position(|(k, _)| eq(k, &key)) {
                Some(i) if identical(&pairs[i].1, &value) => Ok(Assoc::Unchanged),
                Some(i) => Ok(Assoc::Replaced(Node::collision(
                    charge,
                    *h,
                    pairs.len(),
                    |copy| {
                        copy.extend_from_slice(pairs);
                        copy[i].1 = value;
                    },
                )?)),
                None => Ok(Assoc::Added(Node::collision(
                    charge,
                    *h,
                    pairs.len() + 1,
                    |copy| {
                        copy.extend_from_slice(pairs);
                        copy.push((key, value));
                    },
                )?)),
            }
        }
    }
}

fn dissoc_in(
    charge: NodeCharge<'_>,
    node: &Node,
    shift: u32,
    hash: u64,
    key: &Value,
    eq: KeyEq<'_>,
) -> Result<Dissoc, CollectionError> {
    match &node.kind {
        Kind::Bitmap { bitmap, entries } => {
            let bit = bit_for(hash, shift);
            if bitmap & bit == 0 {
                return Ok(Dissoc::Absent);
            }
            let pos = position(*bitmap, bit);
            let replacement = match &entries[pos] {
                Entry::Pair {
                    hash: h, key: k, ..
                } if *h == hash && eq(k, key) => None,
                Entry::Pair { .. } => return Ok(Dissoc::Absent),
                Entry::Child(child) => match dissoc_in(charge, child, shift + BITS, hash, key, eq)?
                {
                    Dissoc::Absent => return Ok(Dissoc::Absent),
                    Dissoc::Empty => None,
                    Dissoc::Single { hash, key, value } => Some(Entry::Pair { hash, key, value }),
                    Dissoc::Node(n) => Some(Entry::Child(Arc::new(n))),
                },
            };
            let (bitmap, len) = match replacement {
                Some(_) => (*bitmap, entries.len()),
                None => (bitmap & !bit, entries.len() - 1),
            };
            if len == 0 {
                return Ok(Dissoc::Empty);
            }
            if len == 1 {
                let only = match &replacement {
                    Some(entry) => entry,
                    None => &entries[1 - pos],
                };
                if let Entry::Pair { hash, key, value } = only {
                    return Ok(Dissoc::Single {
                        hash: *hash,
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }
            let node = Node::bitmap(charge, bitmap, len, |copy| {
                copy.extend_from_slice(&entries[..pos]);
                copy.extend(replacement);
                copy.extend_from_slice(&entries[pos + 1..]);
            })?;
            Ok(Dissoc::Node(node))
        }
        Kind::Collision { hash: h, pairs } => {
            let Some(i) = pairs.iter().position(|(k, _)| eq(k, key)) else {
                return Ok(Dissoc::Absent);
            };
            match pairs.len() {
                1 => Ok(Dissoc::Empty),
                2 => {
                    let (key, value) = &pairs[1 - i];
                    Ok(Dissoc::Single {
                        hash: *h,
                        key: key.clone(),
                        value: value.clone(),
                    })
                }
                len => Ok(Dissoc::Node(Node::collision(
                    charge,
                    *h,
                    len - 1,
                    |copy| {
                        copy.extend_from_slice(&pairs[..i]);
                        copy.extend_from_slice(&pairs[i + 1..]);
                    },
                )?)),
            }
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

    /// The bytes charged for every distinct node reachable from `maps`.
    fn reachable_node_bytes<'a>(maps: impl IntoIterator<Item = &'a PMap>) -> usize {
        fn walk(node: &Arc<Node>, seen: &mut HashSet<usize>) -> usize {
            if !seen.insert(Arc::as_ptr(node) as usize) {
                return 0;
            }
            match &node.kind {
                Kind::Bitmap { entries, .. } => {
                    let children: usize = entries
                        .iter()
                        .map(|entry| match entry {
                            Entry::Child(child) => walk(child, seen),
                            Entry::Pair { .. } => 0,
                        })
                        .sum();
                    children + super::super::node_bytes::<Node, Entry>(entries.len()).unwrap()
                }
                Kind::Collision { pairs, .. } => {
                    super::super::node_bytes::<Node, (Value, Value)>(pairs.len()).unwrap()
                }
            }
        }
        let mut seen = HashSet::new();
        maps.into_iter()
            .filter_map(|map| map.root.as_ref())
            .map(|root| walk(root, &mut seen))
            .sum()
    }

    /// Under a byte limit the budget holds exactly the nodes some live
    /// version reaches: an update adds only the nodes it copied, and
    /// dropping a version releases only the nodes no other version shares.
    /// Truncated hashes force deep merges and full-hash collision nodes.
    #[test]
    fn versions_are_charged_for_new_nodes_and_shared_nodes_until_the_last_holder() {
        let heap = crate::heap::Heap::with_byte_limit(usize::MAX / 2);
        let budget = heap.byte_budget();
        let baseline = budget.used();
        let mut seed: u64 = 0x5851_f42d_4c95_7f2d;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize
        };
        for mask in [u64::MAX, 0x3ff] {
            let hash = |key: &Value| key_hash(key) & mask;
            let mut live: Vec<PMap> = vec![PMap::default()];
            for step in 0..3000 {
                let map = live[next() % live.len()].clone();
                let key = num((next() % 700) as f64);
                let updated = if next() % 4 == 0 {
                    map.try_dissoc_with(&heap, hash(&key), &key, &key_eq)
                } else {
                    map.try_assoc_with(&heap, hash(&key), key, num(step as f64), &key_eq)
                };
                drop(map);
                if let Some(updated) = updated.unwrap() {
                    live.push(updated);
                }
                if live.len() > 12 {
                    live.swap_remove(next() % live.len());
                }
                assert_eq!(
                    budget.used() - baseline,
                    reachable_node_bytes(&live),
                    "step {step}"
                );
            }
        }
        assert_eq!(budget.used(), baseline);
    }

    /// A reservation denied partway through a multi-node update (a copied
    /// path, a merge chain down to a collision node) refunds the nodes
    /// already built and leaves the input intact; with the headroom the
    /// update needs, it succeeds.
    #[test]
    fn denied_path_copy_refunds_partial_nodes() {
        let heap = crate::heap::Heap::with_byte_limit(usize::MAX / 2);
        let budget = heap.byte_budget();
        let mut wide = PMap::default();
        for i in 0..5000 {
            wide = wide
                .try_assoc(&heap, num(i as f64), num(i as f64))
                .unwrap()
                .unwrap();
        }
        // Every key hashes to 0: the second key merges 13 levels down to a
        // collision node.
        let one = PMap::default()
            .try_assoc_with(&heap, 0, num(0.0), num(0.0), &key_eq)
            .unwrap()
            .unwrap();
        let two = one
            .try_assoc_with(&heap, 0, num(1.0), num(1.0), &key_eq)
            .unwrap()
            .unwrap();
        type Op<'a> = &'a dyn Fn() -> Result<Option<PMap>, CollectionError>;
        let ops: [(&str, Op<'_>); 6] = [
            ("add", &|| wide.try_assoc(&heap, num(-1.0), Value::Null)),
            ("replace", &|| wide.try_assoc(&heap, num(17.0), Value::Null)),
            ("remove", &|| wide.try_dissoc(&heap, &num(17.0))),
            ("merge", &|| {
                one.try_assoc_with(&heap, 0, num(1.0), num(1.0), &key_eq)
            }),
            ("collide", &|| {
                two.try_assoc_with(&heap, 0, num(2.0), num(2.0), &key_eq)
            }),
            ("collapse", &|| {
                two.try_dissoc_with(&heap, 0, &num(1.0), &key_eq)
            }),
        ];
        for (name, op) in ops {
            let before = budget.used();
            let needed = {
                let _result = op().unwrap().unwrap();
                budget.used() - before
            };
            assert_eq!(budget.used(), before, "{name}");
            for headroom in (0..needed).step_by((needed / 300).max(1)) {
                budget.set_limit(before + headroom);
                assert_eq!(op().unwrap_err(), CollectionError::HeapLimit, "{name}");
                assert_eq!(budget.used(), before, "{name} with {headroom} of {needed}");
            }
            budget.set_limit(before + needed);
            drop(op().unwrap());
            assert_eq!(budget.used(), before, "{name}");
            budget.set_limit(usize::MAX / 2);
        }
        let model: Vec<(Value, Value)> =
            (0..5000).map(|i| (num(i as f64), num(i as f64))).collect();
        check(&wide, &model);
        assert_eq!(two.get_with(0, &num(1.0), &key_eq), Some(num(1.0)));
        assert_eq!(two.len(), 2);
    }

    /// One `assoc` on a large map admits its path copy, not the map.
    #[test]
    fn an_update_of_a_large_map_is_charged_for_its_path_only() {
        let heap = crate::heap::Heap::with_byte_limit(usize::MAX / 2);
        let budget = heap.byte_budget();
        let baseline = budget.used();
        let mut map = PMap::default();
        for i in 0..5000 {
            map = map
                .try_assoc(&heap, num(i as f64), num(i as f64))
                .unwrap()
                .unwrap();
        }
        let whole = reachable_node_bytes([&map]);
        let before = budget.used();
        let updated = map
            .try_assoc(&heap, num(1234.0), Value::Null)
            .unwrap()
            .unwrap();
        let path = budget.used() - before;
        assert_eq!(path, reachable_node_bytes([&map, &updated]) - whole);
        // At most three copied levels of at most 32 entries each.
        let widest = super::super::node_bytes::<Node, Entry>(32).unwrap();
        assert!(path <= 3 * widest && path * 50 < whole, "{path} of {whole}");
        // The new version keeps the old one's untouched nodes charged.
        drop(map);
        let retained = reachable_node_bytes([&updated]);
        assert_eq!(budget.used() - baseline, retained);
        assert!(
            retained + 3 * widest >= whole,
            "only the old path is released"
        );
        drop(updated);
        assert_eq!(budget.used(), baseline);
    }
}
