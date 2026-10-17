//! An engine stack budget is relative to the outer operation's native frame.
//! Clamp it to the owning thread's OS stack bounds, including unwind headroom.
use crate::evaluator::EvalError;

pub(super) struct StackLimit {
    low: usize,
    high: usize,
    bytes: usize,
}

unsafe extern "C" {
    fn light_cljs_v8_set_stack_limit(isolate: v8::UnsafeRawIsolatePtr, boundary: usize);
    fn light_cljs_thread_stack_bounds(low: *mut usize, high: *mut usize) -> bool;
}

impl StackLimit {
    pub(super) fn new(bytes: usize) -> Result<Self, EvalError> {
        let (low, high) = bounds()?;
        let limit = Self { low, high, bytes };
        limit.boundary()?;
        Ok(limit)
    }

    pub(super) fn boundary(&self) -> Result<*mut u32, EvalError> {
        let marker = 0_u32;
        let here = std::hint::black_box(&marker) as *const u32 as usize;
        let floor = self.low.checked_add(64 * 1024).ok_or_else(exhausted)?;
        if !(floor..self.high).contains(&here) {
            return Err(exhausted());
        }
        // A zero configured limit means no additional engine cap, as in
        // QuickJS. The OS stack boundary still cannot safely be crossed.
        let requested = if self.bytes == 0 {
            floor
        } else {
            here.saturating_sub(self.bytes)
        };
        let boundary = requested.max(floor).checked_add(3).ok_or_else(exhausted)? & !3;
        if boundary >= here {
            return Err(exhausted());
        }
        Ok(std::ptr::with_exposed_provenance_mut(boundary))
    }

    pub(super) fn refresh(&self, isolate: &mut v8::Isolate) -> Result<(), EvalError> {
        let boundary = self.boundary()?;
        // SAFETY: the !Send evaluator stays on the thread whose bounds were
        // recorded. boundary checks this operation's frame and leaves native
        // unwind space. V8 compares the address; it never dereferences it.
        unsafe {
            light_cljs_v8_set_stack_limit(isolate.as_raw_isolate_ptr(), boundary as usize);
        }
        Ok(())
    }
}

fn exhausted() -> EvalError {
    EvalError("JavaScript session stack limit exhausted".into())
}

fn bounds() -> Result<(usize, usize), EvalError> {
    let (mut low, mut high) = (0, 0);
    // SAFETY: both outputs point to writable pointer-sized values. The shim
    // queries only the calling thread and rejects unsupported platforms.
    if unsafe { light_cljs_thread_stack_bounds(&mut low, &mut high) } && low < high {
        Ok((low, high))
    } else {
        Err(EvalError(
            "V8 stack bounds are unavailable on this thread".into(),
        ))
    }
}
