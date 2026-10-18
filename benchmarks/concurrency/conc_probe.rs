//! TEMPORARY research instrumentation (not for commit): per-namespace timing.
use std::cell::Cell;
use std::time::Instant;
thread_local! {
    static JS_DEPTH: Cell<u32> = const { Cell::new(0) };
    static JS_START: Cell<Option<Instant>> = const { Cell::new(None) };
    pub(crate) static JS_NANOS: Cell<u128> = const { Cell::new(0) };
    pub(crate) static JS_CALLS: Cell<u64> = const { Cell::new(0) };
}
pub(crate) fn enabled() -> bool {
    std::env::var_os("LIGHT_NS_PROBE").is_some()
}
pub(crate) fn js_enter() {
    JS_DEPTH.with(|d| {
        if d.get() == 0 {
            JS_START.with(|s| s.set(Some(Instant::now())));
            JS_CALLS.with(|c| c.set(c.get() + 1));
        }
        d.set(d.get() + 1);
    });
}
pub(crate) fn js_exit() {
    JS_DEPTH.with(|d| {
        d.set(d.get() - 1);
        if d.get() == 0
            && let Some(start) = JS_START.with(|s| s.take())
        {
            JS_NANOS.with(|n| n.set(n.get() + start.elapsed().as_nanos()));
        }
    });
}
pub(crate) fn js_nanos() -> u128 {
    JS_NANOS.with(|n| n.get())
}
pub(crate) fn js_calls() -> u64 {
    JS_CALLS.with(|n| n.get())
}
thread_local! {
    pub(crate) static TOP_DRAIN: Cell<bool> = const { Cell::new(false) };
    pub(crate) static UNIT_MICROS: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}
