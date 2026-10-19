//! Native compiler macro sessions with bounded V8 execution.
use crate::{
    compiler_data::{Datum, ExpansionOrigin, Form},
    compiler_evaluator::{CompilerEvaluator, UnitService},
    evaluator::{
        EvalError, EvaluatorOptions, ExecutionControl, MacroEvaluator, NEXT_SESSION,
        RUNTIME_SOURCE, ValueId,
    },
    form_bridge::ConversionLimits,
    v8_bridge::{self as bridge, BridgeState},
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::{Arc, OnceLock, atomic::Ordering},
    time::{Duration, Instant},
};
use v8::{Local, PinScope};

#[cfg(test)]
#[path = "v8_buffer_tests.rs"]
mod buffer_tests;
#[path = "v8_buffers.rs"]
mod buffers;
#[path = "v8_stack.rs"]
mod stack;
#[path = "v8_watchdog.rs"]
mod watchdog;

struct State {
    buffers: Arc<buffers::Budget>,
    memory_sample: Cell<Instant>,
    namespaces: RefCell<NamespaceCache>,
    control: ExecutionControl,
    generation: u64,
    bridge: RefCell<BridgeState>,
    services: RefCell<Option<v8::Global<v8::Object>>>,
    callbacks: RefCell<Vec<UnitService>>,
    string_services: RefCell<Vec<crate::compiler_evaluator::StringService>>,
    interrupt: Arc<watchdog::Task>,
    pending_error: RefCell<Option<EvalError>>,
    source_bytes: Cell<usize>,
    source_units: Cell<usize>,
}
#[derive(Default)]
struct NamespaceCache {
    entries: HashMap<String, (String, v8::Global<v8::Function>)>,
    order: VecDeque<String>,
    bytes: usize,
    hits: usize,
    misses: usize,
    compiled_bytes: usize,
}
impl NamespaceCache {
    fn insert(&mut self, name: &str, source: &str, function: v8::Global<v8::Function>) {
        const BYTES: usize = 4 * 1024 * 1024;
        if source.len() > BYTES {
            return;
        }
        if let Some((old, _)) = self.entries.remove(name) {
            self.bytes -= old.len();
            self.order.retain(|key| key != name);
        }
        while self.entries.len() >= 1024 || self.bytes + source.len() > BYTES {
            let oldest = self.order.pop_front().unwrap();
            if let Some((old, _)) = self.entries.remove(&oldest) {
                self.bytes -= old.len();
            }
        }
        self.bytes += source.len();
        self.order.push_back(name.to_owned());
        self.entries
            .insert(name.to_owned(), (source.to_owned(), function));
    }
}
impl State {
    fn check_memory(&self, isolate: &mut v8::Isolate, force: bool) {
        // Without backing storage the existing near-heap-limit callback owns
        // enforcement. Heap statistics retire V8's allocation areas, so avoid
        // asking for them on every compiler/bridge call or interrupt.
        if self.buffers.used() == 0
            || (!force && self.memory_sample.get().elapsed() < Duration::from_millis(5))
        {
            return;
        }
        self.memory_sample.set(Instant::now());
        self.buffers
            .observe_heap(isolate.get_heap_statistics().used_heap_size());
        if self.buffers.exhausted() {
            // The first snapshot may include unreachable heap objects and
            // backing stores. Give the collector its normal chance to release
            // them before making the session terminal.
            isolate.low_memory_notification();
            self.buffers
                .observe_heap(isolate.get_heap_statistics().used_heap_size());
            if self.buffers.exhausted() {
                self.control.memory_exhausted();
                isolate.terminate_execution();
            }
        }
    }

    fn remember_error(&self, error: &EvalError) {
        let mut pending = self.pending_error.borrow_mut();
        if pending.is_none()
            || !error
                .0
                .contains("JavaScript session invalidated after engine or callback failure")
        {
            *pending = Some(error.clone());
        }
    }
}
pub(crate) struct V8Evaluator {
    context: Option<v8::Global<v8::Context>>,
    isolate: RefCell<v8::OwnedIsolate>,
    state: Rc<State>,
    stack: stack::StackLimit,
    memory: Box<MemoryGuard>,
}
struct MemoryGuard {
    handle: v8::IsolateHandle,
    control: ExecutionControl,
    buffers: Arc<buffers::Budget>,
    maximum: usize,
    adaptive: bool,
    #[cfg(test)]
    growths: Cell<usize>,
}
struct Entered<'a>(&'a mut v8::OwnedIsolate);
impl<'a> Entered<'a> {
    fn new(isolate: &'a mut v8::OwnedIsolate) -> Self {
        // This synchronous evaluator never moves across threads. Scope this
        // entry to one outer operation; active callbacks use their own scopes.
        unsafe {
            isolate.enter();
        }
        Self(isolate)
    }
}
impl Drop for Entered<'_> {
    fn drop(&mut self) {
        unsafe {
            self.0.exit();
        }
    }
}
extern "C" fn interrupt(mut isolate: v8::UnsafeRawIsolatePtr, _: *mut std::ffi::c_void) {
    // V8 invokes this on the isolate's owning execution thread. No JavaScript
    // is entered. At most one request is queued while Rust is analyzing.
    let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut isolate) };
    let state = isolate.get_slot::<Rc<State>>().unwrap().clone();
    state.interrupt.completed();
    if state.control.poll().is_err() {
        isolate.terminate_execution();
    } else {
        state.check_memory(isolate, false);
    }
}
unsafe extern "C" fn after_gc(
    mut isolate: v8::UnsafeRawIsolatePtr,
    _: v8::GCType,
    _: v8::GCCallbackFlags,
    _: *mut std::ffi::c_void,
) {
    // Runs on the isolate's owning thread, including allocation retries. Only
    // refresh accounting here: a minor collection need not reclaim all garbage.
    let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut isolate) };
    let state = isolate.get_slot::<Rc<State>>().unwrap().clone();
    state
        .buffers
        .observe_heap(isolate.get_heap_statistics().used_heap_size());
}
extern "C" fn near_heap_limit(data: *mut std::ffi::c_void, current: usize, _: usize) -> usize {
    let state = unsafe { &*data.cast::<MemoryGuard>() };
    let available = state.maximum.saturating_sub(state.buffers.used());
    if state.adaptive && current < available {
        // Start with a modest collection threshold, but allow genuinely live
        // heaps to grow toward the configured session budget after V8 has
        // collected. The initial threshold is not an extra hard memory cap.
        let next = current.saturating_add((current / 2).max(16 * 1024 * 1024));
        #[cfg(test)]
        state.growths.set(state.growths.get() + 1);
        return next.min(available);
    }
    state.control.memory_exhausted();
    state.handle.terminate_execution();
    current.saturating_add(16 * 1024 * 1024)
}
fn platform() -> v8::SharedRef<v8::Platform> {
    static PLATFORM: OnceLock<v8::SharedRef<v8::Platform>> = OnceLock::new();
    PLATFORM
        .get_or_init(|| {
            if let Ok(flags) = std::env::var("LIGHT_V8_FLAGS") {
                v8::V8::set_flags_from_string(&flags);
            }
            let platform = v8::new_default_platform(0, false).make_shared();
            v8::V8::initialize_platform(platform.clone());
            v8::cppgc::initialize_process(platform.clone());
            v8::V8::initialize();
            platform
        })
        .clone()
}
fn script<'s>(
    scope: &mut PinScope<'s, '_>,
    source: &str,
) -> Result<Local<'s, v8::Value>, EvalError> {
    let source = bridge::string(scope, source)?;
    v8::Script::compile(scope, source, None)
        .and_then(|script| script.run(scope))
        .ok_or_else(|| bridge::error("V8 script failed"))
}
fn invoke<'s>(
    scope: &mut PinScope<'s, '_>,
    state: &State,
    name: &str,
    args: &[Local<'s, v8::Value>],
) -> Result<Local<'s, v8::Value>, EvalError> {
    let service = name;
    let services = Local::new(scope, state.services.borrow().as_ref().unwrap());
    let name = bridge::string(scope, name)?;
    let function = services
        .get(scope, name.into())
        .ok_or_else(|| bridge::error("V8 service lookup failed"))?;
    let function: Local<v8::Function> = function
        .try_into()
        .map_err(|_| bridge::error("V8 service is not callable"))?;
    let receiver = v8::undefined(scope).into();
    let traced = crate::compile_trace::engine_enter();
    let result = function.call(scope, receiver, args);
    crate::compile_trace::engine_exit(traced, service);
    result.ok_or_else(|| bridge::error("V8 service failed"))
}
impl V8Evaluator {
    pub(crate) fn new(options: EvaluatorOptions) -> Result<Self, EvalError> {
        Self::new_with_startup(options, || {
            include_str!("v8_services.js").replace("__LIGHT_RUNTIME_SOURCE__", RUNTIME_SOURCE)
        })
    }

    fn new_with_startup(
        options: EvaluatorOptions,
        source: impl FnOnce() -> String,
    ) -> Result<Self, EvalError> {
        Self::new_with_initial_heap_limit(options, 256 * 1024 * 1024, source)
    }

    fn new_with_initial_heap_limit(
        options: EvaluatorOptions,
        initial_heap_limit: usize,
        source: impl FnOnce() -> String,
    ) -> Result<Self, EvalError> {
        if options.memory_limit < 1024 * 1024 {
            return Err(bridge::error(
                "JavaScript memory limit must be at least 1048576 bytes for engine startup",
            ));
        }
        crate::generated_names::forget_installed_scopes();
        // Isolate allocation precedes the near-limit callback. Retain the
        // conservative startup minimum until that native startup allocation
        // has a separate failure boundary.
        if options.memory_limit < 16 * 1024 * 1024 {
            return Err(bridge::error(
                "JavaScript session memory limit exhausted during V8 startup",
            ));
        }
        let control = ExecutionControl::new(&options)?;
        control.poll()?;
        let platform = platform();
        let heap = v8::cppgc::Heap::create(
            platform,
            v8::cppgc::HeapCreateParams {
                marking_support: v8::cppgc::MarkingType::Atomic,
                sweeping_support: v8::cppgc::SweepingType::Atomic,
            },
        );
        let heap_limit = initial_heap_limit.clamp(16 * 1024 * 1024, options.memory_limit);
        let stack = stack::StackLimit::new(options.stack_limit)?;
        let buffers = buffers::Budget::new(options.memory_limit, control.clone());
        // SAFETY: the stack helper verifies a boundary within this thread's
        // stack. Rc state prevents moving the evaluator to another thread.
        let params = unsafe {
            v8::CreateParams::default()
                .array_buffer_allocator(buffers.allocator()?)
                .cpp_heap(heap)
                .heap_limits(0, heap_limit)
                .set_stack_limit(stack.boundary()?)
        };
        let mut isolate = v8::Isolate::new(params);
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        let state = Rc::new(State {
            buffers,
            memory_sample: Cell::new(Instant::now()),
            namespaces: RefCell::new(NamespaceCache::default()),
            control: control.clone(),
            generation: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            bridge: RefCell::new(BridgeState::default()),
            services: RefCell::new(None),
            callbacks: RefCell::new(Vec::new()),
            string_services: RefCell::new(Vec::new()),
            interrupt: watchdog::Task::new(isolate.thread_safe_handle())?,
            pending_error: RefCell::new(None),
            source_bytes: Cell::new(0),
            source_units: Cell::new(0),
        });
        isolate.set_slot(state.clone());
        isolate.add_gc_epilogue_callback(after_gc, std::ptr::null_mut(), v8::GCType::kGCTypeAll);
        let memory = Box::new(MemoryGuard {
            handle: isolate.thread_safe_handle(),
            control: control.clone(),
            buffers: state.buffers.clone(),
            maximum: options.memory_limit,
            adaptive: heap_limit < options.memory_limit,
            #[cfg(test)]
            growths: Cell::new(0),
        });
        isolate.add_near_heap_limit_callback(
            near_heap_limit,
            (&*memory as *const MemoryGuard).cast_mut().cast(),
        );
        // OwnedIsolate begins entered. Sessions may outlive or be dropped in
        // any order, so release that constructor entry between operations.
        unsafe {
            isolate.exit();
        }
        let mut evaluator = Self {
            context: None,
            isolate: RefCell::new(isolate),
            state,
            stack,
            memory,
        };
        evaluator.initialize(source)?;
        Ok(evaluator)
    }

    fn initialize(&mut self, source: impl FnOnce() -> String) -> Result<(), EvalError> {
        self.state.control.poll()?;
        let entered = Entered::new(self.isolate.get_mut());
        self.stack.refresh(entered.0)?;
        let result = (|| {
            v8::scope!(let scope, &mut **entered.0);
            let local = v8::Context::new(scope, Default::default());
            // Publish the root immediately so normal Drop handles partial
            // initialization, including a terminated script or failed service.
            self.context = Some(v8::Global::new(scope, local));
            self.state
                .buffers
                .observe_heap(scope.get_heap_statistics().used_heap_size());
            // Context::new's native prelude is not fallible in rusty_v8.
            // Check its elapsed time before enabling interrupts for JS startup.
            self.state.control.poll()?;
            let _execution = self.state.interrupt.enter();
            let scope = &mut v8::ContextScope::new(scope, local);
            v8::tc_scope!(let scope, scope);
            let result = script(scope, &source()).and_then(|_| {
                let services: Local<v8::Object> = script(scope, "__lightEvaluator")?
                    .try_into()
                    .map_err(|_| bridge::error("Invalid evaluator services"))?;
                *self.state.services.borrow_mut() = Some(v8::Global::new(scope, services));
                Ok(())
            });
            result.map_err(|error| {
                scope
                    .exception()
                    .map(|e| EvalError(e.to_rust_string_lossy(scope)))
                    .unwrap_or(error)
            })
        })();
        // Termination has no catchable JS exception. Preserve the owned
        // timeout/fuel/memory diagnostic instead of the generic engine error.
        self.state.check_memory(entered.0, true);
        self.state.control.poll()?;
        result
    }
    fn with_active<T>(
        &self,
        f: impl for<'a, 's, 'i> FnOnce(&Active<'a, 's, 'i>) -> Result<T, EvalError>,
    ) -> Result<T, EvalError> {
        self.state.control.poll()?;
        let mut isolate = self.isolate.borrow_mut();
        let entered = Entered::new(&mut isolate);
        self.stack.refresh(entered.0)?;
        v8::scope!(let scope, &mut **entered.0);
        let context = Local::new(scope, self.context.as_ref().unwrap());
        let scope = &mut v8::ContextScope::new(scope, context);
        f(&Active {
            scope: RefCell::new(scope),
            state: &self.state,
        })
    }
}
impl Drop for V8Evaluator {
    fn drop(&mut self) {
        // Restore the entry OwnedIsolate's destructor consumes. This also
        // permits dropping an old session while another one remains alive.
        unsafe {
            self.isolate.get_mut().enter();
        }
        if std::env::var_os("LIGHT_V8_STATS").is_some() {
            let cache = self.state.namespaces.borrow();
            eprintln!(
                "v8-namespace-cache hits={} misses={} compiled-bytes={} entries={} source-bytes={}",
                cache.hits,
                cache.misses,
                cache.compiled_bytes,
                cache.entries.len(),
                cache.bytes
            );
            eprintln!(
                "v8-source units={} bytes={}",
                self.state.source_units.get(),
                self.state.source_bytes.get()
            );
            for stage in ["before-final-gc", "after-final-gc"] {
                if stage == "after-final-gc" {
                    self.isolate.get_mut().low_memory_notification();
                }
                let stats = self.isolate.get_mut().get_heap_statistics();
                eprintln!(
                    "v8-stats stage={stage} used={} total={} physical={} external={} rust-provenance-estimate={} backing-store-bytes={}",
                    stats.used_heap_size(),
                    stats.total_heap_size(),
                    stats.total_physical_size(),
                    stats.external_memory(),
                    self.state.bridge.borrow().external_estimate(),
                    self.state.buffers.used()
                );
            }
        }
        if let Ok(path) = std::env::var("LIGHT_V8_SNAPSHOT") {
            use std::io::Write;
            let mut output = std::fs::File::create(path).unwrap();
            self.isolate
                .get_mut()
                .take_heap_snapshot(|chunk| output.write_all(chunk).is_ok());
        }
        self.isolate
            .get_mut()
            .remove_near_heap_limit_callback(near_heap_limit, 0);
        self.isolate
            .get_mut()
            .remove_gc_epilogue_callback(after_gc, std::ptr::null_mut());
        self.state.services.borrow_mut().take();
        self.state.namespaces.borrow_mut().entries.clear();
        self.state.bridge.borrow_mut().clear_engine_roots();
        self.context.take();
        // memory callback data stays alive until after removal.
        let _ = &self.memory;
    }
}
struct Active<'a, 's, 'i> {
    scope: RefCell<&'a mut PinScope<'s, 'i>>,
    state: &'a State,
}
impl Active<'_, '_, '_> {
    fn enter<T>(
        &self,
        f: impl for<'s, 'i> FnOnce(&mut PinScope<'s, 'i>) -> Result<T, EvalError>,
    ) -> Result<T, EvalError> {
        self.state.control.poll()?;
        let execution = self.state.interrupt.enter();
        let result = {
            let mut parent = self.scope.borrow_mut();
            v8::scope!(let scope, &mut **parent);
            v8::tc_scope!(let scope, scope);
            self.state.bridge.borrow_mut().sync_external(scope);
            let result = f(scope).and_then(|value| {
                invoke(scope, self.state, "checkBridge", &[])?;
                Ok(value)
            });
            self.state.check_memory(scope, true);
            match result {
                Err(error) if scope.has_terminated() => {
                    Err(self.state.pending_error.borrow().clone().unwrap_or(error))
                }
                Err(error) => Err(scope
                    .exception()
                    .map(|exception| EvalError(exception.to_rust_string_lossy(scope)))
                    .unwrap_or(error)),
                result => result,
            }
        };
        drop(execution);
        if let Err(error) = self.state.control.poll()
            && (result.is_ok() || !error.0.contains("invalidated"))
        {
            return Err(error);
        }
        if let Err(error) = &result {
            self.state.remember_error(error);
            self.state.control.invalidate();
        }
        result
    }
    fn check(&self, id: ValueId) -> Result<u64, EvalError> {
        if id.session == self.state.generation {
            Ok(id.index)
        } else {
            Err(bridge::error(
                "Value ID belongs to another JavaScript session",
            ))
        }
    }
    fn value(&self, index: u64) -> ValueId {
        ValueId {
            session: self.state.generation,
            index,
        }
    }
    fn import(
        &self,
        data: &Datum,
        limits: ConversionLimits,
        service: &str,
    ) -> Result<ValueId, EvalError> {
        let index = self.enter(|scope| {
            let descriptor =
                self.state
                    .bridge
                    .borrow_mut()
                    .encode(scope, data, &self.state.control, limits)?;
            let index = invoke(scope, self.state, service, &[descriptor.into()])?;
            let index = bridge::number(scope, index)? as u64;
            let args = [
                v8::Number::new(scope, index as f64).into(),
                v8::Number::new(scope, limits.nodes as f64).into(),
                v8::Number::new(scope, limits.depth as f64).into(),
            ];
            let snapshot = (|| {
                let snapshot = invoke(scope, self.state, "exportForm", &args)?;
                self.state.bridge.borrow_mut().decode(
                    scope,
                    bridge::array(snapshot)?,
                    &self.state.control,
                    limits,
                    true,
                )?;
                Ok(())
            })();
            if let Err(error) = snapshot {
                let _ = invoke(scope, self.state, "release", &args[..1]);
                return Err(error);
            }
            Ok(index)
        })?;
        Ok(self.value(index))
    }
}
impl MacroEvaluator for Active<'_, '_, '_> {
    fn control(&self) -> ExecutionControl {
        self.state.control.clone()
    }
    fn register_compiler_root(
        &self,
        name: &str,
        unit: Option<usize>,
        once: bool,
        location: &str,
        test: Option<usize>,
    ) -> Result<(), EvalError> {
        self.enter(|scope| {
            if !invoke(scope, self.state, "canCacheRegistration", &[])?.is_true() {
                let source =
                    crate::evaluator::compiler_root_source(name, unit, once, location, test);
                let source = bridge::string(scope, &source)?;
                let id = invoke(scope, self.state, "evaluate", &[source.into()])?;
                invoke(scope, self.state, "release", &[id])?;
                return Ok(());
            }
            let name = bridge::string(scope, name)?.into();
            let unit = unit.map_or_else(
                || v8::null(scope).into(),
                |n| v8::Number::new(scope, n as f64).into(),
            );
            let once = v8::Boolean::new(scope, once).into();
            let location = bridge::string(scope, location)?.into();
            let test = test.map_or_else(
                || v8::null(scope).into(),
                |n| v8::Number::new(scope, n as f64).into(),
            );
            invoke(
                scope,
                self.state,
                "registerRoot",
                &[name, unit, once, location, test],
            )?;
            Ok(())
        })
    }
    fn evaluate_registration(
        &self,
        source: &str,
        chunks: &[(String, std::ops::Range<usize>)],
    ) -> Result<(), EvalError> {
        let mut end = 0;
        let complete = chunks.iter().all(|(_, range)| {
            let contiguous = range.start == end && source.get(range.clone()).is_some();
            end = range.end;
            contiguous
        }) && end == source.len();
        let enabled = complete
            && std::env::var("LIGHT_V8_NAMESPACE_CACHE").as_deref() != Ok("0")
            && self.enter(|scope| {
                Ok(invoke(scope, self.state, "canCacheRegistration", &[])?.is_true())
            })?;
        if !enabled {
            let value = self.evaluate_unit(&format!("rt => {{ {source} return null; }}"))?;
            return self.release(value);
        }
        self.enter(|scope| {
            let functions = v8::Array::new(scope, 0);
            for (index, (name, range)) in chunks.iter().enumerate() {
                self.state.control.poll()?;
                v8::scope!(let scope, scope);
                let source = &source[range.clone()];
                let function = {
                    let cache = self.state.namespaces.borrow();
                    cache
                        .entries
                        .get(name)
                        .filter(|(old, _)| old == source)
                        .map(|(_, function)| Local::new(scope, function))
                };
                let function = if let Some(function) = function {
                    self.state.namespaces.borrow_mut().hits += 1;
                    function
                } else {
                    let function: Local<v8::Function> =
                        script(scope, &format!("rt => {{ {source} return null; }}"))?
                            .try_into()
                            .map_err(|_| {
                                bridge::error("invalid namespace registration function")
                            })?;
                    let mut cache = self.state.namespaces.borrow_mut();
                    cache.misses += 1;
                    cache.compiled_bytes += source.len();
                    cache.insert(name, source, v8::Global::new(scope, function));
                    function
                };
                bridge::put(scope, functions, index as u32, function.into())?;
            }
            invoke(scope, self.state, "registerNamespaces", &[functions.into()])?;
            Ok(())
        })
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.state
            .source_bytes
            .set(self.state.source_bytes.get() + source.len());
        self.state
            .source_units
            .set(self.state.source_units.get() + 1);
        let index = self.enter(|scope| {
            let source = bridge::string(scope, source)?;
            let index = invoke(scope, self.state, "evaluate", &[source.into()])?;
            Ok(bridge::number(scope, index)? as u64)
        })?;
        Ok(self.value(index))
    }
    fn compiler_frontier(&self, next: Option<usize>) -> Result<(), EvalError> {
        self.enter(|scope| {
            let value = next
                .map(|n| v8::Number::new(scope, n as f64).into())
                .unwrap_or_else(|| v8::null(scope).into());
            invoke(scope, self.state, "compilerFrontier", &[value])?;
            Ok(())
        })
    }
    fn heap(&self) -> usize {
        std::ptr::from_ref(self.state) as usize
    }
    fn gensym_scope(&self, serial: u64, prefix: &str, retired: &str) -> Result<(), EvalError> {
        self.enter(|scope| {
            let serial = v8::Number::new(scope, serial as f64).into();
            let prefix = bridge::string(scope, prefix)?.into();
            let retired = bridge::string(scope, retired)?.into();
            invoke(scope, self.state, "gensymScope", &[serial, prefix, retired])?;
            Ok(())
        })
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        let function = self.check(function)?;
        let arguments = arguments
            .iter()
            .map(|id| self.check(*id))
            .collect::<Result<Vec<_>, _>>()?;
        let index = self.enter(|scope| {
            let function = v8::Number::new(scope, function as f64);
            let args: Vec<_> = arguments
                .iter()
                .map(|id| v8::Number::new(scope, *id as f64).into())
                .collect();
            let arguments = v8::Array::new_with_elements(scope, &args);
            let index = invoke(
                scope,
                self.state,
                "call",
                &[function.into(), arguments.into()],
            )?;
            Ok(bridge::number(scope, index)? as u64)
        })?;
        Ok(self.value(index))
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.state.bridge.borrow_mut().retain(form);
        self.import(&form.data(), limits, "importForm")
    }
    fn import_macro_form(
        &self,
        form: &Form,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.state.bridge.borrow_mut().retain(form);
        self.import(&form.data(), limits, "importMacroForm")
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.import(data, limits, "importForm")
    }
    fn import_environment(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        if self.state.bridge.borrow().importing_environment {
            return self.import_data(data, limits);
        }
        self.state.bridge.borrow_mut().importing_environment = true;
        let result = self.enter(|scope| {
            let descriptor = self.state.bridge.borrow_mut().encode_environment(
                scope,
                data,
                &self.state.control,
                limits,
            )?;
            let args = [
                descriptor.into(),
                v8::Number::new(scope, limits.nodes as f64).into(),
                v8::Number::new(scope, limits.depth as f64).into(),
            ];
            let result = invoke(scope, self.state, "importEnvironment", &args)?;
            let result = bridge::array(result)?;
            let index = bridge::get(scope, result, 0)?;
            let index = bridge::number(scope, index)? as u64;
            let snapshot = (|| {
                let snapshots = bridge::get(scope, result, 1)?;
                let snapshots = bridge::array(snapshots)?;
                for i in 0..snapshots.length() {
                    let snapshot = bridge::get(scope, snapshots, i)?;
                    self.state.bridge.borrow_mut().decode(
                        scope,
                        bridge::array(snapshot)?,
                        &self.state.control,
                        limits,
                        true,
                    )?;
                }
                if !bridge::get(scope, result, 2)?.is_true() {
                    self.state.bridge.borrow_mut().reset_environment_children();
                }
                Ok(())
            })();
            if let Err(error) = snapshot {
                let value = v8::Number::new(scope, index as f64);
                let _ = invoke(scope, self.state, "release", &[value.into()]);
                return Err(error);
            }
            Ok(self.value(index))
        });
        self.state.bridge.borrow_mut().importing_environment = false;
        if result.is_err() {
            self.state.bridge.borrow_mut().reset_environment_cache();
        }
        result
    }
    fn macro_arguments(
        &self,
        form: ValueId,
        _: &Form,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        let form = self.check(form)?;
        let indices = self.enter(|scope| {
            let args = [
                v8::Number::new(scope, form as f64).into(),
                v8::Number::new(scope, limits.nodes as f64).into(),
                v8::Number::new(scope, limits.depth as f64).into(),
            ];
            let result = invoke(scope, self.state, "macroArguments", &args)?;
            let result = bridge::array(result)?;
            let indices = bridge::get(scope, result, 0)?;
            let indices = bridge::array(indices)?;
            let mut ids = Vec::new();
            for i in 0..indices.length() {
                let value = bridge::get(scope, indices, i)?;
                ids.push(bridge::number(scope, value)? as u64);
            }
            let snapshots = (|| {
                let snapshots = bridge::get(scope, result, 1)?;
                let snapshots = bridge::array(snapshots)?;
                for i in 0..snapshots.length() {
                    let snapshot = bridge::get(scope, snapshots, i)?;
                    self.state.bridge.borrow_mut().decode(
                        scope,
                        bridge::array(snapshot)?,
                        &self.state.control,
                        limits,
                        true,
                    )?;
                }
                Ok(())
            })();
            if let Err(error) = snapshots {
                for id in ids {
                    let id = v8::Number::new(scope, id as f64);
                    let _ = invoke(scope, self.state, "release", &[id.into()]);
                }
                return Err(error);
            }
            Ok(ids)
        })?;
        Ok(indices.into_iter().map(|id| self.value(id)).collect())
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        let id = self.check(id)?;
        let data = self.enter(|scope| {
            let args = [
                v8::Number::new(scope, id as f64).into(),
                v8::Number::new(scope, limits.nodes as f64).into(),
                v8::Number::new(scope, limits.depth as f64).into(),
            ];
            let descriptor = invoke(scope, self.state, "exportForm", &args)?;
            self.state.bridge.borrow_mut().decode(
                scope,
                bridge::array(descriptor)?,
                &self.state.control,
                limits,
                false,
            )
        })?;
        Ok(Form::from_bridge_expansion(
            data,
            call,
            origin,
            &self.state.bridge.borrow().forms,
        ))
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        let id = self.check(id)?;
        self.enter(|scope| {
            let id = v8::Number::new(scope, id as f64);
            invoke(scope, self.state, "release", &[id.into()])?;
            Ok(())
        })
    }
}
impl MacroEvaluator for V8Evaluator {
    fn control(&self) -> ExecutionControl {
        self.state.control.clone()
    }
    fn register_compiler_root(
        &self,
        name: &str,
        unit: Option<usize>,
        once: bool,
        location: &str,
        test: Option<usize>,
    ) -> Result<(), EvalError> {
        self.with_active(|e| e.register_compiler_root(name, unit, once, location, test))
    }
    fn evaluate_registration(
        &self,
        source: &str,
        chunks: &[(String, std::ops::Range<usize>)],
    ) -> Result<(), EvalError> {
        self.with_active(|e| e.evaluate_registration(source, chunks))
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.with_active(|e| e.evaluate_unit(source))
    }
    fn compiler_frontier(&self, next: Option<usize>) -> Result<(), EvalError> {
        self.with_active(|e| e.compiler_frontier(next))
    }
    fn heap(&self) -> usize {
        Rc::as_ptr(&self.state) as usize
    }
    fn gensym_scope(&self, serial: u64, prefix: &str, retired: &str) -> Result<(), EvalError> {
        self.with_active(|e| e.gensym_scope(serial, prefix, retired))
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        self.with_active(|e| e.call(function, arguments))
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.with_active(|e| e.import_form(form, limits))
    }
    fn import_macro_form(
        &self,
        form: &Form,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.with_active(|e| e.import_macro_form(form, limits))
    }
    fn macro_arguments(
        &self,
        form: ValueId,
        call: &Form,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        self.with_active(|e| e.macro_arguments(form, call, limits))
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.with_active(|e| e.import_data(data, limits))
    }
    fn import_environment(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.with_active(|e| e.import_environment(data, limits))
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        self.with_active(|e| e.export_form(id, call, origin, limits))
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        self.with_active(|e| e.release(id))
    }
}
fn compile_unit(
    scope: &mut PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(state) = scope.get_slot::<Rc<State>>().cloned() else {
        scope.terminate_execution();
        return;
    };
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Do not coerce objects here: valueOf can throw, reenter or never
        // return. Compiler unit IDs are nonnegative, exact Number integers.
        let index = |value| -> Result<u64, String> {
            let number = bridge::number(scope, value)
                .map_err(|_| "invalid compiler unit service ID".to_owned())?;
            if !(0.0..=9_007_199_254_740_991.0).contains(&number) || number.fract() != 0.0 {
                return Err("invalid compiler unit service ID".into());
            }
            Ok(number as u64)
        };
        let callback_index = index(args.data())? as usize;
        let id = index(args.get(0))?;
        let callback = state
            .callbacks
            .borrow()
            .get(callback_index)
            .cloned()
            .ok_or("unknown compiler unit service")?;
        let active = Active {
            scope: RefCell::new(scope),
            state: &state,
        };
        state.control.poll().map_err(|e| e.to_string())?;
        let source = callback(&active, id)?;
        state.control.poll().map_err(|e| e.to_string())?;
        Ok::<_, String>(source)
    }));
    let failure = match output {
        Ok(Ok(source)) => {
            state
                .source_bytes
                .set(state.source_bytes.get() + source.len());
            state.source_units.set(state.source_units.get() + 1);
            match bridge::string(scope, &source) {
                Ok(source) => {
                    result.set(source.into());
                    return;
                }
                Err(error) => error,
            }
        }
        Ok(Err(error)) => EvalError(error),
        Err(_) => bridge::error("compiler unit service panicked"),
    };
    // A failed compiler callback already poisons the session. Keep the owned
    // diagnostic without allocating a JS exception, and terminate immediately
    // so a catch-and-spin cannot conceal it or wait for the watchdog.
    state.remember_error(&failure);
    state.control.invalidate();
    scope.terminate_execution();
}
/// A [`crate::compiler_evaluator::StringService`]: one primitive string in,
/// one string out. A non-string argument is a TypeError, never coerced.
fn string_service(
    scope: &mut PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(state) = scope.get_slot::<Rc<State>>().cloned() else {
        scope.terminate_execution();
        return;
    };
    let request = args.get(0);
    if !request.is_string() {
        let message = v8::String::new(scope, "compiler service expects a string").unwrap();
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
        return;
    }
    let request = request.to_rust_string_lossy(scope);
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let index = bridge::number(scope, args.data())
            .map_err(|_| "invalid compiler string service".to_owned())?
            as usize;
        let callback = state
            .string_services
            .borrow()
            .get(index)
            .cloned()
            .ok_or("unknown compiler string service")?;
        state.control.poll().map_err(|e| e.to_string())?;
        let answer = callback(&request);
        state.control.poll().map_err(|e| e.to_string())?;
        Ok::<_, String>(answer)
    }));
    let failure = match output {
        Ok(Ok(answer)) => match bridge::string(scope, &answer) {
            Ok(answer) => {
                result.set(answer.into());
                return;
            }
            Err(error) => error,
        },
        Ok(Err(error)) => EvalError(error),
        Err(_) => bridge::error("compiler string service panicked"),
    };
    state.remember_error(&failure);
    state.control.invalidate();
    scope.terminate_execution();
}
impl CompilerEvaluator for V8Evaluator {
    fn install_string_service(
        &self,
        name: &str,
        callback: crate::compiler_evaluator::StringService,
    ) -> Result<(), EvalError> {
        let index = self.state.string_services.borrow().len();
        self.state.string_services.borrow_mut().push(callback);
        self.with_active(|e| {
            e.enter(|scope| {
                let data = v8::Number::new(scope, index as f64);
                let callback = v8::Function::builder(string_service)
                    .data(data.into())
                    .build(scope)
                    .ok_or_else(|| bridge::error("V8 callback allocation failed"))?;
                let name = bridge::string(scope, name)?;
                let global = scope.get_current_context().global(scope);
                global
                    .set(scope, name.into(), callback.into())
                    .ok_or_else(|| bridge::error("V8 callback installation failed"))?;
                Ok(())
            })
        })
    }
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        self.evaluate_unit(&format!("(rt) => ({expression}\n)"))
    }
    #[cfg(test)]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        self.with_active(|e| {
            let id = e.check(id)?;
            e.enter(|scope| {
                let id = v8::Number::new(scope, id as f64);
                let value = invoke(scope, e.state, "number", &[id.into()])?;
                bridge::number(scope, value)
            })
        })
    }
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError> {
        let index = self.state.callbacks.borrow().len();
        self.state.callbacks.borrow_mut().push(callback);
        self.with_active(|e| {
            e.enter(|scope| {
                let data = v8::Number::new(scope, index as f64);
                let callback = v8::Function::builder(compile_unit)
                    .data(data.into())
                    .build(scope)
                    .ok_or_else(|| bridge::error("V8 callback allocation failed"))?;
                let name = bridge::string(scope, name)?;
                let global = scope.get_current_context().global(scope);
                global
                    .set(scope, name.into(), callback.into())
                    .ok_or_else(|| bridge::error("V8 callback installation failed"))?;
                Ok(())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn startup_error(options: EvaluatorOptions, source: &str) -> EvalError {
        match V8Evaluator::new_with_startup(options, || source.into()) {
            Ok(_) => panic!("startup must fail"),
            Err(error) => error,
        }
    }

    #[test]
    fn startup_interrupts_catch_and_spin_and_leaves_other_sessions_usable() {
        let other = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let error = startup_error(
            EvaluatorOptions {
                timeout: Duration::from_millis(200),
                ..Default::default()
            },
            "try { for (;;) {} } catch (_) { for (;;) {} }",
        );
        assert!(error.0.contains("deadline exceeded"), "{error}");
        let result = other.evaluate_js("42").unwrap();
        assert_eq!(other.number(result).unwrap(), 42.0);
        other.release(result).unwrap();
        let next = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let result = next.evaluate_js("43").unwrap();
        assert_eq!(next.number(result).unwrap(), 43.0);
        next.release(result).unwrap();
    }

    #[test]
    fn startup_heap_exhaustion_is_owned_even_when_javascript_catches() {
        let error = startup_error(
            EvaluatorOptions {
                memory_limit: 32 * 1024 * 1024,
                ..Default::default()
            },
            "try { const kept = []; for (;;) kept.push(new Array(10000).fill(1)); } catch (_) { for (;;) {} }",
        );
        assert!(error.0.contains("memory limit exhausted"), "{error}");
        let next = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let result = next.evaluate_js("42").unwrap();
        assert_eq!(next.number(result).unwrap(), 42.0);
        next.release(result).unwrap();
    }

    #[test]
    fn startup_rejects_exhausted_control_before_generating_source() {
        for (options, diagnostic) in [
            (
                EvaluatorOptions {
                    timeout: Duration::ZERO,
                    ..Default::default()
                },
                "deadline exceeded",
            ),
            (
                EvaluatorOptions {
                    poll_budget: 0,
                    ..Default::default()
                },
                "poll budget exhausted",
            ),
        ] {
            let result =
                V8Evaluator::new_with_startup(options, || panic!("source must not be evaluated"));
            let Err(error) = result else {
                panic!("exhausted startup must fail")
            };
            assert!(error.0.contains(diagnostic), "{error}");
        }
    }

    #[test]
    fn startup_exceptions_and_invalid_services_clean_up_partial_sessions() {
        let other = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        for (source, diagnostic) in [
            ("throw new Error('startup sentinel')", "startup sentinel"),
            ("const __lightEvaluator = 1", "Invalid evaluator services"),
            ("const __lightEvaluator =", "SyntaxError"),
        ] {
            let error = startup_error(EvaluatorOptions::default(), source);
            assert!(error.0.contains(diagnostic), "{error}");
            let result = other.evaluate_js("42").unwrap();
            assert_eq!(other.number(result).unwrap(), 42.0);
            other.release(result).unwrap();
        }
    }

    fn recursion_depth(stack_limit: usize) -> f64 {
        let evaluator = V8Evaluator::new(EvaluatorOptions {
            stack_limit,
            ..Default::default()
        })
        .unwrap();
        let result = evaluator.evaluate_js("(() => { function descend(n) { try { return descend(n + 1); } catch (e) { if (!(e instanceof RangeError)) throw e; return n; } } return descend(0); })()").unwrap();
        let depth = evaluator.number(result).unwrap();
        evaluator.release(result).unwrap();
        let result = evaluator.evaluate_js("42").unwrap();
        assert_eq!(evaluator.number(result).unwrap(), 42.0);
        evaluator.release(result).unwrap();
        depth
    }

    #[test]
    fn configured_stack_budget_limits_recursion_and_caught_errors_recover() {
        let small = recursion_depth(128 * 1024);
        let large = recursion_depth(512 * 1024);
        assert!(
            small > 0.0 && large > small * 2.0,
            "small={small}, large={large}"
        );
    }

    #[test]
    fn nested_compiler_callbacks_share_the_outer_stack_budget() {
        fn depth(limit: usize) -> usize {
            let evaluator = Rc::new(
                V8Evaluator::new(EvaluatorOptions {
                    stack_limit: limit,
                    ..Default::default()
                })
                .unwrap(),
            );
            let calls = Rc::new(Cell::new(0));
            let count = calls.clone();
            evaluator
                .clone()
                .install_compiler_service(
                    "recursiveUnit",
                    Rc::new(move |active, _| {
                        count.set(count.get() + 1);
                        let value = active
                            .evaluate_unit("rt => recursiveUnit(0)")
                            .map_err(|e| e.to_string())?;
                        active.release(value).map_err(|e| e.to_string())?;
                        Ok("null".into())
                    }),
                )
                .unwrap();
            let error = evaluator.evaluate_js("recursiveUnit(0)").unwrap_err();
            assert!(
                error.0.contains("call stack") || error.0.contains("stack limit"),
                "{error}"
            );
            assert!(evaluator.evaluate_js("1").is_err());
            calls.get()
        }
        let small = depth(128 * 1024);
        let large = depth(512 * 1024);
        assert!(small > 0 && large > small, "small={small}, large={large}");
    }

    #[test]
    fn outer_operations_refresh_stack_limits_after_deeper_rust_entry() {
        let evaluator = V8Evaluator::new(EvaluatorOptions {
            stack_limit: 256 * 1024,
            ..Default::default()
        })
        .unwrap();
        #[inline(never)]
        fn deeper(evaluator: &V8Evaluator, depth: usize) {
            let padding = std::hint::black_box([0_u8; 16 * 1024]);
            if depth == 0 {
                let result = evaluator.evaluate_js("42").unwrap();
                assert_eq!(evaluator.number(result).unwrap(), 42.0);
                evaluator.release(result).unwrap();
            } else {
                deeper(evaluator, depth - 1);
            }
            std::hint::black_box(&padding);
        }
        deeper(&evaluator, 32);
        let result = evaluator.evaluate_js("42").unwrap();
        assert_eq!(evaluator.number(result).unwrap(), 42.0);
        evaluator.release(result).unwrap();
    }

    #[test]
    fn unlimited_and_oversized_stack_budgets_respect_native_thread_bounds() {
        for budget in [0, usize::MAX] {
            std::thread::Builder::new()
                .stack_size(1024 * 1024)
                .spawn(move || {
                    assert!(recursion_depth(budget) > 0.0);
                })
                .unwrap()
                .join()
                .unwrap();
        }
        let error = match V8Evaluator::new(EvaluatorOptions {
            stack_limit: 1,
            ..Default::default()
        }) {
            Ok(_) => panic!("one-byte stack budget must fail"),
            Err(error) => error,
        };
        assert!(error.0.contains("stack limit exhausted"), "{error}");
    }

    #[test]
    fn shared_scheduler_interrupts_concurrent_sessions_and_keeps_failures_terminal() {
        let ready = Arc::new(std::sync::Barrier::new(4));
        let jobs: Vec<_> = (0..4)
            .map(|_| {
                let ready = ready.clone();
                std::thread::spawn(move || {
                    let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
                    ready.wait();
                    evaluator
                        .state
                        .control
                        .begin(&EvaluatorOptions {
                            timeout: Duration::from_millis(100),
                            ..Default::default()
                        })
                        .unwrap();
                    let error = evaluator
                        .evaluate_js("(() => { for (;;) {} })()")
                        .unwrap_err();
                    assert!(error.0.contains("deadline exceeded"), "{error}");
                    assert!(
                        evaluator
                            .evaluate_js("1")
                            .unwrap_err()
                            .0
                            .contains("deadline exceeded")
                    );
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
        let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let result = evaluator.evaluate_js("42").unwrap();
        assert_eq!(evaluator.number(result).unwrap(), 42.0);
        evaluator.release(result).unwrap();
    }

    #[test]
    fn shared_scheduler_does_not_consume_idle_session_fuel() {
        let idle = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        idle.state
            .control
            .begin(&EvaluatorOptions {
                poll_budget: 32,
                ..Default::default()
            })
            .unwrap();
        let active = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        active
            .state
            .control
            .begin(&EvaluatorOptions {
                timeout: Duration::from_millis(100),
                ..Default::default()
            })
            .unwrap();
        assert!(
            active
                .evaluate_js("(() => { for (;;) {} })()")
                .unwrap_err()
                .0
                .contains("deadline exceeded")
        );
        for _ in 0..31 {
            idle.state.control.poll().unwrap();
        }
        assert!(
            idle.state
                .control
                .poll()
                .unwrap_err()
                .0
                .contains("poll budget exhausted")
        );
    }

    #[test]
    fn execution_activity_is_released_when_a_host_operation_unwinds() {
        let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evaluator.with_active::<()>(|active| active.enter(|_| panic!("host operation failed")))
        }));
        assert!(panic.is_err());
        assert!(!evaluator.state.interrupt.running());
        let result = evaluator.evaluate_js("42").unwrap();
        assert_eq!(evaluator.number(result).unwrap(), 42.0);
        evaluator.release(result).unwrap();
    }

    #[test]
    fn direct_root_registration_preserves_values_and_dynamic_callbacks() {
        let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let done = evaluator.evaluate_unit("rt => { globalThis.__lightRegisterUnit = (...args) => { globalThis.registered = args; return 99; }; return null; }").unwrap();
        evaluator.release(done).unwrap();
        evaluator
            .register_compiler_root("a\"\n\\λ", Some(3), false, "line\r\n\"\\x", None)
            .unwrap();
        let check = evaluator.evaluate_js(r#"Number(registered[0] === 'a"\n\\λ' && registered[1] === 3 && registered[2] === false && registered[3] === 'line\r\n"\\x' && registered[4] === null)"#).unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 1.0);
        evaluator.release(check).unwrap();
        evaluator
            .register_compiler_root("next", None, true, "file", Some(7))
            .unwrap();
        let check = evaluator.evaluate_js("Number(registered[0] === 'next' && registered[1] === null && registered[2] === true && registered[4] === 7)").unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 1.0);
        evaluator.release(check).unwrap();
        let done = evaluator.evaluate_unit("rt => { globalThis.savedEval = globalThis.eval; globalThis.evalCalls = 0; globalThis.eval = source => { evalCalls++; return savedEval(source); }; return null; }").unwrap();
        evaluator.release(done).unwrap();
        evaluator
            .register_compiler_root("fallback", Some(8), false, "fallback-location", None)
            .unwrap();
        let check = evaluator.evaluate_unit("rt => { globalThis.eval = savedEval; return Number(evalCalls === 2 && registered[0] === 'fallback' && registered[1] === 8); }").unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 1.0);
        evaluator.release(check).unwrap();
    }

    #[test]
    fn direct_root_registration_failure_is_owned_and_terminal() {
        let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let done = evaluator.evaluate_unit("rt => { globalThis.__lightRegisterUnit = () => { throw new Error('root registration rejected'); }; return null; }").unwrap();
        evaluator.release(done).unwrap();
        let error = evaluator
            .register_compiler_root("failed", None, false, "file", None)
            .unwrap_err();
        assert!(error.0.contains("root registration rejected"), "{error}");
        assert!(evaluator.evaluate_js("1").is_err());
    }

    fn registration(evaluator: &dyn MacroEvaluator, name: &str, source: &str) {
        evaluator
            .evaluate_registration(source, &[(name.into(), 0..source.len())])
            .unwrap();
    }

    #[test]
    fn cached_namespace_registration_reexecutes_and_restores_each_source_view() {
        let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let first = "globalThis.registrations = (globalThis.registrations || 0) + 1; rt.core.registerReaderNamespace('probe', {aliases: [['x', 'first']], bindings: ['one']});";
        let second = first.replace("first", "second").replace("one", "two");
        registration(&evaluator, "probe", first);
        let done = evaluator.evaluate_unit("rt => { const ns = rt.core.theNs(rt.symbol(null, 'probe')); globalThis.previousContext = ns.readerContext; ns.readerContext.aliases.set('x', 'mutated'); ns.readerContext.bindings.add('bad'); return null; }").unwrap();
        evaluator.release(done).unwrap();
        registration(&evaluator, "probe", first);
        let check = evaluator.evaluate_unit("rt => { const c = rt.core.theNs(rt.symbol(null, 'probe')).readerContext; return Number(c !== previousContext && c.aliases.get('x') === 'first' && !c.bindings.has('bad')); }").unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 1.0);
        evaluator.release(check).unwrap();
        assert_eq!(evaluator.state.namespaces.borrow().hits, 1);
        registration(&evaluator, "probe", &second);
        registration(&evaluator, "probe", first);
        let check = evaluator.evaluate_unit("rt => { const c = rt.core.theNs(rt.symbol(null, 'probe')).readerContext; return Number(registrations === 4 && c.aliases.get('x') === 'first' && c.bindings.has('one') && !c.bindings.has('two')); }").unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 1.0);
        evaluator.release(check).unwrap();
    }

    #[test]
    fn namespace_cache_eviction_keeps_execution_and_observes_eval_replacement() {
        let evaluator = V8Evaluator::new(EvaluatorOptions::default()).unwrap();
        let source = "globalThis.registrations = (globalThis.registrations || 0) + 1;";
        for index in 0..1025 {
            registration(&evaluator, &format!("ns{index}"), source);
        }
        assert_eq!(evaluator.state.namespaces.borrow().entries.len(), 1024);
        registration(&evaluator, "ns0", source);
        let check = evaluator.evaluate_js("registrations").unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 1026.0);
        evaluator.release(check).unwrap();
        let done = evaluator.evaluate_unit("rt => { globalThis.savedEval = globalThis.eval; globalThis.evalCalls = 0; globalThis.eval = source => { evalCalls++; return savedEval(source); }; return null; }").unwrap();
        evaluator.release(done).unwrap();
        registration(&evaluator, "ns0", source);
        let check = evaluator
            .evaluate_unit("rt => { globalThis.eval = savedEval; return evalCalls; }")
            .unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 2.0);
        evaluator.release(check).unwrap();
    }

    #[test]
    fn namespace_registration_allows_reentry_and_stops_at_first_exception() {
        let evaluator = Rc::new(V8Evaluator::new(EvaluatorOptions::default()).unwrap());
        evaluator
            .clone()
            .install_compiler_service(
                "nestedRegistration",
                Rc::new(|active, _| {
                    let source = "globalThis.trace.push(2);";
                    active
                        .evaluate_registration(source, &[("same".into(), 0..source.len())])
                        .map_err(|e| e.to_string())?;
                    Ok("null".into())
                }),
            )
            .unwrap();
        registration(
            evaluator.as_ref(),
            "same",
            "globalThis.trace = [1]; nestedRegistration(0); globalThis.trace.push(3);",
        );
        let check = evaluator
            .evaluate_js("Number(trace.join(',') === '1,2,3')")
            .unwrap();
        assert_eq!(evaluator.number(check).unwrap(), 1.0);
        evaluator.release(check).unwrap();
        let observed = Rc::new(Cell::new(0));
        let record = observed.clone();
        evaluator
            .clone()
            .install_compiler_service(
                "recordRegistration",
                Rc::new(move |_, id| {
                    record.set(id);
                    Ok("null".into())
                }),
            )
            .unwrap();
        let chunks = [
            "recordRegistration(1);",
            "throw new Error('registration stopped');",
            "recordRegistration(3);",
        ];
        let source = chunks.concat();
        let mut end = 0;
        let ranges = chunks
            .iter()
            .enumerate()
            .map(|(i, part)| {
                let start = end;
                end += part.len();
                (format!("chunk{i}"), start..end)
            })
            .collect::<Vec<_>>();
        let error = evaluator
            .evaluate_registration(&source, &ranges)
            .unwrap_err();
        assert!(error.0.contains("registration stopped"), "{error}");
        assert_eq!(observed.get(), 1);
        assert!(evaluator.evaluate_js("1").is_err());
    }

    #[test]
    fn compiler_callback_rejects_invalid_ids_without_running_coercion_hooks() {
        for argument in [
            "Symbol('bad')",
            "NaN",
            "Infinity",
            "-1",
            "1.5",
            "9007199254740992",
            "({ valueOf() { for (;;) {} } })",
        ] {
            let evaluator = Rc::new(V8Evaluator::new(EvaluatorOptions::default()).unwrap());
            evaluator
                .clone()
                .install_compiler_service("unitService", Rc::new(|_, id| Ok(id.to_string())))
                .unwrap();
            let valid = evaluator.evaluate_js("Number(unitService(7))").unwrap();
            assert_eq!(evaluator.number(valid).unwrap(), 7.0);
            evaluator.release(valid).unwrap();
            let error = evaluator
                .evaluate_js(&format!("unitService({argument})"))
                .unwrap_err();
            assert!(
                error.0.contains("invalid compiler unit service ID"),
                "{argument}: {error}"
            );
            assert!(evaluator.evaluate_js("1").is_err());
        }
    }

    #[test]
    fn compiler_callback_panic_is_owned_and_terminal() {
        let evaluator = Rc::new(V8Evaluator::new(EvaluatorOptions::default()).unwrap());
        evaluator
            .clone()
            .install_compiler_service("panicUnitService", Rc::new(|_, _| panic!("service failed")))
            .unwrap();
        let error = evaluator
            .evaluate_js("(() => { try { panicUnitService(0); } catch (_) { for (;;) {} } })()")
            .unwrap_err();
        assert!(
            error.0.contains("compiler unit service panicked"),
            "{error}"
        );
        assert!(evaluator.evaluate_js("1").is_err());
    }

    #[test]
    fn poisoned_callback_keeps_owned_diagnostic_when_javascript_catches_and_spins() {
        let evaluator = Rc::new(
            V8Evaluator::new(EvaluatorOptions {
                timeout: Duration::from_secs(2),
                ..Default::default()
            })
            .unwrap(),
        );
        evaluator
            .clone()
            .install_compiler_service(
                "failCompileUnit",
                Rc::new(|_, _| Err("specific owned compiler diagnostic".into())),
            )
            .unwrap();
        let function = evaluator
            .evaluate_unit("rt => () => { try { failCompileUnit(0); } catch (_) { for (;;) {} } }")
            .unwrap();
        let error = evaluator.call(function, &[]).unwrap_err();
        assert!(
            error.0.contains("specific owned compiler diagnostic"),
            "{error}"
        );
        assert!(evaluator.evaluate_js("1").is_err());
    }
}
