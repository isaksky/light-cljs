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
    clock_origin: Instant,
    deadline_nanos: AtomicU64,
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
        } else if self.0.clock_origin.elapsed().as_nanos()
            >= u128::from(self.0.deadline_nanos.load(Ordering::Relaxed))
        {
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
    preserve_language_errors: bool,
}
impl Evaluator {
    pub fn new(options: EvaluatorOptions) -> Result<Self, EvalError> {
        Self::create(options, false, None)
    }
    pub(crate) fn interactive(options: EvaluatorOptions) -> Result<Self, EvalError> {
        Self::create(options, true, None)
    }
    pub(crate) fn interactive_with_control(
        options: EvaluatorOptions,
        control: ExecutionControl,
    ) -> Result<Self, EvalError> {
        Self::create(options, true, Some(control))
    }
    fn create(
        options: EvaluatorOptions,
        preserve_language_errors: bool,
        shared: Option<ExecutionControl>,
    ) -> Result<Self, EvalError> {
        if options.memory_limit < 1024 * 1024 {
            return Err(EvalError(
                "JavaScript memory limit must be at least 1048576 bytes for engine startup".into(),
            ));
        }
        let control = shared.unwrap_or(ExecutionControl(Arc::new(ControlState {
            cancelled: AtomicBool::new(false),
            terminal: AtomicU8::new(0),
            remaining: AtomicU64::new(options.poll_budget),
            clock_origin: Instant::now(),
            deadline_nanos: AtomicU64::new(
                u64::try_from(options.timeout.as_nanos())
                    .map_err(|_| EvalError("invalid session timeout".into()))?,
            ),
        })));
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
            preserve_language_errors,
        };
        session.enter(|ctx| ctx.eval::<(), _>(format!(r#"
            const __lightEvaluator = (() => {{
              const rt = {RUNTIME_SOURCE};
              const values = new Map(); let next = 1; let lastError = null;
              let hostDiagnostic = null;
              const exceptionMessage = error => {{
                if (hostDiagnostic !== null && Object.is(error, hostDiagnostic.value)) {{ const message = hostDiagnostic.message; hostDiagnostic = null; return message; }}
                return rt.compilerExceptionMessage(error);
              }};
              const bridge = rt.compilerForms;
              if (bridge.version !== {FORM_BRIDGE_VERSION}) throw new Error("Compiler form bridge version mismatch");
              const clearValues = Map.prototype.clear.bind(values);
              const save = value => {{ const id = next++; values.set(id, value); return id; }};
              const get = id => {{ if (!values.has(id)) throw new Error('Released or unknown value ID'); return values.get(id); }};
              return Object.freeze({{
                save,
                bind: (name, id) => {{ (globalThis.__lightApplicationRoots ??= Object.create(null))[name] = get(id); }},
                coreBinding: (name, id) => {{ rt.core[name] = get(id); }},
                coreLookup: name => save(rt.core[name]),
                lookup: name => save((globalThis.__lightApplicationRoots ??= Object.create(null))[name]),
                hostFunction: callback => (...args) => {{
                  const ids = args.map(save);
                  try {{ const [result, ...owned] = callback(ids); try {{ return get(result); }} finally {{ owned.forEach(id => values.delete(id)); }} }}
                  finally {{ ids.forEach(id => values.delete(id)); }}
                }},
                inputProvider: (name, provider) => save(rt.core.readerInputProvider(name, provider)),
                importForm: descriptor => save(bridge.decode(descriptor)),
                exportForm: (id, nodes, depth) => bridge.encode(get(id), nodes, depth),
                formServices: (one, all, nodes, depth) => rt.setMacroexpandServices({{macroexpand1: form => bridge.decode(one(bridge.encode(form, nodes, depth))), macroexpand: form => bridge.decode(all(bridge.encode(form, nodes, depth)))}}),
                bridgeVersion: bridge.version,
                checkBridge: bridge.check,
                errorData: () => {{ const error = lastError; return save(rt.core.exData(error)); }},
                errorValue: () => lastError,
                rethrowError: (error, message) => {{
                  hostDiagnostic = {{value: error, message}};
                  throw error;
                }},
                evaluate: source => {{
                  lastError = null;
                  try {{ const result = save((0, eval)(source)(rt)); hostDiagnostic = null; return result; }}
                  catch (error) {{ lastError = error; const message = exceptionMessage(error); if (message !== undefined) throw new Error(message); throw error; }}
                }},
                call: (id, ids) => {{
                  try {{ const result = save(Reflect.apply(get(id), null, ids.map(get))); hostDiagnostic = null; return result; }}
                  catch (error) {{
                    lastError = error;
                    const message = exceptionMessage(error);
                    if (message !== undefined) throw new Error(message);
                    throw error;
                  }}
                }},
                number: id => {{ const v = get(id); if (typeof v !== 'number') throw new TypeError('Expected Number'); return v; }},
                string: id => {{ const v = get(id); if (typeof v !== 'string') throw new TypeError('Expected string'); return v; }},
                lastError: () => save(lastError),
                clearError: () => {{ lastError = null; }},
                inputWindow: more => rt.core.compilerInputWindow(more),
                inputConsume: units => rt.core.compilerInputConsume(units),
                inputRecover: () => rt.core.compilerInputRecover(),
                inputDiscard: () => rt.core.compilerInputDiscard(),
                inputCompact: () => rt.core.compilerInputCompact(),
                print: id => {{ try {{ return rt.core.printForHost(get(id)); }} catch (error) {{ lastError = error; const message = rt.compilerExceptionMessage(error); if (message !== undefined) throw new Error(message); throw error; }} }},
                validate: id => {{ get(id); }},
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
    pub(crate) fn begin_operation(&self, options: &EvaluatorOptions) -> Result<(), EvalError> {
        if self.control.0.terminal.load(Ordering::Relaxed) != 0
            || self.control.0.cancelled.load(Ordering::Relaxed)
        {
            return self.control.poll();
        }
        let deadline = self
            .control
            .0
            .clock_origin
            .elapsed()
            .as_nanos()
            .checked_add(options.timeout.as_nanos())
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(|| EvalError("invalid operation timeout".into()))?;
        self.control
            .0
            .deadline_nanos
            .store(deadline, Ordering::Relaxed);
        self.control
            .0
            .remaining
            .store(options.poll_budget, Ordering::Relaxed);
        self.control.poll()
    }

    fn enter<T>(
        &self,
        f: impl for<'js> FnOnce(Ctx<'js>) -> rquickjs::Result<T>,
    ) -> Result<T, EvalError> {
        self.control.poll()?;
        let result = self.context.as_ref().unwrap().with(|ctx| {
            // rquickjs refreshes this only with its parallel feature. Our synchronous
            // compiler enters from different Rust analysis depths; budget JS stack
            // from this outer boundary, not the runtime constructor's stack frame.
            // The context is live under Context::with; no pointer is retained and
            // nested callbacks do not call enter or reset this outer stack budget.
            unsafe {
                rquickjs::qjs::JS_UpdateStackTop(rquickjs::qjs::JS_GetRuntime(
                    ctx.as_raw().as_ptr(),
                ));
            }

            let operation = f(ctx.clone());
            let operation = operation.and_then(|value| {
                ctx.eval::<(), _>("__lightEvaluator.checkBridge()")?;
                Ok(value)
            });
            operation
                .catch(&ctx)
                .map_err(|error| EvalError(error.to_string()))
        });
        if let Err(error) = self.control.poll()
            && (result.is_ok() || self.control.0.terminal.load(Ordering::Relaxed) != 4)
        {
            return Err(error);
        }
        // Engine failures poison this operation. Never retry through a native path.
        if result.is_err() && !self.preserve_language_errors {
            self.control.invalidate();
        }
        result
    }
    /// Borrow the already-active engine context without acquiring its runtime lock.
    /// The context must belong to this evaluator; the handle cannot outlive it.
    pub fn active<'a, 'js>(&'a self, ctx: Ctx<'js>) -> Result<ActiveEvaluator<'a, 'js>, EvalError> {
        if self.context.as_ref().unwrap().as_raw() != ctx.as_raw() {
            return Err(EvalError(
                "Context belongs to another JavaScript session".into(),
            ));
        }
        Ok(ActiveEvaluator {
            ctx,
            control: &self.control,
            generation: self.generation,
            bridge: &self.bridge,
            preserve_language_errors: self.preserve_language_errors,
        })
    }

    fn with_active<T>(
        &self,
        f: impl for<'js> FnOnce(ActiveEvaluator<'_, 'js>) -> Result<T, EvalError>,
    ) -> Result<T, EvalError> {
        self.control.poll()?;
        self.context.as_ref().unwrap().with(|ctx| {
            // Refresh only at the outer runtime-lock boundary.
            unsafe {
                rquickjs::qjs::JS_UpdateStackTop(rquickjs::qjs::JS_GetRuntime(
                    ctx.as_raw().as_ptr(),
                ));
            }
            f(self.active(ctx)?)
        })
    }
    pub fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.with_active(|active| active.evaluate_unit(source))
    }
    pub fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        self.with_active(|active| active.evaluate_js(expression))
    }
    pub fn evaluate(&self, program: &crate::Program) -> Result<ValueId, EvalError> {
        self.with_active(|active| active.evaluate(program))
    }
    pub fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        self.with_active(|active| active.call(function, arguments))
    }
    pub(crate) fn error_data(&self) -> Option<Datum> {
        self.with_active(|active| {
            let index = active.enter(|ctx| {
                let f: Function = ctx.eval("__lightEvaluator.errorData")?;
                f.call(())
            })?;
            let id = ValueId {
                session: self.generation,
                index,
            };
            let result = active.export_data(id, ConversionLimits::default());
            let _ = active.release(id);
            result
        })
        .ok()
        .filter(|data| !matches!(data.kind(), crate::compiler_data::DatumKind::Nil))
    }
    pub(crate) fn validate(&self, id: ValueId) -> Result<(), EvalError> {
        self.with_active(|active| {
            let id = active.check(id)?;
            active.enter(|ctx| {
                let f: Function = ctx.eval("__lightEvaluator.validate")?;
                f.call((id,))
            })
        })
    }
    pub(crate) fn bind(&self, name: &str, id: ValueId, core: bool) -> Result<(), EvalError> {
        self.with_active(|active| active.bind(name, id, core))
    }
    pub(crate) fn lookup(&self, name: &str, core: bool) -> Result<ValueId, EvalError> {
        self.with_active(|active| active.lookup(name, core))
    }
    pub fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        self.with_active(|active| active.number(id))
    }
    pub fn string(&self, id: ValueId) -> Result<String, EvalError> {
        self.with_active(|active| active.string(id))
    }
    pub fn print(&self, id: ValueId) -> Result<String, EvalError> {
        self.with_active(|active| active.print(id))
    }
    pub fn release(&self, id: ValueId) -> Result<(), EvalError> {
        self.with_active(|active| active.release(id))
    }
    pub fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.with_active(|active| active.import_form(form, limits))
    }
    pub fn import_data(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.with_active(|active| active.import_data(data, limits))
    }
    pub fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError> {
        self.with_active(|active| active.export_data(id, limits))
    }
    pub fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: crate::compiler_data::ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        self.with_active(|active| active.export_form(id, call, origin, limits))
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
    /// Install a compiler service that returns emitted source for a plain request ID.
    /// The callback receives the current context so nested macro execution does not
    /// reacquire the runtime lock. Captures must retain only Rust data and weak
    /// evaluator references, never engine values or a strong evaluator reference.
    pub fn install_unit_service(
        &self,
        name: &str,
        callback: impl for<'js> Fn(Ctx<'js>, u64) -> Result<String, String> + 'static,
    ) -> Result<(), EvalError> {
        let weak = Arc::downgrade(&self.control.0);
        self.enter(|ctx| {
            let f = Function::new(ctx.clone(), move |ctx: Ctx<'_>, request: u64| {
                let control = ExecutionControl(weak.upgrade().ok_or(rquickjs::Error::Unknown)?);
                control
                    .poll()
                    .map_err(|e| rquickjs::Exception::throw_message(&ctx, &e.0))?;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    callback(ctx.clone(), request)
                }));
                let output = match result {
                    Ok(Ok(output)) => output,
                    Ok(Err(message)) => {
                        control.invalidate();
                        return Err(rquickjs::Exception::throw_message(&ctx, &message));
                    }
                    Err(_) => {
                        control.invalidate();
                        return Err(rquickjs::Exception::throw_message(
                            &ctx,
                            "compiler unit service panicked",
                        ));
                    }
                };
                control
                    .poll()
                    .map_err(|e| rquickjs::Exception::throw_message(&ctx, &e.0))?;
                Ok(output)
            })?;
            ctx.globals().set(name, f)
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
    /// Install a typed, reentrant host capability. Callback handles are scoped
    /// to the call; only explicit session handles may be retained by the host.
    pub(crate) fn install_host_function(
        &self,
        name: &str,
        arity: HostArity,
        callback: crate::eval_session::HostCallback,
    ) -> Result<ValueId, EvalError> {
        let control = self.control.clone();
        let generation = self.generation;
        let bridge = self.bridge.clone();
        let name = name.to_owned();
        self.enter(|ctx| {
            let callback = Function::new(ctx.clone(), move |ctx: Ctx<'_>, ids: Vec<u64>| {
                if !arity.accepts(ids.len()) {
                    return Err(rquickjs::Exception::throw_message(
                        &ctx,
                        &format!("{name}: invalid arity {}", ids.len()),
                    ));
                }
                let call = HostCall {
                    active: ActiveEvaluator {
                        ctx: ctx.clone(),
                        control: &control,
                        generation,
                        bridge: &bridge,
                        preserve_language_errors: true,
                    },
                    owned: RefCell::new(Vec::new()),
                    unit_exception: RefCell::new(None),
                };
                let args = ids
                    .into_iter()
                    .map(|index| ValueId {
                        session: generation,
                        index,
                    })
                    .collect::<Vec<_>>();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    callback(&call, &args)
                }));
                let result = match result {
                    Ok(result) => result.and_then(|id| call.active.check(id)),
                    Err(_) => {
                        control.invalidate();
                        Err(EvalError(format!("{name}: host callback panicked")))
                    }
                }
                .and_then(|index| {
                    control.poll()?;
                    Ok(index)
                });
                let owned = call.owned.into_inner();
                match result {
                    Ok(index) => Ok(std::iter::once(index)
                        .chain(owned.into_iter().map(|id| id.index))
                        .collect::<Vec<_>>()),
                    Err(error) => {
                        for id in owned {
                            let _ = call.active.release(id);
                        }
                        if let Some(exception) = call.unit_exception.into_inner() {
                            let rethrow: Function = ctx.eval("__lightEvaluator.rethrowError")?;
                            return rethrow.call((exception, format!("{name}: {error}")));
                        }
                        Err(rquickjs::Exception::throw_message(
                            &ctx,
                            &format!("{name}: {error}"),
                        ))
                    }
                }
            })?;
            let factory: Function = ctx.eval("__lightEvaluator.hostFunction")?;
            let function: Function = factory.call((callback,))?;
            // Save through the same registry without exposing engine handles.
            let save: Function = ctx.eval("__lightEvaluator.save")?;
            let index = save.call((function,))?;
            Ok(ValueId {
                session: generation,
                index,
            })
        })
    }

    /// Create an explicit synchronous input capability. Providers return nonempty
    /// chunks or `None` at EOF; the canonical JavaScript cursor performs parsing.
    /// The closure retains Rust data only and shares the session's control budget.
    pub fn input_provider(
        &self,
        name: &str,
        provider: impl Fn(ExecutionControl) -> Result<Option<String>, EvalError> + 'static,
    ) -> Result<ValueId, EvalError> {
        let control = self.control.clone();
        self.enter(|ctx| {
            let callback = Function::new(ctx.clone(), move |ctx: Ctx<'_>| {
                control
                    .poll()
                    .map_err(|e| rquickjs::Exception::throw_message(&ctx, &e.0))?;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    provider(control.clone())
                }));
                let chunk = match result {
                    Ok(Ok(chunk)) => chunk,
                    Ok(Err(error)) => {
                        return Err(rquickjs::Exception::throw_message(
                            &ctx,
                            &format!("input provider failed: {error}"),
                        ));
                    }
                    Err(_) => {
                        control.invalidate();
                        return Err(rquickjs::Exception::throw_message(
                            &ctx,
                            "input provider panicked",
                        ));
                    }
                };
                control
                    .poll()
                    .map_err(|e| rquickjs::Exception::throw_message(&ctx, &e.0))?;
                Ok(chunk)
            })?;
            let create: Function = ctx.eval("__lightEvaluator.inputProvider")?;
            let index = create.call((name, callback))?;
            Ok(ValueId {
                session: self.generation,
                index,
            })
        })
    }
    pub(crate) fn input_window(
        &self,
        more: bool,
    ) -> Result<(String, usize, bool, usize), EvalError> {
        self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.inputWindow")?;
            let a: Array = f.call((more,))?;
            Ok((a.get(0)?, a.get(1)?, a.get(2)?, a.get(3)?))
        })
    }
    pub(crate) fn input_action(&self, action: &str, units: usize) -> Result<(), EvalError> {
        self.enter(|ctx| {
            let f: Function = ctx.eval(format!("__lightEvaluator.{action}"))?;
            f.call((units,))
        })
    }
    pub(crate) fn last_exception(&self) -> Result<ValueId, EvalError> {
        self.enter(|ctx| {
            let f: Function = ctx.eval("__lightEvaluator.lastError")?;
            Ok(ValueId {
                session: self.generation,
                index: f.call(())?,
            })
        })
    }
    /// Live engine allocation accounting, useful for focused retention measurements.
    pub fn memory_usage_bytes(&self) -> i64 {
        self.runtime.memory_usage().memory_used_size
    }
}
/// Common macro operations at either an outer boundary or an active callback.
pub trait MacroEvaluator {
    fn control(&self) -> ExecutionControl;
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError>;
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError>;
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError>;
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError>;
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: crate::compiler_data::ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError>;
    fn release(&self, id: ValueId) -> Result<(), EvalError>;
}
impl MacroEvaluator for Evaluator {
    fn control(&self) -> ExecutionControl {
        Evaluator::control(self)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        Evaluator::evaluate_unit(self, source)
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        Evaluator::call(self, function, arguments)
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        Evaluator::import_form(self, form, limits)
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        Evaluator::import_data(self, data, limits)
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: crate::compiler_data::ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        Evaluator::export_form(self, id, call, origin, limits)
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        Evaluator::release(self, id)
    }
}
impl MacroEvaluator for ActiveEvaluator<'_, '_> {
    fn control(&self) -> ExecutionControl {
        ActiveEvaluator::control(self)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        ActiveEvaluator::evaluate_unit(self, source)
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        ActiveEvaluator::call(self, function, arguments)
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        ActiveEvaluator::import_form(self, form, limits)
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        ActiveEvaluator::import_data(self, data, limits)
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: crate::compiler_data::ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        ActiveEvaluator::export_form(self, id, call, origin, limits)
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        ActiveEvaluator::release(self, id)
    }
}
/// A scoped view of an evaluator while its JavaScript context is already active.
/// Nested compiler services must use this handle instead of entering Context::with.
pub struct ActiveEvaluator<'a, 'js> {
    ctx: Ctx<'js>,
    control: &'a ExecutionControl,
    generation: u64,
    bridge: &'a Rc<RefCell<BridgeState>>,
    preserve_language_errors: bool,
}
impl<'js> ActiveEvaluator<'_, 'js> {
    pub fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn enter<T>(&self, f: impl FnOnce(Ctx<'js>) -> rquickjs::Result<T>) -> Result<T, EvalError> {
        self.control.poll()?;
        let result = f(self.ctx.clone())
            .and_then(|value| {
                self.ctx.eval::<(), _>("__lightEvaluator.checkBridge()")?;
                Ok(value)
            })
            .catch(&self.ctx)
            .map_err(|error| EvalError(error.to_string()));
        // Preserve the owned original diagnostic while nested scopes unwind a
        // poisoned session; cancellation/resource limits still take precedence.
        if let Err(error) = self.control.poll()
            && (result.is_ok() || self.control.0.terminal.load(Ordering::Relaxed) != 4)
        {
            return Err(error);
        }
        if result.is_err() && !self.preserve_language_errors {
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
    pub(crate) fn bind(&self, name: &str, id: ValueId, core: bool) -> Result<(), EvalError> {
        let id = self.check(id)?;
        self.enter(|ctx| {
            let f: Function = ctx.eval(if core {
                "__lightEvaluator.coreBinding"
            } else {
                "__lightEvaluator.bind"
            })?;
            f.call((name, id))
        })
    }
    pub(crate) fn lookup(&self, name: &str, core: bool) -> Result<ValueId, EvalError> {
        let index = self.enter(|ctx| {
            let f: Function = ctx.eval(if core {
                "__lightEvaluator.coreLookup"
            } else {
                "__lightEvaluator.lookup"
            })?;
            f.call((name,))
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
                .encode(&ctx, data, self.control, limits)?;
            let f: Function = ctx.eval("__lightEvaluator.importForm")?;
            let index: u64 = f.call((descriptor,))?;
            // Snapshot canonical payloads so mutable host literals are never reused
            // after modification. Reader lexical distinctions remain in originals.
            let export: Function = ctx.eval("__lightEvaluator.exportForm")?;
            let snapshot: Array = export.call((index, limits.nodes, limits.depth))?;
            self.bridge
                .borrow_mut()
                .decode(snapshot, self.control, limits, true)?;
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
                .decode(descriptor, self.control, limits, false)
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

#[cfg(test)]
mod active_tests {
    use super::*;
    use crate::compiler_data::DatumKind;

    #[test]
    fn unit_service_recurses_with_active_context_and_bridge() {
        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        let weak = Rc::downgrade(&evaluator);
        evaluator.install_unit_service("compileNested", move |ctx, request| {
            let evaluator = weak.upgrade().unwrap();
            let active = evaluator.active(ctx).map_err(|e| e.0)?;
            let run = || -> Result<String, EvalError> {
                let limits = ConversionLimits::default();
                let input = active.import_data(&Datum::new(DatumKind::Integer(41)), limits)?;
                let identity = active.evaluate_js("x => x")?;
                let result = active.call(identity, &[input])?;
                assert!(matches!(active.export_data(result, limits)?.kind(), DatumKind::Integer(n) if *n == 41));
                active.release(result)?;
                active.release(identity)?;
                active.release(input)?;
                if request > 0 {
                    let nested = active.evaluate_js(&format!("compileNested({})", request - 1))?;
                    assert_eq!(active.string(nested)?, "(rt) => 42");
                    active.release(nested)?;
                }
                Ok("(rt) => 42".into())
            };
            run().map_err(|e| e.0)
        }).unwrap();
        let result = evaluator.evaluate_js("compileNested(3)").unwrap();
        assert_eq!(evaluator.string(result).unwrap(), "(rt) => 42");
        evaluator.release(result).unwrap();
        assert_eq!(Rc::strong_count(&evaluator), 1);
    }

    #[test]
    fn unit_service_cancellation_is_sticky_even_when_js_catches() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let control = evaluator.control();
        evaluator
            .install_unit_service("cancelCompile", move |_, _| {
                control.cancel();
                Ok("(rt) => 42".into())
            })
            .unwrap();
        let error = evaluator
            .evaluate_js("(() => { try { cancelCompile(0) } catch (_) {} return 1 })()")
            .unwrap_err();
        assert!(error.0.contains("cancelled"), "{error}");
        assert!(evaluator.evaluate_js("1").is_err());
    }

    #[test]
    fn unit_service_failure_is_sticky_even_when_js_catches() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        evaluator
            .install_unit_service("failCompile", |_, _| Err("compile failed".into()))
            .unwrap();
        assert!(
            evaluator
                .evaluate_js("(() => { try { failCompile(0) } catch (_) {} return 1 })()")
                .is_err()
        );
        assert!(evaluator.evaluate_js("1").is_err());
    }

    #[test]
    fn active_handle_rejects_other_contexts() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let other = Evaluator::new(EvaluatorOptions::default()).unwrap();
        other.context.as_ref().unwrap().with(|ctx| {
            assert!(evaluator.active(ctx).is_err());
        });
    }

    #[test]
    fn backtracking_deadline_is_terminal_and_fresh_operations_recover() {
        let host = crate::MacroHost::new();
        let evaluator = host.new_evaluator().unwrap();
        let short = EvaluatorOptions {
            timeout: Duration::from_millis(40),
            ..Default::default()
        };
        // Keep runtime startup outside the intentionally short regex window.
        evaluator.begin_operation(&short).unwrap();
        let work = "(()=>{try { /^(a+)+$/.test('a'.repeat(28)+'!'); } catch(e) {} return 42;})()";
        let error = evaluator.evaluate_js(work).unwrap_err();
        assert!(error.0.contains("deadline exceeded"), "{error}");
        assert!(
            evaluator
                .evaluate_js("42")
                .unwrap_err()
                .0
                .contains("deadline exceeded")
        );
        assert!(
            evaluator
                .begin_operation(&EvaluatorOptions::default())
                .unwrap_err()
                .0
                .contains("deadline exceeded")
        );

        // Recovery has its own normal operation timeout, preserving the short
        // bound above without imposing it on a fresh heap's initialization.
        let fresh = host.new_evaluator().unwrap();
        fresh.begin_operation(&host.evaluator_options).unwrap();
        assert_eq!(
            fresh.number(fresh.evaluate_js("42").unwrap()).unwrap(),
            42.0
        );
        assert!(
            evaluator
                .evaluate_js("42")
                .unwrap_err()
                .0
                .contains("deadline exceeded")
        );
    }
}

/// Arity checked before entering an embedding callback.
#[derive(Clone, Copy, Debug)]
pub enum HostArity {
    Exact(usize),
    AtLeast(usize),
    Range { min: usize, max: usize },
}
impl HostArity {
    fn accepts(self, n: usize) -> bool {
        match self {
            Self::Exact(a) => n == a,
            Self::AtLeast(a) => n >= a,
            Self::Range { min, max } => n >= min && n <= max,
        }
    }
}
/// Engine-neutral view of one active callback. It never reacquires the engine
/// lock; recursive invocation uses the already active context. IDs created here
/// and argument IDs expire when the callback returns.
pub struct HostCall<'a, 'js> {
    active: ActiveEvaluator<'a, 'js>,
    owned: RefCell<Vec<ValueId>>,
    unit_exception: RefCell<Option<rquickjs::Value<'js>>>,
}
impl HostCall<'_, '_> {
    fn own(&self, value: Result<ValueId, EvalError>) -> Result<ValueId, EvalError> {
        let value = value?;
        self.owned.borrow_mut().push(value);
        Ok(value)
    }
    pub fn number(&self, value: ValueId) -> Result<f64, EvalError> {
        self.active.number(value)
    }
    pub fn string(&self, value: ValueId) -> Result<String, EvalError> {
        self.active.string(value)
    }
    pub fn print(&self, value: ValueId) -> Result<String, EvalError> {
        self.active.print(value)
    }
    pub fn data(&self, value: ValueId) -> Result<Datum, EvalError> {
        self.active.export_data(value, ConversionLimits::default())
    }
    pub fn value(&self, data: &Datum) -> Result<ValueId, EvalError> {
        self.own(self.active.import_data(data, ConversionLimits::default()))
    }
    pub fn invoke(&self, function: ValueId, args: &[ValueId]) -> Result<ValueId, EvalError> {
        self.own(self.active.call(function, args))
    }
    pub fn control(&self) -> ExecutionControl {
        self.active.control()
    }
    pub(crate) fn release_scoped(&self, value: ValueId) -> Result<(), EvalError> {
        let mut owned = self.owned.borrow_mut();
        let index = owned
            .iter()
            .position(|id| *id == value)
            .ok_or_else(|| EvalError("value is not owned by this callback".into()))?;
        self.active.release(value)?;
        owned.swap_remove(index);
        Ok(())
    }
    pub(crate) fn unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.unit_exception.borrow_mut().take();
        let result = self.active.evaluate_unit(source);
        if result.is_err()
            && let Ok(read) = self
                .active
                .ctx
                .eval::<Function, _>("__lightEvaluator.errorValue")
            && let Ok(exception) = read.call(())
        {
            *self.unit_exception.borrow_mut() = Some(exception);
        }
        self.own(result)
    }
}
