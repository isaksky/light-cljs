use std::{hint::black_box, rc::Rc};
#[cfg(not(count_allocations))]
use std::time::Instant;

#[cfg(count_allocations)]
mod allocation {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};
    pub static CALLS: AtomicUsize = AtomicUsize::new(0);
    pub static BYTES: AtomicUsize = AtomicUsize::new(0);
    pub struct Count;
    unsafe impl GlobalAlloc for Count {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size, Ordering::Relaxed);
            unsafe { System.realloc(ptr, old, size) }
        }
    }
}
#[cfg(count_allocations)]
#[global_allocator]
static ALLOCATOR: allocation::Count = allocation::Count;

#[inline(never)]
fn old(text: &str) -> Rc<[u16]> { text.encode_utf16().collect() }
#[inline(never)]
fn candidate(text: &str) -> Rc<[u16]> {
    if text.is_ascii() { text.bytes().map(u16::from).collect() }
    else { text.encode_utf16().collect() }
}

fn main() {
    let mode = std::env::args().nth(1).expect("old or candidate");
    let constructor = match mode.as_str() {
        "old" => old,
        "candidate" => candidate,
        _ => panic!("old or candidate"),
    };
    let cases = ["", "ns", "cljs.core", "app.main.data.workspace", "render-svg",
        "binding-form?", "update-layout-position-data", "a\0b\n", "Ω/名", "x😀y"];
    for case in cases {
        assert_eq!(&*candidate(case), &*old(case));
    }
    for code in 0..=0x10ffff {
        if let Some(character) = char::from_u32(code) {
            let text = format!("prefix{character}suffix");
            assert_eq!(&*candidate(&text), &*old(&text));
        }
    }
    #[cfg(count_allocations)]
    {
        use std::sync::atomic::Ordering;
        allocation::CALLS.store(0, Ordering::Relaxed);
        allocation::BYTES.store(0, Ordering::Relaxed);
        for case in cases { black_box(constructor(black_box(case))); }
        let calls = allocation::CALLS.load(Ordering::Relaxed);
        let bytes = allocation::BYTES.load(Ordering::Relaxed);
        println!("{mode}: {calls} allocations/reallocations, {bytes} requested bytes across {} cases", cases.len());
    }
    #[cfg(not(count_allocations))]
    {
        let start = Instant::now();
        for _ in 0..300_000 {
            for case in cases { black_box(constructor(black_box(case))); }
        }
        println!("{mode}: {} ms for {} components", start.elapsed().as_millis(), 300_000 * cases.len());
    }
}
