//! Persistent vector body: a 32-way radix trie with path copying.
//!
//! Every node is immutable once built and shared through `Arc`. An update
//! copies only the path from the root to the changed leaf (at most
//! `ceil(log32 n)` nodes); every other node is shared with the previous
//! version. Under a byte limit an update is charged for exactly the nodes
//! it allocates ([`NodeCharge`]). The trie is left-packed: all leaves are
//! full except the last, and `shift` is the bit offset of the root's index
//! digit (0 when the root is the only leaf).
//!
//! Elements are raw [`Value`]s. Heap references among them are reported by
//! [`Trace`]; the trie nodes themselves are not heap objects (see the
//! storage boundary in [`crate::collections`]).

use std::sync::{Arc, LazyLock};

use super::{CollectionError, NodeCharge};
use crate::heap::budget::ByteReservation;
use crate::heap::gc::Trace;
use crate::value::Value;

const BITS: u32 = 5;
const WIDTH: usize = 1 << BITS;
const MASK: usize = WIDTH - 1;

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
    Branch(Vec<Arc<Node>>),
    Leaf(Vec<Value>),
}

impl Node {
    /// A branch of exactly `len` children written by `fill`, charged before
    /// its buffer is allocated.
    fn branch(
        charge: NodeCharge<'_>,
        len: usize,
        fill: impl FnOnce(&mut Vec<Arc<Node>>),
    ) -> Result<Node, CollectionError> {
        let _charge = charge.reserve::<Node, Arc<Node>>(len)?;
        let mut children = Vec::with_capacity(len);
        fill(&mut children);
        debug_assert_eq!(children.len(), len);
        Ok(Node {
            kind: Kind::Branch(children),
            _charge,
        })
    }

    /// A leaf of exactly `len` values written by `fill`.
    fn leaf(
        charge: NodeCharge<'_>,
        len: usize,
        fill: impl FnOnce(&mut Vec<Value>),
    ) -> Result<Node, CollectionError> {
        let _charge = charge.reserve::<Node, Value>(len)?;
        let mut values = Vec::with_capacity(len);
        fill(&mut values);
        debug_assert_eq!(values.len(), len);
        Ok(Node {
            kind: Kind::Leaf(values),
            _charge,
        })
    }
}

/// The empty leaf every empty vector shares. It is allocated once and never
/// freed, so it is not charged to any heap.
static EMPTY: LazyLock<Arc<Node>> = LazyLock::new(|| {
    Arc::new(Node {
        kind: Kind::Leaf(Vec::new()),
        _charge: ByteReservation::default(),
    })
});

/// The result of an update whose nodes are uncharged, which cannot fail.
#[cfg(test)]
fn uncharged<T>(result: Result<T, CollectionError>) -> T {
    result.expect("uncharged nodes reserve nothing")
}

/// An immutable vector body. Cloning is O(1) (one `Arc` clone).
#[derive(Clone, Debug)]
pub(crate) struct PVector {
    len: usize,
    shift: u32,
    root: Arc<Node>,
}

impl Default for PVector {
    fn default() -> Self {
        PVector {
            len: 0,
            shift: 0,
            root: Arc::clone(&EMPTY),
        }
    }
}

impl PVector {
    pub(crate) fn try_from_values(
        heap: &crate::heap::Heap,
        values: &[Value],
    ) -> Result<Self, CollectionError> {
        Self::build(NodeCharge::of(heap), values.len(), values.iter().cloned())
    }

    pub(crate) fn try_push(
        &self,
        heap: &crate::heap::Heap,
        value: Value,
    ) -> Result<Self, CollectionError> {
        self.push_charged(NodeCharge::of(heap), value)
    }

    pub(crate) fn try_set(
        &self,
        heap: &crate::heap::Heap,
        index: usize,
        value: Value,
    ) -> Result<Self, CollectionError> {
        if index >= self.len {
            return Err(CollectionError::IndexOutOfBounds {
                index,
                count: self.len,
            });
        }
        self.set_charged(NodeCharge::of(heap), index, value)
    }

    pub(crate) fn try_pop(&self, heap: &crate::heap::Heap) -> Result<Self, CollectionError> {
        if self.len == 0 {
            return Err(CollectionError::IndexOutOfBounds { index: 0, count: 0 });
        }
        self.pop_charged(NodeCharge::of(heap))
    }

    /// A new vector of `value` followed by every element. Every node is
    /// new, so it is charged like a vector built from scratch.
    pub(crate) fn try_prepend(
        &self,
        heap: &crate::heap::Heap,
        value: Value,
    ) -> Result<Self, CollectionError> {
        let len = self.len.checked_add(1).ok_or(CollectionError::HeapLimit)?;
        fn leaves<'a>(node: &'a Node, out: &mut Vec<&'a [Value]>) {
            match &node.kind {
                Kind::Branch(children) => children.iter().for_each(|c| leaves(c, out)),
                Kind::Leaf(values) => out.push(values),
            }
        }
        let mut parts = Vec::with_capacity(self.len.div_ceil(WIDTH));
        leaves(&self.root, &mut parts);
        let values = std::iter::once(value).chain(parts.into_iter().flatten().cloned());
        Self::build(NodeCharge::of(heap), len, values)
    }

    /// A vector of `values`, built bottom-up in O(n).
    #[cfg(test)]
    pub(crate) fn from_values(values: &[Value]) -> PVector {
        uncharged(Self::build(
            NodeCharge::NONE,
            values.len(),
            values.iter().cloned(),
        ))
    }

    /// A vector of the `len` values `values` yields, built bottom-up.
    fn build(
        charge: NodeCharge<'_>,
        len: usize,
        mut values: impl Iterator<Item = Value>,
    ) -> Result<PVector, CollectionError> {
        if len == 0 {
            return Ok(PVector::default());
        }
        let mut level = Vec::with_capacity(len.div_ceil(WIDTH));
        let mut remaining = len;
        while remaining > 0 {
            let n = remaining.min(WIDTH);
            let leaf = Node::leaf(charge, n, |leaf| leaf.extend(values.by_ref().take(n)))?;
            level.push(Arc::new(leaf));
            remaining -= n;
        }
        let mut shift = 0;
        while level.len() > 1 {
            level = level
                .chunks(WIDTH)
                .map(|c| Node::branch(charge, c.len(), |b| b.extend_from_slice(c)).map(Arc::new))
                .collect::<Result<_, _>>()?;
            shift += BITS;
        }
        let root = level.pop().expect("a nonempty vector has a root");
        Ok(PVector { len, shift, root })
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The element at `index`, or `None` when out of bounds.
    pub(crate) fn get(&self, index: usize) -> Option<Value> {
        if index >= self.len {
            return None;
        }
        let mut node = &*self.root;
        let mut level = self.shift;
        loop {
            match &node.kind {
                Kind::Branch(children) => {
                    node = children.get((index >> level) & MASK)?;
                    level = level.saturating_sub(BITS);
                }
                Kind::Leaf(values) => return values.get(index & MASK).cloned(),
            }
        }
    }

    /// A new version with `index` set to `value`; `None` when out of bounds.
    #[cfg(test)]
    fn set(&self, index: usize, value: Value) -> Option<PVector> {
        (index < self.len).then(|| uncharged(self.set_charged(NodeCharge::NONE, index, value)))
    }

    /// A new version with `value` appended.
    #[cfg(test)]
    fn push(&self, value: Value) -> PVector {
        uncharged(self.push_charged(NodeCharge::NONE, value))
    }

    /// A new version without the last element; `None` when empty.
    #[cfg(test)]
    fn pop(&self) -> Option<PVector> {
        (self.len > 0).then(|| uncharged(self.pop_charged(NodeCharge::NONE)))
    }

    /// Set `index`, which is in bounds, to `value`.
    fn set_charged(
        &self,
        charge: NodeCharge<'_>,
        index: usize,
        value: Value,
    ) -> Result<PVector, CollectionError> {
        debug_assert!(index < self.len);
        Ok(PVector {
            root: Arc::new(set_in(charge, &self.root, self.shift, index, value)?),
            ..self.clone()
        })
    }

    fn push_charged(
        &self,
        charge: NodeCharge<'_>,
        value: Value,
    ) -> Result<PVector, CollectionError> {
        if self.len == usize::MAX {
            return Err(CollectionError::HeapLimit);
        }
        // `checked_shl` fails only for a 64-level-deep trie, which cannot exist.
        let capacity = 1usize.checked_shl(self.shift + BITS).unwrap_or(usize::MAX);
        if self.len == capacity {
            let path = Arc::new(new_path(charge, self.shift, value)?);
            let root = Node::branch(charge, 2, |children| {
                children.push(Arc::clone(&self.root));
                children.push(path);
            })?;
            Ok(PVector {
                len: self.len + 1,
                shift: self.shift + BITS,
                root: Arc::new(root),
            })
        } else {
            let root = push_in(charge, &self.root, self.shift, self.len, value)?;
            Ok(PVector {
                len: self.len + 1,
                shift: self.shift,
                root: Arc::new(root),
            })
        }
    }

    /// Remove the last element of this nonempty vector.
    fn pop_charged(&self, charge: NodeCharge<'_>) -> Result<PVector, CollectionError> {
        debug_assert!(self.len > 0);
        if self.len <= 1 {
            return Ok(PVector::default());
        }
        let Some(root) = pop_in(charge, &self.root, self.shift, self.len - 1)? else {
            return Ok(PVector::default());
        };
        let mut root = Arc::new(root);
        let mut shift = self.shift;
        // Drop root levels with a single child so the trie stays minimal
        // (and `push`'s capacity check stays exact).
        while shift > 0 {
            let only = match &root.kind {
                Kind::Branch(children) if children.len() == 1 => Arc::clone(&children[0]),
                _ => break,
            };
            root = only;
            shift -= BITS;
        }
        Ok(PVector {
            len: self.len - 1,
            shift,
            root,
        })
    }

    /// Call `f` on every element in index order.
    pub(crate) fn for_each(&self, mut f: impl FnMut(&Value)) {
        fn walk(node: &Node, f: &mut dyn FnMut(&Value)) {
            match &node.kind {
                Kind::Branch(children) => children.iter().for_each(|c| walk(c, f)),
                Kind::Leaf(values) => values.iter().for_each(f),
            }
        }
        walk(&self.root, &mut f);
    }

    /// Every element in index order.
    pub(crate) fn to_vec(&self) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.len);
        self.for_each(|v| out.push(v.clone()));
        out
    }

    /// Addresses of every trie node, root first (tests of structural sharing).
    #[cfg(test)]
    pub(crate) fn node_addresses(&self) -> Vec<usize> {
        fn walk(node: &Arc<Node>, out: &mut Vec<usize>) {
            out.push(Arc::as_ptr(node) as usize);
            if let Kind::Branch(children) = &node.kind {
                children.iter().for_each(|c| walk(c, out));
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }
}

fn set_in(
    charge: NodeCharge<'_>,
    node: &Node,
    level: u32,
    index: usize,
    value: Value,
) -> Result<Node, CollectionError> {
    match &node.kind {
        Kind::Leaf(values) => Node::leaf(charge, values.len(), |copy| {
            let i = index & MASK;
            copy.extend_from_slice(&values[..i]);
            copy.push(value);
            copy.extend_from_slice(&values[i + 1..]);
        }),
        Kind::Branch(children) => {
            let i = (index >> level) & MASK;
            let child = Arc::new(set_in(charge, &children[i], level - BITS, index, value)?);
            Node::branch(charge, children.len(), |copy| {
                copy.extend_from_slice(&children[..i]);
                copy.push(child);
                copy.extend_from_slice(&children[i + 1..]);
            })
        }
    }
}

/// A fresh spine of single-child branches down to a leaf holding `value`.
fn new_path(charge: NodeCharge<'_>, level: u32, value: Value) -> Result<Node, CollectionError> {
    if level == 0 {
        Node::leaf(charge, 1, |leaf| leaf.push(value))
    } else {
        let child = Arc::new(new_path(charge, level - BITS, value)?);
        Node::branch(charge, 1, |children| children.push(child))
    }
}

/// Append `value` at position `index` (the current length) below `node`,
/// which has room for it.
fn push_in(
    charge: NodeCharge<'_>,
    node: &Node,
    level: u32,
    index: usize,
    value: Value,
) -> Result<Node, CollectionError> {
    match &node.kind {
        Kind::Leaf(values) => Node::leaf(charge, values.len() + 1, |copy| {
            copy.extend_from_slice(values);
            copy.push(value);
        }),
        Kind::Branch(children) => {
            let i = (index >> level) & MASK;
            match children.get(i) {
                Some(child) => {
                    let child = Arc::new(push_in(charge, child, level - BITS, index, value)?);
                    Node::branch(charge, children.len(), |copy| {
                        copy.extend_from_slice(&children[..i]);
                        copy.push(child);
                        copy.extend_from_slice(&children[i + 1..]);
                    })
                }
                None => {
                    let child = Arc::new(new_path(charge, level - BITS, value)?);
                    Node::branch(charge, children.len() + 1, |copy| {
                        copy.extend_from_slice(children);
                        copy.push(child);
                    })
                }
            }
        }
    }
}

/// Remove the element at `index` (the last one) below `node`; `None` when the
/// node becomes empty.
fn pop_in(
    charge: NodeCharge<'_>,
    node: &Node,
    level: u32,
    index: usize,
) -> Result<Option<Node>, CollectionError> {
    match &node.kind {
        Kind::Leaf(values) => {
            let len = values.len().saturating_sub(1);
            if len == 0 {
                return Ok(None);
            }
            Node::leaf(charge, len, |copy| copy.extend_from_slice(&values[..len])).map(Some)
        }
        Kind::Branch(children) => {
            let i = (index >> level) & MASK;
            match pop_in(charge, &children[i], level - BITS, index)? {
                Some(child) => {
                    let child = Arc::new(child);
                    Node::branch(charge, children.len(), |copy| {
                        copy.extend_from_slice(&children[..i]);
                        copy.push(child);
                        copy.extend_from_slice(&children[i + 1..]);
                    })
                    .map(Some)
                }
                // The emptied child is the last one.
                None if i == 0 => Ok(None),
                None => {
                    Node::branch(charge, i, |copy| copy.extend_from_slice(&children[..i])).map(Some)
                }
            }
        }
    }
}

impl Trace for PVector {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.for_each(|v| v.trace(visit));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn num(n: usize) -> Value {
        Value::Number(n as f64)
    }

    fn check(v: &PVector, model: &[Value]) {
        assert_eq!(v.len(), model.len());
        assert_eq!(v.to_vec(), model);
        for (i, m) in model.iter().enumerate() {
            assert_eq!(v.get(i), Some(m.clone()), "index {i}");
        }
        assert_eq!(v.get(model.len()), None);
    }

    #[test]
    fn construction_matches_repeated_push_across_level_boundaries() {
        for n in [0, 1, 31, 32, 33, 1023, 1024, 1025, 33 * 32 + 7] {
            let model: Vec<Value> = (0..n).map(num).collect();
            let built = PVector::from_values(&model);
            check(&built, &model);
            let pushed = model
                .iter()
                .fold(PVector::default(), |v, x| v.push(x.clone()));
            check(&pushed, &model);
            assert_eq!(built.shift, pushed.shift, "n = {n}");
        }
    }

    #[test]
    fn popping_to_empty_collapses_levels_like_fresh_construction() {
        let model: Vec<Value> = (0..1100).map(num).collect();
        let mut v = PVector::from_values(&model);
        for len in (0..model.len()).rev() {
            v = v.pop().unwrap();
            check(&v, &model[..len]);
            assert_eq!(
                v.shift,
                PVector::from_values(&model[..len]).shift,
                "len {len}"
            );
            // Pushing again after a pop lands in the right place.
            check(&v.push(num(9999)), &[&model[..len], &[num(9999)]].concat());
        }
        assert!(v.pop().is_none());
    }

    #[test]
    fn random_operations_match_a_vec_model_and_never_change_old_versions() {
        // Deterministic LCG; no external randomness.
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize
        };
        let mut versions: Vec<(PVector, Vec<Value>)> = vec![(PVector::default(), Vec::new())];
        for step in 0..3000 {
            let (v, model) = versions.last().unwrap().clone();
            let (v, model) = match next() % 8 {
                0 if !model.is_empty() => {
                    let mut m = model.clone();
                    m.pop();
                    (v.pop().unwrap(), m)
                }
                1 | 2 if !model.is_empty() => {
                    let i = next() % model.len();
                    let mut m = model.clone();
                    m[i] = num(step);
                    (v.set(i, num(step)).unwrap(), m)
                }
                _ => {
                    let mut m = model.clone();
                    m.push(num(step));
                    (v.push(num(step)), m)
                }
            };
            check(&v, &model);
            versions.push((v, model));
        }
        for (v, model) in &versions {
            check(v, model);
        }
        assert_eq!(PVector::default().pop().map(|v| v.len()), None);
        assert!(PVector::default().set(0, num(0)).is_none());
    }

    #[test]
    fn set_copies_only_the_path_to_the_changed_leaf() {
        let model: Vec<Value> = (0..2000).map(num).collect();
        let old = PVector::from_values(&model);
        let new = old.set(1500, Value::Null).unwrap();
        let old_nodes: HashSet<usize> = old.node_addresses().into_iter().collect();
        let new_nodes = new.node_addresses();
        let fresh: Vec<&usize> = new_nodes
            .iter()
            .filter(|a| !old_nodes.contains(a))
            .collect();
        // 2000 elements: root (shift 10) -> branch -> leaf, so 3 fresh nodes.
        assert_eq!(old.shift, 10);
        assert_eq!(fresh.len(), 3);
        assert_eq!(new_nodes.len(), old_nodes.len());
        assert_eq!(
            old.get(1500),
            Some(num(1500)),
            "the old version is unchanged"
        );
    }

    /// The bytes charged for every distinct node reachable from `vectors`
    /// (the shared empty leaf is not charged).
    fn reachable_node_bytes<'a>(vectors: impl IntoIterator<Item = &'a PVector>) -> usize {
        fn walk(node: &Arc<Node>, seen: &mut HashSet<usize>) -> usize {
            if Arc::ptr_eq(node, &EMPTY) || !seen.insert(Arc::as_ptr(node) as usize) {
                return 0;
            }
            match &node.kind {
                Kind::Branch(children) => {
                    children.iter().map(|c| walk(c, seen)).sum::<usize>()
                        + super::super::node_bytes::<Node, Arc<Node>>(children.len()).unwrap()
                }
                Kind::Leaf(values) => {
                    super::super::node_bytes::<Node, Value>(values.len()).unwrap()
                }
            }
        }
        let mut seen = HashSet::new();
        vectors.into_iter().map(|v| walk(&v.root, &mut seen)).sum()
    }

    /// Under a byte limit the budget holds exactly the nodes some live
    /// version reaches, across every update kind and level boundary.
    #[test]
    fn versions_are_charged_for_new_nodes_and_shared_nodes_until_the_last_holder() {
        let heap = crate::heap::Heap::with_byte_limit(usize::MAX / 2);
        let budget = heap.byte_budget();
        let baseline = budget.used();
        let mut seed: u64 = 0x1405_7b7e_f767_814f;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize
        };
        let model: Vec<Value> = (0..1100).map(num).collect();
        let mut live = vec![PVector::try_from_values(&heap, &model).unwrap()];
        for step in 0..3000 {
            let v = live[next() % live.len()].clone();
            let updated = match next() % 10 {
                0 if v.len() > 0 => v.try_pop(&heap).unwrap(),
                1 | 2 if v.len() > 0 => v.try_set(&heap, next() % v.len(), num(step)).unwrap(),
                3 => v.try_prepend(&heap, num(step)).unwrap(),
                4 => PVector::try_from_values(&heap, &model[..next() % 70]).unwrap(),
                _ => v.try_push(&heap, num(step)).unwrap(),
            };
            drop(v);
            live.push(updated);
            if live.len() > 12 {
                live.swap_remove(next() % live.len());
            }
            assert_eq!(
                budget.used() - baseline,
                reachable_node_bytes(&live),
                "step {step}"
            );
        }
        drop(live);
        assert_eq!(budget.used(), baseline);
    }

    /// A reservation denied partway through a multi-node update refunds the
    /// nodes already built and leaves the input intact; with the headroom
    /// the update needs, it succeeds.
    #[test]
    fn denied_path_copy_refunds_partial_nodes() {
        let heap = crate::heap::Heap::with_byte_limit(usize::MAX / 2);
        let budget = heap.byte_budget();
        let model: Vec<Value> = (0..1100).map(num).collect();
        let full = PVector::try_from_values(&heap, &model[..1024]).unwrap();
        let wide = PVector::try_from_values(&heap, &model).unwrap();
        type Op<'a> = &'a dyn Fn() -> Result<PVector, CollectionError>;
        let ops: [(&str, Op<'_>); 6] = [
            ("push growing the root", &|| {
                full.try_push(&heap, Value::Null)
            }),
            ("push", &|| wide.try_push(&heap, Value::Null)),
            ("set", &|| wide.try_set(&heap, 700, Value::Null)),
            ("pop", &|| wide.try_pop(&heap)),
            ("prepend", &|| wide.try_prepend(&heap, Value::Null)),
            ("build", &|| PVector::try_from_values(&heap, &model)),
        ];
        for (name, op) in ops {
            let before = budget.used();
            let needed = {
                let _result = op().unwrap();
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
        check(&full, &model[..1024]);
        check(&wide, &model);
    }

    /// One `conj` onto a large vector admits its path copy, not the vector.
    #[test]
    fn an_update_of_a_large_vector_is_charged_for_its_path_only() {
        let heap = crate::heap::Heap::with_byte_limit(usize::MAX / 2);
        let budget = heap.byte_budget();
        let model: Vec<Value> = (0..5000).map(num).collect();
        let old = PVector::try_from_values(&heap, &model).unwrap();
        let whole = reachable_node_bytes([&old]);
        let before = budget.used();
        let new = old.try_push(&heap, Value::Null).unwrap();
        let path = budget.used() - before;
        assert_eq!(path, reachable_node_bytes([&old, &new]) - whole);
        assert!(path * 50 < whole, "{path} of {whole}");
    }
}
