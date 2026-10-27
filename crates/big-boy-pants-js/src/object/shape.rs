//! Immutable ordinary-object layouts. Transitions are shared across threads;
//! a bounded thread-local front cache keeps repeated construction lock-free.
//! Layouts contain slot keys, never heap handles. They are hints, not GC roots:
//! a cache hit additionally checks the live slot key (key IDs can be reused).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use super::PropertyKey;

#[derive(Debug)]
pub(super) struct Shape {
    pub(super) id: u32,
    keys: Vec<PropertyKey>,
}

type Edge = (u32, PropertyKey);
type Transitions = HashMap<Edge, Arc<ShapeHandle>>;
const LIMIT: usize = 4096;

struct Local {
    transitions: Transitions,
    front: Vec<Option<(Edge, Arc<ShapeHandle>)>>,
}

impl Default for Local {
    fn default() -> Self {
        Self {
            transitions: HashMap::new(),
            // Initialize directly in heap storage: a fixed array creates
            // large TLS initialization temporaries on 64 KiB VM workers.
            front: (0..64).map(|_| None).collect(),
        }
    }
}

/// Repeated construction changes the worker-local outer reference count,
/// not the immutable shape's globally shared count. Migration is safe: both
/// layers remain ordinary owning Arcs and contain no interior mutability.
#[derive(Debug)]
pub(super) struct ShapeHandle(Arc<Shape>);

impl std::ops::Deref for ShapeHandle {
    type Target = Shape;
    fn deref(&self) -> &Shape {
        &self.0
    }
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
    static EMPTY_HANDLE: Arc<ShapeHandle> = Arc::new(ShapeHandle(Shape::empty_shared()));
}

impl Shape {
    pub(super) fn append_empty(key: PropertyKey) -> Option<Arc<ShapeHandle>> {
        EMPTY_HANDLE.with(|shape| shape.append(key))
    }

    fn empty_shared() -> Arc<Self> {
        static EMPTY: OnceLock<Arc<Shape>> = OnceLock::new();
        Arc::clone(EMPTY.get_or_init(|| {
            Arc::new(Shape {
                id: 1,
                keys: Vec::new(),
            })
        }))
    }

    pub(super) fn append(&self, key: PropertyKey) -> Option<Arc<ShapeHandle>> {
        // Large or deletion-heavy objects use the existing dictionary store.
        if self.keys.len() >= 64 {
            return None;
        }
        let edge = (self.id, key);
        LOCAL.with(|local| {
            let mut local = local.borrow_mut();
            let front = (self.id as usize ^ (key.0 as usize).wrapping_mul(31)) & 63;
            if let Some((cached, shape)) = &local.front[front]
                && *cached == edge
            {
                return Some(Arc::clone(shape));
            }
            if let Some(shape) = local.transitions.get(&edge).cloned() {
                local.front[front] = Some((edge, Arc::clone(&shape)));
                return Some(shape);
            }
            static SHARED: OnceLock<Mutex<HashMap<Edge, Weak<Shape>>>> = OnceLock::new();
            static NEXT: AtomicU64 = AtomicU64::new(2);
            let mut shared = SHARED
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let shape = match shared.get(&edge).and_then(Weak::upgrade) {
                Some(shape) => shape,
                None => {
                    let id = u32::try_from(NEXT.fetch_add(1, Ordering::Relaxed)).ok()?;
                    let mut keys = self.keys.clone();
                    keys.push(key);
                    let shape = Arc::new(Shape { id, keys });
                    if shared.len() >= LIMIT {
                        shared.retain(|_, shape| shape.strong_count() != 0);
                    }
                    if shared.len() >= LIMIT {
                        shared.clear();
                    }
                    shared.insert(edge, Arc::downgrade(&shape));
                    shape
                }
            };
            if local.transitions.len() >= LIMIT {
                local.transitions.clear();
                local.front.fill(None);
            }
            let handle = Arc::new(ShapeHandle(shape));
            local.front[front] = Some((edge, Arc::clone(&handle)));
            local.transitions.insert(edge, Arc::clone(&handle));
            Some(handle)
        })
    }
}

/// One atomic word prevents a concurrent reader observing another shape's
/// slot. Cache replacement is harmless, and neither a hit nor a miss locks.
#[derive(Debug, Default)]
pub(crate) struct InlineCache(AtomicU64);

impl InlineCache {
    pub(crate) fn slot(&self, shape: u32) -> Option<usize> {
        let word = self.0.load(Ordering::Relaxed);
        (shape != 0 && word >> 32 == u64::from(shape)).then_some(word as u32 as usize)
    }

    pub(crate) fn remember(&self, shape: u32, slot: usize) {
        let word = (u64::from(shape) << 32) | slot as u64;
        if self.0.load(Ordering::Relaxed) != word {
            self.0.store(word, Ordering::Relaxed);
        }
    }
}
