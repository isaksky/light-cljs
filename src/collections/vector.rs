//! Persistent vector body: a 32-way radix trie with path copying.
//!
//! Every node is immutable once built and shared through `Arc`. An update
//! copies only the path from the root to the changed leaf (at most
//! `ceil(log32 n)` nodes); every other node is shared with the previous
//! version. The trie is left-packed: all leaves are full except the last,
//! and `shift` is the bit offset of the root's index digit (0 when the root
//! is the only leaf).
//!
//! Elements are raw [`Value`]s. Heap references among them are reported by
//! [`Trace`]; the trie nodes themselves are not heap objects (see the
//! storage boundary in [`crate::collections`]).

use std::sync::Arc;

use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::value::Value;

const BITS: u32 = 5;
const WIDTH: usize = 1 << BITS;
const MASK: usize = WIDTH - 1;

#[derive(Debug)]
enum Node {
    Branch(Vec<Arc<Node>>),
    Leaf(Vec<Value>),
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
            root: Arc::new(Node::Leaf(Vec::new())),
        }
    }
}

impl PVector {
    /// A vector of `values`, built bottom-up in O(n).
    pub(crate) fn from_values(values: &[Value]) -> PVector {
        let mut level: Vec<Arc<Node>> = values
            .chunks(WIDTH)
            .map(|c| Arc::new(Node::Leaf(c.to_vec())))
            .collect();
        let mut shift = 0;
        while level.len() > 1 {
            level = level
                .chunks(WIDTH)
                .map(|c| Arc::new(Node::Branch(c.to_vec())))
                .collect();
            shift += BITS;
        }
        match level.pop() {
            Some(root) => PVector {
                len: values.len(),
                shift,
                root,
            },
            None => PVector::default(),
        }
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
            match node {
                Node::Branch(children) => {
                    node = children.get((index >> level) & MASK)?;
                    level = level.saturating_sub(BITS);
                }
                Node::Leaf(values) => return values.get(index & MASK).cloned(),
            }
        }
    }

    /// A new version with `index` set to `value`; `None` when out of bounds.
    pub(crate) fn set(&self, index: usize, value: Value) -> Option<PVector> {
        if index >= self.len {
            return None;
        }
        Some(PVector {
            root: Arc::new(set_in(&self.root, self.shift, index, value)),
            ..self.clone()
        })
    }

    /// A new version with `value` appended.
    pub(crate) fn push(&self, value: Value) -> PVector {
        // `checked_shl` fails only for a 64-level-deep trie, which cannot exist.
        let capacity = 1usize.checked_shl(self.shift + BITS).unwrap_or(usize::MAX);
        if self.len == capacity {
            let root = Node::Branch(vec![
                Arc::clone(&self.root),
                Arc::new(new_path(self.shift, value)),
            ]);
            PVector {
                len: self.len + 1,
                shift: self.shift + BITS,
                root: Arc::new(root),
            }
        } else {
            PVector {
                len: self.len + 1,
                shift: self.shift,
                root: Arc::new(push_in(&self.root, self.shift, self.len, value)),
            }
        }
    }

    /// A new version without the last element; `None` when empty.
    pub(crate) fn pop(&self) -> Option<PVector> {
        match self.len {
            0 => return None,
            1 => return Some(PVector::default()),
            _ => {}
        }
        let mut root = Arc::new(pop_in(&self.root, self.shift, self.len - 1)?);
        let mut shift = self.shift;
        // Drop root levels with a single child so the trie stays minimal
        // (and `push`'s capacity check stays exact).
        while shift > 0 {
            let only = match &*root {
                Node::Branch(children) if children.len() == 1 => Arc::clone(&children[0]),
                _ => break,
            };
            root = only;
            shift -= BITS;
        }
        Some(PVector {
            len: self.len - 1,
            shift,
            root,
        })
    }

    /// Call `f` on every element in index order.
    pub(crate) fn for_each(&self, mut f: impl FnMut(&Value)) {
        fn walk(node: &Node, f: &mut dyn FnMut(&Value)) {
            match node {
                Node::Branch(children) => children.iter().for_each(|c| walk(c, f)),
                Node::Leaf(values) => values.iter().for_each(f),
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
            if let Node::Branch(children) = &**node {
                children.iter().for_each(|c| walk(c, out));
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }
}

fn set_in(node: &Node, level: u32, index: usize, value: Value) -> Node {
    match node {
        Node::Leaf(values) => {
            let mut values = values.clone();
            values[index & MASK] = value;
            Node::Leaf(values)
        }
        Node::Branch(children) => {
            let i = (index >> level) & MASK;
            let mut copy = children.clone();
            copy[i] = Arc::new(set_in(&children[i], level - BITS, index, value));
            Node::Branch(copy)
        }
    }
}

/// A fresh spine of single-child branches down to a leaf holding `value`.
fn new_path(level: u32, value: Value) -> Node {
    if level == 0 {
        Node::Leaf(vec![value])
    } else {
        Node::Branch(vec![Arc::new(new_path(level - BITS, value))])
    }
}

/// Append `value` at position `index` (the current length) below `node`,
/// which has room for it.
fn push_in(node: &Node, level: u32, index: usize, value: Value) -> Node {
    match node {
        Node::Leaf(values) => {
            let mut values = values.clone();
            values.push(value);
            Node::Leaf(values)
        }
        Node::Branch(children) => {
            let i = (index >> level) & MASK;
            let mut copy = children.clone();
            match children.get(i) {
                Some(child) => copy[i] = Arc::new(push_in(child, level - BITS, index, value)),
                None => copy.push(Arc::new(new_path(level - BITS, value))),
            }
            Node::Branch(copy)
        }
    }
}

/// Remove the element at `index` (the last one) below `node`; `None` when the
/// node becomes empty.
fn pop_in(node: &Node, level: u32, index: usize) -> Option<Node> {
    match node {
        Node::Leaf(values) => {
            let mut values = values.clone();
            values.pop();
            (!values.is_empty()).then_some(Node::Leaf(values))
        }
        Node::Branch(children) => {
            let i = (index >> level) & MASK;
            let mut copy = children.clone();
            match pop_in(&children[i], level - BITS, index) {
                Some(child) => copy[i] = Arc::new(child),
                // The emptied child is the last one.
                None => copy.truncate(i),
            }
            (!copy.is_empty()).then_some(Node::Branch(copy))
        }
    }
}

impl Trace for PVector {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
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
}
