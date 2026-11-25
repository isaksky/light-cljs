//! Backing stores can outlive an isolate and may be released on GC threads.
//! Their shared allocator owns this atomic budget independently of JS roots.
use crate::evaluator::{EvalError, ExecutionControl, MemoryPool};
use std::ffi::c_void;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) enum BudgetKind {
    Standalone,
    Reader,
    Build { pool: Arc<MemoryPool>, main: bool },
}

pub(super) struct Budget {
    used: AtomicUsize,
    heap: AtomicUsize,
    /// What the isolate's heap and backing stores may hold together: the
    /// current allowance. A reader grows from V8's startup minimum to its
    /// own cap. A pooled macro grows past its base using shared headroom,
    /// which it gives back when the budget is dropped.
    allowance: AtomicUsize,
    limit: usize,
    /// The build's pool and the isolate's base in it: the main isolate's
    /// or a worker's.
    pool: Option<(Arc<MemoryPool>, usize)>,
    /// The allowance a worker isolate keeps between jobs, its base and
    /// [`MemoryPool::keep`]; unbounded for any other.
    kept: usize,
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
    pub(super) fn new(limit: usize, control: ExecutionControl, kind: BudgetKind) -> Arc<Self> {
        let (allowance, pool, kept) = match kind {
            BudgetKind::Standalone => (limit, None, usize::MAX),
            BudgetKind::Reader => (super::MINIMUM_HEAP_LIMIT, None, usize::MAX),
            BudgetKind::Build { pool, main } => {
                let base = if main { pool.main_base() } else { pool.base() };
                let kept = if main { usize::MAX } else { base + pool.keep() };
                (base, Some((pool, base)), kept)
            }
        };
        Arc::new(Self {
            used: AtomicUsize::new(0),
            heap: AtomicUsize::new(0),
            allowance: AtomicUsize::new(allowance),
            limit,
            pool,
            kept,
            refused: AtomicUsize::new(0),
            control,
        })
    }

    /// What the heap and backing stores may hold together now.
    pub(super) fn allowance(&self) -> usize {
        self.allowance.load(Ordering::Relaxed)
    }

    /// Grow by up to `wanted` bytes, provided at least `needed` are available.
    #[allow(deprecated)] // fetch_update supports the stable compiler used in CI.
    pub(super) fn grow(&self, wanted: usize, needed: usize) -> bool {
        let needed = needed.max(1);
        if let Some((pool, _)) = &self.pool {
            let taken = pool.take(wanted, needed);
            self.allowance.fetch_add(taken, Ordering::Relaxed);
            return taken > 0;
        }
        self.allowance
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |allowance| {
                let growth = wanted.min(self.limit - allowance);
                (growth >= needed).then_some(allowance + growth)
            })
            .is_ok()
    }

    /// The headroom the budget took from its build's pool.
    pub(super) fn headroom_held(&self) -> usize {
        self.pool
            .as_ref()
            .map_or(0, |(_, base)| self.allowance().saturating_sub(*base))
    }

    /// The allowance a worker isolate keeps between jobs
    /// ([`MemoryPool::keep`]): it grows toward it before past it.
    pub(super) fn kept(&self) -> usize {
        self.kept
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
            // Try growing the allowance before V8 collects garbage and
            // retries. Failure becomes terminal only when no retry fits.
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
        if let Some((pool, base)) = &self.pool {
            pool.give(self.allowance().saturating_sub(*base));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluator::{EvaluatorOptions, WORKER_SHARE};

    #[test]
    fn the_macro_limit_and_one_worker_share_per_used_reader_bound_the_total() {
        let limit = 17 * WORKER_SHARE + (32 << 20);
        let pool = MemoryPool::new(limit, 17);
        let control = ExecutionControl::new(&EvaluatorOptions::default()).unwrap();
        let macros: Vec<_> = (0..17)
            .map(|slot| {
                Budget::new(
                    limit,
                    control.clone(),
                    BudgetKind::Build {
                        pool: pool.clone(),
                        main: slot == 0,
                    },
                )
            })
            .collect();
        let headroom = pool.headroom();
        assert!(headroom > 0);
        let readers: Vec<_> = (0..17)
            .map(|_| Budget::new(WORKER_SHARE, control.clone(), BudgetKind::Reader))
            .collect();
        for reader in &readers {
            let initial = super::super::MINIMUM_HEAP_LIMIT;
            assert_eq!(reader.allowance(), initial);
            assert!(!reader.grow(WORKER_SHARE, WORKER_SHARE));
            assert_eq!(reader.allowance(), initial);
            assert!(reader.grow(WORKER_SHARE, WORKER_SHARE - initial));
            assert_eq!(reader.allowance(), WORKER_SHARE);
            assert!(!reader.grow(1, 1));
        }
        assert_eq!(pool.headroom(), headroom);
        assert!(macros[0].grow(headroom, headroom));
        assert!(!macros[1].grow(1, 1));
        assert_eq!(macros.iter().map(|m| m.allowance()).sum::<usize>(), limit);
        assert_eq!(
            macros
                .iter()
                .chain(&readers)
                .map(|budget| budget.allowance())
                .sum::<usize>(),
            limit + readers.len() * WORKER_SHARE
        );
    }
}
