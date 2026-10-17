//! Backing stores can outlive an isolate and may be released on GC threads.
//! Their shared allocator owns this atomic budget independently of JS roots.
use crate::evaluator::{EvalError, ExecutionControl};
use std::ffi::c_void;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) struct Budget {
    used: AtomicUsize,
    heap: AtomicUsize,
    limit: usize,
    control: ExecutionControl,
}

#[repr(C)]
struct Callbacks {
    reserve: unsafe extern "C" fn(*const c_void, usize) -> bool,
    release: unsafe extern "C" fn(*const c_void, usize),
    failed: unsafe extern "C" fn(*const c_void),
    drop: unsafe extern "C" fn(*const c_void),
}

unsafe extern "C" {
    fn light_v8_buffer_allocator(
        state: *const c_void,
        callbacks: *const Callbacks,
    ) -> *mut v8::Allocator;
}

impl Budget {
    pub(super) fn new(limit: usize, control: ExecutionControl) -> Arc<Self> {
        Arc::new(Self {
            used: AtomicUsize::new(0),
            heap: AtomicUsize::new(0),
            limit,
            control,
        })
    }

    pub(super) fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub(super) fn observe_heap(&self, bytes: usize) {
        self.heap.store(bytes, Ordering::Relaxed);
    }

    pub(super) fn exhausted(&self) -> bool {
        self.used()
            .saturating_add(self.heap.load(Ordering::Relaxed))
            > self.limit
    }

    pub(super) fn allocator(self: &Arc<Self>) -> Result<v8::UniqueRef<v8::Allocator>, EvalError> {
        static CALLBACKS: Callbacks = Callbacks {
            reserve,
            release,
            failed,
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
        let Some(next) = current.checked_add(length).filter(|&n| {
            n <= state
                .limit
                .saturating_sub(state.heap.load(Ordering::Relaxed))
        }) else {
            // V8 may collect garbage and retry. Failure becomes terminal only
            // through `failed`, after the engine exhausts its allocation retries.
            return false;
        };
        match state
            .used
            .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

unsafe extern "C" fn release(state: *const c_void, length: usize) {
    unsafe { &*state.cast::<Budget>() }
        .used
        .fetch_sub(length, Ordering::Relaxed);
}

unsafe extern "C" fn failed(state: *const c_void) {
    unsafe { &*state.cast::<Budget>() }
        .control
        .memory_exhausted();
}

unsafe extern "C" fn drop_budget(state: *const c_void) {
    drop(unsafe { Arc::from_raw(state.cast::<Budget>()) });
}
