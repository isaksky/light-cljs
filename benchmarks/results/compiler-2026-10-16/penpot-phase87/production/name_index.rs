//! Persistent global-name lookup for definition snapshots. A version shares its
//! root; an insertion copies only a bounded hash-trie path, never a name table.
use crate::analyzer::GlobalId;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::rc::Rc;

type Key = (String, String);

#[derive(Clone, Default)]
pub(crate) struct NameIndex {
    root: Option<Rc<Node>>,
    hash: RandomState,
}

#[derive(Clone)]
enum Node {
    Leaf {
        hash: u64,
        entries: Vec<(Key, GlobalId)>,
    },
    Branch {
        bitmap: u32,
        children: Vec<Rc<Node>>,
    },
}

fn bit(hash: u64, shift: u32) -> u32 {
    1 << ((hash >> shift) & 31)
}
fn position(bitmap: u32, bit: u32) -> usize {
    (bitmap & (bit - 1)).count_ones() as usize
}

impl NameIndex {
    pub fn get(&self, key: &Key) -> Option<&GlobalId> {
        let hash = self.hash.hash_one(key);
        let mut node = self.root.as_deref()?;
        let mut shift = 0;
        loop {
            match node {
                Node::Leaf { hash: old, entries } => {
                    return (*old == hash)
                        .then(|| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v))
                        .flatten();
                }
                Node::Branch { bitmap, children } => {
                    let bit = bit(hash, shift);
                    if bitmap & bit == 0 {
                        return None;
                    }
                    node = &children[position(*bitmap, bit)];
                    shift += 5;
                }
            }
        }
    }

    pub fn insert(&mut self, key: Key, value: GlobalId) -> Option<GlobalId> {
        let hash = self.hash.hash_one(&key);
        match &mut self.root {
            Some(root) => insert(root, hash, key, value, 0),
            None => {
                self.root = Some(Rc::new(Node::Leaf {
                    hash,
                    entries: vec![(key, value)],
                }));
                None
            }
        }
    }
}

// Different hashes share branches until their next five bits differ. Equal
// hashes stay in a collision leaf and are compared by the complete key.
fn join(old: Rc<Node>, old_hash: u64, new: Rc<Node>, new_hash: u64, shift: u32) -> Rc<Node> {
    let a = bit(old_hash, shift);
    let b = bit(new_hash, shift);
    let children = if a == b {
        vec![join(old, old_hash, new, new_hash, shift + 5)]
    } else if a < b {
        vec![old, new]
    } else {
        vec![new, old]
    };
    Rc::new(Node::Branch {
        bitmap: a | b,
        children,
    })
}

fn insert(
    node: &mut Rc<Node>,
    hash: u64,
    key: Key,
    value: GlobalId,
    shift: u32,
) -> Option<GlobalId> {
    if let Node::Leaf {
        hash: old_hash,
        entries,
    } = node.as_ref()
    {
        if *old_hash != hash {
            *node = join(
                node.clone(),
                *old_hash,
                Rc::new(Node::Leaf {
                    hash,
                    entries: vec![(key, value)],
                }),
                hash,
                shift,
            );
            return None;
        }
        // Reconfirming a declaration's ID should not copy even the leaf.
        if entries.iter().any(|(k, v)| k == &key && *v == value) {
            return Some(value);
        }
    }
    match Rc::make_mut(node) {
        Node::Leaf { entries, .. } => {
            if let Some((_, previous)) = entries.iter_mut().find(|(k, _)| *k == key) {
                Some(std::mem::replace(previous, value))
            } else {
                entries.push((key, value));
                None
            }
        }
        Node::Branch { bitmap, children } => {
            let bit = bit(hash, shift);
            let index = position(*bitmap, bit);
            if *bitmap & bit == 0 {
                children.insert(
                    index,
                    Rc::new(Node::Leaf {
                        hash,
                        entries: vec![(key, value)],
                    }),
                );
                *bitmap |= bit;
                None
            } else {
                insert(&mut children[index], hash, key, value, shift + 5)
            }
        }
    }
}

impl std::ops::Index<&Key> for NameIndex {
    type Output = GlobalId;
    fn index(&self, key: &Key) -> &GlobalId {
        self.get(key).expect("global name")
    }
}
impl FromIterator<(Key, GlobalId)> for NameIndex {
    fn from_iter<I: IntoIterator<Item = (Key, GlobalId)>>(items: I) -> Self {
        let mut index = Self::default();
        for (key, value) in items {
            index.insert(key, value);
        }
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn definition_versions_preserve_overwritten_and_new_names() {
        let mut names = NameIndex::default();
        let mut oracle = HashMap::new();
        let mut snapshots = Vec::new();
        for i in 0..3000 {
            let key = (format!("ns{}", i % 13), format!("name{}", i % 71));
            assert_eq!(
                names.insert(key.clone(), GlobalId(i)),
                oracle.insert(key, GlobalId(i))
            );
            if i % 113 == 0 {
                snapshots.push((names.clone(), oracle.clone()));
            }
        }
        snapshots.push((names, oracle));
        for (names, oracle) in snapshots {
            for (key, value) in oracle {
                assert_eq!(names.get(&key), Some(&value));
            }
            assert_eq!(names.get(&("missing".into(), "missing".into())), None);
        }
    }

    #[test]
    fn collision_leaves_and_last_hash_bits_preserve_every_key() {
        let key = |n| ("ns".into(), format!("k{n}"));
        let mut root = Rc::new(Node::Leaf {
            hash: 0,
            entries: vec![(key(0), GlobalId(0))],
        });
        for i in 1..8 {
            assert_eq!(insert(&mut root, 0, key(i), GlobalId(i), 0), None);
        }
        let old = root.clone();
        assert_eq!(
            insert(&mut root, 0, key(4), GlobalId(99), 0),
            Some(GlobalId(4))
        );
        let Node::Leaf { entries, .. } = old.as_ref() else {
            panic!("collision leaf")
        };
        assert_eq!(entries[4].1, GlobalId(4));
        insert(&mut root, 1 << 63, key(8), GlobalId(8), 0);
        let mut node = root.as_ref();
        let mut shift = 0;
        while let Node::Branch { bitmap, children } = node {
            node = &children[position(*bitmap, bit(1 << 63, shift))];
            shift += 5;
        }
        let Node::Leaf { entries, .. } = node else {
            unreachable!()
        };
        assert_eq!(entries, &[(key(8), GlobalId(8))]);
    }
}
