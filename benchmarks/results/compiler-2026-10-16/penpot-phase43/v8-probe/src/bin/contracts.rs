use std::{
    cell::RefCell,
    ffi::c_void,
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    time::{Duration, Instant},
};

fn eval<'s>(scope: &mut v8::PinScope<'s, '_>, text: &str) -> Option<v8::Local<'s, v8::Value>> {
    let text = v8::String::new(scope, text)?;
    v8::Script::compile(scope, text, None)?.run(scope)
}

struct Counters {
    created: AtomicUsize,
    dropped: AtomicUsize,
    wrong_thread: AtomicUsize,
    owner: std::thread::ThreadId,
}

struct Provenance {
    counters: Arc<Counters>,
    payload: RefCell<Vec<u16>>,
}

// This probe deliberately uses atomic marking/sweeping, since compiler Datum
// and Form contain Rc rather than Arc. No engine handles live in this payload.
unsafe impl v8::cppgc::GarbageCollected for Provenance {
    fn trace(&self, _: &mut v8::cppgc::Visitor) {}
    fn get_name(&self) -> &'static std::ffi::CStr { c"CompilerProvenanceProbe" }
}
impl Drop for Provenance {
    fn drop(&mut self) {
        if std::thread::current().id() != self.counters.owner {
            self.counters.wrong_thread.fetch_add(1, Ordering::SeqCst);
        }
        self.counters.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
const TAG: u16 = 1;

fn empty(_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {}

fn make_provenance(scope: &mut v8::PinScope<'_, '_>, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let string = v8::Local::<v8::String>::try_from(args.get(0)).unwrap();
    let mut payload = vec![0; string.length()];
    string.write_v2(scope, 0, &mut payload, v8::WriteFlags::empty());
    let counters = scope.get_slot::<Arc<Counters>>().unwrap().clone();
    counters.created.fetch_add(1, Ordering::SeqCst);
    let constructor = v8::FunctionTemplate::new(scope, empty).get_function(scope).unwrap();
    let wrapper = constructor.new_instance(scope, &[]).unwrap();
    let value = unsafe {
        v8::cppgc::make_garbage_collected(scope.get_cpp_heap().unwrap(), Provenance { counters, payload: RefCell::new(payload) })
    };
    unsafe { v8::Object::wrap::<TAG, Provenance>(scope, wrapper, &value); }
    rv.set(wrapper.into());
}

fn read_provenance(scope: &mut v8::PinScope<'_, '_>, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let wrapper = v8::Local::<v8::Object>::try_from(args.get(0)).unwrap();
    let value = unsafe { v8::Object::unwrap::<TAG, Provenance>(scope, wrapper).unwrap() };
    // The live wrapper roots the payload, and its borrow ends before reentry.
    let payload = unsafe { value.as_ref() }.payload.borrow();
    rv.set(v8::String::new_from_two_byte(scope, &payload, v8::NewStringType::Normal).unwrap().into());
}

fn compile_unit(scope: &mut v8::PinScope<'_, '_>, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = args.get(0).integer_value(scope).unwrap();
    if id > 0 {
        let source = format!("globalThis.callbacks.push({id}); nestedCompiler({})", id - 1);
        let value = eval(scope, &source).unwrap();
        assert_eq!(value.integer_value(scope), Some(42));
    }
    let source = v8::String::new(scope, "() => 42").unwrap();
    rv.set(source.into());
}

fn exercise_bridge(platform: v8::SharedRef<v8::Platform>, runtime: &str) {
    let counters = Arc::new(Counters {
        created: AtomicUsize::new(0), dropped: AtomicUsize::new(0),
        wrong_thread: AtomicUsize::new(0), owner: std::thread::current().id(),
    });
    {
        let heap = v8::cppgc::Heap::create(platform, v8::cppgc::HeapCreateParams {
            marking_support: v8::cppgc::MarkingType::Atomic,
            sweeping_support: v8::cppgc::SweepingType::Atomic,
        });
        let isolate = &mut v8::Isolate::new(v8::CreateParams::default().cpp_heap(heap));
        isolate.set_slot(counters.clone());
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        let context;
        {
            v8::scope!(let scope, isolate);
            let local = v8::Context::new(scope, Default::default());
            context = v8::Global::new(scope, local);
            let scope = &mut v8::ContextScope::new(scope, local);
            let global = local.global(scope);
            for (name, function) in [
                ("makeProvenance", v8::Function::new(scope, make_provenance).unwrap()),
                ("readProvenance", v8::Function::new(scope, read_provenance).unwrap()),
                ("compileUnit", v8::Function::new(scope, compile_unit).unwrap()),
            ] {
                let name = v8::String::new(scope, name).unwrap();
                global.set(scope, name.into(), function.into()).unwrap();
            }
            let source = format!("globalThis.rt = {runtime};");
            eval(scope, &source).unwrap();
            let result = eval(scope, r#"
              globalThis.check = (ok, message) => { if (!ok) throw Error(message); };
              (() => {
                const text = '\ud800\u0000\udfff\ud83d\ude00';
                const p = makeProvenance(text);
                check(readProvenance(p) === text, 'native UTF-16');
                const d = ['vector', p, null, [['number', null, null, -0], ['number', null, null, NaN]]];
                const value = rt.compilerForms.decode(d);
                const snapshot = rt.compilerForms.encode(value, 10000, 128);
                check(snapshot[1] === p, 'opaque provenance identity');
                check(Object.is(snapshot[3][0][3], -0), 'negative zero');
                check(Number.isNaN(snapshot[3][1][3]), 'NaN');
                globalThis.retained = value;
              })();
              globalThis.callbacks = [];
              globalThis.nestedCompiler = id => (0, eval)(compileUnit(id))();
              check(nestedCompiler(3) === 42 && callbacks.join(',') === '3,2,1', 'nested Rust/JS callback');
              globalThis.job = false;
              Promise.resolve().then(() => { job = true; });
              check(!job, 'microtasks run only at explicit boundary');
              'bridge and callbacks passed'
            "#).unwrap();
            println!("{}", result.to_rust_string_lossy(scope));
        }
        isolate.low_memory_notification();
        assert_eq!(counters.dropped.load(Ordering::SeqCst), 0, "retained macro input must retain provenance");
        {
            v8::scope!(let scope, isolate);
            let local = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, local);
            eval(scope, "check(!job, 'implicit job checkpoint'); check(readProvenance(rt.compilerForms.encode(retained, 10000, 128)[1]) === '\\ud800\\u0000\\udfff\\ud83d\\ude00', 'retained provenance payload'); retained = null;").unwrap();
            scope.perform_microtask_checkpoint();
            eval(scope, "check(job, 'explicit job checkpoint');").unwrap();
        }
        isolate.low_memory_notification();
        assert_eq!(counters.dropped.load(Ordering::SeqCst), 1, "unreachable macro input must release Rust payload");
        {
            v8::scope!(let scope, isolate);
            let local = v8::Local::new(scope, &context);
            let scope = &mut v8::ContextScope::new(scope, local);
            eval(scope, "globalThis.retainedAtDrop = makeProvenance('drop');").unwrap();
        }
        drop(context);
    }
    assert_eq!(counters.created.load(Ordering::SeqCst), 2);
    assert_eq!(counters.dropped.load(Ordering::SeqCst), 2);
    assert_eq!(counters.wrong_thread.load(Ordering::SeqCst), 0);
    println!("provenance released by GC and isolate drop, on the owning thread");
}

fn exercise_termination() {
    let isolate = &mut v8::Isolate::new(Default::default());
    let handle = isolate.thread_safe_handle();
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if wait.recv_timeout(Duration::from_millis(50)).is_err() {
            handle.terminate_execution();
        }
    });
    let start = Instant::now();
    {
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        assert!(eval(scope, "try { for (;;) {} } catch (_) { globalThis.caught = true; }").is_none());
        assert!(scope.has_terminated());
    }
    let _ = done.send(());
    watchdog.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
    println!("uncatchable external termination: {} ms", start.elapsed().as_millis());
}

struct HeapLimit { handle: v8::IsolateHandle, calls: AtomicUsize }
extern "C" fn heap_limit(data: *mut c_void, current: usize, _: usize) -> usize {
    let state = unsafe { &*(data.cast::<HeapLimit>()) };
    state.calls.fetch_add(1, Ordering::SeqCst);
    state.handle.terminate_execution();
    // Emergency headroom lets V8 unwind instead of invoking fatal OOM.
    current + 16 * 1024 * 1024
}
fn exercise_heap_limit() {
    let isolate = &mut v8::Isolate::new(v8::CreateParams::default().heap_limits(0, 16 * 1024 * 1024));
    let state = Box::new(HeapLimit { handle: isolate.thread_safe_handle(), calls: AtomicUsize::new(0) });
    let ptr = (&*state as *const HeapLimit).cast_mut().cast();
    isolate.add_near_heap_limit_callback(heap_limit, ptr);
    {
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        assert!(eval(scope, "const retained = []; try { for (;;) retained.push(new Array(10000).fill(42)); } catch (_) { globalThis.caught = true; }").is_none());
        assert!(scope.has_terminated());
    }
    isolate.remove_near_heap_limit_callback(heap_limit, 0);
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    println!("heap callback terminated allocation loop without process OOM");
}

struct BufferBudget {
    used: AtomicUsize,
    refused: AtomicUsize,
    limit: usize,
}
unsafe extern "C" fn allocate_buffer(state: &Arc<BufferBudget>, len: usize) -> *mut c_void {
    let bytes = len.max(1);
    if state.used.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
        n.checked_add(bytes).filter(|next| *next <= state.limit)
    }).is_err() {
        state.refused.fetch_add(1, Ordering::SeqCst);
        return std::ptr::null_mut();
    }
    let layout = std::alloc::Layout::from_size_align(bytes, 8).unwrap();
    let pointer = unsafe { std::alloc::alloc_zeroed(layout) };
    if pointer.is_null() {
        state.used.fetch_sub(bytes, Ordering::SeqCst);
        state.refused.fetch_add(1, Ordering::SeqCst);
    }
    pointer.cast()
}
unsafe extern "C" fn free_buffer(state: &Arc<BufferBudget>, pointer: *mut c_void, len: usize) {
    let bytes = len.max(1);
    let layout = std::alloc::Layout::from_size_align(bytes, 8).unwrap();
    unsafe { std::alloc::dealloc(pointer.cast(), layout); }
    state.used.fetch_sub(bytes, Ordering::SeqCst);
}
unsafe extern "C" fn drop_allocator(state: *const Arc<BufferBudget>) {
    drop(unsafe { Box::from_raw(state.cast_mut()) });
}
static ALLOCATOR: v8::RustAllocatorVtable<Arc<BufferBudget>> = v8::RustAllocatorVtable {
    allocate: allocate_buffer, allocate_uninitialized: allocate_buffer,
    free: free_buffer, drop: drop_allocator,
};
fn exercise_buffers() {
    let state = Arc::new(BufferBudget {
        used: AtomicUsize::new(0), refused: AtomicUsize::new(0), limit: 1024 * 1024,
    });
    {
        let allocator = unsafe { v8::new_rust_allocator(Box::into_raw(Box::new(state.clone())), &ALLOCATOR) };
        let isolate = &mut v8::Isolate::new(v8::CreateParams::default().array_buffer_allocator(allocator));
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let result = eval(scope, r#"
          globalThis.keep = new Uint8Array(512 * 1024);
          keep[0] = 13;
          let failed = false;
          try { globalThis.tooLarge = new Uint8Array(2 * 1024 * 1024); } catch (_) { failed = true; }
          failed && keep[0] === 13
        "#).unwrap();
        assert!(result.is_true());
        assert_eq!(state.used.load(Ordering::SeqCst), 512 * 1024);
        assert!(state.refused.load(Ordering::SeqCst) > 0);
    }
    assert_eq!(state.used.load(Ordering::SeqCst), 0);
    assert_eq!(Arc::strong_count(&state), 1);
    println!("ArrayBuffer budget refusal survives JS catch, all backing stores freed at drop");
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform.clone());
    v8::cppgc::initialize_process(platform.clone());
    v8::V8::initialize();
    if args[1] == "heap" {
        exercise_heap_limit();
    } else if args[1] == "buffers" {
        exercise_buffers();
    } else {
        exercise_bridge(platform, &std::fs::read_to_string(&args[1]).unwrap());
        exercise_termination();
    }
    unsafe { v8::cppgc::shutdown_process(); v8::V8::dispose(); }
    v8::V8::dispose_platform();
}
