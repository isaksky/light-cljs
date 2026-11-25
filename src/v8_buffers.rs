//! Backing stores can outlive an isolate and may be released on GC threads.
//! Their shared allocator owns this atomic budget independently of JS roots.
use crate::evaluator::{EvalError, ExecutionControl, MemoryPool};
use std::ffi::c_void;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) enum Share {
    Macro {
        slot: usize,
        readers_registered: bool,
    },
    Reader(Option<usize>),
}

impl Share {
    fn slot(&self) -> Option<usize> {
        match *self {
            Self::Macro { slot, .. } => Some(slot),
            Self::Reader(slot) => slot,
        }
    }
}

struct PooledBudget {
    memory: Arc<MemoryPool>,
    share: Share,
    base: usize,
    slot_held: AtomicUsize,
}

pub(super) struct Budget {
    used: AtomicUsize,
    heap: AtomicUsize,
    /// What the isolate's heap and backing stores may hold together: the
    /// session's memory limit, or with a build's [`MemoryPool`] its base
    /// plus headroom taken from the slot and build. The headroom is given
    /// back when the budget is dropped.
    allowance: AtomicUsize,
    pool: Option<PooledBudget>,
    /// The length of the last reservation the budget refused, or 0. V8
    /// retries a refused allocation after collecting garbage; a retry that
    /// fits clears it, and one still set when the evaluator next checks
    /// memory makes the session terminal, so JavaScript cannot catch its
    /// way past the limit.
    refused: AtomicUsize,
    control: ExecutionControl,
}

#[repr(C)]
struct Callbacks {
    reserve: unsafe extern "C" fn(*const c_void, usize) -> bool,
    release: unsafe extern "C" fn(*const c_void, usize),
    drop: unsafe extern "C" fn(*const c_void),
}

unsafe extern "C" {
    fn light_v8_buffer_allocator(
        state: *const c_void,
        callbacks: *const Callbacks,
    ) -> *mut v8::Allocator;
}

impl Budget {
    /// The budget of `limit` bytes, or a base allowance with growth
    /// charged to the build's pool.
    pub(super) fn new(
        limit: usize,
        control: ExecutionControl,
        pool: Option<(Arc<MemoryPool>, Share)>,
    ) -> Result<Arc<Self>, EvalError> {
        let (pool, allowance) = if let Some((memory, share)) = pool {
            let startup = match share {
                Share::Macro {
                    slot,
                    readers_registered,
                } => {
                    let reserved = if slot == 0 {
                        memory.main_base()
                    } else {
                        memory.base()
                    };
                    let budget = memory.v8_slot(slot);
                    let base = budget.take_macro_startup();
                    if base >= super::MINIMUM_HEAP_LIMIT {
                        // Keep the original heap threshold in builds without
                        // readers. At 32 MiB larger macro workloads can spend
                        // too much time collecting (light-cljs-441).
                        let local = if readers_registered {
                            0
                        } else {
                            budget.take(reserved - base)
                        };
                        Some((base, local))
                    } else {
                        budget.give_macro_startup(base);
                        None
                    }
                }
                Share::Reader(_) => {
                    let base = super::MINIMUM_HEAP_LIMIT;
                    memory.take_reader_base(base).then_some((base, 0))
                }
            };
            let Some((base, local)) = startup else {
                control.memory_exhausted();
                return Err(EvalError(
                    "JavaScript session memory limit exhausted".into(),
                ));
            };
            (
                Some(PooledBudget {
                    memory,
                    share,
                    base,
                    slot_held: AtomicUsize::new(local),
                }),
                base + local,
            )
        } else {
            (None, limit)
        };
        Ok(Arc::new(Self {
            used: AtomicUsize::new(0),
            heap: AtomicUsize::new(0),
            allowance: AtomicUsize::new(allowance),
            pool,
            refused: AtomicUsize::new(0),
            control,
        }))
    }

    /// What the heap and backing stores may hold together now.
    pub(super) fn allowance(&self) -> usize {
        self.allowance.load(Ordering::Relaxed)
    }

    /// Take up to `wanted` bytes from the slot and then the shared pool.
    /// If they cannot cover `needed`, leave both balances as they were.
    pub(super) fn grow(&self, wanted: usize, needed: usize) -> bool {
        let Some(pool) = &self.pool else {
            return false;
        };
        let slot = pool.share.slot().map(|slot| pool.memory.v8_slot(slot));
        let local = slot.map_or(0, |slot| slot.take(wanted));
        let needed = needed.max(1);
        let shared = pool
            .memory
            .take(wanted - local, needed.saturating_sub(local));
        let taken = local + shared;
        if taken < needed {
            if let Some(slot) = slot {
                slot.give(local);
            }
            return false;
        }
        pool.slot_held.fetch_add(local, Ordering::Relaxed);
        self.allowance.fetch_add(taken, Ordering::Relaxed);
        true
    }

    pub(super) fn slot(&self) -> Option<usize> {
        self.pool.as_ref().and_then(|pool| pool.share.slot())
    }

    /// The headroom taken outside the slot's reserved share.
    pub(super) fn headroom_held(&self) -> usize {
        self.pool.as_ref().map_or(0, |pool| {
            self.allowance()
                .saturating_sub(pool.base + pool.slot_held.load(Ordering::Relaxed))
        })
    }

    /// The allowance a worker isolate keeps between jobs
    /// ([`MemoryPool::keep`]): it grows toward it before past it.
    pub(super) fn kept(&self) -> usize {
        let Some(pool) = &self.pool else {
            return usize::MAX;
        };
        if matches!(pool.share, Share::Macro { slot: 0, .. }) {
            return usize::MAX;
        }
        let local = pool
            .share
            .slot()
            .map_or(0, |slot| pool.memory.v8_slot(slot).headroom());
        self.allowance()
            .saturating_sub(self.headroom_held())
            .saturating_add(local)
            .saturating_add(pool.memory.keep())
    }

    /// Whether an allocation was refused and not satisfied by a retry.
    pub(super) fn refused(&self) -> bool {
        self.refused.load(Ordering::Relaxed) != 0
    }

    pub(super) fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub(super) fn observe_heap(&self, bytes: usize) {
        self.heap.store(bytes, Ordering::Relaxed);
    }

    pub(super) fn exhausted(&self) -> bool {
        let held = self
            .used()
            .saturating_add(self.heap.load(Ordering::Relaxed));
        let allowance = self.allowance();
        held > allowance && !self.grow(held - allowance, held - allowance)
    }

    pub(super) fn allocator(self: &Arc<Self>) -> Result<v8::UniqueRef<v8::Allocator>, EvalError> {
        static CALLBACKS: Callbacks = Callbacks {
            reserve,
            release,
            drop: drop_budget,
        };
        let state = Arc::into_raw(self.clone()).cast();
        // SAFETY: C++ owns this Arc reference on success and releases it only
        // after all shared allocator/backing-store owners have gone away.
        let allocator =
            unsafe { v8::UniquePtr::from_raw(light_v8_buffer_allocator(state, &CALLBACKS)) };
        if allocator.is_null() {
            unsafe { drop_budget(state) };
            self.control.memory_exhausted();
            return Err(EvalError(
                "JavaScript session memory limit exhausted".into(),
            ));
        }
        Ok(allocator.unwrap())
    }
}

unsafe extern "C" fn reserve(state: *const c_void, length: usize) -> bool {
    // SAFETY: the allocator retains its Arc until after all callbacks finish.
    let state = unsafe { &*state.cast::<Budget>() };
    let mut current = state.used.load(Ordering::Relaxed);
    loop {
        let Some(next) = current.checked_add(length) else {
            return false;
        };
        let room = state
            .allowance()
            .saturating_sub(state.heap.load(Ordering::Relaxed));
        if next > room {
            // A build's isolate takes what it lacks from the pool, or V8
            // may collect garbage and retry. Failure becomes terminal only
            // when no retry fits (`refused`).
            if state.grow(next - room, next - room) {
                continue;
            }
            state.refused.store(length.max(1), Ordering::Relaxed);
            return false;
        }
        match state
            .used
            .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => {
                let _ = state.refused.compare_exchange(
                    length.max(1),
                    0,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                return true;
            }
            Err(observed) => current = observed,
        }
    }
}

unsafe extern "C" fn release(state: *const c_void, length: usize) {
    unsafe { &*state.cast::<Budget>() }
        .used
        .fetch_sub(length, Ordering::Relaxed);
}

unsafe extern "C" fn drop_budget(state: *const c_void) {
    drop(unsafe { Arc::from_raw(state.cast::<Budget>()) });
}

impl Drop for Budget {
    fn drop(&mut self) {
        if let Some(pool) = &self.pool {
            pool.memory.give(self.headroom_held());
            if let Some(slot) = pool.share.slot() {
                pool.memory
                    .v8_slot(slot)
                    .give(pool.slot_held.load(Ordering::Relaxed));
            }
            match pool.share {
                Share::Macro { slot, .. } => {
                    pool.memory.v8_slot(slot).give_macro_startup(pool.base)
                }
                Share::Reader(_) => pool.memory.give_reader_base(pool.base),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluator::{DEFAULT_MEMORY_LIMIT, EvaluatorOptions, WORKER_SHARE};

    #[test]
    fn parallel_reader_bases_fit_and_cannot_exceed_a_quarter_of_the_build_limit() {
        let workers = 16;
        let limit = DEFAULT_MEMORY_LIMIT + workers * WORKER_SHARE;
        let pool = MemoryPool::new(limit, 1 + workers);
        let budget = |share| {
            Budget::new(
                limit,
                ExecutionControl::new(&EvaluatorOptions::default()).unwrap(),
                Some((pool.clone(), share)),
            )
        };
        let mut readers: Vec<_> = (0..=workers)
            .map(|slot| budget(Share::Reader(Some(slot))).unwrap())
            .collect();
        let base_bytes: usize = readers.iter().map(|reader| reader.allowance()).sum();
        assert_eq!(base_bytes, limit / 4);
        assert_eq!(pool.headroom(), 0);
        assert!(budget(Share::Reader(None)).is_err());

        let macros: Vec<_> = (0..=workers)
            .map(|slot| {
                budget(Share::Macro {
                    slot,
                    readers_registered: true,
                })
                .unwrap()
            })
            .collect();
        for (reader, macros) in readers.iter().zip(&macros) {
            // A rejected growth leaves the slot's memory available for a retry.
            assert!(!reader.grow(WORKER_SHARE, WORKER_SHARE));
            let free = WORKER_SHARE - macros.allowance();
            assert!(reader.grow(free, free));
            assert!(!macros.grow(1, 1));
        }
        let allowances: usize = macros
            .iter()
            .chain(&readers)
            .map(|budget| budget.allowance())
            .sum();
        assert_eq!(allowances, limit + limit / 4);

        drop(readers.pop().unwrap());
        let reader = budget(Share::Reader(Some(workers))).unwrap();
        let free = WORKER_SHARE - macros[workers].allowance();
        assert!(reader.grow(free, free));
    }

    #[test]
    fn parallel_macros_with_no_readers_start_with_the_full_slot_share() {
        let limit = 2 << 30;
        let pool = MemoryPool::new(limit, 3);
        for slot in [0, 1] {
            let budget = Budget::new(
                limit,
                ExecutionControl::new(&EvaluatorOptions::default()).unwrap(),
                Some((
                    pool.clone(),
                    Share::Macro {
                        slot,
                        readers_registered: false,
                    },
                )),
            )
            .unwrap();
            assert_eq!(budget.allowance(), 256 << 20);
            assert_eq!(budget.headroom_held(), 0);
        }
    }
}
