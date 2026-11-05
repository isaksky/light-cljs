//! Saved Tarjan walk shared by linking and synchronous evaluation. Each call
//! processes one edge, return, or SCC member; graph depth never uses Rust frames.

use super::ModuleKey;
use super::link::PreparedModule;
use std::collections::HashMap;

#[derive(Debug)]
struct Node {
    index: usize,
    ancestor: usize,
    active: bool,
}

#[derive(Debug)]
pub(super) struct Dfs {
    nodes: HashMap<ModuleKey, Node>,
    frames: Vec<(ModuleKey, usize)>,
    active: Vec<ModuleKey>,
    closing: Option<ModuleKey>,
}

pub(super) enum Event {
    Continue,
    Body(ModuleKey),
    Member { key: ModuleKey, root: ModuleKey },
    Complete,
}

impl Dfs {
    pub(super) fn new(entry: ModuleKey) -> Self {
        let mut walk = Self {
            nodes: HashMap::new(),
            frames: Vec::new(),
            active: Vec::new(),
            closing: None,
        };
        walk.enter(entry);
        walk
    }

    fn enter(&mut self, key: ModuleKey) {
        let index = self.nodes.len();
        self.nodes.insert(
            key,
            Node {
                index,
                ancestor: index,
                active: true,
            },
        );
        self.frames.push((key, 0));
        self.active.push(key);
    }

    pub(super) fn active(&self) -> impl Iterator<Item = ModuleKey> + '_ {
        self.active.iter().copied()
    }

    pub(super) fn step(&mut self, modules: &HashMap<ModuleKey, PreparedModule>) -> Event {
        if let Some(root) = self.closing {
            let key = self.active.pop().expect("SCC root on active stack");
            self.nodes.get_mut(&key).unwrap().active = false;
            if key == root {
                self.closing = None;
            }
            return Event::Member { key, root };
        }
        let Some((key, next)) = self.frames.last_mut() else {
            return Event::Complete;
        };
        let key = *key;
        if let Some(dep) = modules[&key].requests.get(*next).copied() {
            *next += 1;
            if let Some(node) = self.nodes.get(&dep) {
                if node.active {
                    let ancestor = node.ancestor;
                    let node = self.nodes.get_mut(&key).unwrap();
                    node.ancestor = node.ancestor.min(ancestor);
                }
            } else {
                self.enter(dep);
            }
            return Event::Continue;
        }
        self.frames.pop();
        let node = &self.nodes[&key];
        if node.index == node.ancestor {
            // The caller executes Body before the next step can pop this SCC.
            self.closing = Some(key);
        } else if let Some((parent, _)) = self.frames.last() {
            let ancestor = node.ancestor;
            let parent = self.nodes.get_mut(parent).unwrap();
            parent.ancestor = parent.ancestor.min(ancestor);
        }
        Event::Body(key)
    }
}
