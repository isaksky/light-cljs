//! Synchronous JavaScript execution. Engine values never cross this boundary.
//!
//! A session owns one heap and canonical runtime. Reusable hosts create fresh
//! sessions; interactive consumers may deliberately retain a session instead.
use crate::compiler_data::{Datum, Form};
use crate::form_bridge::{BridgeState, ConversionLimits, FORM_BRIDGE_VERSION};
use rquickjs::allocator::{Allocator, RustAllocator};
use rquickjs::{Array, CatchResultExt, Context, Ctx, Function, Runtime};
use std::{cell::RefCell, rc::Rc};
use std::{
    fmt,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const RUNTIME_SOURCE: &str = include_str!("js_runtime.js");
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct EvaluatorOptions {
    pub memory_limit: usize,
    pub stack_limit: usize,
    /// Shared poll budget, including engine interrupts and Rust bridge work.
    pub poll_budget: u64,
    pub timeout: Duration,
}
impl Default for EvaluatorOptions {
    fn default() -> Self {
        Self {
            memory_limit: 64 * 1024 * 1024,
            stack_limit: 1024 * 1024,
            poll_budget: 1_000_000,
            timeout: Duration::from_secs(10),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalError(pub String);
impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EvalError {}

struct ControlState {
    cancelled: AtomicBool,
    terminal: AtomicU8,
    remaining: AtomicU64,
    deadline: Instant,
}
/// Clones share sticky cancellation, deadline and fuel across nested Rust work.
#[derive(Clone)]
pub struct ExecutionControl(Arc<ControlState>);
impl ExecutionControl {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }
    #[allow(deprecated)] // fetch_update supports the stable compiler used in CI.
    pub fn poll(&self) -> Result<(), EvalError> {
        let cause = if self.0.cancelled.load(Ordering::Relaxed) {
            1
        } else if Instant::now() >= self.0.deadline {
            2
        } else if self
            .0
            .remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_err()
        {
            3
        } else {
            0
        };
        if cause != 0 {
            let _ =
                self.0
                    .terminal
                    .compare_exchange(0, cause, Ordering::Relaxed, Ordering::Relaxed);
        }
        match self.0.terminal.load(Ordering::Relaxed) {
            0 => Ok(()),
            1 => Err(EvalError("JavaScript session cancelled".into())),
            2 => Err(EvalError("JavaScript session deadline exceeded".into())),
            3 => Err(EvalError("JavaScript session poll budget exhausted".into())),
            5 => Err(EvalError(
                "JavaScript session memory limit exhausted".into(),
            )),
            _ => Err(EvalError(
                "JavaScript session invalidated after engine or callback failure".into(),
            )),
        }
    }
    pub(crate) fn invalidate(&self) {
        let _ = self
            .0
            .terminal
            .compare_exchange(0, 4, Ordering::Relaxed, Ordering::Relaxed);
    }
}

/// Opaque identity, valid only in its originating session, until release/drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueId {
    session: u64,
    index: u64,
}

pub struct Evaluator {
    // Drop context roots before runtime. No Persistent or Rust-owned JS edges.
    context: Option<Context>,
    runtime: Runtime,
    control: ExecutionControl,
    generation: u64,
    bridge: Rc<RefCell<BridgeState>>,
}
impl Evaluator {
    pub fn new(options: EvaluatorOptions) -> Result<Self, EvalError> {
        let control = ExecutionControl(Arc::new(ControlState {
            cancelled: AtomicBool::new(false),
            terminal: AtomicU8::new(0),
            remaining: AtomicU64::new(options.poll_budget),
            deadline: Instant::now()
                .checked_add(options.timeout)
                .ok_or_else(|| EvalError("invalid session timeout".into()))?,
        }));
        let runtime = Runtime::new_with_alloc(LimitedAllocator {
            used: 0,
            limit: options.memory_limit,
            control: Arc::downgrade(&control.0),
        })
        .map_err(|e| EvalError(e.to_string()))?;
        runtime.set_max_stack_size(options.stack_limit);
        let weak = Arc::downgrade(&control.0);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            weak.upgrade()
                .is_none_or(|state| ExecutionControl(state).poll().is_err())
        })));
        let context = Context::full(&runtime).map_err(|e| EvalError(e.to_string()))?;
        let session = Self {
            context: Some(context),
            runtime,
            control,
            generation: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            bridge: Rc::new(RefCell::new(BridgeState::default())),
        };
        session.enter(|ctx| ctx.eval::<(), _>(format!(r#"
            const __lightEvaluator = (() => {{
              const rt = {RUNTIME_SOURCE};
              const values = new Map(); let next = 1;
              const bridge = rt.compilerForms;
              if (bridge.version !== {FORM_BRIDGE_VERSION}) throw new Error("Compiler form bridge version mismatch");
              const clearValues = Map.prototype.clear.bind(values);
              const save = value => {{ const id = next++; values.set(id, value); return id; }};
              const get = id => {{ if (!values.has(id)) throw new Error('Released or unknown value ID'); return values.get(id); }};
              return Object.freeze({{
                importForm: descriptor => save(bridge.decode(descriptor)),
                exportForm: (id, nodes, depth) => bridge.encode(get(id), nodes, depth),
                formServices: (one, all, nodes, depth) => rt.setMacroexpandServices({{macroexpand1: form => bridge.decode(one(bridge.encode(form, nodes, depth))), macroexpand: form => bridge.decode(all(bridge.encode(form, nodes, depth)))}}),
                bridgeVersion: bridge.version,
                checkBridge: bridge.check,
                evaluate: source => save((0, eval)(source)(rt)),
                call: (id, ids) => save(Reflect.apply(get(id), null, ids.map(get))),
                number: id => {{ const v = get(id); if (typeof v !== 'number') throw new TypeError('Expected Number'); return v; }},
                string: id => {{ const v = get(id); if (typeof v !== 'string') throw new TypeError('Expected string'); return v; }},
                print: id => rt.core.prStr(get(id)),
                release: id => {{ get(id); values.delete(id); }},
                clear: clearValues
              }});
            }})();
        "#)))?;
        Ok(session)
    }
    pub fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn enter<T>(
        &self,
        f: impl for<'js> FnOnce(Ctx<'js>) -> rquickjs::Result<T>,
    ) -> Result<T, EvalError> {
        self.control.poll()?;
        let result = self.context.as_ref().unwrap().with(|ctx| {
            let operation = f(ctx.clone());
            let operation = operation.and_then(|value| {
                ctx.eval::<(), _>("__lightEvaluator.checkBridge()")?;
                Ok(value)
            });
            operation
                .catch(&ctx)
                .map_err(|error| EvalError(error.to_string()))
        });
        self.control.poll()?;
        // Engine failures poison this operation. Never retry through a native path.
        if result.is_err() {
            self.control.invalidate();
        }
        result
    }
    fn check(&self, id: ValueId) -> Result<u64, EvalError> {
        if id.session == self.generation {
            Ok(id.index)
        } else {
            Err(EvalError(
                "Value ID belongs to another JavaScript session".into(),
            ))
        }
    }
    /// Execute a compiler unit produced by emit_js_unit against this heap's runtime.
    pub fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        let index = self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.evaluate")?;
            f.call((source,))
        })?;
        Ok(ValueId {
            session: self.generation,
            index,
        })
    }
    /// Explicit JavaScript capability; `rt` is the canonical runtime instance.
    pub fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        self.evaluate_unit(&format!("(rt) => ({expression}\n)"))
    }
    pub fn evaluate(&self, program: &crate::Program) -> Result<ValueId, EvalError> {
        self.control.poll()?;
        let source = crate::emitter::emit_js_unit(program);
        self.control.poll()?;
        self.evaluate_unit(&source)
    }
    pub fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        let function = self.check(function)?;
        let arguments = arguments
            .iter()
            .map(|id| self.check(*id))
            .collect::<Result<Vec<_>, _>>()?;
        let index = self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.call")?;
            f.call((function, arguments))
        })?;
        Ok(ValueId {
            session: self.generation,
            index,
        })
    }
    pub fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        let id = self.check(id)?;
        self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.number")?;
            f.call((id,))
        })
    }
    pub fn string(&self, id: ValueId) -> Result<String, EvalError> {
        let id = self.check(id)?;
        self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.string")?;
            f.call((id,))
        })
    }
    pub fn print(&self, id: ValueId) -> Result<String, EvalError> {
        let id = self.check(id)?;
        self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.print")?;
            f.call((id,))
        })
    }
    pub fn release(&self, id: ValueId) -> Result<(), EvalError> {
        let id = self.check(id)?;
        self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.release")?;
            f.call((id,))
        })
    }
    /// Convert owned compiler forms directly into canonical values in this heap.
    pub fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.bridge.borrow_mut().retain(form);
        self.import_data(&form.data(), limits)
    }
    pub fn import_data(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        let index = self.enter(|ctx| {
            let descriptor = self
                .bridge
                .borrow_mut()
                .encode(&ctx, data, &self.control, limits)?;
            let f: Function = ctx.eval("__lightEvaluator.importForm")?;
            let index: u64 = f.call((descriptor,))?;
            // Snapshot canonical payloads so mutable host literals are never reused
            // after modification. Reader lexical distinctions remain in originals.
            let export: Function = ctx.eval("__lightEvaluator.exportForm")?;
            let snapshot: Array = export.call((index, limits.nodes, limits.depth))?;
            self.bridge
                .borrow_mut()
                .decode(snapshot, &self.control, limits, true)?;
            Ok(index)
        })?;
        Ok(ValueId {
            session: self.generation,
            index,
        })
    }
    pub fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError> {
        let id = self.check(id)?;
        self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.exportForm")?;
            let descriptor: Array = f.call((id, limits.nodes, limits.depth))?;
            self.bridge
                .borrow_mut()
                .decode(descriptor, &self.control, limits, false)
        })
    }
    pub fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: crate::compiler_data::ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        let data = self.export_data(id, limits)?;
        Ok(Form::from_bridge_expansion(
            data,
            call,
            origin,
            &self.bridge.borrow().forms,
        ))
    }
    /// Install the compiler's binding-resolution/expansion service. Both nested
    /// entry points share this service and the session's conversion/engine fuel.
    /// Brief table borrows end before calling compiler code or nested JavaScript.
    pub fn install_form_services(
        &self,
        call: Form,
        origin: crate::compiler_data::ExpansionOrigin,
        limits: ConversionLimits,
        service: impl Fn(ExecutionControl, Form, bool) -> Result<Form, EvalError> + 'static,
    ) -> Result<(), EvalError> {
        let service = Rc::new(service);
        self.enter(|ctx| {
            let create = |all: bool| {
                let service = service.clone();
                let call = call.clone();
                let origin = origin.clone();
                let bridge = self.bridge.clone();
                let control = self.control.clone();
                Function::new(
                    ctx.clone(),
                    move |descriptor: Array<'_>| -> rquickjs::Result<BridgeOutput> {
                        let ctx = descriptor.ctx().clone();
                        let data = bridge
                            .borrow_mut()
                            .decode(descriptor, &control, limits, false)?;
                        let form = Form::from_bridge_expansion(
                            data,
                            &call,
                            origin.clone(),
                            &bridge.borrow().forms,
                        );
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            service(control.clone(), form, all)
                        }));
                        let output = match result {
                            Ok(Ok(form)) => form,
                            Ok(Err(e)) => {
                                return Err(rquickjs::Exception::throw_message(&ctx, &e.0));
                            }
                            Err(_) => {
                                control.invalidate();
                                return Err(rquickjs::Exception::throw_message(
                                    &ctx,
                                    "compiler macro service panicked",
                                ));
                            }
                        };
                        control
                            .poll()
                            .map_err(|e| rquickjs::Exception::throw_message(&ctx, &e.0))?;
                        Ok(BridgeOutput {
                            data: output,
                            bridge: bridge.clone(),
                            control: control.clone(),
                            limits,
                        })
                    },
                )
            };
            let one = create(false)?;
            let all = create(true)?;
            let install: Function = ctx.eval("__lightEvaluator.formServices")?;
            install.call((one, all, limits.nodes, limits.depth))
        })
    }
    /// Force collection at an outer boundary. Callbacks must use the active Ctx,
    /// never this runtime-locking method.
    pub fn collect_garbage(&self) {
        self.runtime.run_gc();
    }

    /// Explicit embedding hook for collection while JavaScript is active. It
    /// captures no engine edges and uses Ctx rather than locking the Runtime.
    pub fn install_gc_callback(&self, name: &str) -> Result<(), EvalError> {
        self.enter(|ctx| {
            let collect = Function::new(ctx.clone(), |ctx: Ctx<'_>| ctx.run_gc())?;
            ctx.globals().set(name, collect)
        })
    }

    /// Register an explicit string callback. Its closure must retain only Rust data.
    /// Poll the supplied shared control during potentially long bridge work.
    pub fn install_callback(
        &self,
        name: &str,
        callback: impl Fn(ExecutionControl, String) -> Result<String, EvalError> + 'static,
    ) -> Result<(), EvalError> {
        let weak: Weak<ControlState> = Arc::downgrade(&self.control.0);
        self.enter(|ctx| {
            let f = Function::new(
                ctx.clone(),
                move |input: String| -> rquickjs::Result<String> {
                    let state = weak.upgrade().ok_or(rquickjs::Error::Unknown)?;
                    let control = ExecutionControl(state);
                    control.poll().map_err(|_| rquickjs::Error::Unknown)?;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        callback(control.clone(), input)
                    }));
                    let result = match result {
                        Ok(Ok(value)) => Ok(value),
                        _ => {
                            control.invalidate();
                            Err(rquickjs::Error::Unknown)
                        }
                    };
                    control.poll().map_err(|_| rquickjs::Error::Unknown)?;
                    result
                },
            )?;
            ctx.globals().set(name, f)
        })
    }
    /// Live engine allocation accounting, useful for focused retention measurements.
    pub fn memory_usage_bytes(&self) -> i64 {
        self.runtime.memory_usage().memory_used_size
    }
}
impl Drop for Evaluator {
    fn drop(&mut self) {
        // Interrupts may prevent explicit clear; Context/Runtime GC still owns all
        // edges, including cycles, with no Persistent roots or strong callbacks.
        if let Some(context) = self.context.take() {
            context.with(|ctx| {
                let _ = ctx.eval::<(), _>("__lightEvaluator.clear()");
            });
            drop(context);
        }
        self.runtime.set_interrupt_handler(None);
        self.runtime.run_gc();
    }
}

// The engine's own memory ceiling can be caught in JavaScript. Track rejected
// allocations at the allocator boundary so that catching cannot reset a limit.
// All pointer layout/alignment work delegates to the upstream RustAllocator.
struct LimitedAllocator {
    used: usize,
    limit: usize,
    control: Weak<ControlState>,
}
impl LimitedAllocator {
    fn reject(&self) -> *mut u8 {
        if let Some(state) = self.control.upgrade() {
            let _ = state
                .terminal
                .compare_exchange(0, 5, Ordering::Relaxed, Ordering::Relaxed);
        }
        std::ptr::null_mut()
    }
    fn fits(&self, old: usize, size: usize) -> bool {
        size.checked_add(7)
            .map(|n| n & !7)
            .and_then(|n| self.used.checked_sub(old)?.checked_add(n))
            .is_some_and(|n| n <= self.limit && n < isize::MAX as usize - 16)
    }
}
// SAFETY: every allocation and its usable size use the same upstream allocator;
// failure leaves an existing realloc allocation untouched. No pointer escapes.
unsafe impl Allocator for LimitedAllocator {
    fn alloc(&mut self, size: usize) -> *mut u8 {
        if !self.fits(0, size) {
            return self.reject();
        }
        let ptr = RustAllocator.alloc(size);
        if ptr.is_null() {
            return self.reject();
        }
        self.used += unsafe { RustAllocator::usable_size(ptr) };
        ptr
    }
    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        let Some(size) = count.checked_mul(size) else {
            return self.reject();
        };
        let ptr = self.alloc(size);
        if !ptr.is_null() {
            unsafe {
                ptr.write_bytes(0, size);
            }
        }
        ptr
    }
    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        self.used -= unsafe { RustAllocator::usable_size(ptr) };
        unsafe {
            RustAllocator.dealloc(ptr);
        }
    }
    unsafe fn realloc(&mut self, ptr: *mut u8, size: usize) -> *mut u8 {
        if ptr.is_null() {
            return self.alloc(size);
        }
        if size == 0 {
            unsafe {
                self.dealloc(ptr);
            }
            return std::ptr::null_mut();
        }
        let old = unsafe { RustAllocator::usable_size(ptr) };
        if !self.fits(old, size) {
            return self.reject();
        }
        let result = unsafe { RustAllocator.realloc(ptr, size) };
        if result.is_null() {
            return self.reject();
        }
        self.used = self.used - old + unsafe { RustAllocator::usable_size(result) };
        result
    }
    unsafe fn usable_size(ptr: *mut u8) -> usize {
        unsafe { RustAllocator::usable_size(ptr) }
    }
}

impl Drop for LimitedAllocator {
    fn drop(&mut self) {
        debug_assert_eq!(
            self.used, 0,
            "QuickJS runtime retained live allocations at teardown"
        );
    }
}

// The callable returns Rust-only data; IntoJs creates all temporary engine edges
// under the callback's actual lifetime. No lifetime cast or persistent capture.
struct BridgeOutput {
    data: Form,
    bridge: Rc<RefCell<BridgeState>>,
    control: ExecutionControl,
    limits: ConversionLimits,
}
impl<'js> rquickjs::IntoJs<'js> for BridgeOutput {
    fn into_js(self, ctx: &Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>> {
        self.bridge.borrow_mut().retain(&self.data);
        let descriptor =
            self.bridge
                .borrow_mut()
                .encode(ctx, &self.data.data(), &self.control, self.limits)?;
        let import: Function = ctx.eval("__lightEvaluator.importForm")?;
        let id: u64 = import.call((descriptor.clone(),))?;
        let export: Function = ctx.eval("__lightEvaluator.exportForm")?;
        let snapshot: Array = export.call((id, self.limits.nodes, self.limits.depth))?;
        self.bridge
            .borrow_mut()
            .decode(snapshot, &self.control, self.limits, true)?;
        let release: Function = ctx.eval("__lightEvaluator.release")?;
        release.call::<_, ()>((id,))?;
        Ok(descriptor.into_value())
    }
}
