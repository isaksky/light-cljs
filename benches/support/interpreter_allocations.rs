//! Benchmark-only requested-layout accounting, not RSS or allocator usable size.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
};

pub struct CountingAllocator;
static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn record(size: usize, previous: usize, realloc: bool) {
    let live = if size >= previous {
        LIVE.fetch_add(size - previous, Relaxed) + size - previous
    } else {
        LIVE.fetch_sub(previous - size, Relaxed) - (previous - size)
    };
    if ENABLED.load(Relaxed) {
        if realloc {
            REALLOCS.fetch_add(1, Relaxed);
        } else {
            ALLOCS.fetch_add(1, Relaxed);
        }
        BYTES.fetch_add(size, Relaxed);
        PEAK.fetch_max(live, Relaxed);
    }
}

// SAFETY: All requests/pointers/layouts are forwarded unchanged to System.
// Atomic bookkeeping allocates nothing; failed requests do not change counts.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies the GlobalAlloc layout contract.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size(), 0, false);
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies the GlobalAlloc layout contract.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size(), 0, false);
        }
        ptr
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: Pointer, old layout and new size are unchanged.
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        if !ptr.is_null() {
            record(size, layout.size(), true);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        // SAFETY: Every allocation was obtained from System with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

pub fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, usize, usize, usize, usize) {
    ALLOCS.store(0, Relaxed);
    REALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(LIVE.load(Relaxed), Relaxed);
    ENABLED.store(true, Relaxed);
    let result = f();
    ENABLED.store(false, Relaxed);
    (
        result,
        ALLOCS.load(Relaxed),
        REALLOCS.load(Relaxed),
        BYTES.load(Relaxed),
        LIVE.load(Relaxed),
        PEAK.load(Relaxed),
    )
}
